use crate::{
    slab::{self, Slab},
    traits::Clear,
    builder::{Builder, settings},
    sync::{atomic, Arc},
};

use std::{
    mem,
    ops::{Deref, DerefMut},
    ptr,
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

#[derive(Debug, Clone)]
pub struct Settings {
    _p: (),
}

impl<T: Default> Default for Pool<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Default> Pool<T> {
    pub fn new() -> Self {
        Builder::default().fixed().finish()
    }

    pub fn with_capacity(cap: usize) -> Self {
        Builder::default().fixed().with_elements(cap).finish()
    }
}

impl<T> Pool<T> {
    pub fn builder() -> Builder<Settings, T, ()> {
        Builder::new().fixed()
    }

    pub fn size(&self) -> usize {
        self.slab.size()
    }

    pub fn used(&self) -> usize {
        self.slab.used()
    }

    pub fn remaining(&self) -> usize {
        self.slab.remaining()
    }
}

impl<T, N> From<Builder<Settings, T, N>> for Pool<T>
where
    N: FnMut() -> T,
{
    fn from(builder: Builder<Settings, T, N>) -> Self {
        builder.finish()
    }
}

impl<T, N> From<N> for Pool<T>
where
    N: FnMut() -> T,
{
    fn from(new: N) -> Self {
        Self::builder().with_fn(new).fixed().finish()
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

// === impl Settings ===

impl Default for Settings {
    fn default() -> Self {
        Self {
            _p: (),
        }
    }
}

impl<T, N> settings::Make<T, N> for Settings
where
    N: FnMut() -> T,
{
    type Pool = Pool<T>;
    fn make(mut builder: Builder<Self, T, N>) -> Self::Pool {
        Pool {
            slab: Arc::new(builder.slab())
        }
    }
}
