#![no_std]
#![no_main]

extern crate libc_routines;

mod bios;

use core::fmt::Write;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

const COM1: u16 = 0x3f8;

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

#[no_mangle]
extern "C" fn _start(_boot_disk_descriptor: u32, _boot_disk_data: u32) -> ! {
    let mut serial = unsafe { SerialPort::new() };

    let mut sequence = 0;

    loop {
        #[repr(C)]
        #[derive(Default, Debug)]
        struct E820Entry {
            base: u64,
            size: u64,
            typ:  u32,
            acpi: u32,
        }

        // Some BIOSes won't set ACPI field so we need to make it valid in the beginning.
        let mut entry = E820Entry {
            acpi: 1,
            ..Default::default()
        };

        // Make sure that the entry is accessible by BIOS.
        assert!((&entry as *const _ as usize) < 0x10000,
                "Entry is in high memory, BIOS won't be able to access it.");

        // Make sure that size matches excpected one.
        assert!(core::mem::size_of::<E820Entry>() == 24, "E820 entry has invalid size.");

        // Load all required magic values for this BIOS service.
        let mut regs = bios::RegisterState {
            eax: 0xe820,
            ebx: sequence,
            ecx: core::mem::size_of::<E820Entry>() as u32,
            edx: u32::from_be_bytes(*b"SMAP"),
            edi: &mut entry as *mut _ as u32,
            ..Default::default()
        };

        unsafe { bios::interrupt(0x15, &mut regs); }

        // Update current sequence so BIOS will know which entry to report in the next iteration.
        sequence = regs.ebx;

        // Don't do anything with this entry if ACPI bit 0 is not set. It should be skipped.
        // If BIOS didn't set it we assume it's valid.
        if entry.acpi & 1 != 0 && entry.size != 0 {
            let _ = write!(serial, "{:x?}\n", entry);
        }

        // CF set indicates error or end of the list, stop iteration. sequence == 0 indicates
        // end of the list.
        if regs.eflags & 1 != 0 || sequence == 0 {
            break;
        }
    }

    let _ = write!(serial, "Done!\n");

    loop {}
}
