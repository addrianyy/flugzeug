#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler, asm, const_in_array_repeat_expressions)]

extern crate alloc;

#[macro_use] mod core_locals;
#[macro_use] mod serial;
mod mm;
mod panic;
mod apic;
mod acpi;

use page_table::PhysAddr;

#[no_mangle]
extern "C" fn _start(boot_block: PhysAddr) -> ! {
    // Make sure that LLVM data layout isn't broken.
    assert!(core::mem::size_of::<u64>() == 8 && core::mem::align_of::<u64>() == 8,
            "U64 has invalid size/alignment.");

    unsafe {
        // Initialize crucial kernel per-core components.
        core_locals::initialize(boot_block);
        apic::initialize();

        if core!().id == 0 {
            // Launch APs.
            acpi::initialize();
        }

        // Notify that this core is online and wait for other cores.
        acpi::notify_core_online();
    }

    println!("Hello from kernel! Core ID: {}. APIC ID {:?}.", core!().id, core!().apic_id());

    cpu::halt();
}
