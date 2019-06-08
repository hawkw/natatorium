use std::marker::PhantomData;
use crate::{growable, fixed, slab};

#[derive(Debug, Clone)]
pub struct Builder<S, T, N = fn() -> T> {
    pub(crate) new: N,
    pub(crate) settings: S,
    capacity: usize,
    item: PhantomData<fn() -> T>,
}

impl<T> Builder<(), T, ()> {
    pub fn new() -> Self {
        Self {
            new: (),
            settings: (),
            capacity: 256,
            item: PhantomData,
        }
    }
}

impl<S, T, N> Builder<S, T, N> {
    pub fn with_elements(self, capacity: usize) -> Self {
        Self { capacity, ..self }
    }

    pub fn with_default(self) -> Builder<S, T>
    where
        T: Default,
    {
        Builder {
            new: T::default,
            capacity: self.capacity,
            settings: self.settings,
            item: PhantomData,
        }
    }

    pub fn with_fn<F>(self, new: F) -> Builder<S, T, F>
    where
        F: FnMut() -> T,
    {
        Builder {
            new,
            capacity: self.capacity,
            settings: self.settings,
            item: PhantomData,
        }
    }

    pub fn growable(self) -> Builder<growable::Settings, T, N> {
        Builder {
            new: self.new,
            capacity: self.capacity,
            settings: growable::Settings::default(),
            item: PhantomData,
        }
    }

    pub fn fixed(self) -> Builder<fixed::Settings, T, N> {
        Builder {
            new: self.new,
            capacity: self.capacity,
            settings: fixed::Settings::default(),
            item: PhantomData,
        }
    }

    pub fn finish(self) -> S::Pool
    where
        S: settings::Make<T, N>,
    {
        S::make(self)
    }

    pub(crate) fn slab<I>(&mut self) -> slab::Slab<I>
    where
        N: FnMut() -> T,
        T: Into<I>,
    {
        slab::Slab::from_fn(self.capacity, &mut || { (self.new)().into()})
    }
}

impl<T, N> Builder<growable::Settings, T, N> {
    pub fn grow_by(self, amount: usize) -> Self {
        Self {
            settings: growable::Settings {
                growth: growable::Growth::Fixed(amount),
                ..self.settings
            },
            ..self
        }
    }

    pub fn grow_double(self) -> Self {
        Self {
            settings: growable::Settings {
                growth: growable::Growth::Double,
                ..self.settings
            },
            ..self
        }
    }

    pub fn grow_by_half(self) -> Self {
        Self {
            settings: growable::Settings {
                growth: growable::Growth::Half,
                ..self.settings
            },
            ..self
        }
    }
}

impl<T: Default> Default for Builder<(), T> {
    fn default() -> Self {
        Builder::new().with_default()
    }
}

pub(crate) mod settings {
    use super::Builder;

    pub trait Make<T, N>: Sized {
        type Pool;
        fn make(builder: Builder<Self, T, N>) -> Self::Pool;
    }
}
