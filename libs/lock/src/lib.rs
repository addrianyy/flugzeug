#![no_std]

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};

#[repr(C)]
pub struct Lock<T: ?Sized> {
    ticket:  AtomicU32,
    release: AtomicU32,
    value:   UnsafeCell<T>,
}

impl<T> Lock<T> {
    pub const fn new(value: T) -> Self {
        Lock {
            value:   UnsafeCell::new(value),
            ticket:  AtomicU32::new(0),
            release: AtomicU32::new(0),
        }
    }
}

impl<T: ?Sized> Lock<T> {
    pub fn lock(&self) -> LockGuard<T> {
        let ticket = self.ticket.fetch_add(1, Ordering::SeqCst);

        while self.release.load(Ordering::SeqCst) != ticket {
            core::sync::atomic::spin_loop_hint();
        }

        LockGuard {
            lock: self,
        }
    }
}

pub struct LockGuard<'a, T: ?Sized> {
    lock: &'a Lock<T>,
}

impl<'a, T: ?Sized> Drop for LockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.release.fetch_add(1, Ordering::SeqCst);
    }
}

impl<'a, T: ?Sized> Deref for LockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe {
            &*self.lock.value.get()
        }
    }
}

impl<'a, T: ?Sized> DerefMut for LockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            &mut *self.lock.value.get()
        }
    }
}

unsafe impl<T: ?Sized + Send> Send for Lock<T> {}
unsafe impl<T: ?Sized + Send> Sync for Lock<T> {}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test() {
        let lock  = Lock::new(1887);
        let mut v = lock.lock();

        *v += 10;
        
        drop(v);
            
        assert!(*lock.lock() == 1897);
    }
}
