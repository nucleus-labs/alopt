mod semaphore;

pub mod guard;

use std::sync::atomic::Ordering::*;
use std::cell::UnsafeCell;

use semaphore::{RwSemaphore, AcquireResult, AcquireError};
use guard::{WorlGuardWrite, WorlGuardRead, WomGuard};

type AtomicCounter = std::sync::atomic::AtomicI16;

/// Write, Option, Read, Lock (Worl)
pub struct Worl<T> {
    sem: RwSemaphore,
    data: Option<UnsafeCell<T>>,
}

/// Write, Option, Mutex
pub struct Wom<T> {
    mtx: AtomicCounter,
    data: Option<UnsafeCell<T>>,
}

impl<T> Worl<T> {
    /// Create a new populated instance
    pub fn new(data: T) -> Self {
        Self{
            sem: RwSemaphore::new(),
            data: Some(UnsafeCell::new(data)),
        }
    }

    /// Create a new empty instance
    pub fn empty() -> Self {
        Self{
            sem: RwSemaphore::new(),
            data: None,
        }
    }

    /// Attempt to acquire a read-guard that automatically releases
    /// its lock upon being dropped. 
    pub async fn read<'a>(&'a mut self) -> AcquireResult<WorlGuardRead<'a, T>> {
        if !self.sem.acquire_read().await {
            return Err(AcquireError::Unavailable);
        }

        if self.data.is_none() {
            Err(AcquireError::ValueNone)
        } else {
            Ok(WorlGuardRead::new(self))
        }
    }

    pub async fn write<'a>(&'a mut self) -> AcquireResult<WorlGuardWrite<'a, T>> {
        if !self.sem.acquire_write().await {
            return Err(AcquireError::Unavailable);
        }

        if self.data.is_none() {
            Err(AcquireError::ValueNone)
        } else {
            if !self.sem.acquire_write().await {
                return Err(AcquireError::Unavailable);
            }
            Ok(WorlGuardWrite::new(self))
        }
    }

    pub async fn read_late<'a>(&'a mut self) -> AcquireResult<WorlGuardRead<'a, T>> {
        if !self.sem.acquire_read_wait().await {
            return Err(AcquireError::Unavailable);
        }

        if self.data.is_none() {
            Err(AcquireError::ValueNone)
        } else {
            Ok(WorlGuardRead::new(self))
        }
    }

    pub async fn write_late<'a>(&'a mut self) -> AcquireResult<WorlGuardWrite<'a, T>> {
        if !self.sem.acquire_read_wait().await {
            return Err(AcquireError::Unavailable);
        }

        if self.data.is_none() {
            Err(AcquireError::ValueNone)
        } else {
            if !self.sem.acquire_write_wait().await {
                return Err(AcquireError::Closed);
            }
            Ok(WorlGuardWrite::new(self))
        }
    }

    pub async fn set(&mut self, data: T) -> AcquireResult {
        let acquired = self.sem.acquire_write_wait();
        #[cfg(feature = "tokio")]
        let acquired = acquired.await;
        
        if !acquired {
            return Err(AcquireError::Closed);
        }
        
        self.data = Some(UnsafeCell::new(data));
        self.sem.release_write();
        Ok(())
    }

    pub async fn clear(&mut self) -> AcquireResult {
        let acquired = self.sem.acquire_write_wait();
        #[cfg(feature = "tokio")]
        let acquired = acquired.await;
        
        if !acquired {
            return Err(AcquireError::Closed);
        }

        self.data = None;
        self.sem.release_write();
        Ok(())
    }
}

impl<T> Wom<T> {
    /// Create a new populated instance
    pub fn new(data: T) -> Self {
        Self{
            mtx: AtomicCounter::new(0),
            data: Some(UnsafeCell::new(data)),
        }
    }

    /// Create a new empty instance
    pub fn empty() -> Self {
        Self{
            mtx: AtomicCounter::new(0),
            data: None,
        }
    }

    pub fn get_mut<'a>(&'a self) -> AcquireResult<&'a mut T> {
        if self.mtx.load(Acquire) != 0 {
            Err(AcquireError::Unavailable)
        } else if let Some(data) = self.data.as_ref() {
            unsafe { Ok(&mut *data.get()) }
        } else {
            Err(AcquireError::ValueNone)
        }
    }

    pub fn lock(&self) -> AcquireResult<WomGuard<'_, T>> {
        if self.data.is_none() {
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
