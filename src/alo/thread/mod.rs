mod semaphore;

pub mod guard;

use std::cell::UnsafeCell;
use std::sync::atomic::Ordering::*;

use guard::{WomGuard, WorlGuardRead, WorlGuardWrite};
use semaphore::{AcquireError, AcquireResult, RwSemaphore};

type AtomicCounter = std::sync::atomic::AtomicBool;

/// Write, Option, Read, Lock ([`Worl`])
/// 
/// This is intended as an alternative for [`std::sync::RwLock`] that
/// has queued buffer requests (so writers cannot be starved), and
/// an integrated [`Option<T>`] that can be checked with a read-lock,
/// and can be set, cleared, or swapped using a write-lock.
/// 
/// Acquiring a read-lock or a write-lock returns a guard that releases
/// the lock when it's dropped. This guard dereferences to the data
/// contained inside the [`Option<T>`] rather than the Option itself. As
/// such, all attempts to obtain a write-lock must first check whether the
/// Option has content. **This itself requires a lock.** Because write-locks
/// are extremely slow to acquire compared to a read-lock, and a read-lock
/// is sufficient for checking the Option, each method that acquires a
/// write-lock attempts to acquire a read-lock first. This is a "fail-fast"
/// mechanism to prevent long wait-times on a write-lock for data that may
/// not even exist.
/// 
/// Because returning a write-lock requires data to be present, in order
/// to acquire a write-lock on an empty [`Worl`] you must use
/// [`Worl::set()`] to populate the [`Option<T>`].
pub struct Worl<T> {
    sem: RwSemaphore,
    data: UnsafeCell<Option<T>>,
}

/// Write, Option, Mutex ([`Wom`])
/// 
/// This is intended as an alternative for [`std::sync::Mutex`] that has
/// an integrated [`Option<T>`] that can be checked, set, cleared, or swapped
/// using a lock.
/// 
/// Acquiring a lock returns a gaurd that releases the lock when it's
/// dropped. This gaurd dereferences to the data contained inside the
/// [`Option<T>`] rather than to the Option itself. As such, all attempts to
/// obtain a lock must first check whether the Option has content. **This
/// itself requires a lock.** Because Mutexes only have write-locks,
/// acquisitions successes and failures take the same amount of time.
/// 
/// Because returning a write-lock requires data to be present, in order
/// to acquire a write-lock on an empty [`Wom`] you must use
/// [`Wom::set()`] to populate the [`Option<T>`].
pub struct Wom<T> {
    locked: AtomicCounter,
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

    pub fn set_timeout(self, timeout_millis: u16) -> Self {
        self.sem.set_timeout(timeout_millis);
        self
    }

    /// Acquires a read-lock. Multiple read-locks may be given at a time.
    /// 
    /// **Guaranteed not to starve.**
    pub fn read(&self) -> AcquireResult<WorlGuardRead<'_, T>> {
        if let Err(err) = self.sem.acquire_read() {
            Err(err)
        } else {
            if !self.has_contents() {
                self.sem.release_read();
                Err(AcquireError::ValueNone)
            } else {
                Ok(WorlGuardRead::new(self))
            }
        }
    }

    /// Acquires a write-lock. Only one write-lock may be active at a time,
    /// and not while *any* read-locks are held.
    /// 
    /// **Guaranteed not to starve.**
    pub fn write(&self) -> AcquireResult<WorlGuardWrite<'_, T>> {
        if let Err(err) = self.sem.acquire_read() {
            Err(err)
        } else {
            self.sem.release_read();
            if !self.has_contents() {
                Err(AcquireError::ValueNone)
            } else {
                if let Err(err) = self.sem.acquire_write() {
                    Err(err)
                } else {
                    Ok(WorlGuardWrite::new(self))
                }
            }
        }
    }

    /// Acquires a read-lock. Multiple read-locks may be given at a time.
    ///
    /// Sequential requests of the same type get batched so that they occupy
    /// fewer spaces within the buffer. However, this does mean that
    /// mixed-type acquisition requests can clog the buffer very quickly.
    /// When this happens, acquiring a lock is not possible until a batch has
    /// been completed.
    ///
    /// Upon batch completion, a message is sent out to callers
    /// of [`Self::acquire_read_unscheduled`] so they may attempt to acquire a
    /// space in the buffer. If they fail again, they just wait for another
    /// notification to try again, unless the failure isn't a result of the buffer
    /// being full.
    /// 
    /// **There are NO guarantees that this request doesn't get starved!**
    pub fn read_unscheduled(&self) -> AcquireResult<WorlGuardRead<'_, T>> {
        if let Err(err) = self.sem.acquire_read_unscheduled() {
            Err(err)
        } else {
            if !self.has_contents() {
                self.sem.release_read();
                Err(AcquireError::ValueNone)
            } else {
                Ok(WorlGuardRead::new(self))
            }
        }
    }

    /// Acquires a write-lock. Only one write-lock may be active at a time,
    /// and not while *any* read-locks are held.
    ///
    /// Sequential requests of the same type get batched so that they occupy
    /// fewer spaces within the buffer. However, this does mean that
    /// mixed-type acquisition requests can clog the buffer very quickly.
    /// When this happens, acquiring a lock is not possible until a batch has
    /// been completed.
    ///
    /// Upon batch completion, a message is sent out to callers
    /// of [`Self::acquire_write_unscheduled`] so they may attempt to acquire a
    /// space in the buffer. If they fail again, they just wait for another
    /// notification to try again, unless the failure isn't a result of the buffer
    /// being full.
    /// 
    /// **There are NO guarantees that this request doesn't get starved!**
    pub fn write_unscheduled(&self) -> AcquireResult<WorlGuardWrite<'_, T>> {
        if let Err(err) = self.sem.acquire_read_unscheduled() {
            Err(err)
        } else {
            self.sem.release_read();
            if !self.has_contents() {
                Err(AcquireError::ValueNone)
            } else {
                if let Err(err) = self.sem.acquire_write_unscheduled() {
                    Err(err)
                } else {
                    Ok(WorlGuardWrite::new(self))
                }
            }
        }
    }

    pub fn clear(&self) -> AcquireResult {
        if let Err(err) = self.sem.acquire_write_unscheduled() {
            return Err(err);
        }

        unsafe {
            *self.data.get() = None;
        }
        self.sem.release_write();
        Ok(())
    }

    pub fn set(&self, data: T) -> AcquireResult<WorlGuardWrite<'_, T>> {
        if let Err(err) = self.sem.acquire_write_unscheduled() {
            return Err(err);
        }

        unsafe {
            *self.data.get() = Some(data);
        }
        Ok(WorlGuardWrite::new(self))
    }

    pub fn swap(&self, data: &mut T) -> AcquireResult<WorlGuardWrite<'_, T>> {
        if let Err(err) = self.sem.acquire_write_unscheduled() {
            return Err(err);
        }
        let current = unsafe { &mut *self.data.get() }.as_mut();
        if let Some(cur) = current {
            std::mem::swap(data, cur);
            return Ok(WorlGuardWrite::new(self));
        } else {
            return Err(AcquireError::ValueNone);
        }
    }
}

impl<'a, T> Wom<T> {
    /// Create a new populated instance
    pub fn new(data: T) -> Self {
        Self {
            locked: AtomicCounter::new(false),
            data: UnsafeCell::new(Some(data)),
        }
    }

    /// Create a new empty instance
    pub fn empty() -> Self {
        Self {
            locked: AtomicCounter::new(false),
            data: UnsafeCell::new(None),
        }
    }

    fn data_as_mut(&'a self) -> Option<&'a mut T> {
        unsafe { &mut *self.data.get() }.as_mut()
    }

    pub fn get_mut(&'a self) -> AcquireResult<&'a mut T> {
        self.data_as_mut().ok_or(AcquireError::ValueNone)
    }

    pub fn lock(&'a self) -> AcquireResult<WomGuard<'a, T>> {
        if self.locked.fetch_not(AcqRel) {
            self.locked.fetch_not(Release);
            Err(AcquireError::WriteUnavailable)
        } else if self.data_as_mut().is_none() {
            self.locked.fetch_not(Release);
            Err(AcquireError::ValueNone)
        } else {
            Ok(WomGuard::new(self))
        }
    }

    pub fn clear(&self) -> AcquireResult {
        if self.locked.fetch_not(AcqRel) {
            self.locked.fetch_not(Release);
            Err(AcquireError::WriteUnavailable)
        } else if self.data_as_mut().is_none() {
            self.locked.fetch_not(Release);
            Err(AcquireError::ValueNone)
        } else {
            unsafe { *self.data.get() = None; }
            Ok(())
        }
    }

    pub fn set(&self, data: T) -> AcquireResult<WomGuard<'_, T>> {
        if self.locked.fetch_not(AcqRel) {
            self.locked.fetch_not(Release);
            Err(AcquireError::WriteUnavailable)
        } else if self.data_as_mut().is_none() {
            self.locked.fetch_not(Release);
            Err(AcquireError::ValueNone)
        } else {
            unsafe {
                *self.data.get() = Some(data);
            }
            Ok(WomGuard::new(self))
        }
    }

    pub fn swap(&self, data: &mut T) -> AcquireResult<WomGuard<'_, T>> {
        if self.locked.fetch_not(AcqRel) {
            self.locked.fetch_not(Release);
            Err(AcquireError::WriteUnavailable)
        } else if let Some(cur) = self.data_as_mut() {
            std::mem::swap(data, cur);

            Ok(WomGuard::new(self))
        } else {
            self.locked.fetch_not(Release);
            Err(AcquireError::ValueNone)
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
