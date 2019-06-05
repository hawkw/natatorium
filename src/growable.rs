use crate::{
    slab::{self, Slab},
    traits::Clear,
};
use owning_ref::OwningHandle;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{
    mem,
    ops::{Deref, DerefMut},
    sync::atomic,
};


#[derive(Debug)]
pub struct Pool<T, F = fn() -> T> {
    slab: RwLock<Slab<T>>,
    new: F,
}

pub struct Owned<'a, T> {
    inner: Option<OwningHandle<RwLockReadGuard<'a, Slab<T>>, RwLockWriteGuard<'a, slab::Slot<T>>>>,
}

pub struct Shared<'a, T> {
    inner: OwningHandle<RwLockReadGuard<'a, Slab<T>>, RwLockReadGuard<'a, slab::Slot<T>>>,
}

impl<T: Default> Default for Pool<T> {
    fn default() -> Self {
        Self::new_from_fn(T::default)
    }
}

impl<T: Default> Pool<T> {
    pub fn new() -> Self {
        Pool::default()
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self::new_from_fn_with_capacity(cap, T::default)
    }
}

impl<T, F> Pool<T, F>
where
    F: Fn() -> T,
{
    pub fn new_from_fn(new: F) -> Self {
        Self::new_from_fn_with_capacity(1, new)
    }

    pub fn new_from_fn_with_capacity(cap: usize, new: F) -> Self {
        Pool {
            slab: RwLock::new(Slab::new_from_fn(cap, &new)),
            new,
        }
    }
}

impl<T, F> Pool<T, F>
where
    F: Fn() -> T,
    T: Clear,
{
    fn try_checkout2<'a>(&'a self) -> Result<Owned<'a, T>, slab::Error> {
        OwningHandle::try_new(self.slab.read(), |slab| {
            let slab = unsafe { &(*slab) };
            slab.try_checkout()
        }).map(|lock| Owned { inner: Some(lock) })
    }

    /// Attempt to check out a pooled resource _without_ growing the slab.
    pub fn try_checkout<'a>(&'a self) -> Option<Owned<'a, T>> {
        loop {
            match self.try_checkout2() {
                Ok(checkout) => return Some(checkout),
                Err(slab::Error::AtCapacity) => return None,
                Err(slab::Error::ShouldRetry) => {}
            }
            atomic::spin_loop_hint();
        }
    }

    pub fn checkout<'a>(&'a self) -> Owned<'a, T> {
        loop {
            match self.try_checkout2() {
                Ok(checkout) => return checkout,
                Err(slab::Error::AtCapacity) => {
                    // We need to grow the slab, and must acquire a write lock.
                    if let Some(mut slab) = self.slab.try_write() {
                        // TODO: grow the slab in chunks to avoid  having to do
                        // this as often.
                        slab.grow_by(1, &self.new);
                    }
                }
                Err(slab::Error::ShouldRetry) => {}
            }

            // If the snapshot got stale, or our attempt to grow the slab
            // failed, spin and retry.
            atomic::spin_loop_hint();
        }
    }
}

// ===== impl Owned =====

impl<'a, T> Deref for Owned<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner
            .as_ref()
            .expect("lock only taken on drop")
            .deref()
            .item()
    }
}

impl<'a, T> DerefMut for Owned<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
            .as_mut()
            .expect("lock only taken on drop")
            .deref_mut()
            .item_mut()
    }
}

impl<'a, T> Owned<'a, T> {
    pub fn downgrade(mut self) -> Shared<'a, T> {
        let lock = self.inner.take().expect("lock only taken on drop");
        let index = lock.index();
        let inner = OwningHandle::new_with_fn(lock.into_owner(), |slab| {
            let slab = unsafe { &(*slab) };
            slab.try_read(index).expect("lock should be released")
        });
        Shared { inner }
    }
}

impl<'a, T> Owned<'a, T>
where
    T: Default,
{
    pub fn detach(mut self) -> T {
        mem::replace(self.deref_mut(), T::default())
    }
}

impl<'a, T> Drop for Owned<'a, T> {
    fn drop(&mut self) {
        if let Some(lock) = self.inner.take() {
            let head = lock.as_owner().head();
            lock.drop_ref(head);
        }
    }
}


// ===== impl Shared =====

impl<'a, T> Deref for Shared<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.deref().item()
    }
}

impl<'a, T> Clone for Shared<'a, T> {
    fn clone(&self) -> Self {
        let index = self.inner.index();

        // Acquire a new outer read lock.
        let lock = RwLockReadGuard::rwlock(self.inner.as_owner())
            .try_read()
            .expect("slab is already read locked");

        // Acquire an additional read lock on the slot.
        let inner = OwningHandle::new_with_fn(lock, |slab| {
            let slab = unsafe { &(*slab) };
            slab.try_read(index).expect("slot is already read locked")
        });
        inner.clone_ref();

        Shared { inner }
    }
}

impl<'a, T> Drop for Shared<'a, T> {
    fn drop(&mut self) {
        let head = self.inner.as_owner().head();
        self.inner.drop_ref(head);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_checkouts_are_empty() {
        let pool: Pool<String> = Pool::with_capacity(3);

        let mut c1 = pool.checkout();
        assert_eq!("", *c1);
        c1.push_str("i'm checkout 1");

        let mut c2 = pool.checkout();
        assert_eq!("", *c2);
        c2.push_str("i'm checkout 2");

        let mut c3 = pool.checkout();
        assert_eq!("", *c3);
        c3.push_str("i'm checkout 3");
    }

    #[test]
    fn capacity_released_when_checkout_is_dropped() {
        let pool: Pool<String> = Pool::with_capacity(1);
        let checkout = pool.checkout();
        assert!(pool.try_checkout().is_none());
        drop(checkout);
        assert!(pool.try_checkout().is_some());
    }

    #[test]
    fn capacity_released_when_all_shared_refs_are_dropped() {
        let pool: Pool<String> = Pool::with_capacity(1);

        let shared1 = pool.checkout().downgrade();
        assert!(pool.try_checkout().is_none());

        let shared2 = shared1.clone();
        assert!(pool.try_checkout().is_none());

        let shared3 = shared2.clone();
        assert!(pool.try_checkout().is_none());

        drop(shared2);
        assert!(pool.try_checkout().is_none());

        drop(shared1);
        assert!(pool.try_checkout().is_none());

        drop(shared3);
        assert!(pool.try_checkout().is_some());
    }

}
