#[macro_export]
macro_rules! print {
    ($($arg: tt)*) => {{
        use core::fmt::Write;
        let _ = write!(core!().boot_block.serial_port.lock().as_mut().unwrap(), $($arg)*);
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        print!("\n");
    }};
    ($($arg: tt)*) => {{
        print!($($arg)*);
        print!("\n");
    }};
}

