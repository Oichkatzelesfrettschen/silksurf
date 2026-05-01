use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use silksurf_core::SilkInterner;

fn dataset(prefix: &str, count: usize) -> Vec<String> {
    (0..count).map(|i| format!("{prefix}_{i:05}")).collect()
}

fn bench_insert_heavy(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_interner_insert");
    for &count in &[1_000usize, 10_000] {
        let keys = dataset("insert", count);
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::new("unique_keys", count), &keys, |b, keys| {
            b.iter(|| {
                let mut interner = SilkInterner::new();
                for key in keys {
                    black_box(interner.intern(black_box(key)));
                }
                black_box(interner.len())
            });
        });
    }
    group.finish();
}

fn bench_resolve_path(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_interner_resolve");
    for &count in &[1_000usize, 10_000] {
        let keys = dataset("resolve", count);
        let mut interner = SilkInterner::new();
        let symbols: Vec<_> = keys.iter().map(|k| interner.intern(k)).collect();

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("symbols", count),
            &symbols,
            |b, symbols| {
                b.iter(|| {
                    let total_len = symbols
                        .iter()
                        .map(|&sym| black_box(interner.resolve(black_box(sym))).len())
                        .sum::<usize>();
                    black_box(total_len)
                });
            },
        );
    }
    group.finish();
}

fn bench_repeated_hit(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_interner_hit");
    for &iters in &[10_000usize, 100_000] {
        group.throughput(Throughput::Elements(iters as u64));
        group.bench_with_input(BenchmarkId::new("same_key", iters), &iters, |b, &iters| {
            let mut interner = SilkInterner::new();
            let baseline = interner.intern("repeated_key");
            b.iter(|| {
                for _ in 0..iters {
                    black_box(interner.intern(black_box("repeated_key")));
                }
                black_box(interner.len());
                black_box(baseline)
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_insert_heavy,
    bench_resolve_path,
    bench_repeated_hit
);
criterion_main!(benches);
