use ringbuffer::{AllocRingBuffer, RingBuffer};

use tokio::sync::{Notify, Mutex};

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::*};
use std::sync::Arc;

use crate::alo::notify::Notify as NotifyBlocking;

const MAX_READS: usize = 128;
const MAX_ACQUIRES: usize = 16;
const ACQUIRE_PADDING_BYTES: usize = 0;

#[derive(Debug)]
pub enum AcquireError {
    ValueNone,
    Unavailable,
    Closed,
}

enum AcquireType {
    Read,
    Write,
}

struct Acquire {
    /// acquire type
    at: AcquireType,
    /// acquire count.
    /// sequential requests can be batched
    ac: usize,
    /// arc notify.
    /// notify listeners that their request has been granted
    an: Arc<Notify>,
    /// acquire padding
    _p: [u8; ACQUIRE_PADDING_BYTES],
}

pub(super) type AcquireResult<T = ()> = Result<T, AcquireError>;
type AcquireList = Mutex<AllocRingBuffer<Acquire>>;

/// A semaphore that allows multiple simultaneous readers but only
/// one active writer. Uses a request buffer so that writers cannot
/// be starved of access by new readers.
/// 
/// Sequential requests of the same type get batched so that they occupy
/// fewer spaces within the buffer. However, this does mean that
/// mixed-type acquisition requests can clog the buffer very quickly.
pub(super) struct RwSemaphore {
    /// reader count
    rc: AtomicUsize,
    /// acquire list.
    /// buffers and batches all acquisition requests so writers cannot be starved
    al: AcquireList,
    /// available notify.
    /// when the buffer is full an error is returned on acquisition request.
    /// if the user truly needs it right now, they can wait to be notified
    /// when there is space available. lack of starvation cannot be guaranteed
    /// with use of this notifier.
    an: Arc<Notify>,
    /// semaphore closed
    sc: AtomicBool,
    /// on-release
    /// a blocking way to subscribe to be notified when a lock is released
    or: Arc<NotifyBlocking>,
}

impl Acquire {
    fn new(at: AcquireType) -> Self {
        Self{
            at,
            ac: 1,
            an: Notify::new().into(),
            _p: [0; ACQUIRE_PADDING_BYTES],
        }
    }

    #[inline(always)]
    fn is_read(&self) -> bool {
        matches!(self.at, AcquireType::Read)
    }

    #[allow(dead_code)]
    #[inline(always)]
    fn is_write(&self) -> bool {
        matches!(self.at, AcquireType::Write)
    }
}

impl RwSemaphore {
    pub(super) fn new() -> Self {
        Self{
            rc: AtomicUsize::new(0),
            al: Mutex::new(AllocRingBuffer::new(MAX_ACQUIRES)),
            an: Notify::new().into(),
            sc: AtomicBool::new(false),
            or: NotifyBlocking::new(),
        }
    }

    /// Checks the status of the semaphore and, if it can, grants permission to
    /// additional readers/writers.
    fn poll(&self, rc: usize) {
        if rc > 0 && rc < MAX_READS {
            #[cfg(feature = "tokio")]
            let mut lock = self.al.blocking_lock();
            #[cfg(not(feature = "tokio"))]
            let mut lock = self.al.lock().unwrap();

            let acquire = lock.front_mut().unwrap();
            
            let permit_count = (MAX_READS - rc).min(acquire.ac);
            acquire.ac -= permit_count;
            self.rc.fetch_add(permit_count, Release);

            for _ in 0..permit_count {
                acquire.an.notify_one();
            }
        } else if rc == 0 {
            #[cfg(feature = "tokio")]
            let mut lock = self.al.blocking_lock();
            #[cfg(not(feature = "tokio"))]
            let mut lock = self.al.lock().unwrap();
            if let Some(acquire) = lock.front_mut() {
                if acquire.ac > 0 {
                    acquire.ac -= 1;
                    acquire.an.notify_one();
                    
                    if acquire.is_read() {
                        self.rc.fetch_add(1, Release);
                        self.poll(1);
                        return;
                    }
                } else { // no more acquisition requests in this batch
                    let _ = lock.dequeue().unwrap();
                    self.an.notify_waiters();
                }
                self.poll(0);
            }
        }
    }
    
    /// Acquires a read-lock. Multiple read-locks may be given at a time.
    /// Returns true if read-lock was acquired, false otherwise.
    pub(super) async fn acquire_read(&self) -> bool {
        if self.sc.load(Acquire) {
            return false;
        }

        let not = {
            let mut lock = self.al.lock().await;
            if lock.is_full() {
                return false;
            }
            
            self.rc.fetch_add(1, Release);
            if let Some(acquire) = lock.back_mut() {
                if acquire.is_read() {
                    acquire.ac += 1;
                    acquire.an.clone()
                } else { // write
                    lock.push(Acquire::new(AcquireType::Read));
                    lock.back().unwrap().an.clone()
                }
            } else { // empty buffer
                lock.push(Acquire::new(AcquireType::Read));
                lock.back().unwrap().an.clone()
            }
        };

        self.poll(self.rc.load(Acquire));

        not.notified().await;

        !self.sc.load(Acquire)
    }

    /// Acquires a write-lock. Only one write-lock may be active at a time,
    /// and not while *any* read-locks are held. Returns true if read-lock
    /// was acquired, false otherwise.
    pub(super) async fn acquire_write(&self) -> bool {
        if self.sc.load(Acquire) {
            return false;
        }

        let not = {
            let mut lock = self.al.lock().await;
            if lock.is_full() {
                return false;
            }

            if let Some(acquire) = lock.back_mut() {
                if acquire.is_read() {
                    lock.push(Acquire::new(AcquireType::Write));
                    lock.back().unwrap().an.clone()
                } else { // write
                    acquire.ac += 1;
                    acquire.an.clone()
                }
            } else {
                lock.push(Acquire::new(AcquireType::Write));
                lock.back().unwrap().an.clone()
            }
        };

        self.poll(self.rc.load(Acquire));

        not.notified().await;

        !self.sc.load(Acquire)
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
    /// of [`Self::acquire_read_wait`] so they may attempt to acquire a space in
    /// the buffer. If they fail again, they just wait for another notification to
    /// try again, unless the failure is because the semaphore is closed.
    pub(super) async fn acquire_read_wait(&self) -> bool {
        while !self.acquire_read().await {
            if self.sc.load(Acquire) {
                return false;
            }
            self.an.notified().await;
        }
        true
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
    /// of [`Self::acquire_write_wait`] so they may attempt to acquire a space in
    /// the buffer. If they fail again, they just wait for another notification to
    /// try again, unless the failure is because the semaphore is closed.
    pub(super) async fn acquire_write_wait(&self) -> bool {
        while !self.acquire_write().await {
            if self.sc.load(Acquire) {
                return false;
            }
            self.an.notified().await;
        }
        true
    }

    /// called to release a read-lock.
    pub(super) fn release_read(&self) {
        if !self.sc.load(Acquire) {
            self.poll(self.rc.fetch_sub(1, Release) - 1);
        }
    }

    /// called to release a write-lock.
    pub(super) fn release_write(&self) {
        if !self.sc.load(Acquire) {
            self.poll(0);
        }
    }

    #[allow(dead_code)]
    pub(super) fn evict(&self) {
        self.close();
        while self.rc.load(Acquire) != 0 {
            self.or.notified();
        }
        self.sc.fetch_not(Release);
    }

    pub(super) fn close(&self) {
        self.sc.fetch_not(Release);
        let lock = self.al.blocking_lock();
        self.an.notify_waiters();
        for acquire in lock.iter() {
            acquire.an.notify_waiters();
        }
    }
}