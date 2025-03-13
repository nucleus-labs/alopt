mod guard;

use std::cell::{UnsafeCell, Cell, RefCell};

pub use guard::{RefGuardRead, RefGuardWrite};

pub struct RefCellOpt<T> {
    refs: Cell<i32>,
    data: Option<UnsafeCell<T>>,
}

impl<T> RefCellOpt<T> {
    /// Create a new populated instance
    pub fn new(data: T) -> Self {
        Self{
            refs: Cell::new(0),
            data: Some(UnsafeCell::new(data)),
        }
    }

    /// Create a new empty instance
    pub fn empty() -> Self {
        Self{
            refs: Cell::new(0),
            data: None,
        }
    }

    pub fn get(&self) -> Option<RefGuardRead<T>> {
        todo!()
    }

    pub fn get_mut(&self) -> Option<RefGuardWrite<T>> {
        todo!()
    }

    fn release(&self) {
        todo!()
    }

    fn release_mut(&self) {
        todo!()
    }
}
