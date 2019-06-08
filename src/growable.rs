use std::sync::{Arc, RwLock};
use crate::{traits, slab::{self, Slab}};

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

struct Inner<T, N> {
    slab: Slab<T>,
    new: N,
}

impl<T, N> Pool<T, N>
where
    T: Clear,
    N: FnMut() -> T,
{
    /// Attempt to check out a pooled resource _without_ growing the slab.
    pub fn try_checkout(&self) -> Option<Owned<T>> {
        loop {
            return match self.try_checkout2() {
                Ok(checkout) => checkout
                Err(slab::Error::AtCapacity) => None,
                Err(slab::Error::ShouldRetry) => {
                    atomic::spin_loop_hint();
                    continue;
                }
            }
        }
    }

    fn try_checkout2(&self) -> Result<Owned<T>, slab::Error> {
        let this = self.inner.read().expect("pool poisoned");
        Ok(Owned {
            slot: this.slab.try_checkout()?,
            slab: self.inner.clone(),
        })
    }

    pub fn checkout(&self) -> Owned<T> {
        loop {
            match self.try_checkout2() {
                Ok(checkout) => return checkout,
                Err(slab::Error::AtCapacity) => {
                    let mut this = self.inner.write().expect("pool poisoned");
                    this.slab.grow_with(1, this.new);
                }
                Err(slab::Error::ShouldRetry) => {}
            }

            atomic::spin_loop_hint();
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
        slot.drop_ref(&self.slab);
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
        T: Default,
    {
        self.detach_with(T::default)
    }

    pub fn detach_with(&mut self, new: impl FnOnce() -> T) -> T {
        unsafe { mem::replace(self.slot.as_mut().item_mut(), new()) }
    }
}

// === impl Shared ===

impl<T, N> Shared<T, N> {
    fn new(slot: &ptr::NonNull<slab::Slot<T>>, slab: &Arc<RwLock<Inner<T, N>>>>) -> Self {
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
        slot.drop_ref(&self.slab);
    }
}
