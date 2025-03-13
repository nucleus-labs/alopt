use ringbuffer::{AllocRingBuffer, RingBuffer};

use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::alo::notify::Notify;

const MAX_READS: usize = 128;
const MAX_ACQUIRES: usize = 16;
const ACQUIRE_PADDING_BYTES: usize = 0;

#[derive(Debug)]
pub enum AcquireError {
    ValueNone,
    ReadUnavailable,
    WriteUnavailable,
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
    rc: Mutex<usize>,
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
            rc: Mutex::new(0),
            al: Mutex::new(AllocRingBuffer::new(MAX_ACQUIRES)),
            an: Notify::new().into(),
            sc: AtomicBool::new(false),
            or: Notify::new(),
        }
    }

    fn poll_inner(&self, rc: &mut MutexGuard<usize>) -> Option<usize> {
        if **rc > 0 && **rc < MAX_READS { // there are active read-guards and not the maximum amount
            let mut lock = self.al.lock().unwrap();
            let acquire = lock.front_mut().unwrap();
            
            let permit_count = (MAX_READS - **rc).min(acquire.ac);
            acquire.ac -= permit_count;
            **rc += permit_count;

            for _ in 0..permit_count {
                acquire.an.notify_one();
            }
        } else if **rc == 0 { // there are no active read-guards
            let mut lock = self.al.lock().unwrap();
            if let Some(acquire) = lock.front_mut() { // there is an acquisition buffer entry
                if acquire.ac > 0 { // the buffer entry has remaining requests
                    acquire.ac -= 1;
                    
                    if acquire.is_read() {
                        **rc += 1;
                        acquire.an.notify_one();
                        // return Some(1);
                    } else {
                        acquire.an.notify_one();
                    }
                } else { // no more acquisition requests in this batch
                    let _ = lock.dequeue().unwrap();
                    // println!("popping read/write");
                    self.an.notify_waiters();
                }
            }
        }
        None
    }

    /// Checks the status of the semaphore and, if it can, grants permission to
    /// additional readers/writers.
    fn poll(&self) {
        let mut rc = self.rc.lock().unwrap();
        if let Some(_) = self.poll_inner(&mut rc) {
            let _ = self.poll_inner(&mut rc);
        }
    }

    #[inline(always)]
    fn is_closed(&self) -> bool {
        self.sc.load(Acquire)
    }

    #[inline]
    fn is_writing(&self) -> bool {
        self.al.lock().is_ok_and(|mtx| {
            // println!("checking acquire is write");
            mtx.front().is_some_and(|ac| ac.is_write())
        })
    }

    #[inline(always)]
    fn is_reading(&self) -> bool {
        *self.rc.lock().unwrap() > 0
    }
    
    /// Acquires a read-lock. Multiple read-locks may be given at a time.
    /// Returns true if read-lock was acquired, false otherwise.
    pub(super) fn acquire_read(&self) -> bool {
        if self.is_closed() || self.is_writing() {
            // println!("read rejected!");
            return false;
        }

        let not = {
            let lock = self.al.lock();
            if lock.is_err() {
                return false;
            }
            let mut lock = lock.unwrap();
            
            let is_full = lock.is_full();
            if let Some(acquire) = lock.back_mut() {
                if acquire.is_read() && !is_full {
                    acquire.ac += 1;
                    acquire.an.clone()
                } else if acquire.is_write() { // write
                    // println!("pushing read");
                    lock.push(Acquire::new(AcquireType::Read));
                    lock.back().unwrap().an.clone()
                } else {
                    return false;
                }
            } else { // if can't get back then is empty, which means not full
                // println!("pushing read");
                lock.push(Acquire::new(AcquireType::Read));
                lock.back().unwrap().an.clone()
            }
        };

        self.poll();

        if let Err(_) = not.notified() {
            for ac in self.al.lock().unwrap().iter() {
                println!("=============");
                println!("{ac:?}");
            }
            println!("=============");
            panic!("notify timed out",)
        }

        !self.is_closed()
    }

    /// Acquires a write-lock. Only one write-lock may be active at a time,
    /// and not while *any* read-locks are held. Returns true if read-lock
    /// was acquired, false otherwise.
    pub(super) fn acquire_write(&self) -> bool {
        if self.is_closed() || self.is_reading() {
            return false;
        }

        let not = {
            let lock = self.al.lock();
            if lock.is_err() {
                return false;
            }
            let mut lock = lock.unwrap();

            let is_full = lock.is_full();
            if let Some(acquire) = lock.back_mut() {
                if acquire.is_read() && !is_full {
                    // println!("pushing write");
                    lock.push(Acquire::new(AcquireType::Write));
                    lock.back().unwrap().an.clone()
                } else if acquire.is_write() {
                    acquire.ac += 1;
                    acquire.an.clone()
                } else {
                    return false;
                }
            } else { // if can't get back then is empty, which means not full
                // println!("pushing write");
                lock.push(Acquire::new(AcquireType::Write));
                lock.back().unwrap().an.clone()
            }
        };

        self.poll();

        not.notified().expect("Notify timed out");

        !self.is_closed()
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
    pub(super) fn acquire_read_wait(&self) -> bool {
        while !self.acquire_read() {
            if self.is_closed() {
                return false;
            }
            self.an.notified().unwrap();
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
    pub(super) fn acquire_write_wait(&self) -> bool {
        while !self.acquire_write() {
            if self.is_closed() {
                return false;
            }
            self.an.notified().unwrap();
        }
        true
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
