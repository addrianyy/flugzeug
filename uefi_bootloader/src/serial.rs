use serial_port::SerialPort;
use crate::BOOT_BLOCK;

pub unsafe fn initialize() {
    let mut serial_port = BOOT_BLOCK.serial_port.lock();

    assert!(serial_port.is_none(), "Serial port was already initialized.");

    *serial_port = Some(SerialPort::new());
}
