use crate::{
    traits::Clear,
    slab::{self, Slab},
};
use std::{
    ops::{Deref, DerefMut},
    sync::atomic::{self, AtomicUsize, Ordering},
};

use owning_ref::OwningHandle;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Debug)]
pub struct Pool<T, F = fn() -> T> {
    slab: RwLock<Slab<T>>,
    new: F,
}

pub type Owned<'a, T> = Key<'a, T, RwLockWriteGuard<'a, T>>;
pub type Shared<'a, T> = Key<'a, T, RwLockReadGuard<'a, T>>;

pub struct Key<'a, T, L>
where
    L: Deref,
{
    lock: OwningHandle<RwLockReadGuard<'a, Slab<T>>, L>,
}

impl<T: Default> Default for Pool<T> {
    fn default() -> Self {
        Self::new_with_constructor(T::default)
    }
}

impl<T: Default> Pool<T> {
    pub fn new() -> Self {
        Pool::default()
    }
}

impl<T, F> Pool<T, F>
where
    F: Fn() -> T,
{
    pub fn new_with_constructor(new: F) -> Self {
        Pool {
            slab: RwLock::new(Slab::new()),
            new,
            next_free: AtomicUsize::new(0),
        }
    }
}

impl<T, F> Pool<T, F>
where
    F: Fn() -> T,
    T: Clear,
{
    pub fn checkout<'a>(&'a self) -> Owned<'a, T> {
        loop {
            let checkout = OwningHandle::try_new(self.slab.read(), |slab| {
                (*slab).try_checkout()
            });

            match checkout {
                Ok(lock) => return Owned { lock },
                Err(slab::Error::AtCapacity) =>
                    // We need to grow the slab, and must acquire a write lock.
                    if let Some(mut slab) = self.slab.try_write() {
                        // TODO: grow the slab in chunks to avoid  having to do
                        // this as often.
                        slab.grow_by(1, self.new);
                    },
                Err(slab::Error::ShouldRetry) => {},
            }

            // If the snapshot got stale, or our attempt to grow the slab
            // failed, spin and retry.
            atomic::spin_loop_hint();
        }
    }
}

// ===== impl Key =====

impl<'a, T, L> Deref for Key<'a, T, L>
where
    L: Deref,
{
    type Target = L::Target;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.lock.deref()
    }
}

impl<'a, T, L> DerefMut for Key<'a, T, L>
where
    L: DerefMut,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.lock.deref_mut()
    }
}

impl<'a, T, L> Drop for Key<'a, T, L>
where
    L: Deref,
{
    fn drop(&mut self) {
        // if the key's index doesn't exist in the slab, we're in a really bad
        // state --- this should never happen. but, we'll use `get` to avoid
        // panicking in a drop impl.
        // if let Some(slot) = self.lock.as_owner().get(self.index) {
        //     slot.drop_ref(self.index, self.head);
        // }

        unimplemented!()
    }
}

impl<'a, T> Owned<'a, T> {
    pub fn downgrade(self) -> Shared<'a, T> {
        // let lock = OwningHandle::new_with_fn(self.lock.into_owner(), |slab| {
        //     let slot = unsafe { &(*slab)[index] };
        //     slot.item.try_read().expect("lock should be released")
        // });
        // Shared {
        //     lock,
        //     index: self.index,
        //     head: self.head,
        // }
        unimplemented!()
    }
}

impl<'a, T> Clone for Shared<'a, T> {
    fn clone(&self) -> Self {
        unimplemented!()
    }
}
