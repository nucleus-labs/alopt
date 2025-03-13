
use super::RefCellOpt;

pub struct RefGuardRead<'a, T> {
    cell: &'a RefCellOpt<T>,
    ptr: *const T,
}

pub struct RefGuardWrite<'a, T> {
    cell: &'a RefCellOpt<T>,
    ptr: *mut T,
}

impl<'a, T> RefGuardRead<'a, T> {
    pub(super) fn new(sem: &'a RefCellOpt<T>, ptr: *const T) -> Self {
        Self{ cell: sem, ptr }
    }
}

impl<'a, T> RefGuardWrite<'a, T> {
    pub(super) fn new(sem: &'a RefCellOpt<T>, ptr: *mut T) -> Self {
        Self{ cell: sem, ptr }
    }
}

impl<'a, T> std::ops::Deref for RefGuardRead<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl<'a, T> std::ops::Deref for RefGuardWrite<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl<'a, T> Drop for RefGuardRead<'a, T> {
    fn drop(&mut self) {
        self.cell.release();
    }
}

impl<'a, T> Drop for RefGuardWrite<'a, T> {
    fn drop(&mut self) {
        self.cell.release_mut();
    }
}
