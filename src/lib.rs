#![deny(rust_2018_idioms)]

pub(crate) mod builder;
pub mod fixed;
pub mod growable;

pub(crate) mod slab;
pub mod traits;
pub use {builder::Builder, traits::Clear};
