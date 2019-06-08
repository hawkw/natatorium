use crate::{
    slab::{self, Slab},
    traits::Clear,
};

use std::{
    mem,
    ops::{Deref, DerefMut},
    ptr,
    sync::{atomic, Arc},
};


#[derive(Debug, Clone)]
pub struct Pool<T> {
    slab: Arc<Slab<T>>,
}


pub struct Owned<T> {
    slot: ptr::NonNull<slab::Slot<T>>,
    slab: Arc<Slab<T>>,
}

pub struct Shared<T> {
    slot: ptr::NonNull<slab::Slot<T>>,
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
    pub fn try_checkout(&self) -> Option<Owned<T>> {
        loop {
            match self.slab.try_checkout() {
                Ok(slot) => {
                    return Some(Owned {
                        slot,
                        slab: self.slab.clone(),
                    })
                }
                Err(slab::Error::AtCapacity) => return None,
                Err(slab::Error::ShouldRetry) => {}
            }
            atomic::spin_loop_hint();
        }
    }

    pub fn checkout(&self) -> Owned<T> {
        loop {
            match self.slab.try_checkout() {
                Ok(slot) => {
                    return Owned {
                        slot,
                        slab: self.slab.clone(),
                    }
                }
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

// == impl Owned ===

impl<T> Deref for Owned<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // An `Owned` checkout requires that we have unique access to this
            // slot.
            self.slot.as_ref().item()
        }
    }
}

impl<T> DerefMut for Owned<T> {

    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // An `Owned` checkout requires that we have unique access to this
            // slot, and an `&mut Owned` ensures the slot cannot be mutably
            // dereferenced with a shared ref to the owned checkout.
            self.slot.as_mut().item_mut()
        }
    }
}

impl<T> Drop for Owned<T> {
    fn drop(&mut self) {
        let slot = unsafe { self.slot.as_ref() };
        slot.drop_ref(&self.slab);
    }
}

impl<T> Owned<T> {
    pub fn downgrade(self) -> Shared<T> {
        // TODO: cloning the slot and slab will cause two ref-count bumps (one
        // for the slot's ref count, and one for the Arc), but we can't move out
        // of `self` since `Owned` implements `Drop`. This may not be a big deal
        // but it would be nice to fix.
        Shared::new(&self.slot, &self.slab)
    }

    pub fn detach(&mut self) -> T
    where
        T: Default,
    {
        self.detach_with(T::default)
    }

    pub fn detach_with(&mut self, new: impl FnOnce() -> T) -> T {
        unsafe { mem::replace(self.slot.as_mut().item_mut(), new()) }
    }
}

// === impl Shared ===

impl<T> Shared<T> {
    fn new(slot: &ptr::NonNull<slab::Slot<T>>, slab: &Arc<Slab<T>>) -> Self {
        unsafe {
            slot.as_ref().clone_ref();
        }
        Self {
            slot: slot.clone(),
            slab: slab.clone(),
        }
    }

    pub fn try_upgrade(self) -> Result<Owned<T>, Self> {
        unimplemented!()
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self::new(&self.slot, &self.slab)
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // A `Shared` checkout implies that the slot may not be mutated.
            self.slot.as_ref().item()
        }
    }
}

impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        let slot = unsafe { self.slot.as_ref() };
        slot.drop_ref(&self.slab);
    }
}
