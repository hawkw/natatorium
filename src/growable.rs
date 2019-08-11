use crate::{
    builder::{settings, Builder},
    slab::{self, Slab},
    Clear,
};
use std::{
    mem,
    ops::{Deref, DerefMut},
    sync::{atomic, Arc},
    ptr,
    fmt,
};

#[derive(Clone)]
pub struct Pool<T, N = fn() -> T> {
    inner: Arc<Inner<T, N>>,
}

/// A uniquely owned checkout of an object in a [growable pool].
///
/// An `Owned` checkout allows mutable access to the pooled object, but cannot
/// be cloned. It may, however, be [downgraded] to a [`Shared`] checkout that
/// allows shared, immutable acccess.
///
/// When an `Owned` checkout is dropped, the underlying object is cleared and
/// released back to the pool.
///
/// [growable pool]: ../struct.Pool.html
/// [downgraded]: #method.downgrade
/// [`Shared`]: ../struct.Shared.html
pub struct Owned<T, N = fn() -> T> {
    item: ptr::NonNull<T>,
    idx: usize,
    slab: Arc<Inner<T, N>>,
}

/// A shared, atomically reference-counted checkout of an object in a [growable pool].
///
/// A `Shared` checkout allows shared access to the pooled object for an arbitrary
/// lifetime, but may not mutate it. If it is the only shared checkout of the
/// object, it may be [upgraded] back into an [`Iwned`] checkout that allows
/// exclusive, mutable acccess.
///
/// When a `Shared` checkout is cloned, the shared count of the pooled object is
/// increased by one, and when it is dropped, the shared count is decreased.
/// If the shared count is 0, the underlying object is cleared and released back
/// to the pool.
///
/// [growable pool]: ../struct.Pool.html
/// [upgraded]: #method.try_upgrade
/// [`Owned`]: ../struct.Owned.html
pub struct Shared<T, N = fn() -> T> {
    item: ptr::NonNull<T>,
    idx: usize,
    slab: Arc<Inner<T, N>>,
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
    slab: Slab<T, slab::List<slab::Slot<T>>>,
    new: N,
}

// === impl Pool ===

impl<T> Pool<T>
where
    T: Default,
{
    pub fn new() -> Self {
        Pool::builder().with_default().with_elements(0).finish()
    }

    pub fn with_capacity(cap: usize) -> Self {
        Pool::builder().with_default().with_elements(cap).finish()
    }
}

impl<T> Pool<T, ()> {
    pub fn builder() -> Builder<Settings, T, ()> {
        Builder::new().growable()
    }
}

impl<T, N> Pool<T, N> {
    pub fn size(&self) -> usize {
        self.inner.slab.size()
    }

    pub fn used(&self) -> usize {
        self.inner.slab.used()
    }

    pub fn remaining(&self) -> usize {
        self.inner.slab.remaining()
    }
}

impl<T, N> Pool<T, N>
where
    T: Clear,
    N: Fn() -> T,
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
            };
        }
    }

    fn try_checkout2(&self) -> Result<Owned<T, N>, slab::Error> {
        let slot = self
            .inner
            .slab
            .try_checkout()?;
        let slot = unsafe { slot.as_ref() };
        let checkout = Owned {
            idx: slot.index(),
            item: slot.item_ptr(),
            slab: self.inner.clone(),
        };
        #[cfg(debug_assertions)]
        {
            // checkout.assert_valid();
            self.inner.assert_valid();
        };
        Ok(checkout)
    }

    pub fn checkout(&self) -> Owned<T, N> {
        loop {
            let ch = self.try_checkout2();
            // println!("checkout -> {:?}", ch);
            match ch {
                Ok(checkout) => return checkout,
                Err(slab::Error::AtCapacity) => self.inner.grow(),
                Err(slab::Error::ShouldRetry) => {}
            }

            atomic::spin_loop_hint();
        }
    }
}

impl<T, N> From<Builder<Settings, T, N>> for Pool<T, N>
where
    N: Fn() -> T,
{
    fn from(builder: Builder<Settings, T, N>) -> Self {
        builder.finish()
    }
}

impl<T, N> From<N> for Pool<T, N>
where
    N: Fn() -> T,
{
    fn from(new: N) -> Self {
        Builder::new().growable().with_fn(new).finish()
    }
}

impl<T> Default for Pool<T>
where
    T: Default,
{
    fn default() -> Self {
        Builder::new().with_default().growable().finish()
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
            self.item.as_ref()
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
            self.item.as_mut()
        }
    }
}

impl<T, N> Drop for Owned<T, N> {
    fn drop(&mut self) {
        self.slab.with_slot(self.idx, |s| s.drop_ref(&self.slab.slab));
    }
}

impl<T, N> Owned<T, N> {
    pub fn downgrade(self) -> Shared<T, N> {
        // TODO: cloning the slot and slab will cause two ref-count bumps (one
        // for the slot's ref count, and one for the Arc), but we can't move out
        // of `self` since `Owned` implements `Drop`. This may not be a big deal
        // but it would be nice to fix.
        Shared::new(self.item, self.idx, self.slab.clone())
    }

    pub fn detach(&mut self) -> T
    where
        N: Fn() -> T,
    {
        let new = &self.slab.new;
        let slot = unsafe { self.item.as_mut() };
        mem::replace(slot, new())
    }

    /// Asserts that the invariants enforced by the pool are currently valid for
    /// this `Owned` reference.
    pub fn assert_valid(&self) {
        let refs = self.slab.slab.with_slot(self.idx, |slot| slot.ref_count(atomic::Ordering::SeqCst))
        .unwrap_or_else(|| panic!("invariant violated: checkout referenced slot {:?} which did not exist", self.idx));
        assert_eq!(
            refs, 1,
            "invariant violated: owned checkout must have exactly one reference"
        );
    }
}

// === impl Shared ===

impl<T, N> Shared<T, N> {
    fn new(item: ptr::NonNull<T>, idx: usize, slab: Arc<Inner<T, N>>) -> Self {
        slab.slab.with_slot(idx, |slot| slot.clone_ref());
        Self {
            item,
            slab,
            idx,
        }
    }

    pub fn try_upgrade(self) -> Result<Owned<T, N>, Self> {
        unimplemented!()
    }
}

impl<T, N> Clone for Shared<T, N> {
    fn clone(&self) -> Self {
        Self::new(self.item, self.idx, self.slab.clone())
    }
}

impl<T, N> Deref for Shared<T, N> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // A `Shared` checkout implies that the slot may not be mutated.
            self.item.as_ref()
        }
    }
}

impl<T, N> Drop for Shared<T, N> {
    fn drop(&mut self) {
        self.slab.slab.with_slot(self.idx, |slot| slot.drop_ref(&self.slab.slab));
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

impl<T, N> settings::Make<T, N> for Settings
where
    N: Fn() -> T,
{
    type Pool = Pool<T, N>;
    fn make(mut builder: Builder<Self, T, N>) -> Self::Pool {
        let capacity = builder.capacity;
        let mut new = builder.new;
        let list = if capacity > 0 {
            let mut i = 0;
            slab::List::from_fn_with_capacity(capacity, || {
                let slot = slab::Slot::new(new(), i);
                i += 1;
                slot
            })
        } else {
            slab::List::new()
        };
        Pool {
            inner: Arc::new(Inner {
                slab: slab::Slab::new(list),
                new,
            }),
        }
    }
}

// === impl Inner ===

impl<T, N> Inner<T, N>
where
    N: Fn() -> T,
{
    fn grow(&self) {
        self.slab.extend_with(&self.new);
    }
}

impl<T, N> Inner<T, N> {
    fn assert_valid(&self) {
        self.slab.assert_valid();
    }

    fn with_slot<O>(&self, idx: usize, f: impl FnOnce(&slab::Slot<T>) -> O) -> Option<O> {
        self.slab.with_slot(idx, f)
    }
}


impl<T, N> fmt::Debug for Owned<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Owned").field("item", &self.item).field("idx", &self.idx).field("inner", &format_args!("<inner>")).finish()
    }
}
