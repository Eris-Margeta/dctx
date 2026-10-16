//! Filesystem fixture. The checked-in tree has no `.git` directory.
//! The fixture `.gitignore` names `secrets.env`, so that file is not in the checkout.
//! The test writes both in a temp copy. `.gitignore` text is not applied unless the caller passes it as selection text.
//! The facade does not open or rewrite `.git/config`.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dctx::memory::CancelFlag;
use dctx::{
    FsSource, HostGrants, IdentityOutcome, RuleTexts, SelectionPolicy, SnapshotRequest, StoragePolicy, VecSink,
};

#[test]
fn gitignore_is_not_automatic_and_git_config_is_not_rewritten() {
    let root = tempfile::tempdir().expect("temp");
    copy_fixture(root.path());
    let git_config = root.path().join(".git/config");
    std::fs::create_dir_all(root.path().join(".git")).expect("git dir");
    let config_bytes = b"[core]\n\trepositoryformatversion = 0\n";
    std::fs::write(&git_config, config_bytes).expect("config");
    // Named by the fixture `.gitignore`, so the checkout cannot contain this file.
    std::fs::write(root.path().join("secrets.env"), b"TOKEN=secret\n").expect("secret");
    let ignore_path = root.path().join(".gitignore");
    let ignore_before = std::fs::read(&ignore_path).expect("ignore");

    let included = run(root.path(), SelectionPolicy::from_texts(&RuleTexts::default()));
    assert_eq!(included.reads, 4);
    assert_eq!(included.secret.as_deref(), Some("secrets.env"));
    assert!(included.git_gone);
    assert_eq!(std::fs::read(&git_config).expect("config after"), config_bytes);
    assert_eq!(std::fs::read(&ignore_path).expect("ignore after"), ignore_before);
    assert!(!root.path().join(".dctx").exists());

    let omitted = run(
        root.path(),
        SelectionPolicy::from_texts(&RuleTexts { local_omit: Some("secrets.env\n"), ..RuleTexts::default() }),
    );
    assert!(omitted.reads < included.reads);
    assert_eq!(omitted.reads, 3);
    assert!(omitted.secret.is_none());
    assert!(omitted.git_gone);
    assert_eq!(std::fs::read(&git_config).expect("config still"), config_bytes);
    assert_eq!(std::fs::read(&ignore_path).expect("ignore still"), ignore_before);
}

struct Probe {
    reads: u64,
    secret: Option<String>,
    git_gone: bool,
}

fn run(root: &Path, selection: SelectionPolicy) -> Probe {
    let source = FsSource::open(root).expect("open");
    let mut session = common::session_parts(
        source,
        Arc::new(CancelFlag::new()),
        HostGrants::PermitAll,
        StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 },
        selection,
    );
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let reads = session.content_reads();
    let mut sink = VecSink::new();
    let secret = session.explain(&handle, "m:secrets.env", &mut sink).expect("secret");
    let secret = secret.records.first().map(|record| record.path.clone());
    let mut sink = VecSink::new();
    let git = session.explain(&handle, "m:.git/config", &mut sink).expect("git");
    let git_gone = matches!(git.outcome, Some(IdentityOutcome::Gone { .. }));
    assert!(git.records.iter().all(|record| record.path != ".git" && !record.path.starts_with(".git/")));
    let mut sink = VecSink::new();
    let ping = session.explain(&handle, "s:src/lib.rs#ping", &mut sink).expect("ping");
    assert_eq!(ping.records[0].summary, "Respond.");
    Probe { reads, secret, git_gone }
}

fn copy_fixture(dest: &Path) {
    let from = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repository");
    copy_dir(&from, dest);
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("list") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        let kind = entry.file_type().expect("kind");
        if kind.is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy");
        }
    }
}
