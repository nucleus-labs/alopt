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

    pub fn clear(self) {
        unsafe {
            *self.worl.data.get() = None;
        }
    }

    pub fn set(&self, data: T) {
        unsafe {
            *self.worl.data.get() = Some(data);
        }
    }

    pub fn swap(&self, data: &mut T) -> bool {
        let current = unsafe { &mut *self.worl.data.get() }.as_mut();
        if let Some(cur) = current {
            std::mem::swap(data, cur);
            true
        } else {
            false
        }
    }
}

impl<'a, T: 'a> WomGuard<'a, T> {
    pub(super) fn new(wom: &'a Wom<T>) -> Self {
        Self { wom }
    }

    pub fn clear(self) {
        unsafe {
            *self.wom.data.get() = None;
        }
    }

    pub fn set(&self, data: T) {
        unsafe {
            *self.wom.data.get() = Some(data);
        }
    }

    pub fn swap(&self, data: &mut T) -> bool {
        let current = self.wom.data_as_mut();
        if let Some(cur) = current {
            std::mem::swap(data, cur);
            true
        } else {
            false
        }
    }
}

impl<'a, T: 'a> std::ops::Deref for WorlGuardRead<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let data_outer = unsafe { &*self.worl.data.get() };
        data_outer.as_ref().unwrap()
    }
}

impl<'a, T: 'a> std::ops::Deref for WorlGuardWrite<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let data_outer = unsafe { &*self.worl.data.get() };
        data_outer.as_ref().unwrap()
    }
}

impl<'a, T: 'a> std::ops::DerefMut for WorlGuardWrite<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let data_outer = unsafe { &mut *self.worl.data.get() };
        data_outer.as_mut().unwrap()
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
        self.wom.data_as_mut().unwrap()
    }
}

impl<'a, T: 'a> std::ops::DerefMut for WomGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.wom.data_as_mut().unwrap()
    }
}

impl<'a, T: 'a> Drop for WomGuard<'a, T> {
    fn drop(&mut self) {
        self.wom.locked.fetch_not(Release);
    }
}
