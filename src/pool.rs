use crate::traits::Clear;
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
    next_free: AtomicUsize,
}

pub type Owned<'a, T> = Key<'a, T, RwLockWriteGuard<'a, T>>;
pub type Shared<'a, T> = Key<'a, T, RwLockReadGuard<'a, T>>;

pub struct Key<'a, T, L>
where
    L: Deref,
{
    lock: OwningHandle<RwLockReadGuard<'a, Slab<T>>, L>,
    index: usize,
    // hmm...
    head: &'a AtomicUsize,
}

type Slab<T> = Vec<Slot<T>>;

#[derive(Debug)]
struct Slot<T> {
    item: RwLock<T>,
    ref_count: AtomicUsize,
    next: AtomicUsize,
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
            slab: RwLock::new(Vec::new()),
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
        // The slab's free list is a modification of Treiber's lock-free stack,
        // using slab indices instead of pointers, and with a provison for
        // growing the slab when needed.
        //
        // In order to insert a new span into the slab, we "pop" the next free
        // index from the stack.
        loop {
            // Acquire a snapshot of the head of the free list.
            let head = &self.next_free;
            let index = head.load(Ordering::Relaxed);

            {
                // Try to insert the span without modifying the overall
                // structure of the stack.
                let slab = self.slab.read();

                // Can we insert without reallocating?
                if index < slab.len() {
                    // If someone else is writing to the head slot, we need to
                    // acquire a new snapshot!
                    if let Ok(mut lock) = OwningHandle::try_new(slab, |slab| {
                        let slot = unsafe { &(*slab)[index] };
                        // If someone else has locked the slot, bail and try again.
                        let lock = slot.item.try_write().ok_or(())?;

                        // Is our snapshot still valid?
                        let next = slot.next.load(Ordering::Relaxed);
                        if head.compare_and_swap(index, next, Ordering::Release) == index {
                            // We can use this slot!
                            Ok(lock)
                        } else {
                            Err(())
                        }
                    }) {
                        lock.clear();
                        return Owned { lock, index, head };
                    }
                    // Our snapshot got stale, try again!
                    atomic::spin_loop_hint();
                    continue;
                }
            }

            // We need to grow the slab, and must acquire a write lock.
            if let Some(mut slab) = self.slab.try_write() {
                // Create a new item.
                let item = (self.new)();
                let slot = Slot::new(item);
                // TODO: can we grow the slab in chunks to avoid having to
                // realloc as often?

                // Make sure our snapshot of the head pointer is still valid,
                // and if it is, push the new slot.
                if head.compare_and_swap(index, index + 1, Ordering::Release) == index {
                    slab.push(slot);

                    let index = index + 1;
                    let slab = RwLockWriteGuard::downgrade(slab);
                    let lock = OwningHandle::new_with_fn(slab, |slab| {
                        let slot = unsafe { &(*slab)[index] };
                        slot.item
                            .try_write()
                            .expect("newly pushed slot cannot already have been acquired")
                    });
                    return Owned { lock, index, head };
                }
            }

            atomic::spin_loop_hint();
        }
    }
}

// ===== impl Slot =====
impl<T> Slot<T> {
    pub(crate) fn new(item: T) -> Self {
        Slot {
            item: RwLock::new(item),
            ref_count: AtomicUsize::new(0),
            next: AtomicUsize::new(0xDEADFACE),
        }
    }

    pub(crate) fn drop_ref(&self, idx: usize, head: &AtomicUsize) {
        if self.ref_count.fetch_sub(1, Ordering::Relaxed) == 1 {
            atomic::fence(Ordering::Acquire);

            let next = head.swap(idx, Ordering::Release);
            self.next.store(next, Ordering::Release);
        }
    }

    pub(crate) fn clone_ref(&self) {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
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
        if let Some(slot) = self.lock.as_owner().get(self.index) {
            slot.drop_ref(self.index, self.head);
        }
    }
}
