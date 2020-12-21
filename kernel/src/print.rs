pub const ALWAYS_USE_SERIAL_PORT: bool = false;

#[macro_export]
macro_rules! print {
    ($($arg: tt)*) => {{
        color_print!(crate::framebuffer::DEFAULT_FOREGROUND_COLOR, $($arg)*);
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        print!("\n");
    }};
    ($($arg: tt)*) => {{
        color_println!(crate::framebuffer::DEFAULT_FOREGROUND_COLOR, $($arg)*);
    }};
}

#[macro_export]
macro_rules! color_print {
    ($color: expr, $($arg: tt)*) => {{
        let color: u32     = $color;
        let mut use_serial = crate::print::ALWAYS_USE_SERIAL_PORT;

        // Print to framebuffer if it is available.
        if let Some(framebuffer) = crate::framebuffer::get().lock().as_mut() {
            if color != crate::framebuffer::DEFAULT_FOREGROUND_COLOR {
                framebuffer.set_color(color);
            }

            let _ = core::fmt::Write::write_fmt(framebuffer, format_args!($($arg)*));

            if color != crate::framebuffer::DEFAULT_FOREGROUND_COLOR {
                framebuffer.reset_color();
            }
        } else {
            use_serial = true;
        }

        if use_serial {
            let mut serial = core!().boot_block.serial_port.lock();
            let     serial = serial.as_mut().unwrap();

            let _ = core::fmt::Write::write_fmt(serial, format_args!($($arg)*));
        }
    }};
}

#[macro_export]
macro_rules! color_println {
    ($color: expr) => {{
        color_print!($color, "\n");
    }};
    ($color: expr, $($arg: tt)*) => {{
        let color: u32     = $color;
        let mut use_serial = crate::print::ALWAYS_USE_SERIAL_PORT;

        // Print to framebuffer if it is available.
        if let Some(framebuffer) = crate::framebuffer::get().lock().as_mut() {
            if color != crate::framebuffer::DEFAULT_FOREGROUND_COLOR {
                framebuffer.set_color(color);
            }

            let _ = core::fmt::Write::write_fmt(framebuffer, format_args!($($arg)*));
            let _ = core::fmt::Write::write_str(framebuffer, "\n");

            if color != crate::framebuffer::DEFAULT_FOREGROUND_COLOR {
                framebuffer.reset_color();
            }
        } else {
            use_serial = true;
        }

        if use_serial {
            let mut serial = core!().boot_block.serial_port.lock();
            let     serial = serial.as_mut().unwrap();

            let _ = core::fmt::Write::write_fmt(serial, format_args!($($arg)*));
            let _ = core::fmt::Write::write_str(serial, "\n");
        }
    }};
}
