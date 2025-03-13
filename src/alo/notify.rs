
use std::sync::{Condvar, Mutex, Arc};

// type NotifyFlagType = u8;

// const NOTIFY_FLAG_BUFFERED: NotifyFlagType = 1 << (NotifyFlagType::BITS - 1);
// const NOTIFY_FLAG_LEAVE_BUFFERED: NotifyFlagType = 1 << (NotifyFlagType::BITS - 2);

const NOTIFY_TIMEOUT_DEFAULT_MILLIS: u32 = 2000;

struct NotifyState {
    count: u16,
    // flags: NotifyFlagType,
    pending: u16,
}

/// A wrapper around [`Condvar`] for [`std::thread`]/atomics that mirrors
/// [`tokio::sync::Notify`](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html).
/// 
/// Performs handling of a [`Mutex`] for you (which you'd normally need to do yourself
/// with a [`Condvar`]), but does not contain or transport any data. [`Notify`] is used
/// only for syncing across parallel threads.
/// 
/// Like [tokio's notify](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html)'s
/// and unlike [`Condvar`], [`Notify`] will buffer a single call to
/// [`Notify::notify_one()`], so if there are no active listeners when
/// [`Notify::notify_one()`] is called, the next call to [`Notify::notified()`] will
/// immediately return (after the mutex is acquired, without waiting on the [`Condvar`]).
pub(super) struct Notify {
    state: Mutex<NotifyState>,
    cov: Condvar,
    timeout: u32,
}

impl NotifyState {
    // #[inline(always)]
    // fn check_flag(&self, flag: NotifyFlagType) -> bool {
    //     (self.flags & flag) != 0
    // }
    
    // #[inline(always)]
    // fn toggle_flag(&mut self, flag: NotifyFlagType) {
    //     self.flags ^= flag;
    // }
}

impl Notify {
    pub(super) fn new() -> Arc<Self> {
        Self{
            // state: Mutex::new(NotifyState{ count: 0, flags: 0, }),
            state: Mutex::new(NotifyState{ count: 0, pending: 0, }),
            cov: Condvar::new(),
            timeout: NOTIFY_TIMEOUT_DEFAULT_MILLIS,
        }.into()
    }

    #[allow(dead_code)]
    pub(super) fn with_timeout(timeout_millis: u32) -> Arc<Self> {
        Self{
            // state: Mutex::new(NotifyState{ count: 0, flags: 0, }),
            state: Mutex::new(NotifyState{ count: 0, pending: 0, }),
            cov: Condvar::new(),
            timeout: timeout_millis,
        }.into()
    }

    pub(super) fn notified(&self) -> Result<(), ()> {
        let mut state = self.state.lock().unwrap();
        // if state.check_flag(NOTIFY_FLAG_BUFFERED) {
        //     state.toggle_flag(NOTIFY_FLAG_BUFFERED);
        //     return Ok(());
        // }
        if state.pending > 0 {
            state.pending -= 1;
            return Ok(());
        }

        let current = state.count;
        let (mut state, timeout) = self.cov.wait_timeout_while(
            state,
            std::time::Duration::from_millis(self.timeout as u64),
            |s| s.count == current
        ).unwrap();

        if timeout.timed_out() {
            return Err(());
        }

        // if state.check_flag(NOTIFY_FLAG_BUFFERED) && !state.check_flag(NOTIFY_FLAG_LEAVE_BUFFERED) {
        //     state.toggle_flag(NOTIFY_FLAG_BUFFERED);
        // }
        if state.pending > 0 {
            state.pending -= 1;
        }

        Ok(())
    }

    pub(super) fn notify_one(&self) {
        let mut state = self.state.lock().unwrap();
        state.count += 1;
        state.pending = state.pending.saturating_add(1);
        // state.toggle_flag(NOTIFY_FLAG_BUFFERED);
        self.cov.notify_one();
    }

    pub(super) fn notify_waiters(&self) {
        let mut generation = self.state.lock().unwrap();
        generation.count += 1;
        // generation.toggle_flag(NOTIFY_FLAG_BUFFERED);
        // generation.toggle_flag(NOTIFY_FLAG_LEAVE_BUFFERED);
        self.cov.notify_all();
    }
}

impl std::fmt::Debug for Notify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notify").finish()
    }
}

mod tests {
    #[test]
    fn test_notify_one() {
        use super::*;
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Barrier,
        };
        use std::thread;
        use std::time::Duration;

        // This test verifies that a single waiting thread is woken up by notify_one.
        let notify = Notify::new();
        let flag = Arc::new(AtomicBool::new(false));
        // Barrier for two parties: the spawned thread and the main thread.
        let barrier = Arc::new(Barrier::new(2));

        // Spawn a thread that waits on notify and then sets flag.
        let notify_clone = notify.clone();
        let flag_clone = flag.clone();
        let barrier_clone = barrier.clone();
        let handle = thread::spawn(move || {
            // Signal that this thread is about to wait.
            barrier_clone.wait();
            // Wait for the notification.
            notify_clone.notified().expect("notified failed");
            flag_clone.store(true, Ordering::SeqCst);
        });

        // Wait until the spawned thread has started waiting.
        barrier.wait();
        // Small sleep to ensure the spawned thread is blocked in notified().
        thread::sleep(Duration::from_millis(50));

        // Wake one waiter.
        notify.notify_one();

        handle.join().expect("Thread panicked");
        // The flag should now be set.
        assert!(flag.load(Ordering::SeqCst), "notify_one did not wake the waiting thread");
    }

    #[test]
    fn test_notify_all() {
        use super::*;
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Barrier,
        };
        use std::thread;
        use std::time::Duration;

        // This test verifies that notify_waiters wakes all waiting threads.
        const THREAD_COUNT: usize = 50;
        let notify = Arc::new(Notify::new());
        // Use a barrier so that all threads start waiting at roughly the same time.
        let barrier = Arc::new(Barrier::new(THREAD_COUNT + 1));
        let counter = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::with_capacity(THREAD_COUNT);
        for _ in 0..THREAD_COUNT {
            let notify_clone = notify.clone();
            let barrier_clone = barrier.clone();
            let counter_clone = counter.clone();
            handles.push(thread::spawn(move || {
                // Wait until all threads (and the main thread) are ready.
                barrier_clone.wait();
                // Wait for the notification.
                notify_clone.notified().expect("notified failed");
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }));
        }

        // Ensure all threads have reached the barrier (and are waiting on notified()).
        barrier.wait();
        // Optionally, a short sleep can ensure that threads are waiting.
        thread::sleep(Duration::from_millis(50));

        // Wake all waiters.
        notify.notify_waiters();

        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        // Confirm that every waiting thread was woken.
        assert_eq!(
            counter.load(Ordering::SeqCst),
            THREAD_COUNT,
            "not all waiters were notified"
        );
    }
}
