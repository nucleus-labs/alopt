use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn thread_worl(c: &mut Criterion) {
    todo!()
}

criterion_group!(benches, thread_worl);
criterion_main!(benches);