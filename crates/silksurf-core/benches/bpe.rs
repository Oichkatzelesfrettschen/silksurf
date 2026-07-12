use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use silksurf_core::bpe::BpeTokenizer;

/*
 * Mirrors the retired C benchmark (src/neural/bpe_bench.c): the same
 * eight-merge HTML vocabulary over the same fixture, reported as byte
 * throughput so the 50 MB/s tokenization target stays checkable.
 */
fn html_vocab() -> BpeTokenizer {
    let mut bpe = BpeTokenizer::new();
    bpe.add_merge(b"<!DOCTYPE html>", 256);
    bpe.add_merge(b"<html>", 257);
    bpe.add_merge(b"<body>", 258);
    bpe.add_merge(b"</div>", 259);
    bpe.add_merge(b"</span>", 260);
    bpe.add_merge(b" class=\"", 261);
    bpe.add_merge(b" id=\"", 262);
    bpe.add_merge(b"<div>", 263);
    bpe
}

fn bench_encode(c: &mut Criterion) {
    let bpe = html_vocab();
    let fixture: &[u8] =
        b"<!DOCTYPE html><html><body><div class=\"test\">Hello</div></body></html>";
    let page: Vec<u8> = fixture.repeat(1024);

    let mut group = c.benchmark_group("core_bpe_encode");
    group.throughput(Throughput::Bytes(page.len() as u64));
    group.bench_function("html_fixture_x1024", |b| {
        b.iter(|| black_box(bpe.encode(black_box(&page))));
    });
    group.finish();
}

criterion_group!(benches, bench_encode);
criterion_main!(benches);
