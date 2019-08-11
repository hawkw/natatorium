use crate::sync::{CausalCell, atomic::{self, AtomicPtr, AtomicUsize, Ordering}};

use std::ptr;

pub type Stack<T> = List<Option<T>, Option::default>;
/// Indexed storage represented by an atomically linked list of chunks.
pub struct List<T, F = fn() -> T> {
    head: Box<Block<T>>,
    tail: AtomicPtr<Block<T>>,
    len: AtomicUsize,
    new: F,
}

unsafe impl<T: Sync, F: Sync> Sync for List<T, F> {}

struct Block<T> {
    next_block: AtomicPtr<Block<T>>,
    push_idx: AtomicUsize,
    last_idx: AtomicUsize,
    block: Box<[CausalCell<T>]>,
}

impl<T> List<T>
where
    T: Default,
{
    pub fn with_capacity(capacity: usize) -> Self {
        Self::from_fn_with_capacity(T::default, capacity)
    }
}

impl<T, F> List<T, F>
where
    F: Fn() -> T,
{
    pub fn from_fn_with_capacity(new: F, capacity: usize) -> Self {
        let capacity = if capacity.is_power_of_two() {
            capacity
        } else {
            capacity.next_power_of_two()
        };
        let block = Block::with_capacity(capacity);
        let tail = AtomicPtr::new(block);
        let head = unsafe {
            // this is safe; we just constructed this box...
            Box::from_raw(block)
        };
        Self {
            head,
            tail,
            len: AtomicUsize::new(0),
            new,
        }
    }

    pub(crate) fn with_idx<I>(&self, mut i: usize, f: impl FnOnce(*const T) -> I) -> Option<I> {
        if i > self.len() {
            return None;
        }

        // The tail block always accounts for half the total capacity of the
        // List, so if the requested index is greater than half the total
        // capacity, it falls in the tail block. We can totally skip link
        // hopping in that case.
        let half_cap = self.tail_capacity();
        if i > half_cap {
            let tail = unsafe {
                // XXX: technically, this is a bad state --- the index is less
                // than `self.len()`, so we know it exists; it *should* be in the
                // tail block, but the tail block is null. Should we expect
                // this to always be `Some`?
                self.tail.load(Ordering::Acquire).as_ref()?
            };
            debug_assert_eq!(tail.capacity(), half_cap);

            tail.with_idx(i - half_cap, f);
        } else {
            let mut curr = self.head.as_ref();
            loop {
                let len = curr.len();
                if i >= len {
                    // The slot's index is higher than this block's length, try the next
                    // block if one exists.
                    curr = curr.next()?;
                    i -= len;
                } else {
                    // Found the block with that slot --- see if it's filled?
                    return curr.with_idx(i, f);
                }
            }
        }
    }

    pub(crate) fn set_last(&self, f: impl FnOnce(&mut T)) -> usize {
        let mut f = Some(f);
        loop {
            let tail = self.tail.load(Ordering::Acquire);
            debug_assert!(
                !tail.is_null(),
                "invariant violated: tail should never be null"
            );

            let block = unsafe { &*tail };
            if block.try_set_last(&mut f) {
                return self.len.fetch_add(1, Ordering::AcqRel);
            }

            if let Some(new_block) = block.try_cons(tail, &self.tail) {
                if new_block.try_set_last(&mut f) {
                    return self.len.fetch_add(1, Ordering::AcqRel);
                }
            }

            atomic::spin_loop_hint()
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        let tail_cap = self.tail_capacity();

        if tail_cap == self.head.as_ref().capacity() {
            // If the tail and head block are the same size, we've not pushed
            // any more blocks, so the tail's capacity is the total capacity.
            tail_cap
        } else {
            // Otherwise, since the capacity of any block is a power of 2, and
            // each block's capacity is 2x the prior block's capacity, then the
            // tail is always equal to half the total capacity.
            tail_cap << 1
        }
    }

    #[inline]
    fn tail_capacity(&self) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        unsafe { tail.as_ref().map(Block::capacity).unwrap_or(0) }
    }
}

impl<T> Stack<T> {
    #[inline]
    pub fn get<'a>(&'a self, mut i: usize) -> Option<&'a T> {
        self.with_idx(i, |slot| unsafe { (&*slot).as_ref() })?
    }

    #[inline]
    pub fn push(&self, elem: T) -> usize {
        self.set_last(|&mut slot| {
            debug_assert!(
                slot.is_none(),
                "invariant violated: tried to overwrite existing slot",
            );
            slot = elem;
        })
    }
}

impl<T> Block<T> {
    #[inline]
    fn next(&self) -> Option<&Self> {
        unsafe { self.next_block.load(Ordering::Acquire).as_ref() }
    }

    fn with_idx<I>(&self, i: usize, f: impl FnOnce(*const T) -> I) -> Option<I> {
        if i > self.last_idx.load(Ordering::Acquire) {
            return None;
        }

        self.block[i].with(f)
    }

    fn with_capacity(capacity: usize, new: &impl Fn() -> T) -> *mut Self {
        let mut block = Vec::with_capacity(capacity);
        block.resize_with(capacity, || CausalCell::new(new()));
        let block = block.into_boxed_slice();
        let block = Block {
            next_block: AtomicPtr::new(ptr::null_mut()),
            push_idx: AtomicUsize::new(0),
            last_idx: AtomicUsize::new(0),
            block,
        };
        Box::into_raw(Box::new(block))
    }

    fn try_set_last(&self, f: &mut Option<impl FnOnce(&mut T)>) -> bool {
        let i = self.push_idx.fetch_add(1, Ordering::AcqRel);

        if i >= self.block.len() {
            // We've reached the end of the block; time to push a new block.
            return false;
        }

        self.block[i].with_mut(|slot| {
            let slot = unsafe { &mut *slot };
            let f = f.take().expect("tried to set last item twice");
            f(slot);
        });

        self.last_idx.fetch_add(1, Ordering::Release);
        true
    }

    #[cold]
    fn try_cons(&self, prev: *mut Self, tail: &AtomicPtr<Self>) -> Option<&Self> {
        let next = self.next_block.load(Ordering::Acquire);

        let block = if !next.is_null() {
            // Someone else has already pushed a new block, we're done.
            next
        } else {
            debug_assert!(self.capacity().is_power_of_two());
            let capacity = self.capacity() << 1;
            Block::with_capacity(capacity)
        };

        if tail.compare_and_swap(prev, block, Ordering::AcqRel) == prev {
            self.next_block.store(block, Ordering::Release);
            return unsafe { block.as_ref() };
        }

        // Someone beat us to it, and a new block has already been pushed.
        // We need to clean up the block we allocated.
        if !block.is_null() {
            unsafe {
                // This is safe, since we just created that block; it is our
                // *responsibility* to destroy it.
                drop(Box::from_raw(block));
            };
        }
        None
    }

    #[inline]
    fn len(&self) -> usize {
        self.last_idx.load(Ordering::Acquire)
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.block.len()
    }
}
