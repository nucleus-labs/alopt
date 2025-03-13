use super::{Wom, Worl};

use std::sync::atomic::Ordering::*;

pub struct WorlGuardRead<'a, T: 'a> {
    worl: &'a Worl<T>,
}

pub struct WorlGuardWrite<'a, T: 'a> {
    worl: &'a Worl<T>,
}

pub struct WomGuard<'a, T: 'a> {
    wom: &'a Wom<T>,
}

impl<'a, T: 'a> WorlGuardRead<'a, T> {
    pub(super) fn new(worl: &'a Worl<T>) -> Self {
        Self { worl }
    }
}

impl<'a, T: 'a> WorlGuardWrite<'a, T> {
    pub(super) fn new(worl: &'a Worl<T>) -> Self {
        Self { worl }
    }
}

impl<'a, T: 'a> WomGuard<'a, T> {
    pub(super) fn new(wom: &'a Wom<T>) -> Self {
        Self { wom }
    }
}

impl<'a, T: 'a> std::ops::Deref for WorlGuardRead<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.worl.data.as_ref().unwrap().get() }
    }
}

impl<'a, T: 'a> std::ops::Deref for WorlGuardWrite<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.worl.data.as_ref().unwrap().get() }
    }
}

impl<'a, T: 'a> std::ops::DerefMut for WorlGuardWrite<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.worl.data.as_ref().unwrap().get() }
    }
}

impl<'a, T: 'a> Drop for WorlGuardRead<'a, T> {
    fn drop(&mut self) {
        self.worl.sem.release_read();
    }
}

impl<'a, T: 'a> Drop for WorlGuardWrite<'a, T> {
    fn drop(&mut self) {
        self.worl.sem.release_write();
    }
}

impl<'a, T: 'a> std::ops::Deref for WomGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        if self.wom.mtx.fetch_add(1, Release) < 0 {
            panic!(
                "cannot access an immutable reference of Wom while there is an active mutable reference!"
            );
        }
        unsafe { &*self.wom.data.as_ref().unwrap().get() }
    }
}

impl<'a, T: 'a> std::ops::DerefMut for WomGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let accesses = self.wom.mtx.fetch_sub(1, Release);
        if accesses < 0 {
            panic!("cannot access a mutable reference of Wom more than once at a time!");
        } else if accesses > 0 {
            panic!(
                "cannot access a mutable reference of Wom while there are active immutable references!"
            );
        }
        unsafe { &mut *self.wom.data.as_ref().unwrap().get() }
    }
}

impl<'a, T: 'a> Drop for WomGuard<'a, T> {
    fn drop(&mut self) {
        if self.wom.mtx.load(Acquire) < 0 {
            self.wom.mtx.fetch_add(1, Release);
        } else {
            self.wom.mtx.fetch_sub(1, Release);
        }
    }
}
