use natatorium::growable::Pool;

#[test]
fn new_checkouts_are_empty() {
    let pool: Pool<String> = Pool::new();

    let mut c1 = pool.checkout();
    assert_eq!("", *c1);
    c1.push_str("i'm checkout 1");

    let mut c2 = pool.checkout();
    assert_eq!("", *c2);
    c2.push_str("i'm checkout 2");

    let mut c3 = pool.checkout();
    assert_eq!("", *c3);
    c3.push_str("i'm checkout 3");
}

#[test]
fn reusing_a_slot_clears_data() {
    use std::fmt::Write;
    let pool: Pool<String> = Pool::with_capacity(1);

    for i in 0..3 {
        let mut c = pool.checkout();
        assert_eq!("", *c);
        write!(*c, "i'm checkout {:?}", i).unwrap();
    }
}

#[test]
fn reusing_a_slot_retains_capacity() {
    use std::fmt::Write;
    let pool: Pool<String> = Pool::with_capacity(1);

    let mut prior_cap = 0;
    for i in 0..3 {
        let mut c = pool.checkout();
        assert_eq!(prior_cap, c.capacity());
        write!(*c, "i'm checkout {:?}", i).unwrap();
        prior_cap = c.capacity();
    }
}

#[test]
fn capacity_released_when_checkout_is_dropped() {
    let pool: Pool<String> = Pool::with_capacity(1);
    let checkout = pool.checkout();
    assert!(pool.try_checkout().is_none());
    drop(checkout);
    assert!(pool.try_checkout().is_some());
}

#[test]
fn capacity_released_when_all_shared_refs_are_dropped() {
    let pool: Pool<String> = Pool::with_capacity(1);

    let shared1 = pool.checkout().downgrade();
    assert!(pool.try_checkout().is_none());

    let shared2 = shared1.clone();
    assert!(pool.try_checkout().is_none());

    let shared3 = shared2.clone();
    assert!(pool.try_checkout().is_none());

    drop(shared2);
    assert!(pool.try_checkout().is_none());

    drop(shared1);
    assert!(pool.try_checkout().is_none());

    drop(shared3);
    assert!(pool.try_checkout().is_some());
}

#[test]
fn checkout_grows_empty_pool() {
    let pool: Pool<String> = Pool::with_capacity(0);
    assert_eq!(pool.size(), 0);
    let c = pool.checkout();

    assert!(pool.size() > 0);
    assert_eq!(pool.used(), 1);
    assert_eq!(pool.remaining(), pool.used() - 1);
}

#[test]
fn checkout_does_not_grow_if_free_capacity_remains() {
    let pool: Pool<String> = Pool::with_capacity(1);
    assert_eq!(pool.size(), 1);
    assert_eq!(pool.used(), 0);
    assert_eq!(pool.remaining(), 1);

    {
        let c = pool.checkout();
        assert_eq!(pool.size(), 1);
        assert_eq!(pool.used(), 1);
        assert_eq!(pool.remaining(), 0);
    }

    assert_eq!(pool.size(), 1);
    assert_eq!(pool.used(), 0);
    assert_eq!(pool.remaining(),1);

    // Dropping the checkout will release its capacity.
    let c = pool.checkout();
    assert_eq!(pool.size(), 1);
    assert_eq!(pool.used(), 1);
    assert_eq!(pool.remaining(), 0);

    let c2 = pool.checkout();
    assert!(pool.size() > 1);
    assert_ne!(pool.used(), 1);;

}
