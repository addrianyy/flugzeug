#![no_std]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

const COM1: u16 = 0x3f8;

#[repr(C)]
pub struct SerialPort {
    port: u16,
}

impl SerialPort {
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

    fn write_byte(&mut self, byte: u8) {
        if byte == b'\n' {
            self.write_byte(b'\r');
        }

        unsafe {
            // Wait for empty transport.
            while cpu::inb(self.port + 5) & 0x20 == 0 {}

            cpu::outb(self.port, byte);
        }
    }
}

impl core::fmt::Write for SerialPort {
    fn write_str(&mut self, string: &str) -> core::fmt::Result {
        for byte in string.bytes() {
            self.write_byte(byte);
        }

        Ok(())
    }
}
