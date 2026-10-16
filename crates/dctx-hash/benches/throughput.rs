//! Throughput of the streaming SHA-256 content hasher.
//! A measurement only: `cargo test` does not run or assert this bench.
//! Depends on criterion and [`dctx_hash::Sha256`]. Used when comparing the strict profile.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use dctx_hash::Sha256;

fn sha256_throughput(c: &mut Criterion) {
    let data = vec![0_u8; 64 * 1024];
    let mut group = c.benchmark_group("sha256");
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function("stream_64kib", |b| {
        b.iter(|| {
            let mut hasher = Sha256::new();
            hasher.update(black_box(&data)).unwrap();
            black_box(hasher.finalize())
        });
    });
    group.finish();
}

criterion_group!(benches, sha256_throughput);
criterion_main!(benches);
