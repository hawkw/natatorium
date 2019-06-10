use natatorium::fixed::Pool;
use loom::thread;
use loom::sync::{Arc, Condvar, Mutex};

#[test]
fn new_checkouts_are_empty() {
    loom::fuzz(|| {
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
    loom::fuzz(|| {
        let pool: Pool<String> = Pool::with_capacity(1);
        (0..3).map(|i| {
            let pool = pool.clone();
            let t = thread::spawn(move || {
                let mut c = pool.checkout();
                assert_eq!("", *c);
                c.push_str("checked out");
            });
            (i, t)
        }).for_each(|(i, t)| {
            t.join().unwrap_or_else(|e| panic!("thread {} panicked: {:?}", i, e));
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
    loom::fuzz(|| {
        let pool: Pool<String> = Pool::with_capacity(1);
        let cv = Arc::new((Mutex::new(0), Condvar::new()));

        let cv2 = cv.clone();
        let pool2 = pool.clone();

        let t = thread::spawn(move || {
            let &(ref lock, ref cv) = &*cv2;
            let checkout = pool2.checkout();

            *lock.lock().unwrap() = 1;
            cv.notify_one();

            let mut seen = lock.lock().unwrap();
            while *seen != 2 {
                seen = cv.wait(seen).unwrap();
            }

            drop(checkout);
            *lock.lock().unwrap() = 3;
            cv.notify_one();
        });


        let &(ref lock, ref cv) = &*cv;
        {
            let mut checked_out = lock.lock().unwrap();
            while *checked_out != 1 {
                checked_out = cv.wait(checked_out).unwrap();
            }
            let ch = pool.try_checkout();
            println!("ch={:?}", ch);
            assert!(ch.is_none());
            *checked_out = 2;
        }

        {
            let mut dropped = lock.lock().unwrap();
            while *dropped != 3 {
                dropped = cv.wait(dropped).unwrap();
            }

            assert!(pool.try_checkout().is_some());
        }

        t.join().unwrap();
    })

}

// #[test]
// fn capacity_released_when_all_shared_refs_are_dropped() {
//     let pool: Pool<String> = Pool::with_capacity(1);

//     let shared1 = pool.checkout().downgrade();
//     assert!(pool.try_checkout().is_none());

//     let shared2 = shared1.clone();
//     assert!(pool.try_checkout().is_none());

//     let shared3 = shared2.clone();
//     assert!(pool.try_checkout().is_none());

//     drop(shared2);
//     assert!(pool.try_checkout().is_none());

//     drop(shared1);
//     assert!(pool.try_checkout().is_none());

//     drop(shared3);
//     assert!(pool.try_checkout().is_some());
// }
