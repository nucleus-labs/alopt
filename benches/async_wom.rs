use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn async_wom(c: &mut Criterion) {
    todo!()
}

criterion_group!(benches, async_wom);
criterion_main!(benches);