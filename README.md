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
    _out_ of the pool, while `natatorium` does not.
  - `pool` provides access to a fixed-size array of bytes for every pooled
    object, which the documentation suggests using for storing user-defined
    metadata associated with that object. `natatorium` does not support this,
    but arbitrary user defined structs may be stored in the pool, allowing types
    to be wrapped with metadata.
  - `pool::Checkout` provides unique, mutable access to a pooled object.
    `natatorium` has `Owned` checkout types with similar guarantees, but allows
    downgrading an `Owned` checkout to a reference-counted `Shared` checkout,
    which can be cloned and allows only immutable access.
  - Because `pool` provides fewer features than `natatorium`, its implementation
    is somewhat simpler, and checkout performance is faster in our benchmarks
    (by a constant factor of about 15-20 nanoseconds).

  <!-- while `natatorium` always expects them to live on the heap --- `natatorium` is
  intended primarily for managing the reuse of growable objects like strings,
  buffers, and maps. -->
  [`pool`]: https://crates.io/crates/pool
- [`object-pool`]: The `object-pool` crate provides a simple lock-free object
  pool for typed objects.

  - Like `natatorium`, both checking out and checking in an object are
    lock-free, so a global lock is not required when sharing `object-pool`'s
    pool across multiple threads.
  - `object-pool`'s pool is fixed size, and cannot be increased in size once it
    has been constructed.
  - `object-pool` does not support downgrading an owned checkout into a shared
    checkout. Instead, all objects checked out from the pool are uniquely owned.
  - `object-pool` does not automatically reset checkouts on drop. Instead, the
    user [is responsible] for manually clearing checked out objects.
  - `object-pool`'s checkout algorithm pool has a worst-case time complexity of
    O(_m_), where _m_ is the size of the pool. Performance approaches the worst
    case as utilization increase (i.e. as more objects are checked out) &mdash
    when the final free object is checked out, `object-pool`'s checkout
    algorithm must iterate over every other object in the pool before finding
    the final block. In comparison, `natatorium::fixed`'s worst-case time
    complexity for checkouts is O(_1_), while `natatorium::growable` is
    O(log<sub>2</sub> _m_) in the worst case, and amortized as utilization
    increases. In our benchmarks, this property can be observed to hurt
    `object-pool` checkout performance significantly as utilization increases.

    [is responsible]: https://docs.rs/object-pool/0.3.1/object_pool/index.html#warning
    [`object-pool`]: https://crates.io/crates/object-pool
