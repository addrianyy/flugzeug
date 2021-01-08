#![allow(dead_code)]

pub mod npt;
mod vmcb;
mod utils;

use core::fmt;

use crate::mm::{PhysicalPage, ContiguousRegion};
use crate::once::Once;

use utils::{XsaveArea, SvmFeatures, Asid};
use vmcb::Vmcb;
use npt::{Npt, GuestAddr, VirtAddr};

pub trait ToInteger<T> {
    fn to_integer(&self) -> T;
}

macro_rules! impl_to_integer {
    ($type: ty) => {
        impl ToInteger<$type> for $type {
            fn to_integer(&self) -> $type { *self }
        }

        impl ToInteger<$type> for &$type {
            fn to_integer(&self) -> $type { **self }
        }

        impl ToInteger<$type> for &mut $type {
            fn to_integer(&self) -> $type { **self }
        }
    }
}

impl_to_integer!(u32);

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
    // Skipped task switches - always off.
    FerrFreeze = MISC_1 | 30,
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
    Df(u32),
    Ts(u32),
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
pub enum AddressSize {
    Bits16,
    Bits32,
    Bits64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OperandSize {
    Bits8,
    Bits16,
    Bits32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IoString {
    pub address_size: AddressSize,
    pub rep:          bool,
    pub segment:      SegmentRegister,
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
    Msr        { write: bool, msr:  u32 },
    Io         { write: bool, port: u16, operand_size: OperandSize, string: Option<IoString> },
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
    FerrFreeze,
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Intr(u8),
    Nmi,
    Exception(Exception),
    SoftwareInterrupt(u8),
}

const CLEAN_INTERCEPTS_AND_TSC: u32 = 0;
const CLEAN_NP:  u32                = 4;
const CLEAN_CR:  u32                = 5;
const CLEAN_DR:  u32                = 6;
const CLEAN_DT:  u32                = 7;
const CLEAN_SEG: u32                = 8;
const CLEAN_CR2: u32                = 9;

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

    /// Set to true if this CPU supports clean VMCB extension.
    support_vmcb_clean: bool,

    /// Last core this VM was run at. !0 if this VM wasn't run.
    last_core: u64,

    /// Value to set `tlb_control` to to flush guest TLB.
    flush_tlb_value: u32,

    /// Permission bitmap for MSRs. Determines if MSR read/write is intercepted.
    msrpm: ContiguousRegion,

    /// Permission bitmap for IO ports. Determines if port input/output is intercepted.
    iopm: ContiguousRegion,
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
            support_vmcb_clean: false,
            last_core:          !0,
            flush_tlb_value:    0,

            // Create zeroed permission bitmaps with correct sizes.
            msrpm: ContiguousRegion::new(8192),
            iopm:  ContiguousRegion::new(12288),
        };

        vm.initialize(&features)?;

        Ok(vm)
    }

    fn initialize(&mut self, features: &SvmFeatures) -> Result<(), VmError> {
        static WARNING_FLUSH_BY_ASID: Once = Once::new();
        static WARNING_VMCB_CLEAN:    Once = Once::new();

        // Make sure that all required SVM features are supported.
        {
            if !features.nested_paging {
                return Err(VmError::NPNotSupported);
            }

            if !features.nrip_save {
                return Err(VmError::NRipSaveNotSupported);
            }
        }

        self.flush_tlb_value = match features.flush_by_asid {
            true => {
                // Flush this guest's TLB entries.
                3
            }
            false => {
                WARNING_FLUSH_BY_ASID.exec(|| {
                    color_println!(0xffff00, "WARNING: SVM flush by ASID is not supported.");
                });

                // Flush entire TLB (Should be used only on legacy hardware).
                1
            }
        };

        // Check if this CPU supports VMCB clean.
        self.support_vmcb_clean = features.vmcb_clean;

        if !features.vmcb_clean {
            WARNING_VMCB_CLEAN.exec(|| {
                color_println!(0xffff00, "WARNING: SVM VMCB clean is not supported.");
            });
        }

        let n_cr3 = self.npt.table().0;
        let asid  = self.asid.get();

        let iopm_pa  = self.iopm.phys_addr().0;
        let msrpm_pa = self.msrpm.phys_addr().0;

        let control = &mut self.vmcb_mut().control;

        // Always intercept VMRUN and shutdown events. Use IOPM and MSRPM.
        // Other intercepts will be configured by the user.
        control.intercept_misc_2 = 1;
        control.intercept_misc_1 = (1 << 27) | (1 << 28) | (1 << 31);

        // Pause filters aren't used.
        control.pause_filter_threshold = 0;
        control.pause_filter_count     = 0;

        // Assign physical addresses of permission bitmaps.
        control.iopm_base_pa  = iopm_pa;
        control.msrpm_base_pa = msrpm_pa;

        // TSC offset starts at 0. Can be adjusted by the user.
        control.tsc_offset = 0;

        // Assign this VM unique ASID. Don't flush anything on VMRUN.
        control.guest_asid  = asid;
        control.tlb_control = 0;

        // Virtualize masking of INTR so physical interrupts can be delivered when VM is running.
        control.vintr = 1 << 24;

        // Enable nested paging and set nested page table CR3. Don't setup any encryption.
        control.feature_control = 1;
        control.n_cr3           = n_cr3;

        // AVIC, GHCB, VMSA, LBR virtualization, VMCB clean bits aren't used.

        Ok(())
    }

    fn intercepted_delivery(&self) -> Option<Event> {
        let info = self.vmcb().control.exit_int_info;
        if  info & (1 << 31) == 0 {
            return None;
        }

        let typ    = ((info >> 8) & 0b111) as u8;
        let vector = ((info >> 0) &  0xff) as u8;

        let error_code = if info & (1 << 11) != 0 {
            Some((info >> 32) as u32)
        } else {
            None
        };

        macro_rules! get_error_code {
            () => { error_code.expect("Excpected error code but there isn't any") }
        }

        let event = match typ {
            0 => Event::Intr(vector),
            2 => Event::Nmi,
            3 => {
                let exception = match vector {
                    0  => Exception::De,
                    1  => Exception::Db,
                    3  => Exception::Bp,
                    4  => Exception::Of,
                    5  => Exception::Br,
                    6  => Exception::Ud,
                    7  => Exception::Nm,
                    8  => Exception::Df(get_error_code!()),
                    10 => Exception::Ts(get_error_code!()),
                    11 => Exception::Np(get_error_code!()),
                    12 => Exception::Ss(get_error_code!()),
                    13 => Exception::Gp(get_error_code!()),
                    14 => Exception::Pf {
                        address:    VirtAddr(self.reg(Register::Cr2)),
                        error_code: get_error_code!(),
                    },
                    16 => Exception::Mf,
                    17 => Exception::Ac(get_error_code!()),
                    18 => Exception::Mc,
                    19 => Exception::Xf,
                    _  => panic!("Invalid exception vector {}.", vector),
                };

                Event::Exception(exception)
            }
            4 => Event::SoftwareInterrupt(vector),
            _ => panic!("Invalid event type {}.", typ),
        };

        if let Event::Exception(_) = event {
        } else {
            // Non-exception interrupts cannot have error codes.
            assert!(error_code.is_none(), "Error code valid for non-exception event.");
        }

        Some(event)
    }

    fn vmcb_dirty(&mut self, bit: u32) {
        if self.support_vmcb_clean {
            // Unset the clean bit to mark that this part of VMCB has been modified.
            self.vmcb_mut().control.vmcb_clean &= !(1 << bit);
        }
    }

    pub fn intercept(&mut self, intercepts: &[Intercept], enable: bool) {
        self.vmcb_dirty(CLEAN_INTERCEPTS_AND_TSC);

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

            let mask = 1 << (intercept & 0xff);

            if enable {
                *field |= mask;
            } else {
                *field &= !mask;
            }
        }
    }

    pub fn intercept_msr(&mut self, msr: u32, read: bool, write: bool) {
        let position = match msr {
            0x0000_0000..=0x0000_1fff => 8 * 0x0000 + (msr - 0x0000_0000) * 2,
            0xc000_0000..=0xc000_1fff => 8 * 0x0800 + (msr - 0xc000_0000) * 2,
            0xc001_0000..=0xc001_1fff => 8 * 0x1000 + (msr - 0xc001_0000) * 2,
            _ => {
                panic!("Specified MSR ({:x}) is outside of supported range and is \
                       intercepted by default.", msr);
            }
        };

        let index = (position / 8) as usize;
        let bit   =  position % 8;

        // First clear intercepts for this MSR.
        self.msrpm[index] &= !(0b11 << bit);

        // Set new intercepts for this MSR.
        self.msrpm[index] |= (read as u8) << (bit + 0) | (write as u8) << (bit + 1);
    }

    pub fn intercept_msrs<M: ToInteger<u32>, I: IntoIterator<Item = M>>(
        &mut self,
        msrs:  I,
        read:  bool,
        write: bool,
    ) {
        for msr in msrs.into_iter() {
            self.intercept_msr(msr.to_integer(), read, write);
        }
    }

    pub fn intercept_all_msrs(&mut self, read: bool, write: bool) {
        let mut value = 0;

        for bit in (0..8).step_by(2) {
            if read {
                value |= 1 << (bit + 0);
            }

            if write {
                value |= 1 << (bit + 1);
            }
        }

        self.msrpm[..0x1800].iter_mut().for_each(|b| *b = value);
    }

    pub fn intercept_port(&mut self, port: u16, enable: bool) {
        let position = port;
        let index    = (position / 8) as usize;
        let bit      =  position % 8;

        if enable {
            self.iopm[index] |= 1 << bit;
        } else {
            self.iopm[index] &= !(1 << bit);
        }
    }

    pub fn intercept_ports<P: ToInteger<u16>, I: IntoIterator<Item = P>>(
        &mut self,
        ports:  I,
        enable: bool,
    ) {
        for port in ports.into_iter() {
            self.intercept_port(port.to_integer(), enable);
        }
    }

    pub fn intercept_all_ports(&mut self, enable: bool) {
        let value = if enable { 0xff } else { 0x00 };

        self.iopm[..0x2000].iter_mut().for_each(|b| *b = value);
    }

    pub unsafe fn run(&mut self) -> (VmExit, Option<Event>) {
        assert!(core!().interrupts_enabled(), "Cannot run VM with interrupts disabled.");

        // Handle case where this VM is ran on different CPU than before (or is ran for the first
        // time).
        if core!().id != self.last_core {
            // If we are running on different core than it may have SVM disabled. Enable it.
            utils::enable_svm()
                .expect("VM was moved to the CPU which doesn't support SVM.");

            if self.support_vmcb_clean {
                // We are required to zero out `vmcb_clean` if we are running for the first
                // time or on different core.
                self.vmcb_mut().control.vmcb_clean = 0;
            }

            self.last_core = core!().id;
        }

        // Copy relevant registers from the cache to the VMCB.
        self.vmcb_mut().state.rax    = self.reg(Register::Rax);
        self.vmcb_mut().state.rsp    = self.reg(Register::Rsp);
        self.vmcb_mut().state.rip    = self.reg(Register::Rip);
        self.vmcb_mut().state.rflags = self.reg(Register::Rflags);

        asm!(
            r#"
                // Disable all interrupts on the system before context switching.
                // If interrupt happens during the switch, handler may overwrite some
                // guest state.
                clgi

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

                // Reenable all interrupts on the system. If we exited for example due to NMI,
                // this will cause us to deliver that NMI to the kernel as it is pending now.
                stgi
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

        // TLB was flushed, clear the flag to avoid flush next time this VM is ran.
        self.vmcb_mut().control.tlb_control = 0;

        if self.support_vmcb_clean {
            // So far nothing has been modified in the VMCB. Even though some bits are reserved
            // AMD allows setting this to `0xffff_ffff`.
            self.vmcb_mut().control.vmcb_clean = 0xffff_ffff;
        }

        let control     = &self.vmcb().control;
        let exit_code   = control.exitcode;
        let exit_info_1 = control.exit_info_1;
        let exit_info_2 = control.exit_info_2;

        const VMSA_BUSY:     u64 = !1;
        const INVALID_STATE: u64 = !0;

        let vmexit = match exit_code {
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
                    8  => Exception::Df(0),
                    10 => Exception::Ts(error_code & 0xffff),
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
            0x7b        => {
                let operand_size = if exit_info_1 & (1 << 4) != 0 {
                    OperandSize::Bits8
                } else if exit_info_1 & (1 << 5) != 0 {
                    OperandSize::Bits16
                } else if exit_info_1 & (1 << 6) != 0 {
                    OperandSize::Bits32
                } else {
                    panic!("Invalid IO port access operand size.");
                };

                let string = if exit_info_1 & (1 << 2) != 0 {
                    let address_size = if exit_info_1 & (1 << 7) != 0 {
                        AddressSize::Bits16
                    } else if exit_info_1 & (1 << 8) != 0 {
                        AddressSize::Bits32
                    } else if exit_info_1 & (1 << 9) != 0 {
                        AddressSize::Bits64
                    } else {
                        panic!("Invalid IO port access address size.");
                    };

                    let rep     = exit_info_1 & (1 << 3) != 0;
                    let segment = match (exit_info_1 >> 10) & 0b111 {
                        0 => SegmentRegister::Es,
                        1 => SegmentRegister::Cs,
                        2 => SegmentRegister::Ss,
                        3 => SegmentRegister::Ds,
                        4 => SegmentRegister::Fs,
                        5 => SegmentRegister::Gs,
                        _ => panic!("Invalid effective segment."),
                    };

                    Some(IoString {
                        rep,
                        address_size,
                        segment,
                    })
                } else {
                    None
                };

                VmExit::Io {
                    write: exit_info_1 & 1 == 0,
                    port:  (exit_info_1 >> 16) as u16,
                    operand_size,
                    string,
                }
            }
            0x7c        => VmExit::Msr {
                msr:   self.reg(Register::Rcx) as u32,
                write: exit_info_1 == 1,
            },
            0x7d        => unreachable!("task switch"),
            0x7e        => VmExit::FerrFreeze,
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
            0x401         => unreachable!("AVIC incomplete IPI"),
            0x402         => unreachable!("AVIC no acceleration"),
            0x403         => unreachable!("VMGEXIT"),
            VMSA_BUSY     => panic!("busy bit in VMSA"),
            INVALID_STATE => panic!("Invalid guest state in VMCB."),
            _             => panic!("Unknown VM exit code 0x{:x}.", exit_code),
        };

        // It is possible for an intercept to occur while the guest is attempting to
        // deliver an exception or interrupt through the IDT.
        let event = self.intercepted_delivery();

        (vmexit, event)
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
                ($($register: pat, $field: ident, $clean: expr, $flush_mask: expr),*) => {
                    match register {
                        $(
                            $register => {
                                let before                  = self.vmcb_mut().state.$field;
                                let flush_mask: Option<u64> = $flush_mask;

                                self.vmcb_mut().state.$field = value;

                                if let Some(clean) = $clean {
                                    self.vmcb_dirty(clean);
                                }

                                if let Some(flush_mask) = flush_mask {
                                    // If bits in `flush_mask` were changed we need TLB flush.
                                    let before = before & flush_mask;
                                    let after  = value  & flush_mask;

                                    if before != after {
                                        self.flush_tlb();
                                    }
                                }
                            }
                        )*
                        _ => unreachable!(),
                    }
                }
            }

            // Software Rule. When the VMM changes a guest's paging mode by changing entries in
            // the guest's VMCB, the VMM must ensure that the guest’s TLB entries are flushed from
            // the TLB. The relevant VMCB state includes:

            // PG, WP, CD, NW.
            let cr0_mask = (1 << 31) | (1 << 16) | (1 << 30) | (1 << 29);

            // All bits.
            let cr3_mask = 0xffff_ffff_ffff_ffff;

            // PGE, PAE, PSE.
            let cr4_mask = (1 << 7) | (1 << 5) | (1 << 4);

            // NXE, LMA, LME.
            let efer_mask = (1 << 11) | (1 << 10) | (1 << 8);

            create_match!(
                Efer,         efer,           Some(CLEAN_CR),  Some(efer_mask),
                Cr0,          cr0,            Some(CLEAN_CR),  Some(cr0_mask),
                Cr2,          cr2,            Some(CLEAN_CR2), None,
                Cr3,          cr3,            Some(CLEAN_CR),  Some(cr3_mask),
                Cr4,          cr4,            Some(CLEAN_CR),  Some(cr4_mask),
                Dr6,          dr6,            Some(CLEAN_DR),  None,
                Dr7,          dr7,            Some(CLEAN_DR),  None,
                Star,         star,           None,            None,
                Lstar,        lstar,          None,            None,
                Cstar,        cstar,          None,            None,
                Sfmask,       sfmask,         None,            None,
                KernelGsBase, kernel_gs_base, None,            None,
                SysenterCs,   sysenter_cs,    None,            None,
                SysenterEsp,  sysenter_esp,   None,            None,
                SysenterEip,  sysenter_eip,   None,            None,
                Pat,          g_pat,          Some(CLEAN_NP),  None
            );
        }
    }

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

        self.vmcb_dirty(CLEAN_SEG);

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
        self.vmcb_dirty(CLEAN_DT);

        let state = &mut self.vmcb_mut().state;
        let state = match register {
            TableRegister::Idt => &mut state.idtr,
            TableRegister::Gdt => &mut state.gdtr,
        };

        state.base  = table.base;
        state.limit = table.limit as u32;
    }

    pub fn flush_tlb(&mut self) {
        self.vmcb_mut().control.tlb_control = self.flush_tlb_value;
    }

    pub fn inject_event(&mut self, event: Event) {
        let (typ, error_code, vector): (u32, Option<u32>, u8) = match event {
            Event::Intr(vector)              => (0, None, vector),
            Event::Nmi                       => (2, None, 2),
            Event::SoftwareInterrupt(vector) => (4, None, vector),
            Event::Exception(exception)      => {
                let (vector, error_code) = match exception {
                    Exception::De     => (0,  None),
                    Exception::Db     => (1,  None),
                    Exception::Bp     => (3,  None),
                    Exception::Of     => (4,  None),
                    Exception::Br     => (5,  None),
                    Exception::Ud     => (6,  None),
                    Exception::Nm     => (7,  None),
                    Exception::Df(ec) => (8,  Some(ec)),
                    Exception::Ts(ec) => (10, Some(ec)),
                    Exception::Np(ec) => (11, Some(ec)),
                    Exception::Ss(ec) => (12, Some(ec)),
                    Exception::Gp(ec) => (13, Some(ec)),
                    Exception::Pf { address, error_code } => {
                        self.set_reg(Register::Cr2, address.0);

                        (14, Some(error_code))
                    }
                    Exception::Mf     => (16, None),
                    Exception::Ac(ec) => (17, Some(ec)),
                    Exception::Mc     => (18, None),
                    Exception::Xf     => (19, None),
                };

                (3, error_code, vector)
            }
        };

        let mut injection: u64 = 0;

        injection |= (vector as u64 &  0xff) << 0;
        injection |= (typ    as u64 & 0b111) << 8;
        injection |= 1                       << 31;

        if let Some(error_code) = error_code {
            injection |= (error_code as u64) << 32;
            injection |= 1 << 11;
        }

        self.vmcb_mut().control.event_injection = injection;
    }

    fn vmcb(&self) -> &Vmcb {
        &self.guest_vmcb
    }

    fn vmcb_mut(&mut self) -> &mut Vmcb {
        &mut self.guest_vmcb
    }

    pub fn npt(&self) -> &Npt {
        &self.npt
    }

    pub fn npt_mut(&mut self) -> &mut Npt {
        &mut self.npt
    }

    pub fn cpl(&self) -> u8 {
        self.vmcb().state.cpl
    }

    pub fn next_rip(&self) -> u64 {
        let next_rip = self.vmcb().control.next_rip;

        // The next sequential instruction pointer (nRIP) is saved in the guest VMCB control area
        // at location C8h on all #VMEXITs that are due to instruction intercepts, as defined
        // in section 15.9, as well as MSR and IOIO intercepts and exceptions caused by the
        // INT3, INTO, and BOUND instructions. For all other intercepts, nRIP is reset to zero.
        assert!(next_rip != 0, "Tried to use next RIP in invalid context.");

        next_rip
    }

    pub fn tsc_offset(&self) -> u64 {
        self.vmcb().control.tsc_offset
    }

    pub fn set_tsc_offset(&mut self, offset: u64) {
        self.vmcb_dirty(CLEAN_INTERCEPTS_AND_TSC);
        self.vmcb_mut().control.tsc_offset = offset;
    }

    pub fn in_interrupt_shadow(&self) -> bool {
        (self.vmcb().control.interruptibility & 1) == 1
    }
}
