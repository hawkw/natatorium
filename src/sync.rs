pub use self::inner::*;

#[cfg(test)]
mod inner {
    pub use loom::sync::Arc;
    // TODO: when `loom` supports `RwLock`, fuzz the growable slab implementation.
    pub use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
    pub mod atomic {
        pub use loom::sync::atomic::AtomicUsize;
        pub use std::sync::atomic::{spin_loop_hint, Ordering};
    }
}

#[cfg(not(test))]
mod inner {
    pub use std::sync::{atomic, Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
}
