use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use core::panic::PanicInfo;
use core::fmt::Write;

use crate::framebuffer::{self, TextFramebuffer};
use crate::processors::{self, CoreState};
use crate::time;

use serial_port::SerialPort;
use crate::lock::{Lock, LockGuard};

const CORE_UNLOCKED: u64 = 0xffff_ffff_ffff_ffff;
const CORE_UNKNOWN:  u64 = 0xffff_ffff_ffff_fffe;

static EMERGENCY_WRITING_CORE: AtomicU64  = AtomicU64::new(CORE_UNLOCKED);
static IS_PANICKING:           AtomicBool = AtomicBool::new(false);

/// We assume 3.5GHz processor. We don't need accurate time in panic subsystem so
/// this assumption is safe.
const ASSUMED_CPU_FREQUENCY_MHZ: u64 = 3_500;

pub fn is_panicking() -> bool {
    IS_PANICKING.load(Ordering::Relaxed)
}

fn has_core_locals() -> bool {
    let gs_base = unsafe { cpu::rdmsr(0xc0000101) };

    gs_base != 0
}

unsafe fn force_acquire_lock<T>(lock: &Lock<T>) -> LockGuard<'_, T> {
    // Try geting the lock normally for a few milliseconds.
    let wait_microseconds = 400_000;
    let wait_cycles       = ASSUMED_CPU_FREQUENCY_MHZ * wait_microseconds;

    let end_tsc = time::get_tsc() + wait_cycles;

    while time::get_tsc() < end_tsc {
        // Try to unsafely lock. This function won't panic on deadlock. It also
        // won't disable interrupts if needed (that's fine, they are already disabled).
        if let Some(locked) = lock.try_lock_unsafe() {
            return locked;
        }
    }

    // This lock is most likely held by this core. Take the lock in unsafe manner.
    lock.force_lock_unsafe()
}

enum SerialPortWrapper {
    Recreated(SerialPort),
    Global(LockGuard<'static, Option<SerialPort>>),
}

struct EmergencyWriter {
    framebuffer: LockGuard<'static, Option<TextFramebuffer>>,
    serial_port: SerialPortWrapper,
}

impl EmergencyWriter {
    unsafe fn new() -> Self {
        let has_core_locals = has_core_locals();
        let core_id         = if has_core_locals {
            // If APIC was not initialized we hope that core ID is equal to APIC ID.
            if let Some(apic_id) = core!().apic_id() {
                apic_id as u64
            } else {
                core!().id
            }
        } else {
            // There can be only one core at a time with unknown ID.
            CORE_UNKNOWN
        };

        // Acquire critical section so there can be only one emergency writer at a time. Allow
        // reentrancy from the same core to allow nested exception/panic.
        loop {
            // Handle case where emergency writer is unlocked.
            if EMERGENCY_WRITING_CORE.compare_exchange(CORE_UNLOCKED, core_id,
                                                       Ordering::Relaxed,
                                                       Ordering::Relaxed).is_ok() {
                break;
            }

            let locked_id = EMERGENCY_WRITING_CORE.load(Ordering::Relaxed);

            // Handle case where emergency writer is locked by our current core.
            if locked_id == core_id {
                break;
            }

            // Handle case where emergency writer is locked by halted core.
            if locked_id != CORE_UNKNOWN && locked_id != CORE_UNLOCKED &&
               processors::core_state(locked_id as u32) == CoreState::Halted
            {
                let ok = EMERGENCY_WRITING_CORE.compare_exchange(locked_id, core_id,
                                                                 Ordering::Relaxed,
                                                                 Ordering::Relaxed).is_ok();
                if ok {
                    break;
                }
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

        if let Some(framebuffer) = self.framebuffer.as_mut() {
            // Use red color for panics.
            framebuffer.set_color(0xff0000);

            let _ = core::fmt::Write::write_str(framebuffer, string);

            framebuffer.reset_color();
        }

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

pub unsafe fn halt() -> ! {
    // Make sure that nobody will interrupt us before we halt. As we won't use any locks here
    // we don't need to use `core!` macro to properly manager interrupt disable depth.
    cpu::disable_interrupts();

    if has_core_locals() {
        if let Some(apic_id) = core!().apic_id() {
            processors::set_core_state(apic_id, CoreState::Halted);
        }
    }

    cpu::halt();
}

unsafe fn begin_panic() -> bool {
    // Make sure to disable interrupts as system is in possibly invalid state.
    cpu::disable_interrupts();

    if has_core_locals() {
        // If we have core locals we have to properly manage interrupt disable depth.
        core!().disable_interrupts();
    } else {
        // We have panicked very early and cannot NMI other cores. That's fine, all of them
        // should be waiting for us.
        return true;
    }

    if let Some(apic) = &mut *core!().apic.bypass() {
        // Set `IS_PANICKING` and make sure that we are panicking only once.
        if IS_PANICKING.compare_exchange(false, true, Ordering::Relaxed,
                                         Ordering::Relaxed).is_err() {
            return false;
        }

        // Halt execution of all cores on the system by sending NMI to them.
        // NMI handler will check if we are panicking and if we are then it will
        // halt the processor.
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
            apic.ipi(apic_id, (1 << 14) | (0b100 << 8));

            {
                // Wait for the CPU to become halted.
                let wait_microseconds = 200_000;
                let wait_cycles       = ASSUMED_CPU_FREQUENCY_MHZ * wait_microseconds;

                let end_tsc = time::get_tsc() + wait_cycles;

                while time::get_tsc() < end_tsc {
                    if processors::core_state(apic_id) == CoreState::Halted {
                        break;
                    }

                    core::sync::atomic::spin_loop_hint();
                }

                // Timeout, we hope that this procesor will get halted soon.
            }

            processors::set_core_state(apic_id, CoreState::Halted);
        }
    }

    true
}

fn dump_panic_info(writer: &mut EmergencyWriter, panic_info: &PanicInfo) {
    let _ = writeln!(writer);

    if has_core_locals() {
        let _ = write!(writer, "Kernel panic on CPU {}", core!().id);
    } else {
        let _ = write!(writer, "Kernel panic on unknown CPU");
    }

    if let Some(location) = panic_info.location() {
        let _ = writeln!(writer, " ({}:{})!", location.file(), location.line());
    } else {
        let _ = writeln!(writer, "!");
    }

    if let Some(message) = panic_info.message() {
        let _ = writeln!(writer, "{}", message);
    }
}

#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    unsafe {
        if begin_panic() {
            dump_panic_info(&mut EmergencyWriter::new(), panic_info);
        }

        halt();
    }
}
