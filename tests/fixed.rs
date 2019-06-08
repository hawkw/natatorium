use natatorium::{
    fixed::Pool,
};

#[test]
fn new_checkouts_are_empty() {
    let pool: Pool<String> = Pool::with_capacity(3);

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
