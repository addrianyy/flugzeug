use core::sync::atomic::{AtomicBool, Ordering};

pub struct Once(AtomicBool);

impl Once {
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub fn exec(&self, callback: impl FnOnce()) {
        if self.0.compare_exchange(false, true, Ordering::Relaxed,
                                   Ordering::Relaxed).is_ok() {
            callback();
        }
    }
}
