//! Re-exports either the Rust `std` library or `core` and `alloc` when `std` is
//! disabled.
//!
//! `crate::stdlib::...` should be used rather than `std::` when adding code that
//! will be available with the standard library disabled.
//!
//! Note that this module is called `stdlib` rather than `std`, as Rust 1.34.0
//! does not permit redefining the name `stdlib` (although this works on the
//! latest stable Rust).
#[cfg(feature = "std")]
pub(crate) use std::*;

#[cfg(not(feature = "std"))]
pub(crate) use self::no_std::*;

#[cfg(not(feature = "std"))]
mod no_std {
    // We pre-emptively export everything from libcore/liballoc, (even modules
    // we aren't using currently) to make adding new code easier. Therefore,
    // some of these imports will be unused.
    #![allow(unused_imports)]

    pub(crate) use core::{
        any, array, ascii, cell, char, clone, cmp, convert, default, f32, f64, ffi, future, hash,
        hint, i128, i16, i8, isize, iter, marker, mem, num, ops, option, pin, ptr, result, task,
        time, u128, u16, u32, u8, usize,
    };

    pub(crate) use alloc::{boxed, collections, rc, string, vec};

    pub(crate) mod borrow {
        pub(crate) use alloc::borrow::*;
        pub(crate) use core::borrow::*;
    }

    pub(crate) mod fmt {
        pub(crate) use alloc::fmt::*;
        pub(crate) use core::fmt::*;
    }

    pub(crate) mod slice {
        pub(crate) use alloc::slice::*;
        pub(crate) use core::slice::*;
    }

    pub(crate) mod str {
        pub(crate) use alloc::str::*;
        pub(crate) use core::str::*;
    }
}

#[cfg(test)]
pub(crate) mod sync {
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
pub(crate) mod sync {
    #[cfg(not(feature = "std"))]
    pub(crate) mod sync {
        pub(crate) use alloc::sync::*;
        pub(crate) use core::sync::*;
    }

    #[cfg(feature = "std")]
    pub(crate) use std::sync::*;

    // TODO: use causal cells for rawptrs;
    #[cfg(feature = "std")]
    use std::cell::UnsafeCell;
    #[cfg(not(feature = "std"))]
    use core::cell::UnsafeCell;

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
