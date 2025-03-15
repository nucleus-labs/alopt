mod semaphore;

pub mod guard;

use std::sync::atomic::Ordering::*;
use std::cell::UnsafeCell;
use std::thread;
use std::hint;

use guard::{WomGuard, WorlGuardRead, WorlGuardWrite};
use semaphore::{AcquireError, AcquireResult, RwSemaphore};

type AtomicCounter = std::sync::atomic::AtomicU8;

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

    /// Set the timeout duration (in milliseconds) for a
    /// lock-acquisition request.
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
        } else if !self.has_contents() {
            self.sem.release_read();
            Err(AcquireError::ValueNone)
        } else {
            Ok(WorlGuardRead::new(self))
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
            } else if let Err(err) = self.sem.acquire_write() {
                Err(err)
            } else {
                Ok(WorlGuardWrite::new(self))
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
        } else if !self.has_contents() {
            self.sem.release_read();
            Err(AcquireError::ValueNone)
        } else {
            Ok(WorlGuardRead::new(self))
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
            } else if let Err(err) = self.sem.acquire_write_unscheduled() {
                Err(err)
            } else {
                Ok(WorlGuardWrite::new(self))
            }
        }
    }

    /// Set the internal [`Option<T>`] value to [`None`]. Requires a write-lock,
    /// but skips the fail-fast read-lock because it doesn't matter what the
    /// value was before. Returns the acquired read-lock guard.
    pub fn clear(&self) -> AcquireResult {
        self.sem.acquire_write_unscheduled()?;

        unsafe {
            *self.data.get() = None;
        }
        self.sem.release_write();
        Ok(())
    }

    /// Set the internal [`Option<T>`] value to [`Some()`]. Requires a write-lock,
    /// but skips the fail-fast read-lock because it doesn't matter what the
    /// value was before. Returns the acquired read-lock guard.
    pub fn set(&self, data: T) -> AcquireResult<WorlGuardWrite<'_, T>> {
        self.sem.acquire_write_unscheduled()?;

        unsafe {
            *self.data.get() = Some(data);
        }
        Ok(WorlGuardWrite::new(self))
    }

    /// Swaps the data at `data` with the data stored internally. **This is done
    /// in-place** with [`std::mem::swap()`]. Because it requires that there was
    /// already data present in the [`Worl`], a fail-fast read-lock is acquired
    /// to read the value of the internal [`Option<T>`]. Then a write-lock is
    /// acquired before swapping the data and returning the acquired write-lock.
    pub fn swap(&self, data: &mut T) -> AcquireResult<WorlGuardWrite<'_, T>> {
        self.sem.acquire_read_unscheduled()?;

        let current = unsafe { &mut *self.data.get() }.as_mut();
        if let Some(cur) = current {
            self.sem.release_read();
            self.sem.acquire_write_unscheduled()?;

            std::mem::swap(data, cur);
            Ok(WorlGuardWrite::new(self))
        } else {
            self.sem.release_read();
            Err(AcquireError::ValueNone)
        }
    }
}

impl<'a, T> Wom<T> {
    const UNLOCKED: u8 = 0;
    const LOCKED: u8 = 1;
    const CONTENDED: u8 = 2;

    /// Create a new populated instance
    pub fn new(data: T) -> Self {
        Self {
            locked: AtomicCounter::new(Self::UNLOCKED),
            data: UnsafeCell::new(Some(data)),
        }
    }

    /// Create a new empty instance
    pub fn empty() -> Self {
        Self {
            locked: AtomicCounter::new(Self::UNLOCKED),
            data: UnsafeCell::new(None),
        }
    }

    fn data_as_mut(&'a self) -> Option<&'a mut T> {
        unsafe { &mut *self.data.get() }.as_mut()
    }

    /// Gets a mutable reference to the contained value, if one exists.
    /// 
    /// Because this borrows [`Wom`] mutably, no checking needs to be
    /// performed. It's statically guaranteed that no locks exist.
    /// 
    #[inline]
    pub fn get_mut(&'a mut self) -> AcquireResult<&'a mut T> {
        self.data.get_mut().as_mut().ok_or(AcquireError::ValueNone)
    }

    fn spin(&self) -> u8 {
        let mut spin = 100;
        loop {
            let state = self.locked.load(Relaxed);
            
            if state != Self::LOCKED || spin == 0 {
                return state;
            }

            hint::spin_loop();
            spin -= 1;
        }
    }

    fn resolve_contention(&self) {
        let mut state = self.spin();

        if state == Self::UNLOCKED {
            match self.locked.compare_exchange(Self::UNLOCKED, Self::LOCKED, Acquire, Relaxed) {
                Ok(_) => return, // Locked!
                Err(s) => state = s,
            }
        }

        loop {
            if state != Self::CONTENDED && self.locked.swap(Self::CONTENDED, AcqRel) == Self::UNLOCKED {
                return;
            }

            unsafe {
                let lock_addr = (&self.locked).as_ptr();
                while *lock_addr == state {
                    // thread::yield_now();
                    thread::sleep(std::time::Duration::from_nanos(0));
                }
            }

            state = self.spin();
        }
    }

    fn release(&self) {
        if self.locked.compare_exchange(Self::LOCKED, Self::UNLOCKED, AcqRel, Relaxed).is_err() {
            let mut state = self.spin();

            if state == Self::CONTENDED {
                match self.locked.compare_exchange(Self::CONTENDED, Self::UNLOCKED, AcqRel, Relaxed) {
                    Ok(_) => return, // Released!
                    Err(s) => state = s,
                }
            }

            loop {
                if state != Self::CONTENDED && self.locked.swap(Self::CONTENDED, Release) == Self::UNLOCKED {
                    return;
                }
    
                unsafe {
                    let lock_addr = (&self.locked).as_ptr();
                    while *lock_addr == state {
                        // thread::yield_now();
                        thread::sleep(std::time::Duration::from_nanos(0));
                    }
                }
    
                state = self.spin();
            }
        }
    }

    pub fn lock(&'a self) -> AcquireResult<WomGuard<'a, T>> {
        if self.locked.compare_exchange(Self::UNLOCKED, Self::LOCKED, Acquire, Relaxed).is_err() {
            self.resolve_contention();
        }

        if self.data_as_mut().is_none() {
            self.release();
            return Err(AcquireError::ValueNone);
        } else {
            Ok(WomGuard::new(self))
        }
    }

    pub fn try_lock(&'a self) -> AcquireResult<WomGuard<'a, T>> {
        if self.locked.compare_exchange(Self::UNLOCKED, Self::LOCKED, Acquire, Relaxed).is_err() {
            Err(AcquireError::WriteUnavailable)
        } else if self.data_as_mut().is_none() {
            Err(AcquireError::ValueNone)
        } else {
            Ok(WomGuard::new(self))
        }
    }

    pub fn clear(&self) -> AcquireResult {
        let _ = self.lock()?;
        unsafe {
            *self.data.get() = None;
        }
        Ok(())
    }

    pub fn set(&'a self, data: T) -> AcquireResult<WomGuard<'a, T>> {
        if self.locked.compare_exchange(Self::UNLOCKED, Self::LOCKED, Acquire, Relaxed).is_err() {
            self.resolve_contention();
        }
        unsafe {
            *self.data.get() = Some(data);
        }
        Ok(WomGuard::new(self))
    }

    pub fn swap(&'a self, data: &mut T) -> AcquireResult<WomGuard<'a, T>> {
        let lock = self.lock()?;
        std::mem::swap(data, self.data_as_mut().unwrap());
        Ok(lock)
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
