use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn std_rwlock(c: &mut Criterion) {
    todo!()
}

criterion_group!(benches, std_rwlock);
criterion_main!(benches);