use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn std_rwlock(c: &mut Criterion) {
    todo!()
}

criterion_group!(benches, std_rwlock);
criterion_main!(benches);
