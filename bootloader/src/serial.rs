use serial_port::SerialPort;
use crate::BOOT_BLOCK;

pub unsafe fn initialize() {
    let mut serial_port = BOOT_BLOCK.serial_port.lock();

    assert!(serial_port.is_none(), "Serial port was already initialized.");

    *serial_port = Some(SerialPort::new());
}

#[macro_export]
macro_rules! print {
    ($($arg: tt)*) => {{
        use core::fmt::Write;
        let _ = write!($crate::BOOT_BLOCK.serial_port.lock().as_mut().unwrap(), $($arg)*);
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
