#![no_std]
#![allow(clippy::identity_op, clippy::missing_safety_doc)]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

/// Port address for COM1.
const COM1: u16 = 0x3f8;

/// Serial port driver.
#[repr(C)]
pub struct SerialPort {
    /// IO address for this serial port.
    port: u16,
}

impl SerialPort {
    /// Initialize COM1 serial port.
    pub unsafe fn new() -> Self {
        let port = COM1;

        cpu::outb(port + 1, 0x00); // Disable all interrupts.
        cpu::outb(port + 3, 0x80); // Enable DLAB.
        cpu::outb(port + 0, 0x01); // Low byte divisor (115200 baud).
        cpu::outb(port + 1, 0x00); // High byte divisor.
        cpu::outb(port + 3, 0x03); // 8 bits, 1 stop bit, no parity.
        cpu::outb(port + 4, 0x03); // RTS/DSR set.

        Self {
            port,
        }
    }

    /// Send a byte to the serial port.
    fn write_byte(&mut self, byte: u8) {
        // Force CLRF.
        if byte == b'\n' {
            self.write_byte(b'\r');
        }

        unsafe {
            // Wait for empty transport.
            while cpu::inb(self.port + 5) & 0x20 == 0 {}

            // Transmit byte.
            cpu::outb(self.port, byte);
        }
    }
}

impl core::fmt::Write for SerialPort {
    /// Send UTF-8 string to the serial port.
    fn write_str(&mut self, string: &str) -> core::fmt::Result {
        // Send every byte of the string to the serial port.
        for byte in string.bytes() {
            self.write_byte(byte);
        }

        Ok(())
    }
}
