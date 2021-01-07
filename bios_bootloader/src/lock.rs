use core::sync::atomic::Ordering;

pub struct EmptyInterrupts;

impl lock::KernelInterrupts for EmptyInterrupts {
    fn in_exception() -> bool { false }
    fn in_interrupt() -> bool { false }

    fn core_id() -> u32 { crate::CORE_ID.load(Ordering::Relaxed) }

    unsafe fn enable_interrupts()  {}
    unsafe fn disable_interrupts() {}
}

pub type Lock<T> = lock::Lock<T, EmptyInterrupts>;
