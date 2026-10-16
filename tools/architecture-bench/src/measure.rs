//! Cold index, warm queries, RSS, cache, parse calls, and five verb answers.
//! Implements ARCHITECTURE-PROPOSAL §3 for the current map mode in Stage 0.
//! Depends on dctx-map-query; the parent process runs one corpus per child.

use crate::error::BenchError;
use crate::fixture::{self, GeneratedFile};
use crate::host;
use crate::manifest::{DctxSpec, FixtureSpec, Manifest};
use crate::pin::{self, TreePin};
use crate::queries::WARM_QUERIES;
use crate::report::{
    CacheFile, CorpusReport, Pin, PrecisionRecall, StaleFile, unix_now,
};
use crate::stats::{cache_bytes_per_thousand, microseconds, percentile};
use crate::verbs;
use dctx_map_query::{FileSource, Index, Request, Verb};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

const PERCENTILE: &str = "ceil(percent * n / 100) nearest rank on sorted ascending samples";
const PRECISION_REASON: &str = "Current map mode does not export an adjudicated declaration corpus. Bounded verb answers are not parser precision or recall.";

/// Measures the dctx worktree with a cold cache outside the place.
pub fn measure_dctx(
    repo: &Path,
    cache_dir: &Path,
    manifest: &Manifest,
) -> Result<CorpusReport, BenchError> {
    let place = repo.canonicalize()?;
    let tree = pin::hash_tree(&place, &manifest.dctx)?;
    let pin = worktree_pin(&manifest.dctx, &tree, pin::git_head(&place));
    measure_place(
        &place,
        cache_dir,
        &manifest.dctx.id,
        pin,
        &manifest.verb_find_target,
    )
}

/// Generates the fixture under `scratch/fixture` and measures that place only.
pub fn measure_fixture(
    scratch: &Path,
    cache_dir: &Path,
    manifest: &Manifest,
) -> Result<CorpusReport, BenchError> {
    let files = fixture::generate(&manifest.fixture)?;
    let place = write_fixture(&scratch.join("fixture"), &files)?;
    let pin = generated_pin(&manifest.fixture, &files)?;
    measure_place(
        &place,
        cache_dir,
        &manifest.fixture.id,
        pin,
        &manifest.verb_find_target,
    )
}

fn worktree_pin(spec: &DctxSpec, tree: &TreePin, git_head: Option<String>) -> Pin {
    Pin {
        kind: "worktree".into(),
        git_head,
        content_sha256: tree.content_sha256.clone(),
        generator: None,
        seed: None,
        file_count: tree.file_count,
        byte_count: tree.byte_count,
        symlink_policy: format!(
            "not-followed; skip_dir_names={}; skip_prefixes={}",
            spec.skip_dir_names.join(","),
            spec.skip_prefixes.join(",")
        ),
    }
}

fn generated_pin(spec: &FixtureSpec, files: &[GeneratedFile]) -> Result<Pin, BenchError> {
    let mut pairs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|file| (file.path.as_str(), file.bytes.as_bytes()))
        .collect();
    pairs.sort_by(|left, right| left.0.cmp(right.0));
    let byte_count = pairs.iter().map(|(_, bytes)| bytes.len() as u64).sum();
    Ok(Pin {
        kind: "generated".into(),
        git_head: None,
        content_sha256: pin::hash_sequences(&pairs),
        generator: Some(spec.generator.clone()),
        seed: Some(spec.seed.clone()),
        file_count: pairs.len() as u64,
        byte_count,
        symlink_policy: "not-followed".into(),
    })
}

fn write_fixture(dir: &Path, files: &[GeneratedFile]) -> Result<PathBuf, BenchError> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir)?;
    for file in files {
        fixture::relative_path_ok(&file.path)?;
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &file.bytes)?;
    }
    Ok(dir.canonicalize()?)
}

fn measure_place(
    place: &Path,
    cache_dir: &Path,
    corpus_id: &str,
    pin: Pin,
    find_target: &str,
) -> Result<CorpusReport, BenchError> {
    ensure_cache_outside(place, cache_dir)?;
    if cache_dir.exists() {
        fs::remove_dir_all(cache_dir)?;
    }
    fs::create_dir_all(cache_dir)?;
    let database = cache_dir.join("index.sqlite");
    let source = FileSource::new(place)?;
    let place_text = place.to_string_lossy().to_string();
    let started = Instant::now();
    let mut index = Index::open(corpus_id, &database)?;
    let map_request = Request::new(Verb::Map, "");
    let map_answer = index.query(&source, &map_request)?;
    let cold_index_us = microseconds(started.elapsed());
    let phases = phases(index.report());
    let parse_calls = index.report().parsed as u64;
    let stale = index.report().stale.clone();
    let mut warm_query_us = Vec::with_capacity(WARM_QUERIES.len());
    for term in WARM_QUERIES {
        let request = Request::new(Verb::Find, *term);
        let started = Instant::now();
        index.answer(&source, &request)?;
        warm_query_us.push(microseconds(started.elapsed()));
    }
    let verbs = verbs::five_verbs(
        &index,
        &source,
        &place_text,
        &map_request,
        &map_answer,
        find_target,
    );
    let resident = host::resident()?;
    let cache_files = cache_listing(cache_dir)?;
    let cache_bytes = cache_files.iter().map(|file| file.bytes).sum();
    let stale_files = stale.len() as u64;
    let included_files = parse_calls.saturating_add(stale_files);
    Ok(CorpusReport {
        schema: "dx-a1-corpus-report-v1".into(),
        id: corpus_id.to_owned(),
        measured_at_unix: unix_now(),
        host: host::host(),
        pin,
        cold_index_us,
        cold_phases_us: phases,
        warm_p50_us: percentile(&warm_query_us, 50)?,
        warm_p95_us: percentile(&warm_query_us, 95)?,
        warm_query_us,
        warm_percentile: PERCENTILE.into(),
        warm_terms: WARM_QUERIES.iter().map(|term| (*term).to_owned()).collect(),
        rss_bytes: resident.bytes,
        rss_source: resident.source,
        cache_bytes,
        cache_bytes_per_1000_files: cache_bytes_per_thousand(cache_bytes, included_files),
        cache_files,
        parse_calls,
        stale_files,
        stale_sample: stale
            .into_iter()
            .take(20)
            .map(|(path, reason)| StaleFile { path, reason })
            .collect(),
        included_files,
        verbs,
        precision_recall: PrecisionRecall {
            status: "not_measured".into(),
            reason: PRECISION_REASON.into(),
        },
    })
}

fn phases(report: &dctx_map_query::IndexReport) -> BTreeMap<String, u64> {
    let mut phases = BTreeMap::new();
    phases.insert("scan".into(), microseconds(report.scan_time));
    phases.insert("parse".into(), microseconds(report.parse_time));
    phases.insert("resolve".into(), microseconds(report.resolve_time));
    phases.insert("store".into(), microseconds(report.store_time));
    phases
}

fn cache_listing(dir: &Path) -> Result<Vec<CacheFile>, BenchError> {
    let mut names = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    names.sort_by_key(|entry| entry.file_name());
    let mut files = Vec::new();
    for entry in names {
        let kind = entry.file_type()?;
        if kind.is_symlink() || !kind.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        files.push(CacheFile {
            name,
            bytes: entry.metadata()?.len(),
        });
    }
    Ok(files)
}

fn ensure_cache_outside(place: &Path, cache_dir: &Path) -> Result<(), BenchError> {
    let place = place.canonicalize()?;
    if let Some(parent) = cache_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let cache = if cache_dir.exists() {
        cache_dir.canonicalize()?
    } else if let Some(parent) = cache_dir.parent() {
        parent.canonicalize()?.join(
            cache_dir
                .file_name()
                .ok_or_else(|| BenchError::Message("cache directory has no name".into()))?,
        )
    } else {
        return Err(BenchError::Message(
            "cache directory has no parent".into(),
        ));
    };
    if cache_is_inside(&place, &cache) {
        return Err(BenchError::Message(
            "map cache must live outside the source place".into(),
        ));
    }
    Ok(())
}

/// Cache directory for one corpus under the shared cache root.
pub fn corpus_cache(root: &Path, id: &str) -> PathBuf {
    root.join(id)
}

/// True when `cache` is the place or a path inside it.
pub fn cache_is_inside(place: &Path, cache: &Path) -> bool {
    cache.starts_with(place)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_inside_the_place_is_refused_and_a_sibling_is_not() {
        let place = Path::new("/repo/place");
        assert!(cache_is_inside(place, Path::new("/repo/place/cache")));
        assert!(cache_is_inside(place, place));
        assert!(!cache_is_inside(place, Path::new("/tmp/cache")));
        assert_eq!(
            corpus_cache(Path::new("/tmp/dx-a1"), "dctx"),
            PathBuf::from("/tmp/dx-a1/dctx")
        );
    }
}
