use std::{ops::DerefMut, ptr};

use crate::{
    sync::atomic::{AtomicUsize, Ordering},
    traits::Clear,
};

#[derive(Debug)]
pub struct Slab<T> {
    inner: Vec<Slot<T>>,
    head: AtomicUsize,
    used: AtomicUsize,
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
            head: AtomicUsize::new(0),
            used: AtomicUsize::new(0),
        }
    }

    pub fn from_fn(cap: usize, new: &mut impl FnMut() -> T) -> Self {
        let mut this = Self::new();
        this.grow_by(cap, new);
        this
    }

    pub fn grow_by(&mut self, cap: usize, new: &mut impl FnMut() -> T) {
        let next = self.inner.len();

        // Avoid multiple allocations.
        self.inner.reserve(cap);
        for i in next..next + cap {
            self.inner.push(Slot::new(new(), i));
        }

        self.head.store(next, Ordering::Release);
    }

    pub fn size(&self) -> usize {
        self.inner.len()
    }

    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    pub fn remaining(&self) -> usize {
        self.size() - self.used()
    }

    pub fn slot(&self, idx: usize) -> &Slot<T> {
        &self.inner[idx]
    }
}

impl<T> Slab<T>
where
    T: Clear,
{
    pub fn try_checkout(&self) -> Result<ptr::NonNull<Slot<T>>, Error> {
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
        let slot = &self.inner[idx];
        let mut lease = slot.try_acquire()?;
        let next = slot.next();

        // Is our snapshot still valid?
        if self.head.compare_and_swap(idx, next, Ordering::Release) == idx {
            // We can use this slot!
            unsafe { lease.as_mut() }.item.clear();
            self.used.fetch_add(1, Ordering::Relaxed);
            Ok(lease)
        } else {
            slot.release();
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

    fn next(&self) -> usize {
        self.next.load(Ordering::Relaxed)
    }

    fn try_acquire(&self) -> Result<ptr::NonNull<Self>, Error> {
        if self.ref_count.compare_and_swap(0, 1, Ordering::Acquire) == 0 {
            Ok(ptr::NonNull::from(self))
        } else {
            Err(Error::ShouldRetry)
        }
    }

    fn release(&self) -> bool {
        self.ref_count.fetch_sub(1, Ordering::Relaxed) == 1
    }

    pub fn clone_ref(&self) {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn drop_ref(&self, slab: &Slab<T>) {
        if self.release() {
            // Free the slot.
            let next = slab.head.swap(self.idx, Ordering::Release);
            self.next.store(next, Ordering::Release);
            slab.used.fetch_sub(1, Ordering::Relaxed);
        }
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


impl<T> Slot<Box<T>> {
    pub fn as_ptr(&mut self) -> ptr::NonNull<T> {
        ptr::NonNull::from(self.item.deref_mut())
    }
}
