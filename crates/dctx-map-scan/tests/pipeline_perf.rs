// SPDX-License-Identifier: Apache-2.0
//! Run the large fixture with --release --test pipeline_perf -- --ignored --nocapture.
#[path = "support/allocations.rs"]
mod allocations;
#[path = "support/repo.rs"]
mod repo;

use allocations::{CountingAllocator, measure};
use dctx_map_scan::{adapters::fs::StdFileSystem, content_hash, scan};
use dctx_map_segment::{SegmentRole, decode};
use std::{fs, path::Path};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn pipeline(root: &Path) -> Vec<u8> {
    let records = scan(root, &repo::engine(root), &StdFileSystem, false).unwrap();
    let (fact, view) = repo::builders(&records);
    let fact = fact.build().unwrap();
    let view = view.build().unwrap();
    decode(&fact, SegmentRole::Fact).unwrap();
    decode(&view, SegmentRole::View).unwrap();
    repo::output(&records, &fact, &view)
}

fn assert_bytes(actual: &[u8], expected: &[u8]) {
    assert_eq!(actual.len(), expected.len(), "output length changed");
    assert!(
        actual == expected,
        "output differs at byte {:?}",
        actual.iter().zip(expected).position(|(a, b)| a != b)
    );
}

fn deterministic(root: &Path, expected: &[u8]) {
    // These APIs are serial (IgnoreEngine is not Sync). Exercise independent
    // callers on real 1/2/4-thread groups; no pretend Rayon thread-count knob.
    for threads in [1, 2, 4] {
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|_| scope.spawn(|| pipeline(root)))
                .collect();
            for worker in workers {
                assert_bytes(&worker.join().unwrap(), expected);
            }
        });
    }
}

#[test]
fn seeded_pipeline_matches_baseline_bytes_and_thread_counts() {
    let temp = tempfile::tempdir().unwrap();
    repo::generate(temp.path(), 1_000);
    let expected =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/pipeline-1000.bin"))
            .unwrap();
    assert_bytes(&pipeline(temp.path()), &expected);
    assert_bytes(&pipeline(temp.path()), &expected);
    deterministic(temp.path(), &expected);
}

#[test]
// Heavy benchmark (1k/10k/50k files); runs only when DCTX_PERF_SIZE or DCTX_PERF=1 is set.
fn seeded_pipeline_benchmark() {
    if std::env::var_os("DCTX_PERF_SIZE").is_none() && std::env::var_os("DCTX_PERF").is_none() {
        return;
    }
    let selected = std::env::var("DCTX_PERF_SIZE")
        .ok()
        .map(|s| s.parse::<usize>().unwrap());
    assert!(selected.is_none_or(|n| [1_000, 10_000, 50_000].contains(&n)));
    for count in [1_000, 10_000, 50_000] {
        if selected.is_some_and(|n| n != count) {
            continue;
        }
        let temp = tempfile::tempdir().unwrap();
        repo::generate(temp.path(), count);
        let mut expected: Option<Vec<u8>> = None;
        for pass in 0..3 {
            let ((records, _engine), scan_time, scan_alloc) = measure(|| {
                let engine = repo::engine(temp.path());
                let records = scan(temp.path(), &engine, &StdFileSystem, false).unwrap();
                (records, engine)
            });
            // Draft preparation is separate: report the segment codec itself.
            let (fact, view) = repo::builders(&records);
            let ((fact, view), segment_time, segment_alloc) =
                measure(|| (fact.build().unwrap(), view.build().unwrap()));
            decode(&fact, SegmentRole::Fact).unwrap();
            decode(&view, SegmentRole::View).unwrap();
            let output = repo::output(&records, &fact, &view);
            println!(
                "files={count} pass={pass} scan_ms={:.3} scan_allocs={} scan_peak={} segment_ms={:.3} segment_allocs={} segment_peak={} digest={:02x?}",
                scan_time.as_secs_f64() * 1000.0,
                scan_alloc.allocations,
                scan_alloc.peak,
                segment_time.as_secs_f64() * 1000.0,
                segment_alloc.allocations,
                segment_alloc.peak,
                content_hash(&output)
            );
            if let Some(expected) = &expected {
                assert_bytes(&output, expected);
            } else {
                expected = Some(output);
            }
        }
        let expected = expected.unwrap();
        assert_eq!(content_hash(&expected), repo::baseline_digest(count));
        // Optional before/after artifacts: create once on the baseline, then
        // require byte equality on every subsequent run. Never overwrite them.
        if let Some(dir) = std::env::var_os("DCTX_PERF_BASELINE") {
            use std::io::Write;
            let path = Path::new(&dir).join(format!("pipeline-{count}.bin"));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => file.write_all(&expected).unwrap(),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    assert_bytes(&expected, &fs::read(path).unwrap())
                }
                Err(error) => panic!("baseline artifact: {error}"),
            }
        }
        deterministic(temp.path(), &expected);
    }
}
