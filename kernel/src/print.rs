#[macro_export]
macro_rules! print {
    ($($arg: tt)*) => {{
        let mut serial = core!().boot_block.serial_port.lock();
        let     serial = serial.as_mut().unwrap();

        let _ = core::fmt::Write::write_fmt(serial, format_args!($($arg)*));

        // Print to framebuffer too if it is available.
        if let Some(framebuffer) = crate::framebuffer::get().lock().as_mut() {
            let _ = core::fmt::Write::write_fmt(framebuffer, format_args!($($arg)*));
        }
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        print!("\n");
    }};
    ($($arg: tt)*) => {{
        let mut serial = core!().boot_block.serial_port.lock();
        let     serial = serial.as_mut().unwrap();

        let _ = core::fmt::Write::write_fmt(serial, format_args!($($arg)*));
        let _ = core::fmt::Write::write_str(serial, "\n");

        // Print to framebuffer too if it is available.
        if let Some(framebuffer) = crate::framebuffer::get().lock().as_mut() {
            let _ = core::fmt::Write::write_fmt(framebuffer, format_args!($($arg)*));
            let _ = core::fmt::Write::write_str(framebuffer, "\n");
        }
    }};
}
