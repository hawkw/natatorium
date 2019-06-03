use std::{collections, hash};

pub trait Clear {
    /// Clear all data in `self`, retaining the allocated capacithy.
    ///
    /// # Note
    ///
    /// This should only be implemented for types whose clear operation *retains
    /// any allocations* for that type. Types such as `BTreeMap`, whose
    /// `clear()` method releases the existing allocation, should *not*
    /// implement this trait.
    fn clear(&mut self);
}

pub trait WithCapacity {
    fn capacity(&self) -> usize;

    fn shrink_to_fit(&mut self);
}

// ===== impl Clear =====

impl<T> Clear for Vec<T> {
    #[inline]
    fn clear(&mut self) {
        Vec::clear(self)
    }
}

impl<K, V, S> Clear for collections::HashMap<K, V, S>
where
    K: hash::Hash + Eq,
    S: hash::BuildHasher,
{
    #[inline]
    fn clear(&mut self) {
        collections::HashMap::clear(self)
    }
}

impl<T, S> Clear for collections::HashSet<T, S>
where
    T: hash::Hash + Eq,
    S: hash::BuildHasher,
{
    #[inline]
    fn clear(&mut self) {
        collections::HashSet::clear(self)
    }
}

impl Clear for String {
    #[inline]
    fn clear(&mut self) {
        String::clear(self)
    }
}

// ===== impl WithCapacity =====

impl<T> WithCapacity for Vec<T> {
    #[inline]
    fn capacity(&self) -> usize {
        Vec::capacity(self)
    }

    #[inline]
    fn shrink_to_fit(&mut self) {
        Vec::shrink_to_fit(self)
    }
}

impl<K, V, S> WithCapacity for collections::HashMap<K, V, S>
where
    K: hash::Hash + Eq,
    S: hash::BuildHasher,
{
    #[inline]
    fn capacity(&self) -> usize {
        collections::HashMap::capacity(self)
    }

    #[inline]
    fn shrink_to_fit(&mut self) {
        collections::HashMap::shrink_to_fit(self)
    }
}

impl WithCapacity for String {
    #[inline]
    fn capacity(&self) -> usize {
        String::capacity(self)
    }

    #[inline]
    fn shrink_to_fit(&mut self) {
        String::shrink_to_fit(self)
    }
}
