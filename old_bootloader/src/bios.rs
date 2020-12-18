#[repr(C)]
#[derive(Clone, Default)]
pub struct RegisterState {
    pub eax:    u32,
    pub ecx:    u32,
    pub edx:    u32,
    pub ebx:    u32,
    pub esp:    u32,
    pub ebp:    u32,
    pub esi:    u32,
    pub edi:    u32,
    pub eflags: u32,
    pub es:     u16,
    pub ds:     u16,
    pub fs:     u16,
    pub gs:     u16,
    pub ss:     u16,
}

/// Execute a BIOS interrupt with given register state. WARNING: Segment registers are only
/// output registers, they will not be loaded. All data accessed by BIOS must be <= 0x10000.
pub unsafe fn interrupt(int: u8, regs: &mut RegisterState) {
    let regs = regs as *mut RegisterState as usize;

    // Make sure that we can access register state in real mode without bothering about
    // segmentation.
    assert!(regs.checked_add(core::mem::size_of::<RegisterState>()).unwrap() <= 0x10000,
            "Register state must be on stack.");
    
    extern "C" {
        fn bios_interrupt(int: u8, regs: *mut RegisterState);
    }

    bios_interrupt(int, regs as *mut RegisterState);
}
