pub struct KernelInterrupts;

impl lock::Interrupts for KernelInterrupts {
    fn in_exception() -> bool { core!().in_exception() }
    fn in_interrupt() -> bool { core!().in_interrupt() }

    fn core_id() -> u32 { core!().id as u32 }

    unsafe fn enable_interrupts()  { core!().enable_interrupts()  }
    unsafe fn disable_interrupts() { core!().disable_interrupts() }
}

pub type Lock<T>          = lock::Lock<T, KernelInterrupts>;
pub type LockGuard<'a, T> = lock::LockGuard<'a, T, KernelInterrupts>;
