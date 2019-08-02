pub use self::inner::*;

#[cfg(test)]
mod inner {
    pub use loom::sync::*;
    // TODO: when `loom` supports `RwLock`, fuzz the growable slab implementation.
    pub use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
    pub mod atomic {
        pub use loom::sync::atomic::*;
        pub use std::sync::atomic::Ordering;
        pub use loom::yield_now as spin_loop_hint;
    }
}

#[cfg(not(test))]
mod inner {
    pub use std::sync::{atomic, Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

    // TODO: use causal cells for rawptrs
    use std::cell::UnsafeCell;

    pub struct CausalCell<T>(UnsafeCell<T>);

    impl<T> CausalCell<T> {
        pub fn new(data: T) -> CausalCell<T> {
            CausalCell(UnsafeCell::new(data))
        }

        pub fn with<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*const T) -> R,
        {
            f(self.0.get())
        }

        pub fn with_mut<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*mut T) -> R,
        {
            f(self.0.get())
        }
    }
}
