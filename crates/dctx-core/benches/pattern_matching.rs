// SPDX-License-Identifier: Apache-2.0
//! Pure selection-policy benchmarks for D3-07: glob cases and a 1000-path workload.
//! Depends on Criterion and the public SelectionPolicy/ItemContext API.
//! The former cold/warm cache comparison becomes cold-policy-build versus
//! repeated-decide: the pure policy has no match cache. Used by Criterion only.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use dctx_core::selection::RuleTexts;
use dctx_core::{ItemContext, SelectionPolicy};

fn bench_glob_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_patterns");
    let test_cases = [
        ("simple_suffix", "*.rs", "main.rs"),
        ("double_star", "**/test.rs", "src/lib/test.rs"),
        ("nested_pattern", "**/*.txt", "docs/guide/readme.txt"),
        ("complex", "src/**/*.rs", "src/core/util.rs"),
        ("no_match", "*.log", "readme.txt"),
    ];

    for (name, pattern, path) in test_cases {
        let policy = SelectionPolicy::from_texts(&RuleTexts {
            local_ignore: Some(pattern),
            ..RuleTexts::default()
        });
        let context = ItemContext {
            relative_path: path,
            name: path.rsplit('/').next().unwrap_or(path),
            is_directory: false,
            debug_mode: false,
        };
        group.bench_with_input(BenchmarkId::new("glob_match", name), &context, |b, item| {
            b.iter(|| black_box(policy.decide(black_box(item))));
        });
    }
    group.finish();
}

fn bench_policy_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_matching");
    let texts = RuleTexts {
        local_ignore: Some("node_modules/\ntarget/\nbuild/\n*.log\n*.tmp"),
        ..RuleTexts::default()
    };
    let policy = SelectionPolicy::from_texts(&texts);
    let test_paths = [
        ("node_modules", "packages/app/node_modules", true),
        ("target", "crates/core/target", true),
        ("build", "frontend/build", true),
        ("error.log", "logs/error.log", false),
        ("temp.tmp", "cache/temp.tmp", false),
    ];
    let contexts = test_paths.map(|(name, relative_path, is_directory)| ItemContext {
        relative_path,
        name,
        is_directory,
        debug_mode: false,
    });

    group.bench_function("cold_policy_build", |b| {
        b.iter(|| {
            let cold_policy = SelectionPolicy::from_texts(black_box(&texts));
            black_box(cold_policy.ignore_rules());
            for context in &contexts {
                black_box(cold_policy.decide(black_box(context)));
            }
        });
    });
    group.bench_function("repeated_decide", |b| {
        b.iter(|| {
            for context in &contexts {
                black_box(policy.decide(black_box(context)));
            }
        });
    });
    group.finish();
}

fn bench_realistic_workload(c: &mut Criterion) {
    let policy = SelectionPolicy::from_texts(&RuleTexts::default());
    let file_paths: Vec<_> = (0..1000)
        .map(|i| {
            let package = i % 10;
            let directory = if i % 5 == 0 {
                "node_modules"
            } else if i % 7 == 0 {
                "target"
            } else {
                "src"
            };
            format!("packages/pkg{package}/{directory}/file{i}.rs")
        })
        .collect();
    let contexts: Vec<_> = file_paths
        .iter()
        .map(|path| ItemContext {
            relative_path: path,
            name: path.rsplit('/').next().unwrap_or(path),
            is_directory: false,
            debug_mode: false,
        })
        .collect();

    c.bench_function("monorepo_scan_1000_files", |b| {
        b.iter(|| {
            for context in &contexts {
                black_box(policy.decide(black_box(context)));
            }
        });
    });
}

criterion_group!(
    benches,
    bench_glob_patterns,
    bench_policy_matching,
    bench_realistic_workload
);
criterion_main!(benches);
