use criterion::Criterion;
use criterion::{criterion_group, criterion_main};
use natatorium::growable;
use pool;

use std::thread;

fn checkout(c: &mut Criterion) {
    let bench = criterion::ParameterizedBenchmark::new("natatorium", |b, i| {
        b.iter(|| {
            let pool: growable::Pool<Vec<i32>> = growable::Pool::new();
            let js = (0..*i).map(|j| {
                let pool = pool.clone();
                thread::spawn(move || {
                    let mut v = pool.checkout();
                    for _ in 0..(j * 2) {
                        v.push(1)
                    }
                })
            }).collect::<Vec<_>>();
            js.into_iter().for_each(|j| j.join().unwrap());
        })

    }, vec![10, 50, 100, 500, 1000]).with_function("alloc", |b, i| {
        b.iter(|| {
            let js = (0..*i).map(|j| {
                thread::spawn(move || {
                    let mut v = Vec::with_capacity(512);
                    for  _ in 0..(j * 2) {
                        v.push(1)
                    }
                })
            }).collect::<Vec<_>>();
            js.into_iter().for_each(|j| j.join().unwrap());
        })
    });
    c.bench("fixed_checkout_contended", bench);
}

criterion_group!(benches, checkout);
criterion_main!(benches);
