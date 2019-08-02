use crate::sync::{CausalCell, atomic::{self, AtomicPtr, AtomicUsize, Ordering}};

use std::ptr;

/// Indexed storage represented by an atomically linked list of chunks.
pub struct List<T> {
    head: Box<Block<T>>,
    tail: AtomicPtr<Block<T>>,
    len: AtomicUsize,
}

unsafe impl<T: Sync> Sync for List<T> {}

struct Block<T> {
    next_block: AtomicPtr<Block<T>>,
    push_idx: AtomicUsize,
    last_idx: AtomicUsize,
    block: Box<[Slot<T>]>,
}

type Slot<T> = CausalCell<Option<T>>;

impl<T> List<T> {
    pub fn with_capacity(capacity: usize) -> Self {
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
        }
    }

    pub fn push(&self, elem: T) -> usize {
        let mut elem = Some(elem);
        loop {
            let tail = self.tail.load(Ordering::Acquire);
            debug_assert!(
                !tail.is_null(),
                "invariant violated: tail should never be null"
            );

            let block = unsafe { &*tail };
            if block.try_push(&mut elem) {
                return self.len.fetch_add(1, Ordering::AcqRel);
            }

            if let Some(new_block) = block.try_cons(tail, &self.tail) {
                if new_block.try_push(&mut elem) {
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

    pub fn get<'a>(&'a self, mut i: usize) -> Option<&'a T> {
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

            return tail.get(i - half_cap);
        }

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
                return curr.get(i);
            }
        }
    }

    #[inline]
    fn tail_capacity(&self) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        unsafe { tail.as_ref().map(Block::capacity).unwrap_or(0) }
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
}

impl<T> Block<T> {
    #[inline]
    fn next(&self) -> Option<&Self> {
        unsafe { self.next_block.load(Ordering::Acquire).as_ref() }
    }

    #[inline]
    fn get<'a>(&'a self, i: usize) -> Option<&'a T> {
        if i > self.last_idx.load(Ordering::Acquire) {
            return None;
        }

        self.block[i].with(|slot| unsafe { (&*slot).as_ref() })
    }

    fn with_capacity(capacity: usize) -> *mut Self {
        let mut block = Vec::with_capacity(capacity);
        block.resize_with(capacity, || CausalCell::new(None));
        let block = block.into_boxed_slice();
        let block = Block {
            next_block: AtomicPtr::new(ptr::null_mut()),
            push_idx: AtomicUsize::new(0),
            last_idx: AtomicUsize::new(0),
            block,
        };
        Box::into_raw(Box::new(block))
    }

    fn try_push(&self, elem: &mut Option<T>) -> bool {
        let i = self.push_idx.fetch_add(1, Ordering::AcqRel);

        if i >= self.block.len() {
            // We've reached the end of the block; time to push a new block.
            return false;
        }

        self.block[i].with_mut(|slot| {
            let slot = unsafe { &mut *slot };
            debug_assert!(
                slot.is_none(),
                "invariant violated: tried to overwrite existing slot ({:?})",
                i
            );

            let elem = elem.take();
            debug_assert!(elem.is_some(), "invariant violated: tried to push nothing");
            *slot = elem;
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
