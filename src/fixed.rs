use crate::{
    slab::{self, Slab},
    traits::Clear,
};

use std::sync::{Arc, atomic};


#[derive(Debug, Clone)]
pub struct Pool<T> {
    slab: Arc<Slab<T>>,
}

impl<T: Default> Default for Pool<T> {
    fn default() -> Self {
        Self::from_fn(T::default)
    }
}

impl<T: Default> Pool<T> {
    pub fn new() -> Self {
        Pool::default()
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self::from_fn_with_capacity(cap, T::default)
    }
}

impl<T> Pool<T> {
    pub fn from_fn(new: impl FnMut() -> T) -> Self {
        Self::from_fn_with_capacity(256, new)
    }

    pub fn from_fn_with_capacity(cap: usize, new: impl FnMut() -> T) -> Self {
        Self {
            slab: Arc::new(Slab::from_fn(cap, new)),
        }
    }
}

impl<T> Pool<T>
where
    T: Clear,
{
    /// Attempt to check out a pooled resource _without_ growing the slab.
    pub fn try_checkout(&self) -> Option<slab::Owned<T>> {
        loop {
            match Slab::try_checkout(&self.slab) {
                Ok(checkout) => return Some(checkout),
                Err(slab::Error::AtCapacity) => return None,
                Err(slab::Error::ShouldRetry) => {}
            }
            atomic::spin_loop_hint();
        }
    }

    pub fn checkout(&self) -> slab::Owned<T> {
        loop {
            match Slab::try_checkout(&self.slab)  {
                Ok(checkout) => return checkout,
                Err(slab::Error::AtCapacity) => {
                    // TODO: Back off, and/or block the thread...
                }
                Err(slab::Error::ShouldRetry) => {}
            }

            // If the snapshot got stale, or our attempt to grow the slab
            // failed, spin and retry.
            atomic::spin_loop_hint();
        }
    }
}
