use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use core::panic::PanicInfo;
use core::fmt::Write;

use crate::framebuffer::{self, TextFramebuffer};
use crate::processors::{self, CoreState};

use serial_port::SerialPort;
use lock::{Lock, LockGuard};

const CORE_UNLOCKED: u64 = 0xffff_ffff_ffff_ffff;
const CORE_UNKNOWN:  u64 = 0xffff_ffff_ffff_fffe;

static EMERGENCY_WRITING_CORE: AtomicU64  = AtomicU64::new(CORE_UNLOCKED);
static IS_PANICKING:           AtomicBool = AtomicBool::new(false);

pub fn is_panicking() -> bool {
    IS_PANICKING.load(Ordering::Relaxed)
}

fn has_core_locals() -> bool {
    let gs_base = unsafe { cpu::rdmsr(0xc0000101) };

    gs_base != 0
}

unsafe fn force_acquire_lock<'a, T>(lock: &'a Lock<T>) -> LockGuard<'a, T> {
    // Try geting the lock normally for a few milliseconds. Time subsystem is possibly not
    // initialized so we assume 3.5GHz processor. We don't need accurate time so this
    // assumption is safe.

    let wait_microseconds = 300_000;
    let tsc_mhz           = 3_500;
    let wait_cycles       = tsc_mhz * wait_microseconds;

    let end_tsc = crate::time::get_tsc() + wait_cycles;

    while crate::time::get_tsc() < end_tsc {
        if let Some(locked) = lock.try_lock() {
            return locked;
        }
    }

    // This lock is most likely held by this core. Take the lock in unsafe manner.
    lock.steal_and_block()
}

enum SerialPortWrapper {
    Recreated(SerialPort),
    Global(LockGuard<'static, Option<SerialPort>>),
}

pub struct EmergencyWriter {
    framebuffer: LockGuard<'static, Option<TextFramebuffer>>,
    serial_port: SerialPortWrapper,
}

impl EmergencyWriter {
    pub unsafe fn new() -> Self {
        let has_core_locals = has_core_locals();
        let core_id         = if has_core_locals {
            core!().id
        } else {
            // There can be only one core at a time with unknown ID.
            CORE_UNKNOWN
        };

        // Acquire critical section so there can be only one emergency writer at a time. Allow
        // reentrancy from the same core to allow nested exception/panic.
        loop {
            // Handle case where emergency writer is unlocked.
            if EMERGENCY_WRITING_CORE.compare_and_swap(CORE_UNLOCKED, core_id,
                                                       Ordering::Relaxed) == CORE_UNLOCKED {
                break;
            }

            // Handle case where emergency writer is locked by our current core.
            if EMERGENCY_WRITING_CORE.load(Ordering::Relaxed) == core_id {
                break;
            }

            core::sync::atomic::spin_loop_hint();
        }

        let serial_port = if has_core_locals {
            // If core locals are already initialized then take the serial port from boot block.
            SerialPortWrapper::Global(force_acquire_lock(&core!().boot_block.serial_port))
        } else {
            // We don't have core locals yet and we need to recreate serial port.
            SerialPortWrapper::Recreated(SerialPort::new())
        };

        Self {
            framebuffer: force_acquire_lock(framebuffer::get()),
            serial_port,
        }
    }
}

impl core::fmt::Write for EmergencyWriter {
    fn write_str(&mut self, string: &str) -> core::fmt::Result {
        if let Some(framebuffer) = self.framebuffer.as_mut() {
            // Use red color for panics.
            framebuffer.set_color(0xff0000);

            let _ = core::fmt::Write::write_str(framebuffer, string);

            framebuffer.reset_color();
        }

        let serial_port = match &mut self.serial_port {
            SerialPortWrapper::Recreated(port) => port,
            SerialPortWrapper::Global(port)    => {
                if let Some(port) = port.as_mut() {
                    port
                } else {
                    // If global port is `None` then recreate it.

                    let port = unsafe { SerialPort::new() };

                    self.serial_port = SerialPortWrapper::Recreated(port);

                    if let SerialPortWrapper::Recreated(port) = &mut self.serial_port {
                        port
                    } else {
                        unreachable!()
                    }
                }
            }
        };

        let _ = core::fmt::Write::write_str(serial_port, string);

        Ok(())
    }
}

impl Drop for EmergencyWriter {
    fn drop(&mut self) {
        // We always unlock writer even in case of nested emergency writers.
        // User must make sure that it's not possible to access previous emergency
        // writers.
        EMERGENCY_WRITING_CORE.store(CORE_UNLOCKED, Ordering::Relaxed);
    }
}

unsafe fn dump_panic_info(panic_info: &PanicInfo) {
    let mut writer = EmergencyWriter::new();

    let _ = writeln!(writer, "Kernel panic!");

    if let Some(message) = panic_info.message() {
        let _ = writeln!(writer, "message: {}", message);
    }

    if let Some(location) = panic_info.location() {
        let _ = writeln!(writer, "location: {}:{}", location.file(), location.line());
    }
}

#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    unsafe {
        // Make sure to disable interrupts as system is in invalid state.
        asm!("cli");

        // Print information about the panic to user.
        dump_panic_info(panic_info);
    }

    if has_core_locals() {
        IS_PANICKING.store(true, Ordering::Relaxed);

        // Halt execution of all cores on the system by sending NMI to them.
        unsafe {
            let apic = &mut *core!().apic.bypass();

            if let Some(apic) = apic {
                for apic_id in 0..processors::MAX_CORES {
                    let apic_id = apic_id as u32;

                    // Skip this core.
                    if Some(apic_id) == core!().apic_id() {
                        continue;
                    }

                    // Skip non-launched cores.
                    if processors::core_state(apic_id) != CoreState::Online {
                        continue;
                    }

                    // Request to halt execution via NMI.
                    apic.ipi(apic_id, (1 << 14) | (4 << 8));
                }
            }
        }
    }

    cpu::halt();
}
