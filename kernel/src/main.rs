#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler, asm)]

mod mm;
mod panic;
#[macro_use] mod serial;
#[macro_use] mod core_locals;

use page_table::PhysAddr;

fn apic_delay() {
    // Just waste a few cycles to wait for APIC.
    for _ in 0..100000 {
        unsafe { asm!("nop"); }
    }
}

#[no_mangle]
extern "C" fn _start(boot_block: PhysAddr) -> ! {
    // Make sure that LLVM data layout isn't broken.
    assert!(core::mem::size_of::<u64>() == 8 && core::mem::align_of::<u64>() == 8,
            "U64 has invalid size/alignment.");

    unsafe {
        // 0x7c08 contains a byte that determines if a stack is available to the bootloader.
        // It is used to prevent two instances of bootloader running when launching APs.
        // Make sure that this address is in sync with the assembly bootloader.
        let stack_available = mm::translate(PhysAddr(0x7c08), 1).unwrap() as *mut u8;

        // Currently stack should be locked.
        assert!(*stack_available == 0,
                "We have just entered the kernel, but boot stack is not locked.");

        // As we are now in the kernel, mark the stack as available.
        core::ptr::write_volatile(stack_available, 1);

        core_locals::initialize(boot_block);
    }

    if core!().id == 0 {
        unsafe {
            // If we are BSP, launch other cores using APIC.

            let a = mm::translate(PhysAddr(0xfee0_0300), 4).unwrap() as *mut u32;
            let b = mm::translate(PhysAddr(0xfee0_00f0), 4).unwrap() as *mut u32;

            core::ptr::write_volatile(b, core::ptr::read_volatile(b) | 0x1000);

            // Send INIT-SIPI-SIPI sequence to all cores. AP entrypoint is hardcoded here to
            // 0x8000. Don't change it without changing the assembly bootloader.
            // Bootloader will perform normal initialization sequence on launched cores
            // and transfer execution to the kernel entrypoint.

            // Delays are required unfortunately.

            core::ptr::write_volatile(a, 0xc4500);
            apic_delay();

            core::ptr::write_volatile(a, 0xc4608);
            apic_delay();

            core::ptr::write_volatile(a, 0xc4608);
        }
    }

    println!("Hello from kernel! Core ID: {}.", core!().id);

    cpu::halt();
}
