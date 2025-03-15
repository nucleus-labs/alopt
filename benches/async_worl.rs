use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn async_worl(c: &mut Criterion) {
    todo!()
}

criterion_group!(benches, async_worl);
criterion_main!(benches);