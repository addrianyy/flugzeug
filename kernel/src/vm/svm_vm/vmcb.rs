use core::mem::MaybeUninit;

use crate::mm::PhysicalPage;

#[repr(C)]
pub struct VmcbSegmentDescriptor {
    pub selector: u16,
    pub attrib:   u16,
    pub limit:    u32,
    pub base:     u64,
}

#[repr(C)]
pub struct VmcbControlArea {
    pub intercept_cr_reads:  u16,
    pub intercept_cr_writes: u16,
    pub intercept_dr_reads:  u16,
    pub intercept_dr_writes: u16,

    pub intercept_exceptions: u32,

    pub intercept_misc_1:          u32,
    pub intercept_misc_2:          u32,
    pub intercept_misc_3:          u32,
    reserved_1:                    [u8; 0x3c - 0x18],
    pub pause_filter_threshold:    u16,
    pub pause_filter_count:        u16,
    pub iopm_base_pa:              u64,
    pub msrpm_base_pa:             u64,
    pub tsc_offset:                u64,
    pub guest_asid:                u32,
    pub tlb_control:               u32,
    pub vintr:                     u64,
    pub interrupt_shadow:          u64,
    pub exitcode:                  u64,
    pub exit_info_1:               u64,
    pub exit_info_2:               u64,
    pub exit_int_info:             u64,
    pub np_control:                u64,
    pub avic_apic_bar:             u64,
    pub ghcb_pa:                   u64,
    pub event_injection:           u64,
    pub n_cr3:                     u64,
    pub lbr_virtualization_enable: u64,
    pub vmcb_clean:                u64,
    pub next_rip:                  u64,
    pub bytes_fetched:             u8,
    pub guest_instruction_bytes:   [u8; 15],
    pub apic_backing_page:         u64,
    reserved_2:                    u64,
    pub avic_logical_table:        u64,
    pub avic_physical_table:       u64,
    reserved_3:                    u64,
    pub vmsa_pointer:              u64,
    reserved_4:                    [u8; 0x400 - 0x110],
}

#[repr(C)]
pub struct VmcbStateSaveArea {
    // Guest segment registers.
    pub es:   VmcbSegmentDescriptor,
    pub cs:   VmcbSegmentDescriptor,
    pub ss:   VmcbSegmentDescriptor,
    pub ds:   VmcbSegmentDescriptor,
    pub fs:   VmcbSegmentDescriptor,
    pub gs:   VmcbSegmentDescriptor,
    pub gdtr: VmcbSegmentDescriptor,
    pub ldtr: VmcbSegmentDescriptor,
    pub idtr: VmcbSegmentDescriptor,
    pub tr:   VmcbSegmentDescriptor,

    reserved_1: [u8; 0xcb - 0xa0],

    // Guest current privilege level.
    pub cpl: u8,

    reserved_2: u32,

    // Guest EFER MSR.
    pub efer: u64,

    reserved_3: [u8; 0x148 - 0xd8],

    // Guest control registers.
    pub cr4: u64,
    pub cr3: u64,
    pub cr0: u64,

    // Guest debug registers.
    pub dr7: u64,
    pub dr6: u64,

    // Guest flags and instruction pointer.
    pub rflags: u64,
    pub rip:    u64,

    reserved_4: [u8; 0x1d8 - 0x180],
 
    // Guest stack pointer.
    pub rsp: u64,

    // Shadow stack related registers.
    pub s_cet:     u64,
    pub ssp:       u64,
    pub isst_addr: u64,

    // Guest RAX GPR.
    pub rax: u64,

    // Various guest MSRs.
    pub star:           u64,
    pub lstar:          u64,
    pub cstar:          u64,
    pub sfmask:         u64,
    pub kernel_gs_base: u64,
    pub sysenter_cs:    u64,
    pub sysenter_esp:   u64,
    pub sysenter_eip:   u64,

    // Address that caused page fault.
    pub cr2: u64,

    reserved_5: [u8; 0x268 - 0x248],

    // Guest PAT - used only when nested paging is enabled.
    pub g_pat: u64,

    // LBR virtualization related registers - used only when LBR virtualization is enabled.
    pub dbgctl:           u64,
    pub br_from:          u64,
    pub br_to:            u64,
    pub last_except_from: u64,
    pub last_except_to:   u64,

    reserved_6: [u8; 0xc00 - 0x298],
}

#[repr(C)]
pub struct Vmcb {
    pub control: VmcbControlArea,
    pub state:   VmcbStateSaveArea,
}

impl Vmcb {
    pub fn new() -> PhysicalPage<Vmcb> {
        // Make sure that all VMCB components have expected sizes.
        assert_eq!(core::mem::size_of::<VmcbControlArea>(), 0x400,
                   "Invalid size of VMCB control area.");
        assert_eq!(core::mem::size_of::<VmcbStateSaveArea>(), 0xc00,
                   "Invalid size of VMCB state save area.");
        assert_eq!(core::mem::size_of::<Vmcb>(), 0x1000,
                   "Invalid size of VMCB.");

        // Create a zeroed VMCB.
        let vmcb = unsafe {
            MaybeUninit::zeroed().assume_init()
        };

        // Move VMCB to a physical page as required by the SVM.
        PhysicalPage::new(vmcb)
    }
}
