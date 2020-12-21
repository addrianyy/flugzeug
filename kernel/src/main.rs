#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler, global_asm, asm,
           const_in_array_repeat_expressions)]

extern crate alloc;

#[macro_use] mod core_locals;
#[macro_use] mod print;
mod mm;
mod apic;
mod acpi;
mod font;
mod panic;
mod interrupts;
mod framebuffer;
mod interrupts_misc;

use page_table::PhysAddr;

fn rdtsc() -> u64 {
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
}

#[no_mangle]
extern "C" fn _start(boot_block: PhysAddr) -> ! {
    // Make sure that LLVM data layout isn't broken.
    assert!(core::mem::size_of::<u64>() == 8 && core::mem::align_of::<u64>() == 8,
            "U64 has invalid size/alignment.");

    unsafe {
        // Initialize crucial kernel per-core components.
        core_locals::initialize(boot_block);

        let main_core = core!().id == 0;

        // Initialize framebuffer early so we can show logs on the screen.
        if main_core {
            framebuffer::initialize();
        }

        interrupts::initialize();
        apic::initialize();

        if main_core {
            // Launch APs.
            acpi::initialize();
        }

        // Notify that this core is online and wait for other cores.
        acpi::notify_core_online();

        if main_core {
            // All cores are now launched and we have finished boot process.
            // Allow memory manager to clean up some things.
            mm::on_finished_boot_process();
        }
    }

    let cs: u16;
    let ds: u16;

    unsafe {
        asm!("mov ax, cs", out("ax") cs);
        asm!("mov ax, ds", out("ax") ds);
    }

    color_println!(0x00ffff, "Core is now initialized. Core ID: {}. APIC ID {:?}. \
                   CS 0x{:x}, DS 0x{:x}.", core!().id, core!().apic_id(), cs, ds);

    if core!().id == 0 {
        color_println!(0xff00ff, "Flugzeug OS loaded! Wilkommen!");

        // We start with legacy area size and XSAVE header size.
        let mut xsave_size = 512 + 64;

        // IA32_XSS is not used.
        let xcr0 = cpu::get_xcr0();

        for component in 2..64 {
            if xcr0 & (1 << component) != 0 {
                let cpuid  = cpu::cpuid(0x0d, component);
                let offset = cpuid.ebx;
                let size   = cpuid.eax;

                xsave_size = xsave_size.max(offset + size);
            }
        }

        let mut data = [3u8; 32];
        unsafe {
            asm!("vmovdqu ymm1, [rax]", in("rax") data.as_ptr());
        }

        unsafe {
            asm!("int3");
        }

        let mut data = [0u8; 32];
        unsafe {
            asm!("vmovdqu [rax], ymm1", in("rax") data.as_mut_ptr());
        }

        println!("{:?}", data);


        /*
        let mut diff = 0;

        for i in 0.. {
            let tsc = rdtsc();

            color_println!(0xff00ff, "running {} ({}K)......", i, diff / 1000);

            diff = rdtsc() - tsc;
        }
        */
    }

    cpu::halt();
}
