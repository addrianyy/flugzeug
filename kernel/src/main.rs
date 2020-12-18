#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler, global_asm, asm,
           const_in_array_repeat_expressions)]

extern crate alloc;

#[macro_use] mod core_locals;
#[macro_use] mod serial;
mod mm;
mod apic;
mod acpi;
mod panic;
mod interrupts;
mod interrupts_misc;

use page_table::PhysAddr;

#[no_mangle]
extern "C" fn _start(boot_block: PhysAddr) -> ! {
    // Make sure that LLVM data layout isn't broken.
    assert!(core::mem::size_of::<u64>() == 8 && core::mem::align_of::<u64>() == 8,
            "U64 has invalid size/alignment.");

    unsafe {
        // Initialize crucial kernel per-core components.
        core_locals::initialize(boot_block);

        interrupts::initialize();
        apic::initialize();

        if core!().id == 0 {
            // Launch APs.
            acpi::initialize();
        }

        // Notify that this core is online and wait for other cores.
        acpi::notify_core_online();

        if core!().id == 0 {
            // All cores are now launched and we can make kernel physical region non-executable.
            // We do this only once because page tables are shared between cores.
            mm::enable_nx_on_physical_region();
        }
    }

    let cs: u16;
    let ds: u16;

    unsafe {
        asm!("mov ax, cs", out("ax") cs);
        asm!("mov ax, ds", out("ax") ds);
    }

    println!("Hello from flugzeug kernel! Core ID: {}. APIC ID {:?}. CS 0x{:x}, DS 0x{:x}",
             core!().id, core!().apic_id(), cs, ds);

    cpu::halt();
}
