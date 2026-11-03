// SPDX-License-Identifier: Apache-2.0
//! Privacy evidence for the filesystem Source adapter.
//! Implements DX-B3 read-spy and the decision 078 plain-folder / `.git` folder contract.
//! Depends on `dctx-adapter-fs` and `tempfile`. The adapter tests call the public API.

use dctx_adapter_fs::{
    FileBody, FileHandlingDecision, FreshnessMode, FsOp, FsSource, SourceOptions, SpyFs, StdFs,
};
use std::{fs, path::Path};

fn options(global: Option<&Path>) -> SourceOptions {
    SourceOptions {
        global_dir: global.map(Path::to_path_buf),
        debug_ignored: false,
        freshness: FreshnessMode::Strict,
    }
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

#[test]
fn ignored_and_omitted_paths_are_never_opened() {
    let root_dir = tempfile::tempdir().unwrap();
    let global_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path();
    write(&global_dir.path().join("ignore"), b"from-global.txt\n");
    write(root.join(".gitignore").as_path(), b"*.rs\nfrom-git.txt\n");
    write(root.join(".dctxignore").as_path(), b"!kept.rs\nfrom-dctx.txt\n");
    write(root.join(".dctxomitcontent").as_path(), b"*.omit\n");
    write(&root.join("kept.rs"), b"fn kept() {}\n");
    write(&root.join("other.rs"), b"fn other() {}\n");
    write(&root.join("from-git.txt"), b"git-secret");
    write(&root.join("from-dctx.txt"), b"dctx-secret");
    write(&root.join("from-global.txt"), b"global-secret");
    write(&root.join("note.omit"), b"omit-secret");

    let source = FsSource::load(
        root.to_path_buf(),
        options(Some(global_dir.path())),
        SpyFs::new(StdFs),
    )
    .unwrap();
    let files = source.inventory().unwrap();
    let excluded = [
        "other.rs",
        "from-git.txt",
        "from-dctx.txt",
        "from-global.txt",
        ".dctxignore",
    ];
    assert!(
        files.iter().all(|file| !excluded.contains(&file.relative.as_str())),
        "{files:?}"
    );
    let kept = files.iter().find(|file| file.relative == "kept.rs").unwrap();
    assert_eq!(kept.decision, FileHandlingDecision::Include);
    assert!(matches!(kept.body, FileBody::Digested { .. }));
    let omitted = files.iter().find(|file| file.relative == "note.omit").unwrap();
    assert_eq!(omitted.decision, FileHandlingDecision::OmitContent);
    assert!(matches!(omitted.body, FileBody::Metadata { .. }));
    for name in excluded.iter().chain(["note.omit"].iter()) {
        assert!(
            source
                .filesystem()
                .read_files()
                .iter()
                .all(|path| path.file_name().and_then(|text| text.to_str()) != Some(*name)),
            "opened {name}"
        );
        assert!(source.read(name).is_err());
    }
    assert!(
        source
            .filesystem()
            .read_files()
            .iter()
            .any(|path| path.ends_with("kept.rs"))
    );
    let authorized = source.read("kept.rs").unwrap();
    assert_eq!(authorized.bytes, b"fn kept() {}\n");
}

#[test]
fn plain_folder_and_git_folder_match() {
    let plain = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    populate(plain.path(), false);
    populate(repo.path(), true);
    let plain_source = FsSource::load(
        plain.path().to_path_buf(),
        options(None),
        SpyFs::new(StdFs),
    )
    .unwrap();
    let repo_source = FsSource::load(
        repo.path().to_path_buf(),
        options(None),
        SpyFs::new(StdFs),
    )
    .unwrap();
    let plain_files = plain_source.inventory().unwrap();
    let repo_files = repo_source.inventory().unwrap();
    assert_eq!(project_view(&plain_files), project_view(&repo_files));
    assert!(plain_files.iter().any(|file| file.relative == "src/main.rs"));
    assert!(plain_files.iter().all(|file| file.relative != "secret.txt"));
    for operation in repo_source.filesystem().operations() {
        let path = match &operation {
            FsOp::Metadata(path)
            | FsOp::ReadDir(path)
            | FsOp::ReadLink(path)
            | FsOp::ReadFile(path)
            | FsOp::ReadPolicy(path) => path,
        };
        assert!(
            path.components().all(|component| component.as_os_str() != ".git"),
            "{path:?}"
        );
    }
}

fn populate(root: &Path, with_git: bool) {
    write(&root.join(".dctxignore"), b"secret.txt\n");
    write(&root.join(".dctxomitcontent"), b"*.omit\n");
    write(&root.join("src/main.rs"), b"fn main() {}\n");
    write(&root.join("secret.txt"), b"hidden\n");
    write(&root.join("note.omit"), b"omit\n");
    if with_git {
        write(&root.join(".git/HEAD"), b"ref: refs/heads/main\n");
        write(&root.join(".git/config"), b"[core]\n\trepositoryformatversion = 0\n");
        write(&root.join(".git/objects/pack/pack.idx"), b"pack-bytes");
    }
}

fn project_view(files: &[dctx_adapter_fs::SourceFile]) -> Vec<(String, FileHandlingDecision, u64, Option<String>)> {
    files
        .iter()
        .map(|file| {
            let (size, digest) = match &file.body {
                FileBody::Metadata { stamp } => (stamp.size, None),
                FileBody::Digested { stamp, digest } => (stamp.size, Some(digest.to_string())),
                FileBody::Stale { reason } => panic!("unexpected stale {reason}"),
            };
            if let FileBody::Digested { digest, .. } = &file.body {
                assert_eq!(digest.algorithm_code(), 1);
            }
            (file.relative.clone(), file.decision, size, digest)
        })
        .collect()
}
