#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler, asm)]

extern crate alloc;

#[macro_use] mod core_locals;
#[macro_use] mod serial;
mod mm;
mod panic;
mod apic;

use page_table::PhysAddr;

#[no_mangle]
extern "C" fn _start(boot_block: PhysAddr) -> ! {
    // Make sure that LLVM data layout isn't broken.
    assert!(core::mem::size_of::<u64>() == 8 && core::mem::align_of::<u64>() == 8,
            "U64 has invalid size/alignment.");

    unsafe {
        // 0x7c08 contains a byte that determines if a stack is available to the bootloader.
        // It is used to prevent two instances of bootloader running when launching APs.
        // Make sure that this address is in sync with the assembly bootloader.
        const STACK_AVAILABLE: PhysAddr = PhysAddr(0x7c08);

        // Currently stack should be locked.
        assert!(mm::read_phys::<u8>(STACK_AVAILABLE) == 0,
                "We have just entered the kernel, but boot stack is not locked.");

        // As we are now in the kernel, mark the stack as available.
        mm::write_phys(STACK_AVAILABLE, 1u8);

        core_locals::initialize(boot_block);
        apic::initialize();
    }

    if core!().id == 0 {
        unsafe {
            // Send INIT-SIPI-SIPI sequence to all cores. AP entrypoint is hardcoded here to
            // 0x8000. Don't change it without changing the assembly bootloader.
            // Bootloader will perform normal initialization sequence on launched cores
            // and transfer execution to the kernel entrypoint.

            let mut apic = core!().apic.lock();
            let apic     = apic.as_mut().unwrap();

            apic.write_icr(0xc4500);
            apic.write_icr(0xc4608);
            apic.write_icr(0xc4608);
        }
    }

    println!("Hello from kernel! Core ID: {}.", core!().id);

    cpu::halt();
}
