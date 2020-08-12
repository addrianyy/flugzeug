use serial_port::SerialPort;
use crate::BOOT_BLOCK;

pub unsafe fn initialize() {
    let mut serial_port = BOOT_BLOCK.serial_port.lock();

    // Skip initialization if the serial port was already initialized by other CPU.
    if serial_port.is_some() {
        return;
    }

    *serial_port = Some(SerialPort::new());
}

#[macro_export]
macro_rules! print {
    ($($arg: tt)*) => {{
        let mut serial = $crate::BOOT_BLOCK.serial_port.lock();

        let _ = core::fmt::Write::write_fmt(serial.as_mut().unwrap(), format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        print!("\n");
    }};
    ($($arg: tt)*) => {{
        let mut serial = $crate::BOOT_BLOCK.serial_port.lock();

        let _ = core::fmt::Write::write_fmt(serial.as_mut().unwrap(), format_args!($($arg)*));
        let _ = core::fmt::Write::write_str(serial.as_mut().unwrap(), "\n");
    }};
}
