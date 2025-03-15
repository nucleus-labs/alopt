use criterion::{black_box, criterion_group, criterion_main, Criterion};

use std::sync::Arc;
use std::thread;

use alopt::thread::Wom;

fn thread_wom(c: &mut Criterion) {
    const THREAD_COUNT: usize = 4;
    const ITERS_PER_THREAD: usize = 100;

    c.bench_function("uncontended_lock", |b| {
        b.iter(|| {
            let wom = Wom::<u8>::new(0);
            let guard = wom.try_lock().unwrap();
            let _unused = black_box(guard);
        })
    });

    c.bench_function("short_critical_section", |b| {
        b.iter(|| {
            let wom = Wom::<u8>::new(0);
            let mut data = wom.try_lock().unwrap();
            *data += 1;
            black_box(*data);
        })
    });

    c.bench_function("try_lock", |b| {
        b.iter(|| {
            let wom = Wom::<u8>::new(0);
            if let Ok(mut data) = wom.try_lock() {
                *data += 1;
                black_box(*data);
            }
        })
    });

    c.bench_function("contended_lock", |b| {
        b.iter(|| {
            let wom = Arc::new(Wom::<u8>::new(0));
            let mut handles = Vec::with_capacity(THREAD_COUNT);
            for _ in 0..THREAD_COUNT {
                let m = wom.clone();
                handles.push(thread::spawn(move || {
                    for _ in 0..ITERS_PER_THREAD {
                        let mut data = m.lock().unwrap();
                        *data += 1;
                    }
                }));
            }
            for h in handles {
                h.join().unwrap();
            }
            black_box(&wom);
        })
    });
    
    c.bench_function("heavy_contention", |b| {
        b.iter(|| {
            let wom = Arc::new(Wom::<u8>::new(0));
            let mut handles = Vec::with_capacity(THREAD_COUNT);
            for _ in 0..(THREAD_COUNT << 3) {
                let m = wom.clone();
                handles.push(thread::spawn(move || {
                    for _ in 0..(ITERS_PER_THREAD << 3) {
                        let mut data = m.lock().unwrap();
                        *data += 1;
                    }
                }));
            }
            for h in handles {
                h.join().unwrap();
            }
            black_box(&wom);
        })
    });

    c.bench_function("multi_thread_try_lock", |b| {
        b.iter(|| {
            let mutex = Arc::new(Wom::<u8>::new(0));
            let mut handles = Vec::with_capacity(THREAD_COUNT);
            for _ in 0..THREAD_COUNT {
                let m = mutex.clone();
                handles.push(thread::spawn(move || {
                    for _ in 0..ITERS_PER_THREAD {
                        let _unused = m.try_lock();
                    }
                }));
            }
            for h in handles {
                h.join().unwrap();
            }
            black_box(&mutex);
        })
    });
}

criterion_group!(benches, thread_wom);
criterion_main!(benches);