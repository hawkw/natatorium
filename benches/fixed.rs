use criterion::Criterion;
use criterion::{criterion_group, criterion_main};
use natatorium::fixed;
use pool;

use std::{
    sync::{Arc, Mutex},
    thread,
};

fn checkout_uncontended(c: &mut Criterion) {
    let bench = criterion::ParameterizedBenchmark::new("natatorium", |b, i| {
        let pool: fixed::Pool<Vec<i32>> = fixed::Pool::with_capacity(*i);
        b.iter(|| {
            (0..*i).map(|_| {
                let pool = pool.clone();
                thread::spawn(move || {
                    let mut v = pool.checkout();
                    v.push(1)
                })
            }).for_each(|j| j.join().unwrap())
        })

    }, vec![10, 50, 100, 200]).with_function("pool", |b, i| {

        let pool: pool::Pool<Vec<i32>> = pool::Pool::with_capacity(*i, 0, || Vec::new());
        let pool = Arc::new(Mutex::new(pool));
        b.iter(|| {
            (0..*i).map(|_| {
                let pool = pool.clone();
                thread::spawn(move || {
                    let mut v = pool.lock().unwrap().checkout().unwrap();
                    v.push(1)
                })
            }).for_each(|j| j.join().unwrap())
        })
    });
    c.bench("fixed_checkout_uncontended", bench);
}

fn checkout_contended(c: &mut Criterion) {
    let bench = criterion::ParameterizedBenchmark::new("natatorium", |b, i| {
        let pool: fixed::Pool<Vec<i32>> = fixed::Pool::with_capacity(*i/2);
        b.iter(|| {
            (0..*i).map(|_| {
                let pool = pool.clone();
                thread::spawn(move || {
                    let mut v = pool.checkout();
                    v.push(1)
                })
            }).for_each(|j| j.join().unwrap())
        })

    }, vec![10, 50, 100, 200]).with_function("pool", |b, i| {
            let pool: pool::Pool<Vec<i32>> = pool::Pool::with_capacity(*i/2, 0, || Vec::new());
            let pool = Arc::new(Mutex::new(pool));
        b.iter(|| {
            (0..*i).map(|_| {
                let pool = pool.clone();
                thread::spawn(move || {
                    let mut v = loop {
                        if let Some(v) = pool.lock().unwrap().checkout() {
                            break v;
                        }
                    };
                    v.push(1)
                })
            }).for_each(|j| j.join().unwrap())
        })
    });
    c.bench("fixed_checkout_contended", bench);
}

criterion_group!(benches, checkout_contended, checkout_uncontended);
criterion_main!(benches);
