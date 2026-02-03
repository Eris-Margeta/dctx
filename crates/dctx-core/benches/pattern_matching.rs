use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use dctx_core::ignore::IgnoreEngine;

/// Benchmark glob matching performance
///
/// Measures performance improvement from iterative implementation.
/// Expected: 20-30% faster than recursive version.
fn bench_glob_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_patterns");

    let test_cases = vec![
        ("simple_suffix", "*.rs", "main.rs"),
        ("double_star", "**/test.rs", "src/lib/test.rs"),
        ("nested_pattern", "**/*.txt", "docs/guide/readme.txt"),
        ("complex", "src/**/*.rs", "src/core/util.rs"),
        ("no_match", "*.log", "readme.txt"),
    ];

    for (name, pattern, text) in test_cases {
        let rules = vec![dctx_core::datatypes::IgnoreRule {
            pattern: pattern.to_string(),
            pattern_type: dctx_core::datatypes::PatternType::Glob,
            is_dir_only: false,
            is_negation: false,
        }];

        group.bench_with_input(
            BenchmarkId::new("glob_match", name),
            &(rules.clone(), text),
            |b, (r, t)| {
                b.iter(|| {
                    dctx_core::ignore::should_ignore_item(
                        black_box(t),
                        black_box(t),
                        black_box(false),
                        black_box(r),
                        false,
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmark cache effectiveness
///
/// Measures performance improvement from pattern result caching.
/// Expected: 40-60% faster on cache hits.
fn bench_cache_effectiveness(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_effectiveness");

    // Create temporary directory for engine
    let temp_dir = tempfile::tempdir().unwrap();
    let engine = IgnoreEngine::build(temp_dir.path(), false).unwrap();

    // Test paths that will hit cache
    let test_paths = vec![
        ("node_modules", "packages/app/node_modules", true),
        ("target", "crates/core/target", true),
        ("build", "frontend/build", true),
        ("*.log", "logs/error.log", false),
        ("*.tmp", "cache/temp.tmp", false),
    ];

    group.bench_function("cold_cache", |b| {
        b.iter(|| {
            // Clear cache for each iteration (cold cache)
            engine.match_cache().borrow_mut().clear();

            for (name, path, is_dir) in &test_paths {
                let _ = dctx_core::ignore::should_ignore_item_cached(
                    black_box(path),
                    black_box(name),
                    black_box(*is_dir),
                    engine.ignore_rules(),
                    false,
                    Some(engine.match_cache()),
                );
            }
        });
    });

    group.bench_function("warm_cache", |b| {
        // Prime the cache once
        for (name, path, is_dir) in &test_paths {
            let _ = dctx_core::ignore::should_ignore_item_cached(
                path,
                name,
                *is_dir,
                engine.ignore_rules(),
                false,
                Some(engine.match_cache()),
            );
        }

        b.iter(|| {
            // Reuse cached results (warm cache)
            for (name, path, is_dir) in &test_paths {
                let _ = dctx_core::ignore::should_ignore_item_cached(
                    black_box(path),
                    black_box(name),
                    black_box(*is_dir),
                    engine.ignore_rules(),
                    false,
                    Some(engine.match_cache()),
                );
            }
        });
    });

    group.finish();
}

/// Benchmark realistic workload
///
/// Simulates scanning a monorepo with repeated patterns.
/// Measures overall performance improvement.
fn bench_realistic_workload(c: &mut Criterion) {
    let temp_dir = tempfile::tempdir().unwrap();
    let engine = IgnoreEngine::build(temp_dir.path(), false).unwrap();

    // Simulate 1000 files in a monorepo
    let file_paths: Vec<_> = (0..1000)
        .map(|i| {
            let pkg = i % 10; // 10 packages
            let name = if i % 5 == 0 {
                "node_modules"
            } else if i % 7 == 0 {
                "target"
            } else {
                "src"
            };
            (
                name,
                format!("packages/pkg{}/{}/file{}.rs", pkg, name, i),
                i % 3 == 0,
            )
        })
        .collect();

    c.bench_function("monorepo_scan_1000_files", |b| {
        b.iter(|| {
            engine.match_cache().borrow_mut().clear();

            for (name, path, is_dir) in &file_paths {
                let _ = dctx_core::ignore::should_ignore_item_cached(
                    black_box(path),
                    black_box(name),
                    black_box(*is_dir),
                    engine.ignore_rules(),
                    false,
                    Some(engine.match_cache()),
                );
            }
        });
    });
}

criterion_group!(
    benches,
    bench_glob_patterns,
    bench_cache_effectiveness,
    bench_realistic_workload
);
criterion_main!(benches);
