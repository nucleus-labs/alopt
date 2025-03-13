mod semaphore;

pub mod guard;

// #[cfg(feature = "tracing")]
// use tracing::{Level, span};

use std::cell::UnsafeCell;
use std::sync::atomic::Ordering::*;

use guard::{WomGuard, WorlGuardRead, WorlGuardWrite};
use semaphore::{AcquireError, AcquireResult, RwSemaphore};

type AtomicCounter = std::sync::atomic::AtomicI16;

/// Write, Option, Read, Lock (Worl)
pub struct Worl<T> {
    sem: RwSemaphore,
    data: UnsafeCell<Option<T>>,
}

/// Write, Option, Mutex
pub struct Wom<T> {
    mtx: AtomicCounter,
    data: UnsafeCell<Option<T>>,
}

impl<T> Worl<T> {
    /// Create a new populated instance
    pub fn new(data: T) -> Self {
        Self {
            sem: RwSemaphore::new(),
            data: UnsafeCell::new(Some(data)),
        }
    }

    /// Create a new empty instance
    pub fn empty() -> Self {
        Self {
            sem: RwSemaphore::new(),
            data: UnsafeCell::new(None),
        }
    }

    #[inline(always)]
    fn has_contents(&self) -> bool {
        unsafe { &*self.data.get() }.is_some()
    }

    /// Attempt to acquire a read-guard that automatically releases
    /// its lock upon being dropped.
    pub fn read(&self) -> AcquireResult<WorlGuardRead<'_, T>> {
        if !self.sem.acquire_read() {
            return Err(AcquireError::ReadUnavailable);
        }

        if !self.has_contents() {
            self.sem.release_read();
            Err(AcquireError::ValueNone)
        } else {
            Ok(WorlGuardRead::new(self))
        }
    }

    pub fn write(&self) -> AcquireResult<WorlGuardWrite<'_, T>> {
        if !self.sem.acquire_read() {
            return Err(AcquireError::ReadUnavailable);
        }
        self.sem.release_read();

        if !self.has_contents() {
            Err(AcquireError::ValueNone)
        } else {
            if !self.sem.acquire_write() {
                return Err(AcquireError::WriteUnavailable);
            }
            Ok(WorlGuardWrite::new(self))
        }
    }

    pub fn read_late(&self) -> AcquireResult<WorlGuardRead<'_, T>> {
        if !self.sem.acquire_read_wait() {
            return Err(AcquireError::Closed);
        }

        if !self.has_contents() {
            self.sem.release_read();
            Err(AcquireError::ValueNone)
        } else {
            Ok(WorlGuardRead::new(self))
        }
    }

    pub fn write_late(&self) -> AcquireResult<WorlGuardWrite<'_, T>> {
        if !self.sem.acquire_read_wait() {
            return Err(AcquireError::Closed);
        }
        self.sem.release_read();

        if !self.has_contents() {
            Err(AcquireError::ValueNone)
        } else {
            if !self.sem.acquire_write_wait() {
                return Err(AcquireError::Closed);
            }
            Ok(WorlGuardWrite::new(self))
        }
    }

    pub fn set(&self, data: T) -> AcquireResult {
        if !self.sem.acquire_write_wait() {
            return Err(AcquireError::Closed);
        }

        unsafe {
            *self.data.get() = Some(data);
        }
        self.sem.release_write();
        Ok(())
    }

    pub fn clear(&self) -> AcquireResult {
        if !self.sem.acquire_write_wait() {
            return Err(AcquireError::Closed);
        }

        unsafe {
            *self.data.get() = None;
        }
        self.sem.release_write();
        Ok(())
    }
}

impl<'a, T> Wom<T> {
    /// Create a new populated instance
    pub fn new(data: T) -> Self {
        Self {
            mtx: AtomicCounter::new(0),
            data: UnsafeCell::new(Some(data)),
        }
    }

    /// Create a new empty instance
    pub fn empty() -> Self {
        Self {
            mtx: AtomicCounter::new(0),
            data: UnsafeCell::new(None),
        }
    }

    fn get_ref(&'a self) -> Option<&'a mut T> {
        unsafe { &mut *self.data.get() }.as_mut()
    }

    pub fn get_mut(&'a self) -> AcquireResult<&'a mut T> {
        if self.mtx.load(Acquire) != 0 {
            Err(AcquireError::WriteUnavailable)
        } else if let Some(data) = self.get_ref() {
            Ok(data)
        } else {
            Err(AcquireError::ValueNone)
        }
    }

    pub fn lock(&'a self) -> AcquireResult<WomGuard<'a, T>> {
        if self.get_ref().is_none() {
            Err(AcquireError::ValueNone)
        } else {
            Ok(WomGuard::new(self))
        }
    }
}

impl<T> Drop for Worl<T> {
    fn drop(&mut self) {
        self.sem.close();
    }
}

unsafe impl<T: Send> Sync for Worl<T> {}
unsafe impl<T: Send> Send for Worl<T> {}

unsafe impl<T: Send> Sync for Wom<T> {}
unsafe impl<T: Send> Send for Wom<T> {}
