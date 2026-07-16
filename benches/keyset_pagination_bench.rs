use criterion::{criterion_group, criterion_main, Criterion};

fn keyset_slice(rows: &[(i64, u64)], cursor: (i64, u64), limit: usize) -> Vec<(i64, u64)> {
    let start = rows.partition_point(|row| *row >= cursor);
    rows[start..start + limit].to_vec()
}

fn offset_slice(rows: &[(i64, u64)], offset: usize, limit: usize) -> Vec<(i64, u64)> {
    rows.iter().skip(offset).take(limit).copied().collect()
}

fn bench_depth_10k(c: &mut Criterion) {
    let rows: Vec<_> = (0..1_000_000).rev().map(|i| (i, i as u64)).collect();
    c.bench_function("keyset_depth_10000", |b| {
        b.iter(|| keyset_slice(&rows, rows[10_000], 50))
    });
    c.bench_function("offset_depth_10000", |b| {
        b.iter(|| offset_slice(&rows, 10_000, 50))
    });
}

criterion_group!(benches, bench_depth_10k);
criterion_main!(benches);
