#![no_std]
#![allow(clippy::identity_op, clippy::missing_safety_doc)]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

#[repr(C)]
pub struct Lock<T: ?Sized> {
    locked:  AtomicBool,
    blocked: AtomicBool,
    value:   UnsafeCell<T>,
}

impl<T> Lock<T> {
    pub const fn new(value: T) -> Self {
        Lock {
            value:   UnsafeCell::new(value),
            locked:  AtomicBool::new(false),
            blocked: AtomicBool::new(false),
        }
    }
}

impl<T: ?Sized> Lock<T> {
    #[inline(always)]
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn lock(&self) -> LockGuard<T> {
        while self.locked.compare_exchange_weak(false, true, Ordering::Acquire,
                                                Ordering::Relaxed).is_err() {
            while self.is_locked() {
                core::sync::atomic::spin_loop_hint();
            }
        }

        LockGuard {
            lock:  self,
            value: unsafe { &mut *self.value.get() },
        }
    }

    #[inline(always)]
    pub fn try_lock(&self) -> Option<LockGuard<T>> {
        if self.locked.compare_exchange(false, true, Ordering::Acquire,
                                        Ordering::Relaxed).is_ok() {
            Some(LockGuard {
                lock:  self,
                value: unsafe { &mut *self.value.get() },
            })
        } else {
            None
        }
    }

    #[inline(always)]
    pub unsafe fn steal_and_block(&self) -> LockGuard<T> {
        // Set the lock state to blocked so it will never be unlocked. As someone may
        // have unlocked this lock before we set `blocked` we also set `locked`.
        self.blocked.store(true, Ordering::Relaxed);
        self.locked.store(true, Ordering::Release);

        LockGuard {
            lock:  self,
            value: &mut *self.value.get(),
        }
    }

    #[inline(always)]
    pub unsafe fn bypass(&self) -> *mut T {
        self.value.get()
    }
}

pub struct LockGuard<'a, T: ?Sized> {
    lock:  &'a Lock<T>,
    value: &'a mut T,
}

impl<'a, T: ?Sized> Drop for LockGuard<'a, T> {
    fn drop(&mut self) {
        // Unlock the lock only if it is not blocked.
        if !self.lock.blocked.load(Ordering::Relaxed) {
            self.lock.locked.store(false, Ordering::Release);
        }
    }
}

impl<'a, T: ?Sized> Deref for LockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'a, T: ?Sized> DerefMut for LockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
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
