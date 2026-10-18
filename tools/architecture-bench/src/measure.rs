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

const MARKER_NAME: &str = ".dctx-bench-owned";
const PERCENTILE: &str = "ceil(percent * n / 100) nearest rank on sorted ascending samples";
const PRECISION_REASON: &str = "Current map mode does not export an adjudicated declaration corpus. Bounded verb answers are not parser precision or recall.";

/// Measures the dctx worktree with a cold cache outside the place.
///
/// `run_id` marks the cache this run creates. A preexisting unowned cache is refused.
pub fn measure_dctx(
    repo: &Path,
    cache_dir: &Path,
    manifest: &Manifest,
    run_id: &str,
) -> Result<CorpusReport, BenchError> {
    let place = repo.canonicalize()?;
    let tree = pin::hash_tree(&place, &manifest.dctx)?;
    let pin = worktree_pin(&manifest.dctx, &tree, pin::revision_label());
    measure_place(
        &place,
        cache_dir,
        &manifest.dctx.id,
        pin,
        &manifest.verb_find_target,
        run_id,
    )
}

/// Generates the fixture under `scratch/fixture` and measures that place only.
///
/// `scratch` must be absent or already marked with `run_id`. The fixture directory
/// is replaced only when this run created it.
pub fn measure_fixture(
    scratch: &Path,
    cache_dir: &Path,
    manifest: &Manifest,
    run_id: &str,
) -> Result<CorpusReport, BenchError> {
    let files = fixture::generate(&manifest.fixture)?;
    claim_owned_dir(scratch, run_id)?;
    let place = write_fixture(&scratch.join("fixture"), &files, run_id)?;
    let pin = generated_pin(&manifest.fixture, &files)?;
    measure_place(
        &place,
        cache_dir,
        &manifest.fixture.id,
        pin,
        &manifest.verb_find_target,
        run_id,
    )
}

fn worktree_pin(spec: &DctxSpec, tree: &TreePin, revision: String) -> Pin {
    Pin {
        kind: "worktree".into(),
        revision: Some(revision),
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
        revision: None,
        content_sha256: pin::hash_sequences(&pairs)?,
        generator: Some(spec.generator.clone()),
        seed: Some(spec.seed.clone()),
        file_count: pairs.len() as u64,
        byte_count,
        symlink_policy: "not-followed".into(),
    })
}

fn write_fixture(
    dir: &Path,
    files: &[GeneratedFile],
    run_id: &str,
) -> Result<PathBuf, BenchError> {
    let marker = external_marker(dir)?;
    remove_owned(dir, &marker, run_id)?;
    claim(dir, &marker, run_id)?;
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
    run_id: &str,
) -> Result<CorpusReport, BenchError> {
    prepare_cache_dir(place, cache_dir, run_id)?;
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
        let request = Request::new(Verb::Find, term);
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
        if name == MARKER_NAME {
            continue;
        }
        files.push(CacheFile {
            name,
            bytes: entry.metadata()?.len(),
        });
    }
    Ok(files)
}

fn ensure_cache_outside(place: &Path, cache_dir: &Path) -> Result<(), BenchError> {
    let place = place.canonicalize()?;
    if cache_dir.exists() {
        let meta = fs::symlink_metadata(cache_dir)?;
        if meta.file_type().is_symlink() {
            return Err(BenchError::Message(
                "refusing a cache path that is a symlink".into(),
            ));
        }
        let cache = cache_dir.canonicalize()?;
        if cache_overlaps_source(&place, &cache) {
            return Err(BenchError::Message(
                "map cache must live outside the source place".into(),
            ));
        }
        return Ok(());
    }
    let name = cache_dir.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
        BenchError::Message("cache directory has no utf-8 name".into())
    })?;
    if !single_component(name) {
        return Err(BenchError::Message(format!(
            "refusing cache directory name that is not a single component: {name}"
        )));
    }
    let Some(parent) = cache_dir.parent().filter(|parent| !parent.as_os_str().is_empty()) else {
        return Err(BenchError::Message(
            "cache directory has no parent".into(),
        ));
    };
    if cache_overlaps_source(&place, cache_dir) {
        return Err(BenchError::Message(
            "map cache must live outside the source place".into(),
        ));
    }
    fs::create_dir_all(parent)?;
    let cache = parent.canonicalize()?.join(name);
    if cache_overlaps_source(&place, &cache) {
        return Err(BenchError::Message(
            "map cache must live outside the source place".into(),
        ));
    }
    Ok(())
}

fn prepare_cache_dir(place: &Path, cache_dir: &Path, run_id: &str) -> Result<(), BenchError> {
    ensure_cache_outside(place, cache_dir)?;
    remove_owned_dir(cache_dir, run_id)?;
    claim_owned_dir(cache_dir, run_id)
}

/// Cache directory for one tool-chosen corpus name under the shared cache root.
///
/// `component` is `dctx` or `fixture` from this process, never a manifest id.
/// Absolute components and `..` are refused so the path cannot leave `root`.
///
/// # Errors
/// Returns when `component` is not one path segment or the joined path leaves `root`.
pub fn corpus_cache(root: &Path, component: &str) -> Result<PathBuf, BenchError> {
    if !single_component(component) {
        return Err(BenchError::Message(format!(
            "refusing cache component that is not a single path segment: {component}"
        )));
    }
    let path = root.join(component);
    if !path.starts_with(root) {
        return Err(BenchError::Message(format!(
            "refusing cache path {} outside {}",
            path.display(),
            root.display()
        )));
    }
    Ok(path)
}

/// True when `cache` is the place or a path inside it.
pub fn cache_is_inside(place: &Path, cache: &Path) -> bool {
    cache.starts_with(place)
}

/// True when the cache is inside the place, equal to it, or an ancestor of it.
fn cache_overlaps_source(place: &Path, cache: &Path) -> bool {
    cache_is_inside(place, cache) || place.starts_with(cache)
}

fn single_component(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

/// Accepts a run id that is stored in a marker file and is not a path.
///
/// # Errors
/// Returns when `id` contains a separator or is empty.
pub(crate) fn validate_run_id(id: &str) -> Result<(), BenchError> {
    if single_component(id)
        && id.len() <= 80
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        Ok(())
    } else {
        Err(BenchError::Message(
            "refusing run id that is not a single token".into(),
        ))
    }
}

/// New id for this process. The parent passes it to children with `--run-id`.
pub(crate) fn fresh_run_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("r{}-{nanos}", std::process::id())
}

fn internal_marker(dir: &Path) -> PathBuf {
    dir.join(MARKER_NAME)
}

fn external_marker(dir: &Path) -> Result<PathBuf, BenchError> {
    let name = dir.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
        BenchError::Message(format!("directory {} has no utf-8 name", dir.display()))
    })?;
    if !single_component(name) {
        return Err(BenchError::Message(format!(
            "refusing path traversal in directory name {name}"
        )));
    }
    let parent = dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| BenchError::Message(format!("directory {} has no parent", dir.display())))?;
    Ok(parent.join(format!("{MARKER_NAME}-{name}")))
}

/// Creates `dir` and records `run_id`, or accepts the same marker.
///
/// A preexisting directory without this run's marker is left in place.
///
/// # Errors
/// Returns when the directory is a symlink, is not owned by `run_id`, or cannot be created.
pub(crate) fn claim_owned_dir(dir: &Path, run_id: &str) -> Result<(), BenchError> {
    claim(dir, &internal_marker(dir), run_id)
}

/// Deletes `dir` only when the marker inside it contains `run_id`.
///
/// # Errors
/// Returns when the directory exists but this run did not create it.
pub(crate) fn remove_owned_dir(dir: &Path, run_id: &str) -> Result<(), BenchError> {
    remove_owned(dir, &internal_marker(dir), run_id)
}

fn claim(dir: &Path, marker: &Path, run_id: &str) -> Result<(), BenchError> {
    validate_run_id(run_id)?;
    match fs::symlink_metadata(dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(BenchError::Message(format!(
                    "refusing to use symlink or non-directory {}",
                    dir.display()
                )));
            }
            let found = fs::read_to_string(marker).map_err(|_| {
                BenchError::Message(format!(
                    "refusing preexisting unowned directory {}",
                    dir.display()
                ))
            })?;
            if found != run_id {
                return Err(BenchError::Message(format!(
                    "refusing preexisting unowned directory {}",
                    dir.display()
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if marker.exists() {
                let found = fs::read_to_string(marker).unwrap_or_default();
                if found != run_id {
                    return Err(BenchError::Message(format!(
                        "refusing to create {} because marker {} already exists",
                        dir.display(),
                        marker.display()
                    )));
                }
            }
            if let Some(parent) = dir.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::create_dir(dir)?;
            fs::write(marker, run_id)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn remove_owned(dir: &Path, marker: &Path, run_id: &str) -> Result<(), BenchError> {
    validate_run_id(run_id)?;
    match fs::symlink_metadata(dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(BenchError::Message(format!(
                    "refusing to delete symlink or non-directory {}",
                    dir.display()
                )));
            }
            let found = fs::read_to_string(marker).map_err(|_| {
                BenchError::Message(format!(
                    "refusing to delete unowned directory {}",
                    dir.display()
                ))
            })?;
            if found != run_id {
                return Err(BenchError::Message(
                    "refusing to delete directory created by another run".into(),
                ));
            }
            let canonical = dir.canonicalize()?;
            if let Some(parent) = dir.parent() {
                let parent_canon = parent.canonicalize()?;
                if !canonical.starts_with(&parent_canon) {
                    return Err(BenchError::Message(format!(
                        "refusing path traversal deleting {}",
                        canonical.display()
                    )));
                }
            }
            fs::remove_dir_all(&canonical)?;
            if marker.is_file() {
                fs::remove_file(marker)?;
            }
            Ok(())
        }
    }
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
            corpus_cache(Path::new("/tmp/dx-a1"), "dctx").unwrap(),
            PathBuf::from("/tmp/dx-a1/dctx")
        );
    }

    /// F2: cache paths come from tool components. An ancestor cache overlaps the source.
    /// Positive control: a sibling cache does not.
    #[test]
    fn cache_component_refuses_manifest_paths_and_ancestors() {
        let root = Path::new("/tmp/dx-a1");
        for component in ["/tmp/review-victim", "..", "../review-victim", "a/b", r"..\victim"] {
            assert!(corpus_cache(root, component).is_err(), "{component}");
        }
        assert_eq!(corpus_cache(root, "dctx").unwrap(), root.join("dctx"));
        assert_eq!(corpus_cache(root, "fixture").unwrap(), root.join("fixture"));
        let place = Path::new("/repo/place");
        assert!(cache_overlaps_source(place, Path::new("/repo")));
        assert!(cache_overlaps_source(place, Path::new("/")));
        assert!(cache_overlaps_source(place, place));
        assert!(cache_overlaps_source(place, Path::new("/repo/place/cache")));
        assert!(!cache_overlaps_source(place, Path::new("/tmp/cache")));
        assert!(!cache_overlaps_source(place, Path::new("/repo/other")));
    }

    /// F2: only a directory this run marked may be deleted. Manifest-shaped ids are not paths.
    #[test]
    fn unowned_directories_are_kept_and_this_run_directories_can_be_replaced() {
        let tree = TempTree::new("owned");
        let victim = tree.0.join("review-victim");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("sentinel"), b"keep").unwrap();
        assert!(corpus_cache(&tree.0, victim.to_str().unwrap()).is_err());
        assert!(remove_owned_dir(&victim, "run-a").is_err());
        assert_eq!(std::fs::read(victim.join("sentinel")).unwrap(), b"keep");

        let owned = tree.0.join("owned");
        claim_owned_dir(&owned, "run-a").unwrap();
        std::fs::write(owned.join("data"), b"temp").unwrap();
        remove_owned_dir(&owned, "run-a").unwrap();
        assert!(!owned.exists());
        claim_owned_dir(&owned, "other-run").unwrap();
        std::fs::write(owned.join("sentinel"), b"keep").unwrap();
        assert!(remove_owned_dir(&owned, "run-a").is_err());
        assert_eq!(std::fs::read(owned.join("sentinel")).unwrap(), b"keep");

        let place = tree.0.join("place");
        std::fs::create_dir_all(&place).unwrap();
        std::fs::write(place.join("sentinel"), b"keep").unwrap();
        let ancestor = tree.0.clone();
        assert!(prepare_cache_dir(&place, &ancestor, "run-a").is_err());
        assert_eq!(std::fs::read(place.join("sentinel")).unwrap(), b"keep");

        let foreign = tree.0.join("foreign-cache");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("sentinel"), b"keep").unwrap();
        assert!(prepare_cache_dir(&place, &foreign, "run-a").is_err());
        assert_eq!(std::fs::read(foreign.join("sentinel")).unwrap(), b"keep");

        let cache = tree.0.join("cache");
        prepare_cache_dir(&place, &cache, "run-a").unwrap();
        std::fs::write(cache.join("stale"), b"x").unwrap();
        prepare_cache_dir(&place, &cache, "run-a").unwrap();
        assert!(!cache.join("stale").exists());
        assert_eq!(
            std::fs::read_to_string(cache.join(MARKER_NAME)).unwrap(),
            "run-a"
        );
        assert!(cache_listing(&cache).unwrap().iter().all(|file| file.name != MARKER_NAME));
        assert_eq!(std::fs::read(place.join("sentinel")).unwrap(), b"keep");

        let scratch = tree.0.join("scratch");
        claim_owned_dir(&scratch, "run-a").unwrap();
        std::fs::write(scratch.join("keep-me"), b"yes").unwrap();
        let fixture_dir = scratch.join("fixture");
        std::fs::create_dir_all(&fixture_dir).unwrap();
        std::fs::write(fixture_dir.join("sentinel"), b"keep").unwrap();
        assert!(write_fixture(&fixture_dir, &[], "run-a").is_err());
        assert_eq!(std::fs::read(fixture_dir.join("sentinel")).unwrap(), b"keep");
        assert_eq!(std::fs::read(scratch.join("keep-me")).unwrap(), b"yes");

        let fresh = scratch.join("fresh-fixture");
        let files = vec![GeneratedFile {
            path: "a.txt".into(),
            bytes: "hello".into(),
        }];
        let written = write_fixture(&fresh, &files, "run-a").unwrap();
        assert_eq!(std::fs::read_to_string(written.join("a.txt")).unwrap(), "hello");
        assert!(!written.join(MARKER_NAME).exists());
        assert!(scratch.join(format!("{MARKER_NAME}-fresh-fixture")).is_file());
        let replaced = vec![GeneratedFile {
            path: "b.txt".into(),
            bytes: "next".into(),
        }];
        write_fixture(&fresh, &replaced, "run-a").unwrap();
        assert!(!fresh.join("a.txt").exists());
        assert_eq!(std::fs::read_to_string(fresh.join("b.txt")).unwrap(), "next");
        assert_eq!(std::fs::read(scratch.join("keep-me")).unwrap(), b"yes");
    }

    /// F2: a symlink cache is not deleted. Positive control: a real cache directory is created.
    #[cfg(unix)]
    #[test]
    fn symlink_cache_is_refused_and_a_real_directory_is_created() {
        let tree = TempTree::new("symlink");
        let place = tree.0.join("place");
        std::fs::create_dir_all(&place).unwrap();
        std::fs::write(place.join("sentinel"), b"keep").unwrap();
        let link = tree.0.join("link");
        std::os::unix::fs::symlink(&place, &link).unwrap();
        assert!(prepare_cache_dir(&place, &link, "run-a").is_err());
        assert_eq!(std::fs::read(place.join("sentinel")).unwrap(), b"keep");
        let cache = tree.0.join("cache");
        prepare_cache_dir(&place, &cache, "run-a").unwrap();
        assert!(cache.join(MARKER_NAME).is_file());
        assert_eq!(std::fs::read(place.join("sentinel")).unwrap(), b"keep");
    }

    struct TempTree(PathBuf);

    impl TempTree {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "dx-rvl3-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
