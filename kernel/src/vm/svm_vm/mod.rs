mod vmcb;

use core::alloc::{GlobalAlloc, Layout};
use core::fmt;

use crate::mm::{self, PhysicalPage};

pub use vmcb::{Vmcb, VmcbSegmentDescriptor};

const VM_CR_MSR:       u32 = 0xc001_0114;
const VM_HSAVE_PA_MSR: u32 = 0xc001_0117;
const EFER_MSR:        u32 = 0xc000_0080;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VmError {
    NonAmdCpu,
    SvmNotSupported,
    SvmDisabled,
}

impl VmError {
    fn to_string(&self) -> &'static str {
        match self {
            VmError::NonAmdCpu       => "CPU vendor is not `AuthenticAMD`.",
            VmError::SvmNotSupported => "CPUID reported that SVM is not supported.",
            VmError::SvmDisabled     => "SVM is disabled by the BIOS.",
        }
    }
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

fn enable_svm() -> Result<(), VmError> {
    // If host save area is allocated than SVM is already enabled on this processor.
    if core!().host_save_area.lock().is_some() {
        return Ok(());
    }

    {
        let cpuid      = cpu::cpuid(0, 0);
        let mut vendor = [0u8; 3 * 4];

        vendor[0.. 4].copy_from_slice(&cpuid.ebx.to_le_bytes());
        vendor[4.. 8].copy_from_slice(&cpuid.edx.to_le_bytes());
        vendor[8..12].copy_from_slice(&cpuid.ecx.to_le_bytes());

        if &vendor != b"AuthenticAMD" {
            return Err(VmError::NonAmdCpu);
        }
    }

    if !cpu::get_features().svm {
        return Err(VmError::SvmNotSupported);
    }

    let svm_cr = unsafe { cpu::rdmsr(VM_CR_MSR) };
    if  svm_cr & (1 << 4) != 0 {
        return Err(VmError::SvmDisabled);
    }

    // Allocate area used by the SVM to store host state on `vmrun`.
    let host_save_area = PhysicalPage::new([0u8; 4096]);

    unsafe {
        // Enable SVM.
        cpu::wrmsr(EFER_MSR, cpu::rdmsr(EFER_MSR) | (1 << 12));

        // Set physical address of the host save area.
        cpu::wrmsr(VM_HSAVE_PA_MSR, host_save_area.phys_addr().0);
    }

    // Store host save area in core locals.
    *core!().host_save_area.lock() = Some(host_save_area);

    Ok(())
}

struct XsaveArea {
    pointer: *mut u8,
}

impl XsaveArea {
    fn new() -> Self {
        unsafe {
            // Allocate the XSAVE area with appropriate size and alignment.
            let xsave_size   = core!().xsave_size();
            let xsave_layout = Layout::from_size_align(xsave_size, 64)
                .expect("Failed to create XSAVE layout.");
            let xsave_area   = mm::GLOBAL_ALLOCATOR.alloc(xsave_layout);

            assert!(!xsave_area.is_null(), "Failed to allocate XSAVE area.");

            // Zero out XSAVE area as required by the architecture.
            core::ptr::write_bytes(xsave_area, 0, xsave_size);

            Self {
                pointer: xsave_area,
            }
        }
    }
}

impl Drop for XsaveArea {
    fn drop(&mut self) {
        unsafe {
            let xsave_size   = core!().xsave_size();
            let xsave_layout = Layout::from_size_align(xsave_size, 64)
                .expect("Failed to create XSAVE layout.");

            // Free the XSAVE area.
            mm::GLOBAL_ALLOCATOR.dealloc(self.pointer, xsave_layout);
        }
    }
}

/// Enum that describes basic x86 registers.
/// Don't change the numbers without changing assembly in `Vm::run()`.
#[repr(usize)]
#[allow(unused)]
pub enum Register {
    Rax    = 0,
    Rcx    = 1,
    Rdx    = 2,
    Rbx    = 3,
    Rsp    = 4,
    Rbp    = 5,
    Rsi    = 6,
    Rdi    = 7,
    R8     = 8,
    R9     = 9,
    R10    = 10,
    R11    = 11,
    R12    = 12,
    R13    = 13,
    R14    = 14,
    R15    = 15,
    Rip    = 16,
    Rflags = 17,
}

pub struct Vm {
    /// Guest state. It must be configured by the user of the VM. It is loaded just before
    /// `vmrun` and saved right after.
    guest_vmcb:  PhysicalPage<Vmcb>,
    guest_xsave: XsaveArea,

    /// Host state. It doesn't require any configuration. It is saved just before `vmrun`
    /// and restored right after. `host_vmcb` is used only by `vmload` and `vmsave`, not `vmrun`.
    host_vmcb:  PhysicalPage<Vmcb>,
    host_xsave: XsaveArea,

    /// Basic register state for the guest. Contains all GPRs, RIP and RFLAGS.
    guest_registers: [u64; 18],
}

impl Vm {
    pub fn new() -> Result<Self, VmError> {
        // This VM requires AMD SVM so enable it first.
        enable_svm()?;

        Ok(Self {
            guest_vmcb:  Vmcb::new(),
            guest_xsave: XsaveArea::new(),

            host_vmcb:  Vmcb::new(),
            host_xsave: XsaveArea::new(),

            guest_registers: [0; 18],
        })
    }

    pub fn reg(&self, register: Register) -> u64 {
        self.guest_registers[register as usize]
    }

    pub fn set_reg(&mut self, register: Register, value: u64) {
        self.guest_registers[register as usize] = value;
    }

    pub fn vmcb(&self) -> &Vmcb {
        &self.guest_vmcb
    }

    pub fn vmcb_mut(&mut self) -> &mut Vmcb {
        &mut self.guest_vmcb
    }

    pub unsafe fn run(&mut self) {
        // Copy relevant registers from the cache to the VMCB.
        self.guest_vmcb.state.rax    = self.reg(Register::Rax);
        self.guest_vmcb.state.rsp    = self.reg(Register::Rsp);
        self.guest_vmcb.state.rip    = self.reg(Register::Rip);
        self.guest_vmcb.state.rflags = self.reg(Register::Rflags);

        asm!(
            r#"
                // Save RBP as it cannot be in the inline assembly clobber list.
                push rbp

                // Prepare XSAVE mask to affect X87, SSE and AVX state.
                xor edx, edx
                mov eax, (1 << 0) | (1 << 1) | (1 << 2)

                // Save host FPU state and load guest FPU state.
                xsave64  [r13]
                xrstor64 [r12]

                // Save host partial processor state.
                mov    rax, r11
                vmsave rax

                // Load guest partial processor state.
                mov    rax, r10
                vmload rax

                // Save all pointers on the stack.
                push r10
                push r11
                push r12
                push r13
                push r14

                // Make guest VMCB accessible later. We must put it on the stack as we can
                // only clobber RAX after loading guest GPRs.
                push r10

                // Get access to the guest GPR area.
                mov rax, r14

                // Load guest GPRs except RAX and RSP as these will be handled by `vmrun`.
                mov rcx, [rax + 1  * 8]
                mov rdx, [rax + 2  * 8]
                mov rbx, [rax + 3  * 8]
                mov rbp, [rax + 5  * 8]
                mov rsi, [rax + 6  * 8]
                mov rdi, [rax + 7  * 8]
                mov r8,  [rax + 8  * 8]
                mov r9,  [rax + 9  * 8]
                mov r10, [rax + 10 * 8]
                mov r11, [rax + 11 * 8]
                mov r12, [rax + 12 * 8]
                mov r13, [rax + 13 * 8]
                mov r14, [rax + 14 * 8]
                mov r15, [rax + 15 * 8]

                // Pop the guest VMCB and run the VM. RAX and RSP will be restored after `vmrun`
                // retires.
                pop   rax
                vmrun rax

                // Before saving guest GPRs we can only clobber RAX.
                // Get access to the guest GPR area. This pop corresponds to `push r14`.
                pop rax

                // Save guest GPRs except RAX and RSP as these were restored by the `vmrun`.
                mov [rax + 1  * 8], rcx
                mov [rax + 2  * 8], rdx
                mov [rax + 3  * 8], rbx
                mov [rax + 5  * 8], rbp
                mov [rax + 6  * 8], rsi
                mov [rax + 7  * 8], rdi
                mov [rax + 8  * 8], r8
                mov [rax + 9  * 8], r9
                mov [rax + 10 * 8], r10
                mov [rax + 11 * 8], r11
                mov [rax + 12 * 8], r12
                mov [rax + 13 * 8], r13
                mov [rax + 14 * 8], r14
                mov [rax + 15 * 8], r15

                // Get all pointers from the stack. `r14` was already popped above.
                mov r14, rax
                pop r13
                pop r12
                pop r11
                pop r10

                // Save guest partial processor state.
                mov    rax, r10
                vmsave rax

                // Load host partial processor state.
                mov    rax, r11
                vmload rax

                // Prepare XSAVE mask to affect X87, SSE and AVX state.
                xor edx, edx
                mov eax, (1 << 0) | (1 << 1) | (1 << 2)

                // Save guest FPU state and load host FPU state.
                xsave64  [r12]
                xrstor64 [r13]

                // Restore RBP pushed at the beginning.
                pop rbp
            "#,
            // All registers except RSP will be clobbered. R8-R14 are also used as inputs
            // so they are not here. RBP is pushed and popped by the inline assembly.
            out("rax") _,
            out("rbx") _,
            out("rcx") _,
            out("rdx") _,
            out("rdi") _,
            out("rsi") _,
            out("r15") _,

            // Pass the virtual VMCB pointers so compiler knows that this memory is used.
            inout("r8") self.guest_vmcb.pointer() => _,
            inout("r9") self.host_vmcb.pointer()  => _,

            // Pass pointers required to launch the VM.
            inout("r10") self.guest_vmcb.phys_addr().0     => _,
            inout("r11") self.host_vmcb.phys_addr().0      => _,
            inout("r12") self.guest_xsave.pointer          => _,
            inout("r13") self.host_xsave.pointer           => _,
            inout("r14") self.guest_registers.as_mut_ptr() => _,
        );

        // Copy relevant registers from the VMCB to the cache.
        self.set_reg(Register::Rax,    self.guest_vmcb.state.rax);
        self.set_reg(Register::Rsp,    self.guest_vmcb.state.rsp);
        self.set_reg(Register::Rip,    self.guest_vmcb.state.rip);
        self.set_reg(Register::Rflags, self.guest_vmcb.state.rflags);
    }
}
