use std::{
    mem,
    ops::{Deref, DerefMut},
    ptr,
    sync::{
        atomic::{self, AtomicUsize, Ordering},
        Arc,
    },
};

use crate::traits::Clear;

#[derive(Debug)]
pub struct Slab<T> {
    inner: Vec<Slot<T>>,
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

pub struct Owned<T> {
    slot: ptr::NonNull<Slot<T>>,
    slab: Arc<Slab<T>>,
}

pub struct Shared<T> {
    slot: ptr::NonNull<Slot<T>>,
    slab: Arc<Slab<T>>,
}

// ===== impl Slot =====

impl<T> Slab<T> {
    pub fn new() -> Self {
        Slab {
            inner: Vec::new(),
            head: AtomicUsize::from(0),
        }
    }

    pub fn from_fn(cap: usize, new: impl FnMut() -> T) -> Self {
        let mut this = Self::new();
        this.grow_by(cap, new);
        this
    }

    pub fn grow_by(&mut self, cap: usize, mut new: impl FnMut() -> T) {
        let next = self.inner.len();

        // Avoid multiple allocations.
        self.inner.reserve(cap);
        for i in next..next + cap {
            self.inner.push(Slot::new(new(), i));
        }

        self.head.store(next, Ordering::Release);
    }
}

impl<T> Slab<T>
where
    T: Clear,
{
    // &Arc<Self> is not a valid method reciever T_T
    pub fn try_checkout(this: &Arc<Self>) -> Result<Owned<T>, Error> {
        // The slab's free list is a modification of Treiber's lock-free stack,
        // using slab indices instead of pointers, and with a provison for
        // growing the slab when needed.
        //
        // In order to check out an item from the slab, we "pop" the next free
        // slot from the stack.
        let idx = this.head.load(Ordering::Acquire);

        // Can we insert without reallocating?
        let len = this.inner.len();

        // println!("try_checkout head={:?}; len={:?}", idx, len);
        if idx >= len {
            return Err(Error::AtCapacity);
        }

        // If someone else has locked the slot, bail and try again.
        let slot_ref = &this.inner[idx];
        let mut slot = slot_ref.try_acquire()?;
        let next = slot_ref.next();

        // Is our snapshot still valid?
        if this.head.compare_and_swap(idx, next, Ordering::Release) == idx {
            // We can use this slot!
            unsafe { slot.as_mut() }.item.clear();
            Ok(Owned {
                slot,
                slab: this.clone(),
            })
        } else {
            slot_ref.release();
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
        if self.ref_count.fetch_sub(1, Ordering::Relaxed) == 1 {
            // We are freeing the slot, synchronize here.
            atomic::fence(Ordering::Release);
            true
        } else {
            false
        }
    }

    pub fn clone_ref(&self) {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
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
        if slot.release() {
            let next = self.slab.head.swap(slot.idx, Ordering::Release);
            slot.next.store(next, Ordering::Release);
        }
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
    fn new(slot: &ptr::NonNull<Slot<T>>, slab: &Arc<Slab<T>>) -> Self {
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
        if slot.release() {
            let next = self.slab.head.swap(slot.idx, Ordering::Release);
            slot.next.store(next, Ordering::Release);
        }
    }
}
