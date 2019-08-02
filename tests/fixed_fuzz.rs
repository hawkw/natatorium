use loom::sync::{Arc, Condvar, Mutex};
use loom::thread;
use natatorium::fixed::Pool;

#[test]
fn new_checkouts_are_empty() {
    loom::model(|| {
        let pool: Pool<String> = Pool::with_capacity(3);

        let p = pool.clone();
        let t1 = thread::spawn(move || {
            let mut c = p.checkout();
            assert_eq!("", *c);
            c.push_str("i'm checkout 1");
        });

        let p = pool.clone();
        let t2 = thread::spawn(move || {
            let mut c = p.checkout();
            assert_eq!("", *c);
            c.push_str("i'm checkout 2");
        });

        let mut c = pool.checkout();
        assert_eq!("", *c);
        c.push_str("i'm checkout 3");

        t1.join().expect("thread 1 panicked");
        t2.join().expect("thread 2 panicked");;
    });
}

#[test]
fn reusing_a_slot_clears_data() {
    loom::model(|| {
        let pool: Pool<String> = Pool::with_capacity(1);
        (0..3)
            .map(|i| {
                let pool = pool.clone();
                let t = thread::spawn(move || {
                    let mut c = pool.checkout();
                    assert_eq!("", *c);
                    c.push_str("checked out");
                });
                (i, t)
            })
            .for_each(|(i, t)| {
                t.join()
                    .unwrap_or_else(|e| panic!("thread {} panicked: {:?}", i, e));
            })
    });
}

// #[test]
// fn reusing_a_slot_retains_capacity() {
//     use std::fmt::Write;
//     let pool: Pool<String> = Pool::with_capacity(1);

//     let mut prior_cap = 0;
//     for i in 8..12 {
//         let prior_cap = AtomicUsize::new(0);
//         let pool = pool.clone();
//         thread::spawn(move || {
//             let mut c = pool.checkout();
//             assert_eq!(prior_cap, c.capacity());
//             write!(*c, "i'm checkout {:?}", i).unwrap();
//             prior_cap = c.capacity();
//     }
// }

#[test]
fn capacity_released_when_checkout_is_dropped() {
    loom::model(|| {
        let pool: Pool<String> = Pool::with_capacity(1);

        let checked_out = Arc::new((Mutex::new(false), Condvar::new()));
        let can_drop = Arc::new((Mutex::new(false), Condvar::new()));

        let checked_out2 = checked_out.clone();
        let can_drop2 = can_drop.clone();
        let pool2 = pool.clone();

        let t = thread::spawn(move || {
            let checkout = pool2.checkout();

            let &(ref lock, ref cv) = &*checked_out2;
            *lock.lock().unwrap() = true;
            cv.notify_one();

            let &(ref lock, ref cv) = &*can_drop2;
            let mut can_drop = lock.lock().unwrap();
            while !*can_drop {
                can_drop = cv.wait(can_drop).unwrap();
            }
            drop(checkout);
        });

        let &(ref lock, ref cv) = &*checked_out;
        let mut checked_out = lock.lock().unwrap();
        while !*checked_out {
            checked_out = cv.wait(checked_out).unwrap();
        }

        let ch = pool.try_checkout();
        assert!(ch.is_none());

        let &(ref lock, ref cv) = &*can_drop;
        *lock.lock().unwrap() = true;
        cv.notify_one();

        t.join().unwrap();

        let ch = pool.try_checkout();
        assert!(ch.is_some());
    })
}

#[test]
fn checkout_waits_for_free_capacity() {
    loom::model(|| {
        let pool: Pool<String> = Pool::with_capacity(1);

        let p = pool.clone();
        thread::spawn(move || {
            let mut ch = p.checkout();
            ch.push_str("hello from thread 1!");
            drop(ch)
        });

        let c = pool.checkout();
        assert_eq!(*c, "");
    });
}

#[test]
fn capacity_released_when_all_shared_refs_are_dropped() {
    loom::model(|| {
        let pool: Pool<String> = Pool::with_capacity(1);

        let shared1 = pool.checkout().downgrade();
        assert!(pool.try_checkout().is_none());

        let shared2 = shared1.clone();
        let pool2 = pool.clone();
        let t1 = thread::spawn(move || {
            assert!(pool2.try_checkout().is_none());
            drop(shared2)
        });

        let shared2 = shared1.clone();
        let pool2 = pool.clone();
        let t2 = thread::spawn(move || {
            assert!(pool2.try_checkout().is_none());
            drop(shared2)
        });

        let pool2 = pool.clone();
        let t3 = thread::spawn(move || {
            assert!(pool2.try_checkout().is_none());
            drop(shared1)
        });

        assert!(pool.try_checkout().is_none());

        t1.join().unwrap();
        t2.join().unwrap();
        t3.join().unwrap();

        assert!(pool.try_checkout().is_some());
    });
}
