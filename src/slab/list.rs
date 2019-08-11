use crate::stdlib::sync::{CausalCell, atomic::{self, AtomicPtr, AtomicUsize, Ordering}};
use crate::stdlib::ptr;

pub type Stack<T> = List<Option<T>>;
/// Indexed storage represented by an atomically linked list of chunks.
pub struct List<T> {
    head: AtomicPtr<Block<T>>,
    tail: AtomicPtr<Block<T>>,
    len: AtomicUsize,
}

unsafe impl<T: Sync> Sync for List<T> {}

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
        Self::from_fn_with_capacity(capacity, T::default)
    }

    pub fn extend(&self) {
        let tail = self.tail.load(Ordering::Acquire);
        if tail.is_null() {
            self.cons_first(T::default);
        } else {
            self.try_cons(tail, T::default);
        }
    }
}


impl<T> List<T> {

    const INITIAL_CAPACITY: usize = 8;

    // XXX: loom::AtomicPtr::new is not const, so this can't be a const fn in
    // tests currently.
    // Fix this with an upstream PR?
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
            tail: AtomicPtr::new(ptr::null_mut()),
            len: AtomicUsize::new(0),

        }
    }

    #[cfg(not(test))]
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
            tail: AtomicPtr::new(ptr::null_mut()),
            len: AtomicUsize::new(0),
        }
    }

    pub fn from_fn_with_capacity(capacity: usize, new: impl FnMut() -> T) -> Self {
        let capacity = if capacity.is_power_of_two() {
            capacity
        } else {
            capacity.next_power_of_two()
        };
        let block = Block::with_capacity(capacity, new);
        let tail = AtomicPtr::new(block);
        let head = AtomicPtr::new(block);
        Self {
            head,
            tail,
            len: AtomicUsize::new(0),
        }
    }

    pub fn extend_with(&self, f: impl FnMut() -> T) {
        let tail = self.tail.load(Ordering::Acquire);
        if tail.is_null() {
            self.cons_first(f);
        } else {
            self.try_cons(tail, f);
        }
    }

    pub(crate) fn with_idx<I>(&self, mut i: usize, f: impl FnOnce(*const T) -> I) -> Option<I> {
        // println!("with_idx[{:?}]", i);
        if i > self.capacity() {
            return None;
        }

        // The tail block always accounts for half the total capacity of the
        // List, so if the requested index is greater than half the total
        // capacity, it falls in the tail block. We can totally skip link
        // hopping in that case.
        let half_cap = self.tail_capacity();
        if i > half_cap {
            let tail = unsafe { self.tail.load(Ordering::Acquire).as_ref() }?;
            debug_assert_eq!(tail.capacity(), half_cap);

            return tail.with_idx(i - half_cap, f);
        }

        let mut curr = unsafe { self.head.load(Ordering::Acquire).as_ref() }?;
        loop {
            let len = curr.capacity();
            // println!("with_idx -> [{:?}], block.len={:?}", i, len);
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

    pub(crate) fn set_last(&self, mut grow: impl FnMut() -> T, f: impl FnOnce(&mut T)) -> usize {
        let mut f = Some(f);
        loop {
            let tail = {
                let t = self.tail.load(Ordering::Relaxed);
                if t.is_null() {
                    self.cons_first(&mut grow)
                } else {
                    t
                }
            };
            let block = unsafe { &*tail };
            if block.try_set_last(&mut f) {
                return self.len.fetch_add(1, Ordering::AcqRel);
            }

            if let Some(new_block) = self.try_cons(tail, &mut grow) {
                if new_block.try_set_last(&mut f) {
                    return self.len.fetch_add(1, Ordering::AcqRel);
                }
            }

            atomic::spin_loop_hint()
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        let tail_cap = self.tail_capacity();
        let head_cap = if let Some(ref head) = unsafe {
            self.head.load(Ordering::Relaxed).as_ref()
        } {
            head.capacity()
        } else {
            return 0;
        };

        if tail_cap == head_cap {
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

    #[cold]
    fn cons_first(&self, new: impl FnMut() -> T) -> *mut Block<T> {
        let block = Block::with_capacity(Self::INITIAL_CAPACITY, new);
        let actual = self.head.compare_and_swap(ptr::null_mut(), block, Ordering::AcqRel);
        if actual.is_null() {
            debug_assert_eq!(
                self.tail.compare_and_swap(ptr::null_mut(), block, Ordering::Release),
                ptr::null_mut(),
                "invariant violated: head was null but tail was not!",
            );

            #[cfg(not(debug_assertions))]
            self.tail.store(block, Ordering::Release);
            block
        } else {
            unsafe {
                drop(Box::from_raw(block));
            }
            actual
        }
    }

    #[cold]
    fn try_cons(&self, tail_ptr: *mut Block<T>, new: impl FnMut() -> T) -> Option<&Block<T>> {
        let tail = unsafe { &*tail_ptr };
        let next = tail.next_block.load(Ordering::Acquire);

        let block = if !next.is_null() {
            // Someone else has already pushed a new block, we're done.
            next
        } else {
            debug_assert!(tail.capacity().is_power_of_two());
            let capacity = tail.capacity() << 1;
            Block::with_capacity(capacity, new)
        };

        if self.tail.compare_and_swap(tail_ptr, block, Ordering::AcqRel) == tail_ptr {
            tail.next_block.store(block, Ordering::Release);
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
    fn tail_capacity(&self) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        unsafe { tail.as_ref() }.map(Block::capacity).unwrap_or(0)
    }
}

impl<T> List<Option<T>> {
    #[inline]
    pub fn get<'a>(&'a self, i: usize) -> Option<&'a T> {
        self.with_idx(i, |slot| unsafe { (&*slot).as_ref() })?
    }

    #[inline]
    pub fn push(&self, elem: T) -> usize {
        let idx = self.set_last(|| None, |slot| {
            debug_assert!(
                slot.is_none(),
                "invariant violated: tried to overwrite existing slot",
            );
            *slot = Some(elem);
        });
        idx
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }
}

impl<T> Block<T> {
    #[inline]
    fn next(&self) -> Option<&Self> {
        unsafe { self.next_block.load(Ordering::Acquire).as_ref() }
    }

    fn with_idx<I>(&self, i: usize, f: impl FnOnce(*const T) -> I) -> Option<I> {
        // if i > self.last_idx.load(Ordering::Acquire) {
        //     return None;
        // }
        // println!("Block::with_index[{:?}]; len={:?}", i, self.block.len());

        self.block.get(i).map(|s| s.with(f))
    }

    fn with_capacity(capacity: usize, mut new: impl FnMut() -> T) -> *mut Self {
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

        // println!("try_set_last: last={:?}; len={:?}", i, self.block.len());
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

    #[inline]
    fn len(&self) -> usize {
        self.last_idx.load(Ordering::Acquire)
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.block.len()
    }
}
