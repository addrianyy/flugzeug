pub mod npt;
mod vmcb;
mod utils;
mod accessors;

use core::fmt;

use crate::mm::PhysicalPage;

use utils::{XsaveArea, SvmFeatures, Asid};
use vmcb::Vmcb;
use npt::{Npt, GuestAddr};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VmError {
    NonAmdCpu,
    SvmNotSupported,
    SvmDisabled,
    OutOfAsids,
    NPNotSupported,
    NRipSaveNotSupported,
}

impl VmError {
    fn to_string(&self) -> &'static str {
        match self {
            VmError::NonAmdCpu            => "CPU vendor is not `AuthenticAMD`.",
            VmError::SvmNotSupported      => "CPUID reported that SVM is not supported.",
            VmError::SvmDisabled          => "SVM is disabled by the BIOS.",
            VmError::OutOfAsids           => "Couldn't assign unique ASID for this VM.",
            VmError::NPNotSupported       => "SVM nested paging is not supported.",
            VmError::NRipSaveNotSupported => "Next RIP saving is not supported.",
        }
    }
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

// Don't change the numbers.
#[allow(unused)]
#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

    Efer,
    Cr0,
    Cr2,
    Cr3,
    Cr4,
    Dr6,
    Dr7,
    Star,
    Lstar,
    Cstar,
    Sfmask,
    KernelGsBase,
    SysenterCs,
    SysenterEsp,
    SysenterEip,
    Pat,
}

#[allow(unused)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TableRegister {
    Gdt,
    Idt,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DescriptorTable {
    pub base:  u64,
    pub limit: u16,
}

impl DescriptorTable {
    pub fn null() -> Self {
        Self {
            base:  0,
            limit: 0,
        }
    }
}

#[allow(unused)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SegmentRegister {
    Es,
    Cs,
    Ss,
    Ds,
    Fs,
    Gs,
    Ldt,
    Tr,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub selector: u16,
    pub attrib:   u16,
    pub limit:    u32,
    pub base:     u64,
}

impl Segment {
    pub fn null(selector: u16) -> Self {
        Self {
            selector,
            attrib: 0,
            limit:  0,
            base:   0,
        }
    }
}

const MISC_1: usize = 1 << 8;
const MISC_2: usize = 2 << 8;
const MISC_3: usize = 3 << 8;
const CR_RW:  usize = 4 << 8;
const DR_RW:  usize = 5 << 8;
const EXC:    usize = 6 << 8;

// Don't change the numbers.
#[allow(unused)]
#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Intercept {
    Intr      = MISC_1 | 0,
    Nmi       = MISC_1 | 1,
    Smi       = MISC_1 | 2,
    Init      = MISC_1 | 3,
    Vintr     = MISC_1 | 4,
    // Skipped "Intercept CR0 writes that change bits other than CR0.TS or CR0.MP." - always off.
    IdtrRead  = MISC_1 | 6,
    GdtrRead  = MISC_1 | 7,
    LdtrRead  = MISC_1 | 8,
    TrRead    = MISC_1 | 9,
    IdtrWrite = MISC_1 | 10,
    GdtrWrite = MISC_1 | 11,
    LdtrWrite = MISC_1 | 12,
    TrWrite   = MISC_1 | 13,
    Rdtsc     = MISC_1 | 14,
    Rdpmc     = MISC_1 | 15,
    Pushf     = MISC_1 | 16,
    Popf      = MISC_1 | 17,
    Cpuid     = MISC_1 | 18,
    Rsm       = MISC_1 | 19,
    Iret      = MISC_1 | 20,
    Int       = MISC_1 | 21,
    Invd      = MISC_1 | 22,
    Pause     = MISC_1 | 23,
    Hlt       = MISC_1 | 24,
    Invlpg    = MISC_1 | 25,
    Invlpga   = MISC_1 | 26,
    // Skipped IOIO_PROT and MSR_PROT - exposed by different API.
    // Skipped task switches and FERR_FREEZE - always off.
    // Skipped shutdown events - always on.

    // Skipped VMRUN - always on.
    Vmmcall = MISC_2 | 1,
    Vmload  = MISC_2 | 2,
    Vmsave  = MISC_2 | 3,
    Stgi    = MISC_2 | 4,
    Clgi    = MISC_2 | 5,
    Skinit  = MISC_2 | 6,
    Rdtscp  = MISC_2 | 7,
    Icebp   = MISC_2 | 8,
    Wbindvd = MISC_2 | 9,
    Monitor = MISC_2 | 10,
    Mwait   = MISC_2 | 11,
    // Skipped "Intercept MWAIT/MWAITX instruction if monitor hardware is armed" - always off.
    Xsetbv  = MISC_2 | 13,
    Rdpru   = MISC_2 | 14,
    // Skipped EFER and CR0-15. Both happen after guest instruction finishes. Always off.

    Invlpgb        = MISC_3 | 0,
    IllegalInvlpgb = MISC_3 | 1,
    Pcid           = MISC_3 | 2,
    Mcommit        = MISC_3 | 3,
    // Skipped TLBSYNC - not always supported. Always off.

    Cr0Read  = CR_RW | (0  + 0),
    Cr2Read  = CR_RW | (0  + 2),
    Cr3Read  = CR_RW | (0  + 3),
    Cr4Read  = CR_RW | (0  + 4),
    Cr8Read  = CR_RW | (0  + 8),
    Cr0Write = CR_RW | (16 + 0),
    Cr2Write = CR_RW | (16 + 2),
    Cr3Write = CR_RW | (16 + 3),
    Cr4Write = CR_RW | (16 + 4),
    Cr8Write = CR_RW | (16 + 8),

    Dr0Read  = DR_RW | (0  + 0),
    Dr1Read  = DR_RW | (0  + 1),
    Dr2Read  = DR_RW | (0  + 2),
    Dr3Read  = DR_RW | (0  + 3),
    Dr6Read  = DR_RW | (0  + 6),
    Dr7Read  = DR_RW | (0  + 7),
    Dr0Write = DR_RW | (16 + 0),
    Dr1Write = DR_RW | (16 + 1),
    Dr2Write = DR_RW | (16 + 2),
    Dr3Write = DR_RW | (16 + 3),
    Dr6Write = DR_RW | (16 + 6),
    Dr7Write = DR_RW | (16 + 7),

    De = EXC | 0,
    Db = EXC | 1,
    Bp = EXC | 3,
    Of = EXC | 4,
    Br = EXC | 5,
    Ud = EXC | 6,
    Nm = EXC | 7,
    Df = EXC | 8,
    Ts = EXC | 10,
    Np = EXC | 11,
    Ss = EXC | 12,
    Gp = EXC | 13,
    Pf = EXC | 14,
    Mf = EXC | 16,
    Ac = EXC | 17,
    Mc = EXC | 18,
    Xf = EXC | 19,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VmExit {
    Vmmcall,
    Hlt,
    Shutdown,
    NestedPageFault {
        address:  GuestAddr,
        present:  bool,
        write:    bool,
        execute:  bool,
    },
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

    /// Nested page tables for the guest.
    npt: Npt,

    /// Unique address space identifier for the guest.
    asid: Asid,
}

impl Vm {
    pub fn new() -> Result<Self, VmError> {
        // This VM requires AMD SVM so enable it first.
        utils::enable_svm()?;

        let features = SvmFeatures::get();
        let asid     = match Asid::new(features.nr_asids) {
            Some(asid) => asid,
            None       => return Err(VmError::OutOfAsids),
        };

        let mut vm = Self {
            guest_vmcb:  Vmcb::new(),
            guest_xsave: XsaveArea::new(),

            host_vmcb:  Vmcb::new(),
            host_xsave: XsaveArea::new(),

            guest_registers: [0; 18],
            npt:             Npt::new(),

            asid,
        };

        vm.initialize(&features)?;

        Ok(vm)
    }

    fn initialize(&mut self, features: &SvmFeatures) -> Result<(), VmError> {
        // Make sure that all required SVM features are supported.
        {
            if !features.nested_paging {
                return Err(VmError::NPNotSupported);
            }

            if !features.nrip_save {
                return Err(VmError::NRipSaveNotSupported);
            }
        }

        let n_cr3 = self.npt.table().0;
        let asid  = self.asid.get();

        let control = &mut self.vmcb_mut().control;

        // Enable nested paging and set nested page table CR3.
        control.np_control = 1;
        control.n_cr3      = n_cr3;

        // Assign this VM unique ASID.
        control.guest_asid = asid;

        // Always intercept VMRUN. Without it we cannot run VMs.
        control.intercept_misc_2 = 1;

        // Always intercept shutdown events. Without it guest triplefault will kill host.
        control.intercept_misc_1 = 1 << 31;

        Ok(())
    }

    fn vmcb(&self) -> &Vmcb {
        &self.guest_vmcb
    }

    fn vmcb_mut(&mut self) -> &mut Vmcb {
        &mut self.guest_vmcb
    }

    #[allow(unused)]
    pub fn intercept(&mut self, intercepts: &[Intercept]) {
        for &intercept in intercepts {
            let intercept = intercept as usize;

            // Pick the misc field for this intercept.
            let control = &mut self.vmcb_mut().control;
            let field   = match intercept & !0xff {
                MISC_1 => &mut control.intercept_misc_1,
                MISC_2 => &mut control.intercept_misc_2,
                MISC_3 => &mut control.intercept_misc_3,
                CR_RW  => &mut control.intercept_cr_rw,
                DR_RW  => &mut control.intercept_dr_rw,
                EXC    => &mut control.intercept_exceptions,
                _ => panic!("Invalid intercept encoding."),
            };

            // Set the intercept bit.
            *field |= (1 << (intercept & 0xff));
        }
    }

    #[allow(unused)]
    pub fn npt(&self) -> &Npt {
        &self.npt
    }

    #[allow(unused)]
    pub fn npt_mut(&mut self) -> &mut Npt {
        &mut self.npt
    }

    pub unsafe fn run(&mut self) -> VmExit {
        // Copy relevant registers from the cache to the VMCB.
        self.vmcb_mut().state.rax    = self.reg(Register::Rax);
        self.vmcb_mut().state.rsp    = self.reg(Register::Rsp);
        self.vmcb_mut().state.rip    = self.reg(Register::Rip);
        self.vmcb_mut().state.rflags = self.reg(Register::Rflags);

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
            inout("r12") self.guest_xsave.pointer()        => _,
            inout("r13") self.host_xsave.pointer()         => _,
            inout("r14") self.guest_registers.as_mut_ptr() => _,
        );

        // Copy relevant registers from the VMCB to the cache.
        self.set_reg(Register::Rax,    self.vmcb().state.rax);
        self.set_reg(Register::Rsp,    self.vmcb().state.rsp);
        self.set_reg(Register::Rip,    self.vmcb().state.rip);
        self.set_reg(Register::Rflags, self.vmcb().state.rflags);

        let control     = &self.vmcb().control;
        let exit_code   = control.exitcode;
        let exit_info_1 = control.exit_info_1;
        let exit_info_2 = control.exit_info_2;

        match exit_code {
            0x78  => VmExit::Hlt,
            0x7f  => VmExit::Shutdown,
            0x81  => VmExit::Vmmcall,
            0x400 => {
                let address = GuestAddr(exit_info_2);
                let present  = exit_info_1 & (1 << 0) != 0;
                let write    = exit_info_1 & (1 << 1) != 0;
                let reserved = exit_info_1 & (1 << 3) != 0;
                let execute  = exit_info_1 & (1 << 4) != 0;

                assert!(!reserved, "NPT entry had reserved bits set.");

                VmExit::NestedPageFault {
                    address,
                    present,
                    write,
                    execute,
                }
            }
            _    => panic!("Unknown VM exit code 0x{:x}.", exit_code),
        }
    }

    #[allow(unused)]
    pub fn cpl(&self) -> u8 {
        self.vmcb().state.cpl
    }

    #[allow(unused)]
    pub fn next_rip(&self) -> u64 {
        self.vmcb().control.next_rip
    }
}
