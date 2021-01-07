#![no_std]
#![allow(clippy::identity_op, clippy::missing_safety_doc)]
#![feature(const_fn)]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};
use core::marker::PhantomData;

pub trait KernelInterrupts {
    fn in_interrupt() -> bool;
    fn in_exception() -> bool;

    fn core_id() -> u32;

    unsafe fn disable_interrupts();
    unsafe fn enable_interrupts();
}

#[repr(C)]
pub struct Lock<T: ?Sized, I: KernelInterrupts> {
    locked: AtomicBool,

    non_preemptible:    bool,
    _kernel_interrupts: PhantomData<I>,

    value: UnsafeCell<T>,
}

impl<T, I: KernelInterrupts> Lock<T, I> {
    pub const fn new(value: T) -> Self {
        Lock {
            value:              UnsafeCell::new(value),
            locked:             AtomicBool::new(false),
            non_preemptible:    false,
            _kernel_interrupts: PhantomData,
        }
    }
}

impl<T: ?Sized, I: KernelInterrupts> Lock<T, I> {
    #[inline(always)]
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn lock(&self) -> LockGuard<T, I> {
        while self.locked.compare_exchange_weak(false, true, Ordering::Acquire,
                                                Ordering::Relaxed).is_err() {
            while self.is_locked() {
                core::sync::atomic::spin_loop_hint();
            }
        }

        LockGuard {
            lock:        self,
            value:       unsafe { &mut *self.value.get() },
            force_taken: false,
        }
    }

    #[inline(always)]
    pub fn try_lock(&self) -> Option<LockGuard<T, I>> {
        if self.locked.compare_exchange(false, true, Ordering::Acquire,
                                        Ordering::Relaxed).is_ok() {
            Some(LockGuard {
                lock:        self,
                value:       unsafe { &mut *self.value.get() },
                force_taken: false,
            })
        } else {
            None
        }
    }

    #[inline(always)]
    pub unsafe fn force_take(&self) -> LockGuard<T, I> {
        LockGuard {
            lock:        self,
            value:       &mut *self.value.get(),
            force_taken: true,
        }
    }

    #[inline(always)]
    pub unsafe fn bypass(&self) -> *mut T {
        self.value.get()
    }
}

pub struct LockGuard<'a, T: ?Sized, I: KernelInterrupts> {
    lock:        &'a Lock<T, I>,
    value:       &'a mut T,
    force_taken: bool,
}

impl<'a, T: ?Sized, I: KernelInterrupts> Drop for LockGuard<'a, T, I> {
    fn drop(&mut self) {
        // Unlock the lock only if it is wasn't taken by force.
        if !self.force_taken {
            self.lock.locked.store(false, Ordering::Release);
        }
    }
}

impl<'a, T: ?Sized, I: KernelInterrupts> Deref for LockGuard<'a, T, I> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'a, T: ?Sized, I: KernelInterrupts> DerefMut for LockGuard<'a, T, I> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

unsafe impl<T: ?Sized + Send, I: KernelInterrupts> Send for Lock<T, I> {}
unsafe impl<T: ?Sized + Send, I: KernelInterrupts> Sync for Lock<T, I> {}
