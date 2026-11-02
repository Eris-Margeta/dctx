//! Stamp fast path equivalence gate (DCX-P-C8).
//! Two hundred random edits to a real directory; after each one a warm session (one persistent cache, stat
//! stamps trusted) and a cold session (empty cache, everything read and hashed) must answer every verb with
//! the same bytes. The edits include same-size byte swaps, touches, appended functions, new and deleted
//! files and a replace-by-rename that keeps size, bytes and mtime but changes the inode.
//! Known blind spot, not tested here: a content change that keeps size, mtime (ns) and inode of a file older
//! than 2 s. `DCTX_FRESHNESS=strict_live` hashes every file and sees it (covered in dctx-cli `warm_reuse`).

#![forbid(unsafe_code)]
#![cfg(all(unix, feature = "fs"))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use dctx::memory::{CancelFlag, FixedClock};
use dctx::{
    AutoCleanConfig, FsSource, HostCeiling, HostGrants, MapRequest, RuleTexts, SelectionPolicy,
    Session, SnapshotHandle, SnapshotRequest, StoragePolicy, Timestamp, VecSink, Verb,
};

const STEPS: u32 = 200;
const FILES: usize = 10;

/// xorshift64*: a fixed seed makes every run (and every failure) reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).expect("bound fits")
    }
}

fn name_of(index: usize) -> String {
    match index % 3 {
        0 => format!("src/m{index}.rs"),
        1 => format!("lib/m{index}.py"),
        _ => format!("docs/m{index}.md"),
    }
}

/// Source text of file `index` at version `version` (a single digit, so a version swap keeps the size).
fn body(index: usize, version: u32) -> String {
    match index % 3 {
        0 => format!(
            "//! module {index}\n\npub fn calc_{index}_{version}(x: u32) -> u32 {{\n    helper_{index}(x) + 1\n}}\n\npub fn helper_{index}(x: u32) -> u32 {{\n    x * {index}\n}}\n\npub fn top_{index}() -> u32 {{\n    calc_{index}_{version}(2) + helper_{next}(1)\n}}\n",
            next = (index + 1) % FILES
        ),
        1 => format!(
            "\"\"\"module {index}\"\"\"\n\ndef calc_{index}_{version}(x):\n    return helper_{index}(x) + 1\n\n\ndef helper_{index}(x):\n    return x * {index}\n\n\ndef top_{index}():\n    return calc_{index}_{version}(2)\n"
        ),
        _ => format!(
            "# Doc {index} v{version}\n\nSome prose about calc_{index}_{version} and helper_{index}.\n\n## Usage\n\nCall top_{index}.\n"
        ),
    }
}

fn set_mtime(path: &Path, when: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open")
        .set_modified(when)
        .expect("set mtime");
}

/// An mtime far enough in the past to be outside the 2 s racy window, different for every call.
fn aged(step: u32, salt: u64) -> SystemTime {
    SystemTime::now()
        - Duration::from_secs(3_600 + u64::from(step) * 7)
        - Duration::from_millis(salt % 997)
}

fn open(root: &Path, cache: &Path) -> Session {
    let auto = AutoCleanConfig { enabled: false, ..AutoCleanConfig::default() };
    Session::builder(FsSource::open(root).expect("source"))
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral {
            max_bytes: 512 * 1024 * 1024,
        })
        .place("fixture")
        .view("default")
        .cache_root(cache, 1)
        .auto_clean(auto)
        .build()
        .expect("session")
}

type Out = Result<Vec<u8>, String>;

fn run(session: &mut Session, handle: &SnapshotHandle, request: &MapRequest) -> Out {
    let mut sink = VecSink::new();
    session
        .query(handle, request, &mut sink)
        .map_err(|error| error.to_string())?;
    Ok(sink.as_bytes().to_vec())
}

fn ids_of(session: &mut Session, handle: &SnapshotHandle, query: &str) -> Vec<String> {
    let mut request = MapRequest::new(Verb::Find);
    request.query = query.to_string();
    let mut sink = VecSink::new();
    match session.query(handle, &request, &mut sink) {
        Ok(answer) => answer.records.into_iter().map(|record| record.id).collect(),
        Err(_) => Vec::new(),
    }
}

/// Every verb the map offers, with targets taken from the answers of this very snapshot.
fn outputs(session: &mut Session, handle: &SnapshotHandle) -> Vec<(String, Out)> {
    let mut rows = Vec::new();
    for query in ["calc", "helper", "top", "module", "Usage"] {
        let mut find = MapRequest::new(Verb::Find);
        find.query = query.to_string();
        rows.push((format!("find {query}"), run(session, handle, &find)));
    }
    let mut targets = ids_of(session, handle, "calc");
    targets.extend(ids_of(session, handle, "top").into_iter().take(2));
    targets.extend(ids_of(session, handle, "Usage").into_iter().take(1));
    for id in targets.iter().take(6) {
        for verb in [Verb::Explain, Verb::Read, Verb::Neighbors, Verb::Map] {
            let mut request = MapRequest::new(verb);
            request.target = id.clone();
            rows.push((
                format!("{} {id}", verb.as_str()),
                run(session, handle, &request),
            ));
        }
    }
    for index in [0, 1, 2, 4] {
        let mut skeleton = MapRequest::new(Verb::Skeleton);
        skeleton.target = format!("m:{}", name_of(index));
        rows.push((format!("skeleton {index}"), run(session, handle, &skeleton)));
    }
    for literal in ["return", "helper", "Doc"] {
        let mut grep = MapRequest::new(Verb::Grep);
        grep.query = literal.to_string();
        rows.push((format!("grep {literal}"), run(session, handle, &grep)));
    }
    let mut hubs_sink = VecSink::new();
    let hubs = session
        .hubs(handle, &MapRequest::new(Verb::Neighbors), &mut hubs_sink)
        .map(|()| hubs_sink.as_bytes().to_vec());
    rows.push(("hubs".to_string(), hubs.map_err(|error| error.to_string())));
    let export = session
        .export_graph(handle, "json", &MapRequest::new(Verb::Map))
        .map(String::into_bytes);
    rows.push((
        "export".to_string(),
        export.map_err(|error| error.to_string()),
    ));
    if let [from, to, ..] = targets.as_slice() {
        let mut path_sink = VecSink::new();
        let path = session
            .shortest_path(
                handle,
                from,
                to,
                &MapRequest::new(Verb::Neighbors),
                &mut path_sink,
            )
            .map(|()| path_sink.as_bytes().to_vec());
        rows.push(("path".to_string(), path.map_err(|error| error.to_string())));
    }
    rows
}

struct World {
    root: tempfile::TempDir,
    warm_cache: tempfile::TempDir,
    version: [u32; FILES],
    present: [bool; FILES],
    extra: u32,
}

impl World {
    fn path(&self, index: usize) -> PathBuf {
        self.root.path().join(name_of(index))
    }

    fn write(&self, index: usize) {
        let path = self.path(index);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(&path, body(index, self.version[index])).expect("write");
    }

    fn live(&self) -> Vec<usize> {
        (0..FILES).filter(|index| self.present[*index]).collect()
    }
}

#[test]
fn stamp_equiv_two_hundred_random_edits_answer_every_verb_like_a_cold_rebuild() {
    let mut world = World {
        root: tempfile::tempdir().expect("root"),
        warm_cache: tempfile::tempdir().expect("warm cache"),
        version: [0; FILES],
        present: [true; FILES],
        extra: 0,
    };
    for index in 0..FILES {
        world.write(index);
        set_mtime(&world.path(index), aged(0, index as u64 * 31));
    }
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut fast_steps = 0u32;
    let mut skipped_total = 0u64;
    let mut kinds = [0u32; 10];

    for step in 0..=STEPS {
        // Step 0 is the cold start of the warm cache; every later step applies one random edit first.
        let mut must_rebuild = false;
        let mut must_reuse = false;
        if step > 0 {
            let live = world.live();
            let pick = live[rng.below(live.len())];
            let kind = rng.below(10);
            kinds[kind] += 1;
            match kind {
                // Same-size byte swap: the digit of the version changes, the length does not.
                0 | 7 => {
                    world.version[pick] =
                        (world.version[pick] + 1 + u32::try_from(rng.below(8)).expect("small"))
                            % 10;
                    world.write(pick);
                    must_rebuild = true;
                    if kind == 7 || rng.below(2) == 0 {
                        set_mtime(&world.path(pick), aged(step, rng.next()));
                    }
                }
                // Append a function: the size changes.
                1 => {
                    world.extra += 1;
                    let mut text = std::fs::read_to_string(world.path(pick)).expect("read");
                    match pick % 3 {
                        0 => text.push_str(&format!(
                            "\npub fn extra_{}() -> u32 {{\n    helper_{pick}(3)\n}}\n",
                            world.extra
                        )),
                        1 => text.push_str(&format!(
                            "\n\ndef extra_{}():\n    return helper_{pick}(3)\n",
                            world.extra
                        )),
                        _ => text.push_str(&format!("\n## Extra {}\n\nmore prose\n", world.extra)),
                    }
                    std::fs::write(world.path(pick), text).expect("append");
                    must_rebuild = true;
                    if rng.below(2) == 0 {
                        set_mtime(&world.path(pick), aged(step, rng.next()));
                    }
                }
                // Touch: same bytes, new mtime.
                2 => set_mtime(&world.path(pick), aged(step, rng.next())),
                // Nothing changes.
                3 => must_reuse = true,
                // Delete a file, keeping at least six.
                4 if live.len() > 6 => {
                    std::fs::remove_file(world.path(pick)).expect("remove");
                    world.present[pick] = false;
                    must_rebuild = true;
                }
                // Add back a deleted file (or rewrite a live one as new bytes).
                5 => {
                    let gone: Vec<usize> =
                        (0..FILES).filter(|index| !world.present[*index]).collect();
                    let target = gone.first().copied().unwrap_or(pick);
                    world.present[target] = true;
                    world.version[target] = (world.version[target] + 3) % 10;
                    world.write(target);
                    must_rebuild = true;
                    set_mtime(&world.path(target), aged(step, rng.next()));
                }
                // Replace by rename: identical bytes, size and mtime, a new inode.
                6 => {
                    let path = world.path(pick);
                    let modified = std::fs::metadata(&path)
                        .expect("meta")
                        .modified()
                        .expect("mtime");
                    let bytes = std::fs::read(&path).expect("read");
                    let temp = path.with_extension("swap");
                    std::fs::write(&temp, bytes).expect("copy");
                    set_mtime(&temp, modified);
                    std::fs::rename(&temp, &path).expect("rename");
                    must_reuse = true;
                }
                // A racy file edited twice with the same size and the very same mtime, the second edit after a
                // snapshot saw the first. Only the 2 s racy window keeps that second edit from being trusted.
                9 => {
                    let pinned = SystemTime::now() - Duration::from_millis(300);
                    world.version[pick] = (world.version[pick] + 1) % 10;
                    world.write(pick);
                    set_mtime(&world.path(pick), pinned);
                    let mut mid = open(world.root.path(), world.warm_cache.path());
                    drop(
                        mid.snapshot_reusable(&SnapshotRequest::strict(), "/src")
                            .expect("mid snapshot"),
                    );
                    drop(mid);
                    world.version[pick] = (world.version[pick] + 4) % 10;
                    world.write(pick);
                    set_mtime(&world.path(pick), pinned);
                    must_rebuild = true;
                }
                // Two edits to two different files in one go, one of them left racy (just written, not aged).
                _ => {
                    let other = live[rng.below(live.len())];
                    for index in [pick, other] {
                        world.version[index] = (world.version[index] + 1) % 10;
                        world.write(index);
                    }
                    set_mtime(&world.path(pick), aged(step, rng.next()));
                    must_rebuild = true;
                }
            }
        }

        let mut warm = open(world.root.path(), world.warm_cache.path());
        let warm_handle = warm
            .snapshot_reusable(&SnapshotRequest::strict(), "/src")
            .expect("warm snapshot");
        let reused = warm_handle.reused();
        let skipped = warm.stamp_skipped_reads();
        let warm_rows = outputs(&mut warm, &warm_handle);

        let cold_cache = tempfile::tempdir().expect("cold cache");
        let mut cold = open(world.root.path(), cold_cache.path());
        let cold_handle = cold
            .snapshot_reusable(&SnapshotRequest::strict(), "/src")
            .expect("cold snapshot");
        assert!(
            !cold_handle.reused(),
            "step {step}: an empty cache cannot reuse"
        );
        let cold_rows = outputs(&mut cold, &cold_handle);

        assert_eq!(warm_rows.len(), cold_rows.len(), "step {step}");
        assert!(
            warm_rows.len() > 30,
            "step {step}: the verb set shrank to {}",
            warm_rows.len()
        );
        for ((label, left), (_, right)) in warm_rows.iter().zip(&cold_rows) {
            if left != right {
                let show = |out: &Out| match out {
                    Ok(bytes) => String::from_utf8_lossy(bytes)
                        .chars()
                        .take(400)
                        .collect::<String>(),
                    Err(error) => format!("error: {error}"),
                };
                panic!(
                    "step {step} (edit kinds so far {kinds:?}): `{label}` differs\nwarm: {}\ncold: {}",
                    show(left),
                    show(right)
                );
            }
        }
        if must_rebuild {
            assert!(
                !reused,
                "step {step}: a changed file must not be served from the old generation"
            );
        }
        if must_reuse {
            assert!(
                reused,
                "step {step}: nothing changed in content, the generation must be reused"
            );
        }
        if reused && skipped > 0 {
            fast_steps += 1;
        }
        skipped_total += skipped;
    }

    assert!(
        kinds.iter().all(|count| *count > 0),
        "every edit kind ran: {kinds:?}"
    );
    assert!(
        fast_steps >= 20,
        "the stamp fast path served only {fast_steps} of {STEPS} steps"
    );
    assert!(
        skipped_total >= 100,
        "only {skipped_total} files were trusted by their stamp"
    );
}
