use crate::stdlib::{
    ops::DerefMut, ptr,
    sync::{self, atomic::{AtomicUsize, Ordering}},
    marker::PhantomData,
};

use crate::traits::Clear;

mod list;
pub use self::list::{List, Stack};


pub(crate) type ArraySlab<T> = Slab<T, ArrayStore<T>>;
pub(crate) type ArrayStore<T> = Box<[sync::CausalCell<Slot<T>>]>;

#[derive(Debug)]
pub(crate) struct Slab<T, S> {
    inner: S,
    head: AtomicUsize,
    used: AtomicUsize,
    _t: PhantomData<T>,
}

#[derive(Debug)]
pub struct Slot<T> {
    item: T,
    idx: usize,
    ref_count: AtomicUsize,
    next: AtomicUsize,
}

#[derive(Debug)]
pub enum Error {
    AtCapacity,
    ShouldRetry,
}

pub(crate) trait Store<T> {
    fn with_slot<F, O>(&self, idx: usize, f: F) -> Option<O>
    where
        F: FnOnce(&Slot<T>) -> O,
    ;

    fn slot_count(&self) -> usize;
}

// ===== impl Slot =====
//
impl<T, S> Slab<T, S> {
    #[cfg(not(test))]
    pub const fn new(inner: S) -> Self {
        Slab {
            inner,
            head: AtomicUsize::new(0),
            used: AtomicUsize::new(0),
            _t: PhantomData,
        }
    }

    #[cfg(test)]
    pub fn new(inner: S) -> Self {
        Slab {
            inner,
            head: AtomicUsize::new(0),
            used: AtomicUsize::new(0),
            _t: PhantomData,
        }
    }
}

impl<T, S> Slab<T, S>
where
    S: Store<T>,
{
    // pub fn from_fn(cap: usize, new: &impl Fn() -> T) -> Self {
    //     let this = Self::new();
    //     this.grow_by(cap, new);
    //     this
    // }

    // pub fn grow_by(&self, cap: usize, new: &impl Fn() -> T) {
    //     let next = self.inner.len();

    //     // Avoid multiple allocations.
    //     for i in next..next + cap {
    //         // self.inner.push(Slot::new(new(), i));
    //         unimplemented!()
    //     }

    //     self.head.store(next, Ordering::Release);
    // }

    pub fn size(&self) -> usize {
        self.inner.slot_count()
    }

    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    pub fn remaining(&self) -> usize {
        self.size() - self.used()
    }

    // pub fn with_slot(&self, idx: usize) -> &Slot<T> {
    //     // self.inner.get(idx).expect("slot should exist")
    //     unimplemented!("FIXME(eliza): T_T")
    // }
    //
    pub fn with_slot<F, O>(&self, idx: usize, f: F) -> Option<O>
    where
        F: FnOnce(&Slot<T>) -> O,
    {
        self.inner.with_slot(idx, f)
    }

    pub fn assert_valid(&self) {
        let used = self.used.load(Ordering::SeqCst);
        // let mut actual_used = 0;
        // for (idx, slot) in self.inner.iter().enumerate() {
        //     assert_eq!(
        //         slot.idx, idx,
        //         "invariant violated: slot index did not match actual slab index",
        //     );
        //     if self.head.load(Ordering::SeqCst) == idx {
        //         assert_eq!(
        //             slot.ref_count(Ordering::SeqCst),
        //             0,
        //             "invariant violated: head slot had non-zero ref count",
        //         );
        //     }
        //     slot.assert_valid();
        //     if slot.ref_count(Ordering::SeqCst) > 0 {
        //         actual_used += 1;
        //     }
        // }
        assert!(
            self.head.load(Ordering::SeqCst) <= self.size(),
            "invariant violated: free list head should not point past the end of the slab",
        );

        // if used == self.used.load(Ordering::SeqCst) {
        //     assert_eq!(
        //         used, actual_used,
        //         "invariant violated: used did not equal number of slots with non-zero ref counts",
        //     );
        // }
    }
}

impl<T, S> Slab<T, S>
where
    T: Clear,
    S: Store<T>,
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
        let len = self.inner.slot_count();

        // println!("try_checkout head={:?}; len={:?}", idx, len);
        if idx >= len {
            return Err(Error::AtCapacity);
        }

        // If someone else has locked the slot, bail and try again.
        self.inner.with_slot(idx, |slot| {
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
        }).ok_or(Error::AtCapacity)?

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

    pub(crate) fn drop_ref<S: Store<T>>(&self, slab: &Slab<T, S>) {
        if self.release() {
            // Free the slot.
            let next = slab.head.swap(self.idx, Ordering::Release);
            self.next.store(next, Ordering::Release);
            slab.used.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub fn ref_count(&self, ordering: Ordering) -> usize {
        self.ref_count.load(ordering)
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

    pub fn item_ptr(&self) -> ptr::NonNull<T> {
        ptr::NonNull::from(&self.item)
    }

    /// Asserts that this slot is currently in a valid state.
    pub fn assert_valid(&self) {
        assert_ne!(
            self.next.load(Ordering::SeqCst),
            self.idx,
            "invariant violated: next pointer may not point to self"
        );
    }
}

impl<T> Slot<Box<T>> {
    pub fn as_ptr(&mut self) -> ptr::NonNull<T> {
        ptr::NonNull::from(self.item.deref_mut())
    }
}

pub(crate) fn new_array<T>(cap: usize, mut f: impl FnMut() -> T) -> ArrayStore<T> {
    let mut v = Vec::with_capacity(cap);
    for i in 0..cap {
        v.push(sync::CausalCell::new(Slot::new(f(), i)));
    }
    v.into_boxed_slice()
}

impl<T> Store<T> for ArrayStore<T> {
    fn with_slot<F, O>(&self, idx: usize, f: F) -> Option<O>
    where
        F: FnOnce(&Slot<T>) -> O,
    {
        self.get(idx).map(|c| c.with(|s| unsafe {
            f(&*s)
        }))
    }

    fn slot_count(&self) -> usize {
        self.as_ref().len()
    }
}

impl<T> Store<T> for List<Slot<T>> {
    fn with_slot<F, O>(&self, idx: usize, f: F) -> Option<O>
    where
        F: FnOnce(&Slot<T>) -> O,
    {
        self.with_idx(idx, |s| unsafe { f(&*s) })
    }

    fn slot_count(&self) -> usize {
        self.capacity()
    }
}

impl<T> Slab<T, List<Slot<T>>> {
    pub(crate) fn extend_with(&self, new: impl Fn() -> T) {
        let mut len = self.inner.capacity();
        self.inner.extend_with(|| {
            let slot = Slot::new(new(), len);
            len += 1;
            slot
        });
        self.head.fetch_add(1, Ordering::Release);
    }
}

unsafe impl<T, S> Sync for Slab<T, S> {}
