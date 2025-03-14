use ringbuffer::{AllocRingBuffer, RingBuffer};

use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{Arc, Mutex};

use crate::alo::notify::{NOTIFY_TIMEOUT_DEFAULT_MILLIS, Notify};

const ACQUIRE_BATCH_SIZE_MAX: u8 = 128;
const ACQUIRE_PADDING_BYTES: usize = 0;
const ACQUIRE_BUFFER_SIZE: usize = 16;
const MAX_READS: u8 = 128;

#[derive(Debug)]
pub enum AcquireError {
    ValueNone,
    ReadUnavailable,
    WriteUnavailable,
    Timeout,
    Poisoned,
    Closed,
}

#[derive(Debug)]
enum AcquireType {
    Read,
    Write,
}

#[derive(Debug)]
struct Acquire {
    /// acquire type
    at: AcquireType,
    /// acquire count.
    /// sequential requests can be batched
    ac: u8,
    /// arc notify.
    /// notify listeners that their request has been granted
    an: Arc<Notify>,
    /// completed flag.
    /// has this acquire completed?
    cf: bool,
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
    rc: Mutex<u8>,
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
    or: Arc<Notify>,
    /// timeout
    to: Mutex<u16>,
}

impl Acquire {
    fn new(at: AcquireType, timeout_millis: u16) -> Self {
        Self {
            at,
            ac: 1,
            an: Notify::with_timeout(timeout_millis),
            cf: false,
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
        Self {
            rc: Mutex::new(0),
            al: Mutex::new(AllocRingBuffer::new(ACQUIRE_BUFFER_SIZE)),
            an: Notify::new(),
            sc: AtomicBool::new(false),
            or: Notify::new(),
            to: Mutex::new(NOTIFY_TIMEOUT_DEFAULT_MILLIS),
        }
    }

    pub(super) fn set_timeout(&self, timeout_millis: u16) {
        *self.to.lock().unwrap() = timeout_millis;
        self.an.set_timeout(timeout_millis);
        self.or.set_timeout(timeout_millis);

        let mut al = self.al.lock().unwrap();
        let al_len = al.len();
        for ac in al.iter_mut() {
            ac.an.set_timeout(timeout_millis);
        }
        assert_eq!(al.len(), al_len);
    }

    /// Checks the status of the semaphore and, if it can, grants permission to
    /// additional readers/writers. Dequeues buffer entries that are marked complete.
    fn poll(&self) {
        let mut rc = self.rc.lock().unwrap();
        if *rc > 0 && *rc < MAX_READS {
            // there are active read-guards and not the maximum amount
            let mut lock = self.al.lock().unwrap();
            let acquire = lock.front_mut().unwrap();

            let permit_count = (MAX_READS - *rc).min(acquire.ac);
            acquire.ac -= permit_count;
            *rc += permit_count;

            acquire.an.notify_many(permit_count);
        } else if *rc == 0 {
            // there are no active read-guards
            let mut lock = self.al.lock().unwrap();
            if let Some(acquire) = lock.front_mut() {
                // there is an acquisition buffer entry
                if acquire.ac > 0 {
                    // the buffer entry has remaining requests
                    acquire.ac -= 1;
                    if acquire.is_read() {
                        *rc += 1;
                    }
                    acquire.an.notify_one();
                } else {
                    acquire.cf = true;
                }
            }
            while lock.peek().is_some_and(|ac| ac.cf) {
                lock.dequeue().unwrap();
            }
        }
    }

    #[inline(always)]
    fn is_closed(&self) -> bool {
        self.sc.load(Acquire)
    }

    /// If a notification times out, it means there was
    fn dequeue(&self, notify: Arc<Notify>) {
        let mut buffer = self.al.lock().unwrap();
        for acquire in buffer.iter_mut() {
            if Arc::ptr_eq(&notify, &acquire.an) {
                acquire.ac -= 1;
                if acquire.ac == 0 {
                    acquire.cf = true;
                }
                break;
            }
        }
    }

    /// Acquires a read-lock. Multiple read-locks may be given at a time.
    ///
    /// **Guaranteed not to starve.**
    pub(super) fn acquire_read(&self) -> AcquireResult {
        if self.is_closed() {
            return Err(AcquireError::Closed);
        }

        let not = {
            let lock = self.al.lock();
            if lock.is_err() {
                return Err(AcquireError::Poisoned);
            }
            let mut buffer = lock.unwrap();

            let buf_full = buffer.is_full();
            if let Some(acquire) = buffer.back_mut() {
                let batch_full = acquire.ac == ACQUIRE_BATCH_SIZE_MAX;
                if acquire.is_read() && !batch_full {
                    acquire.ac += 1;
                    acquire.an.clone()
                } else if !buf_full && (acquire.is_write() || batch_full) {
                    buffer.push(Acquire::new(AcquireType::Read, *self.to.lock().unwrap()));
                    buffer.back().unwrap().an.clone()
                } else {
                    return Err(AcquireError::ReadUnavailable);
                }
            } else {
                // if can't get back then is empty, which means not full
                buffer.push(Acquire::new(AcquireType::Read, *self.to.lock().unwrap()));
                buffer.back().unwrap().an.clone()
            }
        };

        self.poll();

        if not.notified().is_ok() {
            if !self.is_closed() {
                Ok(())
            } else {
                Err(AcquireError::Closed)
            }
        } else {
            self.dequeue(not);
            self.poll();
            Err(AcquireError::Timeout)
        }
    }

    /// Acquires a write-lock. Only one write-lock may be active at a time,
    /// and not while *any* read-locks are held.
    ///
    /// **Guaranteed not to starve.**
    pub(super) fn acquire_write(&self) -> AcquireResult {
        if self.is_closed() {
            return Err(AcquireError::Closed);
        }

        let not = {
            let lock = self.al.lock();
            if lock.is_err() {
                return Err(AcquireError::Poisoned);
            }
            let mut lock = lock.unwrap();

            let buf_full = lock.is_full();
            if let Some(acquire) = lock.back_mut() {
                let batch_full = acquire.ac < ACQUIRE_BATCH_SIZE_MAX;
                if acquire.is_write() && !batch_full {
                    acquire.ac += 1;
                    acquire.an.clone()
                } else if !buf_full && (acquire.is_read() || batch_full) {
                    lock.push(Acquire::new(AcquireType::Write, *self.to.lock().unwrap()));
                    lock.back().unwrap().an.clone()
                } else {
                    return Err(AcquireError::WriteUnavailable);
                }
            } else {
                // if can't get back then is empty, which means not full
                lock.push(Acquire::new(AcquireType::Write, *self.to.lock().unwrap()));
                lock.back().unwrap().an.clone()
            }
        };

        self.poll();

        if not.notified().is_ok() {
            if !self.is_closed() {
                Ok(())
            } else {
                Err(AcquireError::Closed)
            }
        } else {
            self.dequeue(not);
            self.poll();
            Err(AcquireError::Timeout)
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
    pub(super) fn acquire_read_unscheduled(&self) -> AcquireResult {
        let mut request = self.acquire_read();
        while let Err(AcquireError::ReadUnavailable) = request {
            self.an.notified().unwrap();
            request = self.acquire_read();
        }
        request
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
    pub(super) fn acquire_write_unscheduled(&self) -> AcquireResult {
        let mut request = self.acquire_write();
        while let Err(AcquireError::WriteUnavailable) = request {
            self.an.notified().unwrap();
            request = self.acquire_write();
        }
        request
    }

    /// called to release a read-lock.
    pub(super) fn release_read(&self) {
        if !self.is_closed() {
            *self.rc.lock().unwrap() -= 1;
            self.poll();
        }
    }

    /// called to release a write-lock.
    pub(super) fn release_write(&self) {
        if !self.is_closed() {
            self.poll();
        }
    }

    #[allow(dead_code)]
    pub(super) fn evict(&self) {
        self.close();
        while *self.rc.lock().unwrap() != 0 {
            self.or.notified().unwrap();
        }
        self.sc.fetch_not(Release);
    }

    pub(super) fn close(&self) {
        self.sc.fetch_not(Release);
        let lock = self.al.lock().unwrap();
        self.an.notify_waiters();
        for acquire in lock.iter() {
            acquire.an.notify_waiters();
        }
    }
}
