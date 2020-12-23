use crate::efi;

use core::sync::atomic::{AtomicPtr, Ordering};

static TEXT_OUTPUT: AtomicPtr<efi::EfiSimpleTextOutputProtocol> =
    AtomicPtr::new(core::ptr::null_mut());

pub struct ConsoleWriter;

impl core::fmt::Write for ConsoleWriter {
    fn write_str(&mut self, string: &str) -> core::fmt::Result {
        let mut buffer = [0u16; 32];
        let mut used   = 0;

        // Cannot print to the console if we have exited boot services.
        let text_output = TEXT_OUTPUT.load(Ordering::Relaxed);
        if  text_output.is_null() {
            return Ok(());
        }

        macro_rules! flush {
            () => {
                if used > 0 {
                    // Write in null terminator.
                    buffer[used] = 0;

                    // Output text to the console.
                    unsafe {
                        ((*text_output).output_string)(text_output, buffer.as_ptr());
                    }

                    #[allow(unused)]
                    {
                        used = 0;
                    }
                }
            }
        }

        // Encode string to UTF-16 as required by UEFI.
        for ch in string.encode_utf16() {
            // Change or `\n` to `\r\n`.
            if ch == b'\n' as u16 {
                buffer[used] = b'\r' as u16;
                used += 1;
            }

            buffer[used] = ch;
            used += 1;

            // Make sure that there is space for at least on character, `\r` and null terminator.
            if used >= buffer.len() - 3 {
                flush!();
            }
        }

        flush!();

        Ok(())
    }
}

pub unsafe fn initialize(system_table: *mut efi::EfiSystemTable) {
    let text_output = (*system_table).stdout;

    TEXT_OUTPUT.store(text_output, Ordering::Relaxed);
}

pub unsafe fn on_exited_boot_services() {
    TEXT_OUTPUT.store(core::ptr::null_mut(), Ordering::Relaxed);
}

pub fn available() -> bool {
    !TEXT_OUTPUT.load(Ordering::Relaxed).is_null()
}

#[macro_export]
macro_rules! print {
    ($($arg: tt)*) => {{
        if crate::print::available() {
            // This will get sent to the serial port too.
            let _ = core::fmt::Write::write_fmt(&mut crate::print::ConsoleWriter,
                                                format_args!($($arg)*));
        } else {
            let mut serial = $crate::BOOT_BLOCK.serial_port.lock();

            if let Some(serial) = serial.as_mut() {
                let _ = core::fmt::Write::write_fmt(serial, format_args!($($arg)*));
            }
        }
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        print!("\n");
    }};
    ($($arg: tt)*) => {{
        if crate::print::available() {
            // This will get sent to the serial port too.
            let _ = core::fmt::Write::write_fmt(&mut crate::print::ConsoleWriter,
                                                format_args!($($arg)*));
            let _ = core::fmt::Write::write_str(&mut crate::print::ConsoleWriter, "\n");
        } else {
            let mut serial = $crate::BOOT_BLOCK.serial_port.lock();

            if let Some(serial) = serial.as_mut() {
                let _ = core::fmt::Write::write_fmt(serial, format_args!($($arg)*));
                let _ = core::fmt::Write::write_str(serial, "\n");
            }
        }
    }};
}
