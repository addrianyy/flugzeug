#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler, asm, const_in_array_repeat_expressions)]

extern crate alloc;

#[macro_use] mod core_locals;
#[macro_use] mod print;
mod mm;
mod apic;
mod acpi;
mod font;
mod time;
mod hpet;
mod panic;
mod processors;
mod interrupts;
mod framebuffer;
mod interrupts_misc;

use page_table::PhysAddr;

#[no_mangle]
extern "C" fn _start(boot_block: PhysAddr, boot_tsc: u64) -> ! {
    // Make sure that LLVM data layout isn't broken.
    assert!(core::mem::size_of::<u64>() == 8 && core::mem::align_of::<u64>() == 8,
            "U64 has invalid size/alignment.");

    unsafe {
        // Zero out the IDT so if there is any exception we will triple fault.
        cpu::zero_idt();

        core_locals::initialize(boot_block, boot_tsc);
        mm::initialize();

        if core!().id == 0 {
            // Initialize framebuffer early so we can show logs on the screen.
            framebuffer::initialize();
        }

        interrupts::initialize();
        apic::initialize();

        if core!().id == 0 {
            acpi::initialize();
            time::initialize();

            // Launch APs.
            processors::initialize();
        }

        color_println!(0x00ffff, "Core initialized in {:.2}ms. Core ID: {}. APIC ID {:?}. \
                       Using {:?}.", time::local_uptime() * 1000.0, core!().id,
                       core!().apic_id(), core!().apic_mode());

        // Notify that this core is online and wait for other cores.
        processors::notify_core_online();

        if core!().id == 0 {
            // All cores are now launched and we have finished boot process.
            // Allow memory manager to clean up some things.
            mm::on_finished_boot_process();
        }
    }

    if core!().id == 0 {
        color_println!(0xff00ff, "Flugzeug OS loaded! Wilkommen! Firmware took {:.2}s, \
                       OS took {:.2}s.", time::uptime_with_firmware() - time::global_uptime(),
                       time::global_uptime());

        println!("{:x}", unsafe { cpu::rdmsr(0x277) });

        let mut tsc = 0;

        for i in 0.. {
            let c = time::get_tsc();
            color_println!(0xff00ff, "Running {} ({}K)", i, tsc / 1000);
            let d = time::get_tsc() - c;

            tsc = d;
        }
    }

    cpu::halt();
}
