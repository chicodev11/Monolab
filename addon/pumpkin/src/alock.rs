//! A lock that waits instead of failing. The studio is held across Pumpkin's async calls (a tick awaits many), and a
//! `try_lock` there dropped whatever came meanwhile: commands, leaves, clicks, model frames. Waiting is safe here:
//! the holder only waits on the host, never on an event handler of ours.

use std::cell::UnsafeCell;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

pub struct AsyncLock<T> {
    locked: AtomicBool,
    waiters: Mutex<Vec<Waker>>,
    value: UnsafeCell<T>,
}

// SAFETY: the value is reached only through a `Guard`, and `locked` lets one guard exist at a time.
unsafe impl<T: Send> Send for AsyncLock<T> {}
// SAFETY: as above.
unsafe impl<T: Send> Sync for AsyncLock<T> {}

pub struct Guard<'a, T> {
    lock: &'a AsyncLock<T>,
}

impl<T> AsyncLock<T> {
    pub const fn new(value: T) -> Self {
        Self { locked: AtomicBool::new(false), waiters: Mutex::new(Vec::new()), value: UnsafeCell::new(value) }
    }

    pub fn try_lock(&self) -> Option<Guard<'_, T>> {
        self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).ok().map(|_| Guard { lock: self })
    }

    pub fn lock(&self) -> Wait<'_, T> {
        Wait { lock: self }
    }
}

pub struct Wait<'a, T> {
    lock: &'a AsyncLock<T>,
}

impl<'a, T> Future for Wait<'a, T> {
    type Output = Guard<'a, T>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Guard<'a, T>> {
        if let Some(guard) = self.lock.try_lock() {
            return Poll::Ready(guard);
        }
        self.lock.waiters.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(context.waker().clone());
        // Freed between the first look and the waker going in: take it now rather than wait for a wake.
        match self.lock.try_lock() {
            Some(guard) => Poll::Ready(guard),
            None => Poll::Pending,
        }
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
        // Everyone waiting looks again; one gets it, the rest wait for the next drop.
        let waiting = std::mem::take(&mut *self.lock.waiters.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
        waiting.into_iter().for_each(Waker::wake);
    }
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: this guard is the only one (see `AsyncLock`).
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: this guard is the only one (see `AsyncLock`).
        unsafe { &mut *self.lock.value.get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_at_a_time() {
        let lock = AsyncLock::new(1);
        let mut held = lock.try_lock().unwrap();
        *held += 1;
        assert!(lock.try_lock().is_none());
        drop(held);
        assert_eq!(*lock.try_lock().unwrap(), 2);
    }
}
