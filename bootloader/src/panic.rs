use core::panic::PanicInfo;
use core::fmt::Write;

use serial_port::SerialPort;

#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    // Don't use normal serial port here. It could be:
    // 1. uninitialized
    // 2. locked
    // Both of these situations will create undesirable effects, so we just reinitialize serial
    // port. This is broken if there are concurrent users of serial port.
    let mut serial_port = unsafe { SerialPort::new() };

    let _ = write!(serial_port, "Bootloader panic.\n");

    if let Some(message) = panic_info.message() {
        let _ = write!(serial_port, "message: {}\n", message);
    }

    if let Some(location) = panic_info.location() {
        let _ = write!(serial_port, "location: {}:{}\n", location.file(), location.line());
    }

    cpu::halt();
}
