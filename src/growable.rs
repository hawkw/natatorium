use std::{
    ptr, sync::{Arc, RwLock, RwLockReadGuard, atomic},
    ops::{Deref, DerefMut}, mem};
use crate::{Clear, slab::{self, Slab}, builder::Builder};

#[derive(Clone)]
pub struct Pool<T, N = fn() -> T> {
    inner: Arc<RwLock<Inner<T, N>>>,
}

pub struct Owned<T, N = fn() -> T> {
    slot: ptr::NonNull<slab::Slot<T>>,
    slab: Arc<RwLock<Inner<T, N>>>,
}

pub struct Shared<T, N = fn() -> T> {
    slot: ptr::NonNull<slab::Slot<T>>,
    slab: Arc<RwLock<Inner<T, N>>>,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub(crate) growth: Growth,
}

#[derive(Debug, Clone)]
pub(crate) enum Growth {
    Double,
    Half,
    Fixed(usize),
}

struct Inner<T, N> {
    slab: Slab<T>,
    new: N,
    settings: Settings,
}


impl<T, N> Pool<T, N> {
    fn read<'a>(&'a self) -> RwLockReadGuard<'a, Inner<T, N>> {
        self.inner.read().expect("pool poisoned")
    }

    pub fn size(&self) -> usize {
        self.read().slab.size()
    }
}

impl<T, N> Pool<T, N>
where
    T: Clear,
    N: FnMut() -> T,
{
    /// Attempt to check out a pooled resource _without_ growing the slab.
    pub fn try_checkout(&self) -> Option<Owned<T, N>> {
        loop {
            return match self.try_checkout2() {
                Ok(checkout) => Some(checkout),
                Err(slab::Error::AtCapacity) => None,
                Err(slab::Error::ShouldRetry) => {
                    atomic::spin_loop_hint();
                    continue;
                }
            }
        }
    }

    fn try_checkout2(&self) -> Result<Owned<T, N>, slab::Error> {
        let slot = self
            .inner
            .read()
            .expect("pool poisoned")
            .slab
            .try_checkout()?;
        Ok(Owned {
            slot,
            slab: self.inner.clone(),
        })
    }

    pub fn checkout(&self) -> Owned<T, N> {
        loop {
            match self.try_checkout2() {
                Ok(checkout) => return checkout,
                Err(slab::Error::AtCapacity) => self
                    .inner
                    .write()
                    .expect("pool poisoned")
                    .grow(),
                Err(slab::Error::ShouldRetry) => {}
            }

            atomic::spin_loop_hint();
        }
    }
}

impl<T, N> From<Builder<Settings, T, N>> for Pool<T, N>
where
    N: FnMut() -> T,
{
    fn from(builder: Builder<Settings, T, N>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                slab: builder.slab(),
                new: builder.new,
                settings: builder.settings,
            }))
        }
    }
}

// == impl Owned ===

impl<T, N> Deref for Owned<T, N> {
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

impl<T, N> DerefMut for Owned<T, N> {

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

impl<T, N> Drop for Owned<T, N> {
    fn drop(&mut self) {
        let slot = unsafe { self.slot.as_ref() };
        // if the pool is poisoned, it'll be destroyed anyway, so don't
        // double panic!
        if let Ok(inner) = self.slab.read() {
            slot.drop_ref(&inner.slab);
        }
    }
}

impl<T, N> Owned<T, N> {
    pub fn downgrade(self) -> Shared<T, N> {
        // TODO: cloning the slot and slab will cause two ref-count bumps (one
        // for the slot's ref count, and one for the Arc), but we can't move out
        // of `self` since `Owned` implements `Drop`. This may not be a big deal
        // but it would be nice to fix.
        Shared::new(&self.slot, &self.slab)
    }

    pub fn detach(&mut self) -> T
    where
        N: FnMut() -> T,
    {
        let mut lock = self.slab.write().expect("pool poisoned");
        let mut new = lock.new;
        let slot = unsafe { self.slot.as_mut() }.item_mut();
        mem::replace(slot, new())
    }

}

// === impl Shared ===

impl<T, N> Shared<T, N> {
    fn new(slot: &ptr::NonNull<slab::Slot<T>>, slab: &Arc<RwLock<Inner<T, N>>>) -> Self {
        unsafe {
            slot.as_ref().clone_ref();
        }
        Self {
            slot: slot.clone(),
            slab: slab.clone(),
        }
    }

    pub fn try_upgrade(self) -> Result<Owned<T, N>, Self> {
        unimplemented!()
    }
}

impl<T, N> Clone for Shared<T, N> {
    fn clone(&self) -> Self {
        Self::new(&self.slot, &self.slab)
    }
}

impl<T, N> Deref for Shared<T, N> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // A `Shared` checkout implies that the slot may not be mutated.
            self.slot.as_ref().item()
        }
    }
}

impl<T, N> Drop for Shared<T, N> {
    fn drop(&mut self) {
        let slot = unsafe { self.slot.as_ref() };
        // if the pool is poisoned, it'll be destroyed anyway, so don't
        // double panic!
        if let Ok(inner) = self.slab.read() {
            slot.drop_ref(&inner.slab);
        }
    }
}

// === impl Settings ===

impl Default for Settings {
    fn default() -> Self {
        Settings {
            growth: Growth::Double,
        }
    }
}


// === impl Inner ===

impl<T, N> Inner<T, N>
where
    N: FnMut() -> T,
{
    fn grow(&mut self) {
        let amt = match self.settings.growth {
            Growth::Double => self.slab.size(),
            Growth::Half => self.slab.size() / 2,
            Growth::Fixed(amt) => amt,
        };
        self.slab.grow_by(amt, &mut self.new);
    }
}

