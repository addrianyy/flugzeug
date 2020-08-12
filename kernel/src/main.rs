#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler, asm)]

mod mm;
mod panic;
#[macro_use] mod serial;
#[macro_use] mod core_locals;

use page_table::PhysAddr;

#[no_mangle]
extern "C" fn _start(boot_block: PhysAddr) -> ! {
    // Make sure that LLVM data layout isn't broken.
    assert!(core::mem::size_of::<u64>() == 8 && core::mem::align_of::<u64>() == 8,
            "U64 has invalid size/alignment.");

    unsafe {
        core_locals::initialize(boot_block);
    }

    println!("Hello from kernel!");

    cpu::halt();
}
