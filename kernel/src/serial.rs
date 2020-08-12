#[macro_export]
macro_rules! print {
    ($($arg: tt)*) => {{
        let mut serial = core!().boot_block.serial_port.lock();

        let _ = core::fmt::Write::write_fmt(serial.as_mut().unwrap(), format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        print!("\n");
    }};
    ($($arg: tt)*) => {{
        let mut serial = core!().boot_block.serial_port.lock();

        let _ = core::fmt::Write::write_fmt(serial.as_mut().unwrap(), format_args!($($arg)*));
        let _ = core::fmt::Write::write_str(serial.as_mut().unwrap(), "\n");
    }};
}
