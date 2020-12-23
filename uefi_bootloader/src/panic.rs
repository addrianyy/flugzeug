use core::panic::PanicInfo;
use core::fmt::Write;

use serial_port::SerialPort;
use crate::print::ConsoleWriter;

#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    // Don't use normal serial port here. It could be uninitialized or locked.
    // Both of these situations will create undesirable effects, so we just reinitialize serial
    // port. This is broken if there are concurrent users of serial port.
    let mut serial_port    = unsafe { SerialPort::new() };
    let     console_writer = &mut ConsoleWriter;

    // Writing to `console_writer` will also send message to the serial port.
    let use_serial = !crate::print::available();

    if use_serial {
        let _ = writeln!(serial_port, "Bootloader panic.");
    }

    let _ = writeln!(console_writer, "Bootloader panic.");

    if let Some(message) = panic_info.message() {
        if use_serial {
            let _ = writeln!(serial_port, "message: {}", message);
        }

        let _ = writeln!(console_writer, "message: {}", message);
    }

    if let Some(location) = panic_info.location() {
        if use_serial {
            let _ = writeln!(serial_port, "location: {}:{}", location.file(), location.line());
        }

        let _ = writeln!(console_writer, "location: {}:{}", location.file(), location.line());
    }

    cpu::halt();
}
