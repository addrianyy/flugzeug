pub mod npt;
mod vmcb;
mod utils;

use core::fmt;

use crate::mm::PhysicalPage;

use utils::{XsaveArea, SvmFeatures, Asid};
use vmcb::Vmcb;
use npt::{Npt, GuestAddr, VirtAddr};

#[derive(Copy, Clone, PartialEq, Eq)]
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

impl fmt::Debug for VmError {
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
    // Skipped IOIO_PROT and MSR_PROT - exposed by the different API.
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
pub enum Interrupt {
    Intr,
    Nmi,
    Smi,
    Init,
    Vintr,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Exception {
    De,
    Db,
    Bp,
    Of,
    Br,
    Ud,
    Nm,
    Df,
    Ts,
    Np(u32),
    Ss(u32),
    Gp(u32),
    Pf {
        address:    VirtAddr,
        error_code: u32,
    },
    Mf,
    Ac(u32),
    Mc,
    Xf,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VmExit {
    NestedPageFault {
        address:  GuestAddr,
        present:  bool,
        write:    bool,
        execute:  bool,
    },
    CrAccess {
        n:     u8,
        write: bool,
    },
    DrAccess {
        n:     u8,
        write: bool,
    },
    Interrupt(Interrupt),
    Exception(Exception),
    IdtrAccess { write: bool },
    GdtrAccess { write: bool },
    LdtrAccess { write: bool },
    TrAccess   { write: bool },
    Msr        { write: bool },
    Rdtsc,
    Rdpmc,
    Pushf,
    Popf,
    Cpuid,
    Rsm,
    Iret,
    Int,
    Invd,
    Pause,
    Hlt,
    Invlpg,
    Invlpga,
    Io,
    Shutdown,
    Vmrun,
    Vmmcall,
    Vmload,
    Vmsave,
    Stgi,
    Clgi,
    Skinit,
    Rdtscp,
    Icebp,
    Wbindvd,
    Monitor,
    Mwait,
    Rdpru,
    Xsetbv,
    Invlpgp,
    IllegalInvlpgp,
    Invpcid,
    Mcommit,
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

        // Always intercept VMRUN and shutdown events. Other intercepts will be configured by
        // the user.
        control.intercept_misc_2 = 1;
        control.intercept_misc_1 = 1 << 31;

        // Pause filters aren't used.
        control.pause_filter_threshold = 0;
        control.pause_filter_count     = 0;

        // IOPM_BASE_PA and MSRPM_BASE_PA aren't used now.
        // TODO: Implement it.
        control.iopm_base_pa  = 0;
        control.msrpm_base_pa = 0;

        // TSC offset starts at 0. Can be adjusted by the user.
        control.tsc_offset = 0;

        // Assign this VM unique ASID. Don't flush anything on VMRUN.
        control.guest_asid  = asid;
        control.tlb_control = 0;

        // We don't support virtual interrupts for now.
        control.vintr = 0;

        // We don't support interrupt shadow for now.
        control.interrupt_shadow = 0;

        // Enable nested paging and set nested page table CR3. Don't setup any encryption.
        control.np_control = 1;
        control.n_cr3      = n_cr3;

        // AVIC, GHCB, VMSA, LBR virtualization, VMCB clean bits aren't used.

        Ok(())
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
                _      => panic!("Invalid intercept encoding."),
            };

            // Set the intercept bit.
            *field |= (1 << (intercept & 0xff));
        }
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
            0x00..=0x0f => VmExit::CrAccess { n: (exit_code - 0x00) as u8, write: false },
            0x10..=0x1f => VmExit::CrAccess { n: (exit_code - 0x10) as u8, write: true  },
            0x20..=0x2f => VmExit::DrAccess { n: (exit_code - 0x20) as u8, write: false },
            0x30..=0x3f => VmExit::DrAccess { n: (exit_code - 0x30) as u8, write: true  },
            0x40..=0x5f => {
                let vector     = exit_code - 0x40;
                let error_code = exit_info_1 as u32;
                let exception  = match vector {
                    0  => Exception::De,
                    1  => Exception::Db,
                    3  => Exception::Bp,
                    4  => Exception::Of,
                    5  => Exception::Br,
                    6  => Exception::Ud,
                    7  => Exception::Nm,
                    8  => Exception::Df,
                    10 => Exception::Ts,
                    11 => Exception::Np(error_code),
                    12 => Exception::Ss(error_code),
                    13 => Exception::Gp(error_code),
                    14 => Exception::Pf {
                        address: VirtAddr(exit_info_2),
                        error_code,
                    },
                    16 => Exception::Mf,
                    17 => Exception::Ac(error_code),
                    18 => Exception::Mc,
                    19 => Exception::Xf,
                    _  => panic!("Invalid exception vector {}.", vector),
                };

                VmExit::Exception(exception)
            }
            0x60        => VmExit::Interrupt(Interrupt::Intr),
            0x61        => VmExit::Interrupt(Interrupt::Nmi),
            0x62        => VmExit::Interrupt(Interrupt::Smi),
            0x63        => VmExit::Interrupt(Interrupt::Init),
            0x64        => VmExit::Interrupt(Interrupt::Vintr),
            0x65        => unreachable!("cr0 selective write"),
            0x66        => VmExit::IdtrAccess { write: false },
            0x67        => VmExit::GdtrAccess { write: false },
            0x68        => VmExit::LdtrAccess { write: false },
            0x69        => VmExit::TrAccess   { write: false },
            0x6a        => VmExit::IdtrAccess { write: true },
            0x6b        => VmExit::GdtrAccess { write: true },
            0x6c        => VmExit::LdtrAccess { write: true },
            0x6d        => VmExit::TrAccess   { write: true },
            0x6e        => VmExit::Rdtsc,
            0x6f        => VmExit::Rdpmc,
            0x70        => VmExit::Pushf,
            0x71        => VmExit::Popf,
            0x72        => VmExit::Cpuid,
            0x73        => VmExit::Rsm,
            0x74        => VmExit::Iret,
            0x75        => VmExit::Int,
            0x76        => VmExit::Invd,
            0x77        => VmExit::Pause,
            0x78        => VmExit::Hlt,
            0x79        => VmExit::Invlpg,
            0x7a        => VmExit::Invlpga,
            0x7b        => VmExit::Io,
            0x7c        => VmExit::Msr { write: exit_info_1 == 1 },
            0x7d        => unreachable!("task switch"),
            0x7e        => unreachable!("FERR freeze"),
            0x7f        => VmExit::Shutdown,
            0x80        => VmExit::Vmrun,
            0x81        => VmExit::Vmmcall,
            0x82        => VmExit::Vmload,
            0x83        => VmExit::Vmsave,
            0x84        => VmExit::Stgi,
            0x85        => VmExit::Clgi,
            0x86        => VmExit::Skinit,
            0x87        => VmExit::Rdtscp,
            0x88        => VmExit::Icebp,
            0x89        => VmExit::Wbindvd,
            0x8a        => VmExit::Monitor,
            0x8b        => VmExit::Mwait,
            0x8c        => unreachable!("conditional MWAIT"),
            0x8e        => VmExit::Rdpru,
            0x8d        => VmExit::Xsetbv,
            0x8f        => unreachable!("EFER trap"),
            0x90..=0x9f => unreachable!("CR trap"),
            0xa0        => VmExit::Invlpgp,
            0xa1        => VmExit::IllegalInvlpgp,
            0xa2        => VmExit::Invpcid,
            0xa3        => VmExit::Mcommit,
            0xa4        => unreachable!("TLB sync"),
            0x400       => {
                let address  = GuestAddr(exit_info_2);
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
            0x401       => unreachable!("AVIC incomplete IPI"),
            0x402       => unreachable!("AVIC no acceleration"),
            0x403       => unreachable!("VMGEXIT"),
            0xffff_fffe => unreachable!("busy bit in VMSA"),
            0xffff_ffff => panic!("Invalid guest state in VMCB."),
            _           => panic!("Unknown VM exit code 0x{:x}.", exit_code),
        }
    }

    pub fn reg(&self, register: Register) -> u64 {
        let index = register as usize;

        if index < self.guest_registers.len() {
            self.guest_registers[index]
        } else {
            use Register::*;

            macro_rules! create_match {
                ($($register: pat, $field: ident),*) => {
                    match register {
                        $(
                            $register => self.vmcb().state.$field,
                        )*
                        _ => unreachable!(),
                    }
                }
            }

            create_match!(
                Efer,         efer,
                Cr0,          cr0,
                Cr2,          cr2,
                Cr3,          cr3,
                Cr4,          cr4,
                Dr6,          dr6,
                Dr7,          dr7,
                Star,         star,
                Lstar,        lstar,
                Cstar,        cstar,
                Sfmask,       sfmask,
                KernelGsBase, kernel_gs_base,
                SysenterCs,   sysenter_cs,
                SysenterEsp,  sysenter_esp,
                SysenterEip,  sysenter_eip,
                Pat,          g_pat
            )
        }
    }

    pub fn set_reg(&mut self, register: Register, value: u64) {
        let index = register as usize;

        if index < self.guest_registers.len() {
            self.guest_registers[index] = value;
        } else {
            use Register::*;

            macro_rules! create_match {
                ($($register: pat, $field: ident),*) => {
                    match register {
                        $(
                            $register => self.vmcb_mut().state.$field = value,
                        )*
                        _ => unreachable!(),
                    }
                }
            }

            create_match!(
                Efer,         efer,
                Cr0,          cr0,
                Cr2,          cr2,
                Cr3,          cr3,
                Cr4,          cr4,
                Dr6,          dr6,
                Dr7,          dr7,
                Star,         star,
                Lstar,        lstar,
                Cstar,        cstar,
                Sfmask,       sfmask,
                KernelGsBase, kernel_gs_base,
                SysenterCs,   sysenter_cs,
                SysenterEsp,  sysenter_esp,
                SysenterEip,  sysenter_eip,
                Pat,          g_pat
            );
        }
    }

    #[allow(unused)]
    pub fn segment_reg(&self, register: SegmentRegister) -> Segment {
        use SegmentRegister::*;

        let state   = &self.vmcb().state;
        let segment = match register {
            Es  => &state.es,
            Cs  => &state.cs,
            Ss  => &state.ss,
            Ds  => &state.ds,
            Fs  => &state.fs,
            Gs  => &state.gs,
            Ldt => &state.ldtr,
            Tr  => &state.tr,
        };

        Segment {
            base:     segment.base,
            limit:    segment.limit,
            attrib:   segment.attrib,
            selector: segment.selector,
        }
    }

    pub fn set_segment_reg(&mut self, register: SegmentRegister, segment: Segment) {
        use SegmentRegister::*;

        if register == SegmentRegister::Cs {
            // Update the CPL when changing CS.
            let rpl = ((segment.selector >> 0) & 3) as u8;
            let dpl = ((segment.attrib   >> 5) & 3) as u8;

            self.vmcb_mut().state.cpl = u8::max(rpl, dpl);
        }

        let state = &mut self.vmcb_mut().state;
        let state = match register {
            Es  => &mut state.es,
            Cs  => &mut state.cs,
            Ss  => &mut state.ss,
            Ds  => &mut state.ds,
            Fs  => &mut state.fs,
            Gs  => &mut state.gs,
            Ldt => &mut state.ldtr,
            Tr  => &mut state.tr,
        };

        state.base     = segment.base;
        state.limit    = segment.limit;
        state.attrib   = segment.attrib;
        state.selector = segment.selector;
    }

    #[allow(unused)]
    pub fn table_reg(&mut self, register: TableRegister) -> DescriptorTable {
        let state = &self.vmcb().state;
        let table = match register {
            TableRegister::Idt => &state.idtr,
            TableRegister::Gdt => &state.gdtr,
        };

        DescriptorTable {
            base:  table.base,
            limit: table.limit as u16,
        }
    }

    pub fn set_table_reg(&mut self, register: TableRegister, table: DescriptorTable) {
        let state = &mut self.vmcb_mut().state;
        let state = match register {
            TableRegister::Idt => &mut state.idtr,
            TableRegister::Gdt => &mut state.gdtr,
        };

        state.base  = table.base;
        state.limit = table.limit as u32;
    }

    fn vmcb(&self) -> &Vmcb {
        &self.guest_vmcb
    }

    fn vmcb_mut(&mut self) -> &mut Vmcb {
        &mut self.guest_vmcb
    }

    #[allow(unused)]
    pub fn npt(&self) -> &Npt {
        &self.npt
    }

    #[allow(unused)]
    pub fn npt_mut(&mut self) -> &mut Npt {
        &mut self.npt
    }

    #[allow(unused)]
    pub fn cpl(&self) -> u8 {
        self.vmcb().state.cpl
    }

    #[allow(unused)]
    pub fn next_rip(&self) -> u64 {
        self.vmcb().control.next_rip
    }

    #[allow(unused)]
    pub fn tsc_offset(&self) -> u64 {
        self.vmcb().control.tsc_offset
    }

    #[allow(unused)]
    pub fn set_tsc_offset(&mut self, offset: u64) {
        self.vmcb_mut().control.tsc_offset = offset;
    }
}
