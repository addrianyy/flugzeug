pub struct KernelInterrupts;

impl lock::Interrupts for KernelInterrupts {
    fn in_exception() -> bool { false }
    fn in_interrupt() -> bool { false }

    fn core_id() -> u32 { core!().id as u32 }

    unsafe fn enable_interrupts() {}
    unsafe fn disable_interrupts() {}
}

pub type Lock<T>          = lock::Lock<T, KernelInterrupts>;
pub type LockGuard<'a, T> = lock::LockGuard<'a, T, KernelInterrupts>;
