use std::sync::{Arc, Condvar, Mutex};

type NotifyFlagType = u8;

const NOTIFY_FLAG_RELEASED: u32 = NotifyFlagType::BITS - 1;

pub const NOTIFY_TIMEOUT_DEFAULT_MILLIS: u16 = 2000;

struct NotifyState {
    count: u8,
    pending: u8,
    flags: NotifyFlagType,
    timeout: u16,
}

/// A wrapper around [`Condvar`] for [`std::thread`]/atomics that mirrors
/// [`tokio::sync::Notify`](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html).
///
/// Performs handling of a [`Mutex`] for you (which you'd normally need to do yourself
/// with a [`Condvar`]), but does not contain or transport any data. [`Notify`] is used
/// only for syncing across parallel threads.
///
/// While [tokio's notify](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html)
/// will allow you to buffer a single notify at a time, [`alopt`](crate)'s [`Notify`]
/// will buffer up to 255 notifies at a time. A notification is buffered if there are
/// no active listeners when [`Notify::notify_one()`] is called, or the surplus of the
/// count provided to [`Notify::notify_many()`] (if there are 3 listeners and
/// `notify_many()` is called with 5, then 2 get buffered). [`Notify::notify_waiters()`]
/// will clear the buffer.
pub(super) struct Notify {
    state: Mutex<NotifyState>,
    cov: Condvar,
}

impl NotifyState {
    #[inline(always)]
    fn get_released(&self) -> bool {
        self.flags & (1 << NOTIFY_FLAG_RELEASED) > 0
    }

    #[inline(always)]
    fn set_released(&mut self, released: bool) {
        self.flags = (self.flags & !(1 << NOTIFY_FLAG_RELEASED))
            | ((released as u8) << NOTIFY_FLAG_RELEASED);
    }
}

impl Notify {
    pub(super) fn new() -> Arc<Self> {
        Self {
            state: Mutex::new(NotifyState {
                count: 0,
                pending: 0,
                flags: 0,
                timeout: NOTIFY_TIMEOUT_DEFAULT_MILLIS,
            }),
            cov: Condvar::new(),
        }
        .into()
    }

    pub(super) fn with_timeout(timeout_millis: u16) -> Arc<Self> {
        Self {
            state: Mutex::new(NotifyState {
                count: 0,
                pending: 0,
                flags: 0,
                timeout: timeout_millis,
            }),
            cov: Condvar::new(),
        }
        .into()
    }

    pub(super) fn set_timeout(&self, timeout_millis: u16) {
        self.state.lock().unwrap().timeout = timeout_millis;
    }

    pub(super) fn notified(&self) -> Result<(), ()> {
        let mut state = self.state.lock().unwrap();

        if state.pending > 0 {
            state.pending -= 1;
            return Ok(());
        }

        let current = state.count;
        let timeout = state.timeout;
        let (mut state, timeout) = self
            .cov
            .wait_timeout_while(
                state,
                std::time::Duration::from_millis(timeout as u64),
                |s| s.count == current || ((s.pending == 0) ^ s.get_released()),
            )
            .unwrap();

        if timeout.timed_out() {
            return Err(());
        }

        if state.pending > 0 {
            state.pending -= 1;
        }
        Ok(())
    }

    pub(super) fn notify_one(&self) {
        let mut state = self.state.lock().unwrap();
        if state.get_released() {
            state.set_released(false);
        }
        state.count = state.count.wrapping_add(1);
        state.pending += 1;
        self.cov.notify_one();
    }

    pub(super) fn notify_many(&self, count: u8) {
        let mut state = self.state.lock().unwrap();
        if state.get_released() {
            state.set_released(false);
        }
        state.count = state.count.wrapping_add(1);
        state.pending += count;
        for _ in 0..count {
            self.cov.notify_one();
        }
    }

    pub(super) fn notify_waiters(&self) {
        let mut state = self.state.lock().unwrap();
        state.count += 1;
        state.pending = 0;
        state.set_released(true);
        self.cov.notify_all();
    }
}

impl std::fmt::Debug for Notify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notify").finish()
    }
}

unsafe impl Send for Notify {}
unsafe impl Sync for Notify {}

mod tests {
    #[test]
    fn notify() {
        use super::*;

        use std::thread;
        use std::time::Duration;

        // ===== SINGLE BASIC NOTIFY =====

        let notify = Notify::with_timeout(u16::MAX);

        let notify_remote = notify.clone();
        let thandle = thread::spawn(move || {
            notify_remote.notified().unwrap();
        });

        thread::sleep(Duration::from_millis(100));
        notify.notify_one();
        thandle.join().unwrap();

        // ===== SINGLE BUFFERED NOTIFY =====

        let notify = Notify::with_timeout(u16::MAX);

        let notify_remote = notify.clone();
        let thandle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            notify_remote.notified().unwrap();
        });

        notify.notify_one();
        thandle.join().unwrap();

        // ===== 2-WAY NOTIFY SYNC =====

        let notify_send = Notify::with_timeout(u16::MAX);
        let notify_receive = Notify::with_timeout(u16::MAX);

        let notify_send_remote = notify_send.clone();
        let notify_receive_remote = notify_receive.clone();
        let thandle = thread::spawn(move || {
            notify_receive_remote.notify_one();
            notify_send_remote.notified().unwrap();
        });

        notify_receive.notified().unwrap();
        notify_send.notify_one();

        thandle.join().unwrap();

        // ===== NOTIFY TIMEOUT =====

        let notify_send = Notify::with_timeout(10);
        let notify_receive = Notify::with_timeout(u16::MAX);

        let notify_send_remote = notify_send.clone();
        let notify_receive_remote = notify_receive.clone();
        let thandle = thread::spawn(move || {
            notify_receive_remote.notify_one();
            assert!(notify_send_remote.notified().is_err());
        });

        notify_receive.notified().unwrap();
        thread::sleep(Duration::from_millis(100));
        notify_send.notify_one();

        thandle.join().unwrap();

        // ===== NOTIFY COUNT VERIFICATION =====

        let notify = Notify::with_timeout(500);

        let mut handles = Vec::new();
        for _ in 0..200 {
            let notify_remote = notify.clone();
            handles.push(thread::spawn(move || {
                notify_remote.notified().unwrap();
            }));
        }

        thread::sleep(Duration::from_millis(200));

        notify.notify_waiters();

        let count = handles
            .into_iter()
            .map(|handle| handle.join())
            .filter(|handle| handle.is_ok())
            .count();
        assert_eq!(count, 200);

        // ===== NOTIFY COUNT VERIFICATION =====

        let notify = Notify::with_timeout(200);

        let mut handles = Vec::new();
        for _ in 0..200 {
            let notify_remote = notify.clone();
            handles.push(thread::spawn(move || notify_remote.notified().is_ok()));
        }

        notify.notify_many(79);

        thread::sleep(Duration::from_millis(200));

        let count = handles
            .into_iter()
            .map(|handle| handle.join())
            .filter(|handle| handle.as_ref().is_ok_and(|h| *h))
            .count();
        assert_eq!(count, 79);
    }
}
