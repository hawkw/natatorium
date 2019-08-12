
use criterion::Criterion;
use criterion::{criterion_group, criterion_main};
use criterion::black_box;
use natatorium::{growable, fixed};
use pool;

static KB: usize = 1024;
static MB: usize = 1024 * KB;
static GB: usize = 1024 * MB;
static SIZES: &[usize] = &[
    4 * KB, 16 * KB, 64 * KB, 128 * KB, 512 * KB, 1 * MB, 16 * MB, 32 * MB, 64 * MB, 128 * MB,
    256 * MB, 512 * MB, 1 * GB, // 2 * GB, 3 * GB,
];


fn checkout(c: &mut Criterion) {
    let bench = criterion::ParameterizedBenchmark::new("natatorium::growable", |b, &&i| {
            let pool = growable::Pool::builder()
                .with_elements(1)
                .with_fn(|| Vec::<u8>::with_capacity(i)).finish();
            b.iter(|| {
                black_box(pool.checkout());
            })
    }, SIZES)
        .with_function("natatorium::fixed", |b, &&i| {
            let pool = growable::Pool::builder()
                .with_elements(1)
                .with_fn(|| Vec::<u8>::with_capacity(i)).finish();
            b.iter(|| {
                black_box(pool.checkout());
            })
        })
        .with_function("alloc", |b, &&i| {
            b.iter(|| {
                black_box(Vec::<u8>::with_capacity(i));
            })
        })
        .with_function("pool", |b, &&i| {
            let mut pool = pool::Pool::with_capacity(1, 0, || Vec::<u8>::with_capacity(i));
            b.iter(|| {
                black_box(pool.checkout().unwrap());
            })
        });
    c.bench("checkout_one", bench);
}

// fn checkout_checkin(c: &mut Criterion) {
//     let bench = criterion::ParameterizedBenchmark::new("natatorium::growable", |b, &&i| {
//             let pool = growable::Pool::builder()
//                 .with_elements(1)
//                 .with_fn(|| Vec::<u8>::with_capacity(i)).finish();
//             b.iter(|| {
//                 let ch = pool.checkout();
//                 black_box(&ch);
//                 drop(ch);
//             })
//     }, SIZES)
//         .with_function("natatorium::fixed", |b, &&i| {
//             let pool = growable::Pool::builder()
//                 .with_elements(1)
//                 .with_fn(|| Vec::<u8>::with_capacity(i)).finish();
//             b.iter(|| {
//                 let ch = pool.checkout();
//                 black_box(&ch);
//                 drop(ch);
//             })
//         })
//         .with_function("alloc", |b, &&i| {
//             b.iter(|| {
//                 let ch = Vec::<u8>::with_capacity(i);
//                 black_box(&ch);
//                 drop(ch);
//             })
//         });
//     c.bench("checkout_one", bench);
// }

criterion_group!(benches, checkout);
criterion_main!(benches);
