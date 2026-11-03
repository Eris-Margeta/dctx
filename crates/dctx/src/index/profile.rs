// SPDX-License-Identifier: Apache-2.0
//! Opt-in monotonic stage and per-file scan counters.
//! Implements DCTX-SERIOUS-GAPS G7 and DCTX-COLD-SCAN C5.
//! Depends on `std::time::Instant`. The walk and the session record samples on this thread.
//! The map CLI prints [`report`] on stderr. Profiling does not change stdout or the graph.
//! A file watch closes before canonicalize, persist and query, so finalization is not that file's parse.
//! `span conversion` is the module newline scan; per-node spans inside the lexer stay in `parse/lex`.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

/// Stages printed by the map profile, in report order.
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    /// Directory walk outside read, hash, parse, span conversion and graph assembly.
    Walk = 0,
    /// Source bytes read for one included file.
    Read = 1,
    /// Content digest of those bytes.
    Hash = 2,
    /// Lexer or structural symbol extraction. Per-node span indexes inside it stay here.
    ParseLex = 3,
    /// Module newline scan in the walk.
    SpanConversion = 4,
    /// Stable ids and contains edges for one file.
    GraphAssembly = 5,
    /// Canonical payload and, on the ephemeral path, segment seal.
    CanonicalizeSeal = 6,
    /// Persisted generation encode, cache store, or shared reuse finalization.
    PersistReuse = 7,
    /// Query against the pinned generation. It starts after the last file watch closed.
    Query = 8,
}

const STAGES: [&str; 9] = [
    "walk",
    "read",
    "hash",
    "parse/lex",
    "span conversion",
    "graph assembly",
    "canonicalize/seal",
    "persist/reuse",
    "query",
];

struct FileSample {
    path: String,
    bytes: u64,
    symbols: u64,
    nanos: u128,
}

struct Profile {
    stages: [u128; 9],
    files: Vec<FileSample>,
    file_work_ns: u128,
    snapshot_accounted: bool,
}

impl Profile {
    fn new() -> Self {
        Self { stages: [0; 9], files: Vec::new(), file_work_ns: 0, snapshot_accounted: false }
    }
}

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static DATA: RefCell<Option<Profile>> = const { RefCell::new(None) };
}

fn enabled() -> bool {
    ENABLED.with(Cell::get)
}

fn with_profile<T>(update: impl FnOnce(&mut Profile) -> T) -> Option<T> {
    DATA.with(|slot| slot.borrow_mut().as_mut().map(update))
}

/// Starts a fresh profile for this thread. A second call drops the previous samples.
pub(crate) fn enable() {
    DATA.with(|slot| *slot.borrow_mut() = Some(Profile::new()));
    ENABLED.with(|flag| flag.set(true));
}

/// Drops samples. Safe when profiling was already off.
pub(crate) fn discard() {
    ENABLED.with(|flag| flag.set(false));
    DATA.with(|slot| *slot.borrow_mut() = None);
}

/// `Some(now)` only while profiling is on, so a cold scan does not sample the clock.
pub(crate) fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}

/// Adds `ns` to `stage`. No effect when profiling is off.
pub(crate) fn add_ns(stage: Stage, ns: u128) {
    with_profile(|profile| {
        if let Some(cell) = profile.stages.get_mut(stage as usize) {
            *cell = cell.saturating_add(ns);
        }
    });
}

/// Adds the elapsed time of `start` when it was taken.
pub(crate) fn add_elapsed(stage: Stage, start: Option<Instant>) {
    if let Some(start) = start {
        add_ns(stage, start.elapsed().as_nanos());
    }
}

/// Nanos recorded for `stage`, or 0 when profiling is off.
#[cfg(test)]
pub(crate) fn stage_ns(stage: Stage) -> u128 {
    DATA.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|profile| profile.stages.get(stage as usize).copied())
            .unwrap_or(0)
    })
}

fn add_file_work(ns: u128) {
    with_profile(|profile| profile.file_work_ns = profile.file_work_ns.saturating_add(ns));
}

fn file_work_ns() -> u128 {
    DATA.with(|slot| slot.borrow().as_ref().map_or(0, |profile| profile.file_work_ns))
}

fn push_file(path: String, bytes: u64, symbols: u64, nanos: u128) {
    with_profile(|profile| profile.files.push(FileSample { path, bytes, symbols, nanos }));
}

/// Marks that `snapshot_inner` already recorded canonicalize and persist.
///
/// A later snapshot-wall remainder is reuse verification. It must not be added on top of a
/// cold build, or the seal would be counted twice and the hooks would land on the last stage.
pub(crate) fn mark_snapshot_accounted() {
    with_profile(|profile| profile.snapshot_accounted = true);
}

/// Charges the part of `wall` that no stage recorded yet to persist/reuse.
///
/// Cold builds set [`mark_snapshot_accounted`] and this is a no-op. A reused generation never
/// enters that path; its metadata walk is in `walk`, and digest verification is in `read`/`hash`.
/// Only the remaining shared cache work belongs in persist/reuse.
pub(crate) fn observe_snapshot_wall(wall: Duration) {
    with_profile(|profile| {
        if profile.snapshot_accounted {
            return;
        }
        let accounted = profile.stages.iter().copied().fold(0u128, u128::saturating_add);
        let extra = wall.as_nanos().saturating_sub(accounted);
        if let Some(cell) = profile.stages.get_mut(Stage::PersistReuse as usize) {
            *cell = cell.saturating_add(extra);
        }
        profile.snapshot_accounted = true;
    });
}

/// Renders the stderr report and clears the profile. `None` when profiling is off.
pub(crate) fn report() -> Option<String> {
    if !enabled() {
        return None;
    }
    ENABLED.with(|flag| flag.set(false));
    let profile = DATA.with(|slot| slot.borrow_mut().take())?;
    Some(render(profile))
}

fn render(mut profile: Profile) -> String {
    profile.files.sort_by(|left, right| right.nanos.cmp(&left.nanos).then_with(|| left.path.cmp(&right.path)));
    profile.files.truncate(5);
    let mut out = String::from("dctx profile\n");
    for (index, name) in STAGES.iter().enumerate() {
        let ns = profile.stages.get(index).copied().unwrap_or(0);
        out.push_str("stage ");
        out.push_str(name);
        out.push(' ');
        out.push_str(&ns.to_string());
        out.push('\n');
    }
    out.push_str("note finalization is canonicalize/seal, persist/reuse and query; file lines exclude it\n");
    for file in profile.files {
        let path = file.path.replace(['\n', '\r'], "?");
        out.push_str("file ");
        out.push_str(&file.nanos.to_string());
        out.push(' ');
        out.push_str(&file.bytes.to_string());
        out.push(' ');
        out.push_str(&file.symbols.to_string());
        out.push(' ');
        out.push_str(&path);
        out.push('\n');
    }
    out
}

/// Charges `stage` when dropped. A `None` start means profiling is off.
pub(crate) struct Elapsed {
    stage: Stage,
    start: Option<Instant>,
}

impl Elapsed {
    /// Starts `stage` if profiling is on.
    pub(crate) fn begin(stage: Stage) -> Self {
        Self { stage, start: start() }
    }
}

impl Drop for Elapsed {
    fn drop(&mut self) {
        add_elapsed(self.stage, self.start.take());
    }
}

/// Exclusive walk time: the whole `walk_source` interval minus file-stage charges inside it.
pub(crate) struct WalkSpan {
    start: Option<Instant>,
    file_work_before: u128,
}

impl WalkSpan {
    /// Starts the exclusive walk sample, or a no-op when profiling is off.
    pub(crate) fn begin() -> Self {
        if !enabled() {
            return Self { start: None, file_work_before: 0 };
        }
        Self { file_work_before: file_work_ns(), start: Some(Instant::now()) }
    }
}

impl Drop for WalkSpan {
    fn drop(&mut self) {
        let Some(start) = self.start.take() else {
            return;
        };
        let elapsed = start.elapsed().as_nanos();
        let charged = file_work_ns().saturating_sub(self.file_work_before);
        add_ns(Stage::Walk, elapsed.saturating_sub(charged));
    }
}

/// One included file, scanned or digest-verified. Drop writes the row before shared finalization.
pub(crate) struct FileWatch {
    path: String,
    bytes: u64,
    symbols: u64,
    nanos: u128,
    active: bool,
}

impl FileWatch {
    /// Opens a row when profiling is on. The inactive watch records nothing.
    pub(crate) fn begin(path: &str) -> Self {
        if !enabled() {
            return Self { path: String::new(), bytes: 0, symbols: 0, nanos: 0, active: false };
        }
        Self { path: path.to_string(), bytes: 0, symbols: 0, nanos: 0, active: true }
    }

    /// Adds `start`'s elapsed time to `stage` and to this file.
    pub(crate) fn charge(&mut self, stage: Stage, start: Option<Instant>) {
        let Some(start) = start else {
            return;
        };
        let ns = start.elapsed().as_nanos();
        add_ns(stage, ns);
        if self.active {
            self.nanos = self.nanos.saturating_add(ns);
            add_file_work(ns);
        }
    }

    /// Source bytes just read. Zero stays zero when the read failed.
    pub(crate) fn set_bytes(&mut self, bytes: u64) {
        self.bytes = bytes;
    }

    /// Symbols produced for this file, not counting its module node.
    pub(crate) fn set_symbols(&mut self, symbols: u64) {
        self.symbols = symbols;
    }
}

impl Drop for FileWatch {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        push_file(std::mem::take(&mut self.path), self.bytes, self.symbols, self.nanos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            discard();
        }
    }

    fn stage_line(report: &str, name: &str) -> u128 {
        report
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix("stage ")?;
                let (label, value) = rest.rsplit_once(' ')?;
                (label == name).then(|| value.parse::<u128>().ok())?
            })
            .expect(name)
    }

    fn file_rows(report: &str) -> Vec<(u128, u64, u64, &str)> {
        report
            .lines()
            .filter_map(|line| {
                let rest = line.strip_prefix("file ")?;
                let mut parts = rest.splitn(4, ' ');
                let nanos = parts.next()?.parse().ok()?;
                let bytes = parts.next()?.parse().ok()?;
                let symbols = parts.next()?.parse().ok()?;
                let path = parts.next()?;
                Some((nanos, bytes, symbols, path))
            })
            .collect()
    }

    #[test]
    fn top_five_are_distinct_sorted_and_finalization_stays_off_the_file() {
        let _reset = Reset;
        assert!(report().is_none());
        enable();
        push_file("d.rs".to_string(), 40, 4, 8);
        push_file("z.rs".to_string(), 80, 8, 80);
        push_file("m.rs".to_string(), 50, 5, 50);
        push_file("a.rs".to_string(), 50, 5, 50);
        push_file("b.rs".to_string(), 20, 2, 10);
        push_file("c.rs".to_string(), 30, 3, 9);
        add_ns(Stage::Read, 80 + 50 + 50 + 10 + 9 + 8);
        add_ns(Stage::CanonicalizeSeal, 1_000);
        add_ns(Stage::PersistReuse, 2_000);
        add_ns(Stage::Query, 4_000);
        let text = report().expect("report");
        assert!(report().is_none());
        let rows = file_rows(&text);
        assert_eq!(rows.len(), 5, "{text}");
        let paths: Vec<_> = rows.iter().map(|row| row.3).collect();
        assert_eq!(paths, ["z.rs", "a.rs", "m.rs", "b.rs", "c.rs"]);
        assert!(paths.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(rows.windows(2).all(|pair| pair[0].0 >= pair[1].0), "{text}");
        assert_eq!(rows[0], (80, 80, 8, "z.rs"));
        assert_eq!(stage_line(&text, "canonicalize/seal"), 1_000);
        assert_eq!(stage_line(&text, "persist/reuse"), 2_000);
        assert_eq!(stage_line(&text, "query"), 4_000);
        let file_sum: u128 = rows.iter().map(|row| row.0).sum();
        assert_eq!(file_sum, 80 + 50 + 50 + 10 + 9);
        assert_eq!(stage_line(&text, "read"), 80 + 50 + 50 + 10 + 9 + 8);
        for name in STAGES {
            assert!(text.lines().any(|line| line.starts_with(&format!("stage {name} "))), "{name} missing\n{text}");
        }
    }

    #[test]
    fn an_accounted_snapshot_does_not_take_the_reuse_remainder() {
        let _reset = Reset;
        enable();
        add_ns(Stage::Walk, 10);
        observe_snapshot_wall(Duration::from_nanos(25));
        assert_eq!(stage_ns(Stage::PersistReuse), 15);
        mark_snapshot_accounted();
        observe_snapshot_wall(Duration::from_mins(16650));
        assert_eq!(stage_ns(Stage::PersistReuse), 15);
        assert_eq!(stage_ns(Stage::Walk), 10);
    }

    #[test]
    fn reuse_remainder_excludes_verification_stages_and_file_samples() {
        let _reset = Reset;
        enable();
        // Synthetic counters keep the accounting check independent of clock resolution.
        add_ns(Stage::Walk, 10);
        add_ns(Stage::Read, 20);
        add_ns(Stage::Hash, 30);
        push_file("verified.rs".to_string(), 100, 0, 50);
        observe_snapshot_wall(Duration::from_nanos(75));
        observe_snapshot_wall(Duration::from_nanos(150));
        let text = report().expect("report");
        assert_eq!(stage_line(&text, "persist/reuse"), 15);
        assert_eq!(stage_line(&text, "read"), 20);
        assert_eq!(stage_line(&text, "hash"), 30);
        assert_eq!(file_rows(&text), [(50, 100, 0, "verified.rs")]);
    }
}
