# natatorium

Pools for reusing heap-allocated objects.

## About the Name

"Natatorium" is a fancy Latin word for "swimming pool". This crate implements
object pooling, and there's already a crate called `pool`.

## Comparison with other Crates

- [`pool`]: Carl Lerche's `pool` crate provides a lock-free pool for typed objects or
  raw slabs of memory.

  Although aspects of `pool`'s implementation influenced `natatorium`, they are
  intended for different use-cases. `natatorium` is intended for managing the
  reuse of _heap-allocated_ dynamic objects, while `pool` is intended for
  lower-level management of a slab of memory.

  - `pool` does not allow changing the size of a pool once it has been created.
    `natatorium` has both a `fixed::Pool` type with a fixed initial size, and a
    `growable::Pool` type which grows when at capacity.
  - In both `pool` and `natatorium`, releasing an object back to the pool is
    lock-free. However, `pool` requires a mutable reference to check an object
    _out_ of the pool, while `natatorium` does not. `natatorium`'s `fixed::Pool`
    type allows lock-free checkouts, while its `growable::Pool` type requires a
    read lock for checking out objects when the pool has capacity, and a write
    lock for checking out objects when growing the pool.
  - `pool` provides access to a fixed-size array of bytes for every pooled
    object, which the documentation suggests using for storing user-defined
    metadata associated with that object. `natatorium` does not support this,
    but arbitrary user defined structs may be stored in the pool, allowing types
    to be wrapped with metadata.
  - `pool::Checkout` provides unique, mutable access to a pooled object.
    `natatorium` has `Owned` checkout types with similar guarantees, but allows
    downgrading an `Owned` checkout to a reference-counted `Shared` checkout,
    which can be cloned and allows only immutable access.
  <!-- while `natatorium` always expects them to live on the heap --- `natatorium` is
  intended primarily for managing the reuse of growable objects like strings,
  buffers, and maps. -->
