#![no_std]
#![allow(clippy::identity_op, clippy::missing_safety_doc)]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use core::marker::PhantomData;

pub trait Interrupts {
    fn in_interrupt() -> bool;
    fn in_exception() -> bool;

    fn core_id() -> u32;

    unsafe fn disable_interrupts();
    unsafe fn enable_interrupts();
}

#[repr(C)]
pub struct Lock<T: ?Sized, I: Interrupts> {
    locked: AtomicBool,

    non_preemptible: bool,
    _interrupts:     PhantomData<I>,
    lock_core:       AtomicU32,

    value: UnsafeCell<T>,
}

impl<T, I: Interrupts> Lock<T, I> {
    pub const fn new(value: T) -> Self {
        Lock {
            value:           UnsafeCell::new(value),
            locked:          AtomicBool::new(false),
            lock_core:       AtomicU32::new(!0),
            non_preemptible: false,
            _interrupts:     PhantomData,
        }
    }

    pub const fn new_non_preemptible(value: T) -> Self {
        Lock {
            value:           UnsafeCell::new(value),
            locked:          AtomicBool::new(false),
            lock_core:       AtomicU32::new(!0),
            non_preemptible: true,
            _interrupts:     PhantomData,
        }
    }
}

impl<T: ?Sized, I: Interrupts> Lock<T, I> {
    #[track_caller]
    unsafe fn pre_lock(&self) {
        if self.non_preemptible {
            // This lock is non preemptible so interrupts must be disabled when we hold it.
            I::disable_interrupts();
        } else {
            assert!(!I::in_interrupt(), "Tried to take preemptible lock in the \
                    interrupt handler.");
        }

        // If this lock is locked by our core that means that we have deadlocked.
        assert!(self.lock_core.load(Ordering::Relaxed) != I::core_id(), "Deadlock detected.");
    }

    unsafe fn post_lock(&self) {
        if self.non_preemptible {
            // If this lock is non preemptible then when we acquired it we
            // disabled interrupts. Reenable them.
            I::enable_interrupts();
        }
    }

    #[inline(always)]
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }

    #[inline(always)]
    #[track_caller]
    pub fn lock(&self) -> LockGuard<T, I> {
        unsafe {
            self.pre_lock();
        }

        while self.locked.compare_exchange_weak(false, true, Ordering::Acquire,
                                                Ordering::Relaxed).is_err() {
            while self.is_locked() {
                core::hint::spin_loop();
            }
        }

        self.lock_core.store(I::core_id(), Ordering::Relaxed);

        LockGuard {
            lock:          self,
            value:         unsafe { &mut *self.value.get() },
            force_taken:   false,
            unsafe_locked: false,
        }
    }

    /// Try to unsafely take a lock. This function won't panic on deadlock. It also won't
    /// disable interrupts if needed.
    #[inline(always)]
    pub unsafe fn try_lock_unsafe(&self) -> Option<LockGuard<T, I>> {
        if self.locked.compare_exchange(false, true, Ordering::Acquire,
                                        Ordering::Relaxed).is_ok() {
            self.lock_core.store(I::core_id(), Ordering::Relaxed);

            Some(LockGuard {
                lock:          self,
                value:         &mut *self.value.get(),
                force_taken:   false,
                unsafe_locked: true,
            })
        } else {
            None
        }
    }

    /// Unsafely take a lock. This function won't panic on deadlock. It also won't
    /// disable interrupts if needed.
    #[inline(always)]
    pub unsafe fn force_lock_unsafe(&self) -> LockGuard<T, I> {
        LockGuard {
            lock:          self,
            value:         &mut *self.value.get(),
            force_taken:   true,
            unsafe_locked: true,
        }
    }

    /// Avoid a lock and get direct access to the underlying data.
    #[inline(always)]
    pub unsafe fn bypass(&self) -> *mut T {
        self.value.get()
    }
}

pub struct LockGuard<'a, T: ?Sized, I: Interrupts> {
    lock:          &'a Lock<T, I>,
    value:         &'a mut T,
    force_taken:   bool,
    unsafe_locked: bool,
}

impl<'a, T: ?Sized, I: Interrupts> Drop for LockGuard<'a, T, I> {
    fn drop(&mut self) {
        // Unlock the lock only if it is wasn't taken by force.
        if !self.force_taken {
            self.lock.lock_core.store(!0, Ordering::Relaxed);
            self.lock.locked.store(false, Ordering::Release);

            // Invoke post lock callback only if we have safely locked this data.
            if !self.unsafe_locked {
                unsafe {
                    self.lock.post_lock();
                }
            }
        }
    }
}

impl<'a, T: ?Sized, I: Interrupts> Deref for LockGuard<'a, T, I> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'a, T: ?Sized, I: Interrupts> DerefMut for LockGuard<'a, T, I> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

unsafe impl<T: ?Sized + Send, I: Interrupts> Send for Lock<T, I> {}
unsafe impl<T: ?Sized + Send, I: Interrupts> Sync for Lock<T, I> {}
