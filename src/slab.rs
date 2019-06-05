use std::sync::atomic::{self, AtomicUsize, Ordering};


use crate::traits::Clear;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
#[derive(Debug)]
pub struct Slab<T> {
    inner: Vec<RwLock<Slot<T>>>,
    head: AtomicUsize,
}

#[derive(Debug)]
pub struct Slot<T> {
    item: T,
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
        Self::new_from_fn(cap, &T::default)
    }

    pub fn new_from_fn<F>(cap: usize, new: &F) -> Self
    where
        F: Fn() -> T,
    {
        let mut this = Self::new();
        this.grow_by(cap, new);
        this
    }

    pub fn grow_by<F>(&mut self, cap: usize, new: &F)
    where
        F: Fn() -> T,
    {
        let next = self.inner.len();

        // Avoid multiple allocations.
        self.inner.reserve(cap);
        for i in next..next + cap {
            self.inner.push(RwLock::new(Slot::new(new(), i)));
        }

        self.head.store(next, Ordering::Release);
    }

    pub fn head<'a>(&'a self) -> &'a AtomicUsize {
        &self.head
    }

    pub fn try_read<'a>(&'a self, index: usize) -> Option<RwLockReadGuard<'a, Slot<T>>> {
        self.inner[index].try_read()
    }
}

impl<T> Slab<T>
where
    T: Clear,
{
    pub fn try_checkout<'a>(&'a self) -> Result<RwLockWriteGuard<'a, Slot<T>>, Error> {
        // The slab's free list is a modification of Treiber's lock-free stack,
        // using slab indices instead of pointers, and with a provison for
        // growing the slab when needed.
        //
        // In order to check out an item from the slab, we "pop" the next free
        // slot from the stack.
        let idx = self.head.load(Ordering::Acquire);

        // Can we insert without reallocating?
        let len = self.inner.len();

        // println!("try_checkout head={:?}; len={:?}", idx, len);
        if idx >= len {
            return Err(Error::AtCapacity);
        }

        // If someone else has locked the slot, bail and try again.
        let mut slot = self.inner[idx].try_write().ok_or(Error::ShouldRetry)?;
        let next = slot.next.load(Ordering::Relaxed);

        // Is our snapshot still valid?
        if self.head.compare_and_swap(idx, next, Ordering::Release) == idx {
            // We can use this slot!
            let refs = slot.ref_count.fetch_add(1, Ordering::Relaxed);
            debug_assert!(refs == 0);

            slot.item.clear();
            Ok(slot)
        } else {
            Err(Error::ShouldRetry)
        }
    }
}

// ===== impl Slot =====

impl<T> Slot<T> {
    pub fn new(item: T, idx: usize) -> Self {
        Slot {
            item,
            ref_count: AtomicUsize::new(0),
            next: AtomicUsize::new(idx + 1),
            idx,
        }
    }

    pub fn drop_ref(&self, head: &AtomicUsize) {
        if self.ref_count.fetch_sub(1, Ordering::Relaxed) == 1 {
            // We are freeing the slot, synchronize here.
            atomic::fence(Ordering::Acquire);

            let next = head.swap(self.idx, Ordering::Release);
            self.next.store(next, Ordering::Release);
        }
    }

    pub fn clone_ref(&self) {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.idx
    }

    #[inline]
    pub fn item(&self) -> &T {
        &self.item
    }

    #[inline]
    pub fn item_mut(&mut self) -> &mut T {
        &mut self.item
    }
}
