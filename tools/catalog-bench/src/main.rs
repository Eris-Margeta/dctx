// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
//! Linux benchmark CLI. Outputs one key=value per line for reproducible capture.

use catalog_bench::{
    Catalog, Entry, Metadata, QueryMode, Tier, walk, write_catalog, write_snapshot,
};
use std::hint::black_box;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Instant;

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmHWM:")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn queries(catalog: &Catalog) -> io::Result<()> {
    if catalog.is_empty() {
        println!("queries=0\nquery_p50_ms=NA\nquery_p99_ms=NA");
        return Ok(());
    }
    let mut state = 0x00dc_7ca7_a101_u64;
    let mut samples = Vec::with_capacity(1000);
    let mut latencies = Vec::with_capacity(1000);
    let mut tokens = Vec::new();
    let mut sample_attempts = 0;
    // Uniform random file, then uniform random nonempty token from its basename.
    // Include common extensions; no selective-query filtering or result limits.
    while samples.len() < 1000 && sample_attempts < 100_000 {
        sample_attempts += 1;
        let id =
            usize::try_from(random(&mut state) % u64::try_from(catalog.len()).unwrap()).unwrap();
        let name = catalog.basename(id)?;
        tokens.clear();
        tokens.extend(
            name.split(|&byte| !(byte.is_ascii_alphanumeric() || byte >= 0x80))
                .filter(|part| !part.is_empty()),
        );
        if tokens.is_empty() {
            continue;
        }
        let at =
            usize::try_from(random(&mut state) % u64::try_from(tokens.len()).unwrap()).unwrap();
        samples.push(tokens[at].to_vec());
    }
    if samples.is_empty() {
        // A tree consisting entirely of punctuation-only names still gets 1,000 miss queries.
        samples.resize(1000, b"catalogmissingtoken".to_vec());
    } else {
        while samples.len() < 1000 {
            samples.push(samples[0].clone());
        }
    }
    for (label, mode, limit) in [
        ("exact_page", QueryMode::Exact, 100),
        ("prefix_page", QueryMode::Prefix, 100),
        ("substring_page", QueryMode::Substring, 100),
        ("exact_full", QueryMode::Exact, usize::MAX),
    ] {
        let mut lookup_latencies = Vec::new();
        let mut matches = 0_u64;
        let mut path_bytes = 0_u64;
        latencies.clear();
        for sample in &samples {
            // Keep short/common tokens. Longer prefix/substring probes use half a token.
            let token = match mode {
                QueryMode::Exact => sample.as_slice(),
                QueryMode::Prefix => &sample[..sample.len().div_ceil(2)],
                QueryMode::Substring => &sample[sample.len() / 4..(sample.len() * 3).div_ceil(4)],
            };
            let start = Instant::now();
            let found = catalog.search(black_box(token), mode, limit);
            if label == "exact_full" {
                lookup_latencies.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            for id in found {
                let entry = catalog.entry(id)?;
                path_bytes += u64::try_from(black_box(entry.path).len()).unwrap();
                matches += 1;
            }
            latencies.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        if !lookup_latencies.is_empty() {
            lookup_latencies.sort_unstable_by(f64::total_cmp);
            println!(
                "exact_full_lookup_p50_ms={:.6}\nexact_full_lookup_p99_ms={:.6}",
                lookup_latencies[499], lookup_latencies[989]
            );
        }
        latencies.sort_unstable_by(f64::total_cmp);
        println!(
            "{label}_queries={}\n{label}_p50_ms={:.6}\n{label}_p99_ms={:.6}\n{label}_matches={matches}\n{label}_path_bytes={path_bytes}",
            samples.len(),
            latencies[499],
            latencies[989]
        );
    }
    println!("query_seed=0xdc7ca7a101\npage_limit=100");
    Ok(())
}

#[allow(
    clippy::cast_precision_loss,
    reason = "human-readable ratios; exact integer counters are also emitted"
)]
fn finish(output: &Path, build_start: Instant) -> io::Result<()> {
    println!(
        "build_seconds={:.6}\nrss_after_build_kib={}",
        build_start.elapsed().as_secs_f64(),
        rss_kib()
    );
    let start = Instant::now();
    let catalog = Catalog::open(output)?;
    println!(
        "reopen_validate_seconds={:.6}\nfiles={}\ncatalog_bytes={}\ncatalog_bytes_per_file={:.6}\nname_index_bytes={}\nbase_catalog_bytes={}\ngit_roots={}\nshards={}",
        start.elapsed().as_secs_f64(),
        catalog.len(),
        catalog.size(),
        if catalog.is_empty() {
            0.0
        } else {
            catalog.size() as f64 / catalog.len() as f64
        },
        catalog.name_index_size(),
        catalog.size() - catalog.name_index_size(),
        catalog.git_roots(),
        catalog.shards()
    );
    queries(&catalog)?;
    println!("rss_peak_kib={}", rss_kib());
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "small experiment keeps CLI dispatch and reporting in one place"
)]
#[allow(
    clippy::cast_precision_loss,
    reason = "throughput is approximate; exact file count and elapsed seconds are emitted"
)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    let Some(command) = args.get(1).and_then(|v| v.to_str()) else {
        return Err("usage: catalog-bench build ROOT OUTPUT [THREADS] | synthetic OUTPUT FILES | query CATALOG TOKEN [exact|prefix|substring] [LIMIT] | inspect CATALOG".into());
    };
    match command {
        "build" if (4..=5).contains(&args.len()) => {
            let root = Path::new(&args[2]);
            let output = Path::new(&args[3]);
            let threads = args
                .get(4)
                .map(|v| {
                    v.to_str()
                        .ok_or("invalid threads")?
                        .parse::<usize>()
                        .map_err(|_| "invalid threads")
                })
                .transpose()?
                .unwrap_or(std::thread::available_parallelism()?.get());
            let output_parent =
                std::fs::canonicalize(output.parent().ok_or("output needs a parent directory")?)?;
            if std::fs::canonicalize(root)? == output_parent {
                return Err("put output in a scratch subdirectory".into());
            }
            let start = Instant::now();
            let snapshot = walk(root, threads, Some(&output_parent))?;
            println!(
                "mode=filesystem\nthreads={threads}\nwalk_seconds={:.6}\nwalk_files_per_second={:.3}\nwalk_files={}\ndirectories={}\nwalk_errors={}\nsymlinks_skipped={}\nrss_after_walk_kib={}",
                snapshot.elapsed.as_secs_f64(),
                snapshot.entries.len() as f64 / snapshot.elapsed.as_secs_f64(),
                snapshot.entries.len(),
                snapshot.directories.len(),
                snapshot.errors.len(),
                snapshot.symlinks_skipped,
                rss_kib()
            );
            for error in snapshot.errors.iter().take(10) {
                eprintln!("walk error: {error}");
            }
            let incomplete = !snapshot.errors.is_empty();
            let encode_start = Instant::now();
            write_snapshot(output, snapshot)?;
            println!(
                "encode_sync_seconds={:.6}",
                encode_start.elapsed().as_secs_f64()
            );
            finish(output, start)?;
            if incomplete {
                return Err("incomplete snapshot: walk errors reported above".into());
            }
        }
        "synthetic" if args.len() == 4 => {
            let output = Path::new(&args[2]);
            let count: usize = args[3].to_str().ok_or("invalid count")?.parse()?;
            let start = Instant::now();
            let mut entries = Vec::with_capacity(count);
            let mut dirs = vec![(Vec::new(), true)];
            for dir in 0..count.div_ceil(1000) {
                dirs.push((format!("src{dir:06}").into_bytes(), false));
            }
            for id in 0..count {
                entries.push(Entry {
                    path: format!("src{:06}/module{id:08}_test.rs", id / 1000).into_bytes(),
                    metadata: Metadata {
                        size: 1234,
                        mtime_seconds: 1_790_000_000,
                        mtime_nanos: 123_456_789,
                        inode: u64::try_from(id)? + 1000,
                        device: 2049,
                        tier: Tier::Code,
                    },
                });
            }
            println!("mode=synthetic_metadata_only\nwalk_seconds=NA\nwalk_files_per_second=NA");
            write_catalog(output, &mut entries, &mut dirs)?;
            drop(entries);
            drop(dirs);
            finish(output, start)?;
        }
        "query" if (4..=6).contains(&args.len()) => {
            let catalog = Catalog::open(Path::new(&args[2]))?;
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let mode = match args.get(4).and_then(|a| a.to_str()).unwrap_or("exact") {
                "exact" => QueryMode::Exact,
                "prefix" => QueryMode::Prefix,
                "substring" => QueryMode::Substring,
                _ => return Err("invalid query mode".into()),
            };
            let limit = args
                .get(5)
                .map(|s| {
                    s.to_str()
                        .ok_or("invalid limit")?
                        .parse::<usize>()
                        .map_err(|_| "invalid limit")
                })
                .transpose()?
                .unwrap_or(usize::MAX);
            for id in catalog.search(args[3].as_bytes(), mode, limit) {
                // NUL-delimited raw path bytes, preserving non-UTF-8 and newlines.
                out.write_all(&catalog.entry(id)?.path)?;
                out.write_all(&[0])?;
            }
        }
        "inspect" if args.len() == 3 => {
            let start = Instant::now();
            let catalog = Catalog::open(Path::new(&args[2]))?;
            println!(
                "files={}\ncatalog_bytes={}\ngit_roots={}\nshards={}\nreopen_validate_seconds={:.6}\nrss_peak_kib={}",
                catalog.len(),
                catalog.size(),
                catalog.git_roots(),
                catalog.shards(),
                start.elapsed().as_secs_f64(),
                rss_kib()
            );
        }
        _ => return Err("invalid command/arguments (run without arguments for usage)".into()),
    }
    Ok(())
}
