use std::sync::atomic::{self, AtomicUsize, Ordering};

use owning_ref::OwningHandle;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use ::traits::Clear;

#[derive(Debug)]
pub struct Slab<T> {
    inner: Vec<Slot<T>>,
    head: AtomicUsize,
}

#[derive(Debug)]
pub struct Slot<T> {
    item: RwLock<T>,
    idx: usize,
    ref_count: AtomicUsize,
    next: AtomicUsize,
}

pub enum Error {
    AtCapacity,
    ShouldRetry,
}

// ===== impl Slot =====

impl<T> Slab<T> {
    pub fn new() -> Self {
        Slab {
            inner: Vec::new(),
            head: AtomicUsize::from(0),
        }
    }

    pub fn new_with_capacity(cap: usize) -> Self
    where
        T: Default,
    {
        Self::new_from_fn(cap, T::default)
    }

    pub fn new_from_fn<F>(cap: usize, new: F) -> Self
    where
        F: FnMut() -> T,
    {
        let mut this = Self::new();
        this.grow_by(cap, new);
        this
    }

    pub fn grow_by<F>(&mut self, cap: usize, new: F) -> usize
    where
        F: FnMut() -> T,
    {
        let next = self.inner.len();
        for i in next..self.inner.len() {
            self.inner.push(Slot::new(new(), i));
        }
        self.head.store(next)
    }
}

impl<T> Slab<T>
where
    T: Clear,
{
    pub fn try_checkout<'a>(&'a self) -> Result<RwLockWriteGuard<'a, T>, Error> {
        // The slab's free list is a modification of Treiber's lock-free stack,
        // using slab indices instead of pointers, and with a provison for
        // growing the slab when needed.
        //
        // In order to check out an item from the slab, we "pop" the next free
        // slot from the stack.
        let idx = self.head.load(Ordering::Acquire);

        // Can we insert without reallocating?
        if idx > self.inner.len() {
            return Err(Error::AtCapacity);
        }

        let slot = &self.inner[idx];
        // If someone else has locked the slot, bail and try again.
        let mut lock = slot.item.try_write().ok_or(Error::ShouldRetry)?;
        let next = slot.next.load(Ordering::Relaxed);

        // Is our snapshot still valid?
        if self.head.compare_and_swap(idx, next, Ordering::Release) == idx {
            // We can use this slot!
            lock.clear();
            Ok(lock)
        } else {
            Err(Error::ShouldRetry)
        }
    }
}

// ===== impl Slot =====

impl<T> Slot<T> {
    pub fn new(item: T, idx: usize) -> Self {
        Slot {
            item: RwLock::new(item),
            ref_count: AtomicUsize::new(0),
            next: AtomicUsize::new(idx + 1),
            idx,
        }
    }

    pub(crate) fn drop_ref(&self, head: &AtomicUsize) {
        if self.ref_count.fetch_sub(1, Ordering::Relaxed) == 1 {
            atomic::fence(Ordering::Acquire);

            let next = head.swap(self.idx, Ordering::Release);
            self.next.store(next, Ordering::Release);
        }
    }

    pub(crate) fn clone_ref(&self) {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
    }
}
