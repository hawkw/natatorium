
use criterion::Criterion;
use criterion::{criterion_group, criterion_main};
use criterion::black_box;
use natatorium::{growable, fixed};
use pool;
use object_pool;

static KB: usize = 1024;
static MB: usize = 1024 * KB;
static GB: usize = 1024 * MB;
static VEC_SIZES: &[usize] = &[
    4 * KB, 16 * KB, 64 * KB, 128 * KB, 512 * KB, 1 * MB, 16 * MB, 32 * MB, 64 * MB, 128 * MB,
    256 * MB, 512 * MB, 1 * GB, 2 * GB, 3 * GB,
];

static POOL_SIZES: &[usize] = &[
    4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048,
];

fn checkout_once(c: &mut Criterion) {
    let bench = criterion::ParameterizedBenchmark::new("natatorium::growable", |b, &&i| {
            let pool = growable::Pool::builder()
                .with_elements(1)
                .with_fn(|| Vec::<u8>::with_capacity(i)).finish();
            b.iter(|| {
                black_box(pool.checkout());
            })
    }, VEC_SIZES)
        .with_function("natatorium::fixed", |b, &&i| {
            let pool = fixed::Pool::builder()
                .with_elements(1)
                .with_fn(|| Vec::<u8>::with_capacity(i)).finish();
            b.iter(|| {
                black_box(pool.checkout());
            })
        })
        .with_function("pool", |b, &&i| {
            let mut pool = pool::Pool::with_capacity(1, 0, || Vec::<u8>::with_capacity(i));
            b.iter(|| {
                black_box(pool.checkout().unwrap());
            })
        })
        .with_function("object_pool", |b, &&i| {
            let pool = object_pool::Pool::new(1, || Vec::<u8>::with_capacity(i));
            b.iter(|| {
                black_box(pool.pull().unwrap());
            })
        })
        .with_function("alloc", |b, &&i| {
            b.iter(|| {
                black_box(Vec::<u8>::with_capacity(i));
            })
        });
    c.bench("checkout_once", bench);
}

fn checkout_twice(c: &mut Criterion) {
    let bench = criterion::ParameterizedBenchmark::new("natatorium::growable", |b, &&i| {
            let pool = growable::Pool::builder()
                .with_elements(1)
                .with_fn(|| Vec::<u8>::with_capacity(i)).finish();
            b.iter(|| {
                black_box(pool.checkout());
                black_box(pool.checkout());
            })
    }, VEC_SIZES)
        .with_function("natatorium::fixed", |b, &&i| {
            let pool = fixed::Pool::builder()
                .with_elements(1)
                .with_fn(|| Vec::<u8>::with_capacity(i)).finish();
            b.iter(|| {
                black_box(pool.checkout());
                black_box(pool.checkout());
            })
        })
        .with_function("pool", |b, &&i| {
            let mut pool = pool::Pool::with_capacity(1, 0, || Vec::<u8>::with_capacity(i));
            b.iter(|| {
                black_box(pool.checkout().unwrap());
                black_box(pool.checkout().unwrap());
            })
        })
        .with_function("object_pool", |b, &&i| {
            let pool = object_pool::Pool::new(1, || Vec::<u8>::with_capacity(i));
            b.iter(|| {
                black_box(pool.pull().unwrap());
            })
        })
        .with_function("alloc", |b, &&i| {
            b.iter(|| {
                black_box(Vec::<u8>::with_capacity(i));
                black_box(Vec::<u8>::with_capacity(i));
            })
        });
    c.bench("checkout_twice", bench);
}

fn checkout_last(c: &mut Criterion) {
    let bench = criterion::ParameterizedBenchmark::new("natatorium::growable", |b, &&i| {
            let pool = growable::Pool::builder()
                .with_elements(i)
                .with_fn(|| Vec::<u8>::new()).finish();
            let mut checkouts = Vec::new();
            for _ in 0..(i - 1) {
                checkouts.push(pool.checkout());
            }
            b.iter(|| {
                black_box(pool.checkout());
            });
            black_box(checkouts);
    }, POOL_SIZES)
        .with_function("natatorium::fixed", |b, &&i| {
            let pool = fixed::Pool::builder()
                .with_elements(i)
                .with_fn(|| Vec::<u8>::new()).finish();
            let mut checkouts = Vec::new();
            for _ in 0..(i - 1) {
                checkouts.push(pool.checkout());
            }
            b.iter(|| {
                black_box(pool.checkout());
            });
            black_box(checkouts);
        })
        .with_function("pool", |b, &&i| {
            let mut pool = pool::Pool::with_capacity(i, 0, || Vec::<u8>::new());
            let mut checkouts = Vec::new();
            for _ in 0..(i - 1) {
                checkouts.push(pool.checkout().unwrap());
            }
            b.iter(|| {
                black_box(pool.checkout().unwrap());
            });
            black_box(checkouts);
        })
        .with_function("object_pool", |b, &&i| {
            let pool = object_pool::Pool::new(i, || Vec::<u8>::new());
            let mut checkouts = Vec::new();
            for _ in 0..(i - 1) {
                checkouts.push(pool.pull().unwrap());
            }
            b.iter(|| {
                black_box(pool.pull().unwrap());
            });
            black_box(checkouts);
        });
    c.bench("checkout_last", bench);
}


fn checkout_third(c: &mut Criterion) {
    let bench = criterion::ParameterizedBenchmark::new("natatorium::growable", |b, &&i| {
            let pool = growable::Pool::builder()
                .with_elements(i)
                .with_fn(|| Vec::<u8>::new()).finish();
            let mut checkouts = Vec::new();
            for _ in 0..(i/3) {
                checkouts.push(pool.checkout());
            }
            b.iter(|| {
                black_box(pool.checkout());
            });
            black_box(checkouts);
    }, POOL_SIZES)
        .with_function("natatorium::fixed", |b, &&i| {
            let pool = fixed::Pool::builder()
                .with_elements(i)
                .with_fn(|| Vec::<u8>::new()).finish();
            let mut checkouts = Vec::new();
            for _ in 0..(i/3) {
                checkouts.push(pool.checkout());
            }
            b.iter(|| {
                black_box(pool.checkout());
            });
            black_box(checkouts);
        })
        .with_function("pool", |b, &&i| {
            let mut pool = pool::Pool::with_capacity(i, 0, || Vec::<u8>::new());
            let mut checkouts = Vec::new();
            for _ in 0..(i/3) {
                checkouts.push(pool.checkout().unwrap());
            }
            b.iter(|| {
                black_box(pool.checkout().unwrap());
            });
            black_box(checkouts);
        })
        .with_function("object_pool", |b, &&i| {
            let pool = object_pool::Pool::new(i, || Vec::<u8>::new());
            let mut checkouts = Vec::new();
            for _ in 0..(i/3) {
                checkouts.push(pool.pull().unwrap());
            }
            b.iter(|| {
                black_box(pool.pull().unwrap());
            });
            black_box(checkouts);
        });
    c.bench("checkout_third", bench);
}


criterion_group!(benches, checkout_once, checkout_twice, checkout_last, checkout_third);
criterion_main!(benches);
