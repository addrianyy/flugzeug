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
mod panic;
mod interrupts;
mod framebuffer;
mod interrupts_misc;

use page_table::PhysAddr;

#[no_mangle]
extern "C" fn _start(boot_block: PhysAddr) -> ! {
    // Zero out the IDT so if there is any exception we will triple fault.
    unsafe {
        cpu::zero_idt();
    }

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

        if let Some(mut graphics) = framebuffer::request_graphics() {
            draw(&mut graphics);

            cpu::halt();
            framebuffer::return_graphics(graphics);
        }

        /*
        let mut diff = 0;

        for i in 0.. {
            let tsc = time::get_tsc();

            color_println!(0xff00ff, "running {} ({}K)", i, diff / 1000);

            diff = time::get_tsc() - tsc;
        }
        */
    }

    cpu::halt();
}

fn esacpe(pr: f32, pi: f32, max_iterations: usize) -> f32 {
    let mut zr = pr;
    let mut zi = pi;

    for iteration in 0..max_iterations {
        let r2 = zr * zr;
        let i2 = zi * zi;

        if r2 + i2 > 4.0 {
            return (iteration as f32) / (max_iterations - 1) as f32;
        }

        zi = 2.0 * zr * zi + pi;
        zr = r2 - i2 + pr;
    }

    0.0
}

fn draw(graphics: &mut framebuffer::GraphicsFramebuffer) {
    let mut line = alloc::vec![0u32; graphics.width()];

    let x0 = -1.5;
    let y0 = -1.0;
    let x1 = 0.5;
    let y1 = 1.0;

    let aspect_ratio = graphics.width() as f32 / graphics.height() as f32;

    for y in 0..graphics.height() {
        for x in 0..graphics.width() {
            let u = (x as f32) / ((graphics.width()  - 1) as f32);
            let v = (y as f32) / ((graphics.height() - 1) as f32);

            let pr = (u * (x1 - x0) + x0) * aspect_ratio;
            let pi = v * (y1 - y0) + y0;

            let r = 0.0;
            let g = 0.0;
            let b = esacpe(pr, pi, 100);

            line[x] = graphics.convert_float_color(r, g, b);
        }

        graphics.set_pixels_in_line(0, y, &line);
    }
}
