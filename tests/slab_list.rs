use loom::{sync::Arc, thread};
use natatorium::SlabStack;

#[test]
fn push_single_threaded() {
    loom::model(|| {
        let l: SlabStack<usize> = SlabStack::with_capacity(8);
        assert_eq!(l.capacity(), 8);
        assert_eq!(l.len(), 0);

        assert_eq!(l.get(0), None);
        assert_eq!(l.get(1), None);

        l.push(0);

        assert_eq!(l.get(0), Some(&0));
        assert_eq!(l.get(1), None);
        assert_eq!(l.len(), 1);

        for i in 1..16 {
            assert_eq!(l.get(i - 1), Some(&(i - 1)));
            assert_eq!(l.get(i), None);
            assert_eq!(l.get(i + 1), None);

            l.push(i);

            assert_eq!(l.get(i - 1), Some(&(i - 1)));
            assert_eq!(l.get(i), Some(&i));
            assert_eq!(l.get(i + 1), None);
            assert_eq!(l.len(), i + 1);
        }
    })
}

#[test]
fn push_multithreaded() {
    let mut loom = loom::model::Builder::new();
    loom.max_threads = 8;
    loom.check(|| {
        let l: SlabStack<usize> = SlabStack::with_capacity(2);
        assert_eq!(l.capacity(), 2);
        assert_eq!(l.len(), 0);

        let l = Arc::new(l);
        (0..3)
            .map(|i| {
                let l = l.clone();
                let j = thread::spawn(move || l.push(i));
                (i, j)
            })
            .map(|(i, j)| {
                let l = l.clone();
                let n = j.join().unwrap();
                thread::spawn(move || {
                    assert_eq!(l.get(n), Some(&i));
                })
            })
            .for_each(|j| j.join().unwrap())
    })
}
