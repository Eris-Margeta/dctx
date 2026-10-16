//! Proves map mode never spawns Git and never opens ignored or omitted paths.
//! Implements decision 078 and SPEC-MAP-MODE sections 6.1 and 7.
//! Uses FileSource, the public snapshot port, and a PATH trap for process spawns.
//! Rule files are still read as plain text. Git metadata is not a revision source.

use dctx_map_query::ports::Source;
use dctx_map_query::FileSource;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

fn exclusive() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn git_spawn_markers() -> Vec<String> {
    let quote = '"';
    [
        format!("Command::new({quote}git{quote})"),
        format!("Command::new({quote}git.exe{quote})"),
        format!("Command::new({quote}/usr/bin/git{quote})"),
        format!("Command::new({quote}/bin/git{quote})"),
        format!("Command::new(r{quote}git{quote})"),
        format!("Command::new({quote}git{quote}.into())"),
    ]
    .into()
}

fn compact(source: &str) -> String {
    source.chars().filter(|c| !c.is_whitespace()).collect()
}

fn walk_sources(dir: &Path, hits: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "target" || name == ".git" {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk_sources(&path, hits);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let compact = compact(&text);
            if git_spawn_markers().iter().any(|marker| compact.contains(marker)) {
                hits.push(path);
            }
        }
    }
}

#[test]
fn crate_sources_do_not_spawn_git() {
    let _guard = exclusive();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates");
    let mut hits = Vec::new();
    walk_sources(&root, &mut hits);
    assert!(hits.is_empty(), "git spawn in {hits:?}");
}

fn mode(path: &Path, bits: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(bits)).unwrap();
}

struct Restore {
    path: PathBuf,
}

impl Drop for Restore {
    fn drop(&mut self) {
        mode(&self.path, 0o755);
    }
}

#[test]
fn ignored_and_omitted_paths_are_not_opened() {
    let _guard = exclusive();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join(".gitignore"), "secret.rs\nhidden/\n").unwrap();
    fs::write(root.join(".dctxomitcontent"), "omitted.txt\n").unwrap();
    fs::write(root.join("keep.rs"), b"abc").unwrap();
    fs::write(root.join("secret.rs"), b"SECRET-IGNORED").unwrap();
    fs::write(root.join("omitted.txt"), b"SECRET-OMITTED").unwrap();
    fs::create_dir(root.join("hidden")).unwrap();
    fs::write(root.join("hidden/note.rs"), b"SECRET-HIDDEN").unwrap();
    mode(&root.join("secret.rs"), 0);
    mode(&root.join("omitted.txt"), 0);
    mode(&root.join("hidden"), 0);
    let _restore = Restore {
        path: root.join("hidden"),
    };

    let source = FileSource::new(root).unwrap();
    let snapshot = source.snapshot().expect("unreadable ignored paths must not be opened");
    assert_eq!(
        snapshot.files["keep.rs"].hash,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert!(!snapshot.files.contains_key("secret.rs"));
    assert!(!snapshot.files.contains_key("hidden/note.rs"));
    assert!(!snapshot.directories.contains_key("hidden"));
    let omitted = &snapshot.files["omitted.txt"];
    assert!(omitted.hash.is_empty());
    assert!(!omitted.allowed);
    assert!(!omitted.readable);
    assert_eq!(omitted.size, b"SECRET-OMITTED".len() as u64);
    assert_eq!(
        snapshot.rules[".gitignore"].text.as_deref(),
        Some("secret.rs\nhidden/\n")
    );
    assert_eq!(source.content_reads(), 5);

    let reads = source.content_reads();
    assert!(source.read("secret.rs").is_err());
    assert!(source.read("omitted.txt").is_err());
    assert!(source.read("hidden/note.rs").is_err());
    assert_eq!(source.read("keep.rs").unwrap(), b"abc");
    assert_eq!(source.content_reads(), reads + 1);
}

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn write_pair(root: &Path) {
    fs::write(root.join("a.rs"), b"abc").unwrap();
    fs::write(root.join("b.rs"), b"abcd").unwrap();
}

#[test]
fn plain_folder_gets_full_freshness_from_metadata_and_hash() {
    let _guard = exclusive();
    let temp = tempfile::tempdir().unwrap();
    write_pair(temp.path());
    let source = FileSource::new(temp.path()).unwrap();
    let cold = source.snapshot().unwrap();
    assert!(!temp.path().join(".git").exists());
    assert_eq!(cold.files.len(), 2);
    assert!(cold.dirty.is_empty());
    assert!(cold.recent.is_empty());
    assert_eq!(cold.revision.len(), 64);
    assert!(cold.revision.chars().all(|c| c.is_ascii_hexdigit()));
    let a = &cold.files["a.rs"];
    let b = &cold.files["b.rs"];
    assert_eq!(a.hash, ABC_SHA256);
    assert_eq!(a.size, 3);
    assert!(a.mtime > 0);
    assert_eq!(b.size, 4);
    assert_ne!(b.hash, a.hash);
    assert_eq!(b.hash.len(), 64);
    let reads = source.content_reads();
    let warm = source.snapshot_since(Some(&cold)).unwrap();
    assert_eq!(warm.revision, cold.revision);
    assert_eq!(source.content_reads(), reads, "mtime and size pre-filter");

    let path = temp.path().join("b.rs");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified + Duration::from_secs(2))
        .unwrap();
    let touched = source.snapshot_since(Some(&warm)).unwrap();
    assert_eq!(touched.files["b.rs"].hash, b.hash, "mtime is only a pre-filter");
    assert_eq!(touched.revision, cold.revision);
    assert_eq!(source.content_reads(), reads + 1);

    fs::write(&path, b"abce-edited").unwrap();
    let edited = source.snapshot_since(Some(&touched)).unwrap();
    assert_ne!(edited.files["b.rs"].hash, b.hash);
    assert_eq!(edited.files["b.rs"].size, b"abce-edited".len() as u64);
    assert_eq!(edited.files["a.rs"].hash, ABC_SHA256);
    assert_ne!(edited.revision, cold.revision);
    assert_eq!(source.content_reads(), reads + 2);
    assert!(edited.dirty.is_empty());
}

struct GitTrap {
    _dir: tempfile::TempDir,
    log: PathBuf,
    path: Option<std::ffi::OsString>,
}

impl GitTrap {
    fn install() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("spawn-log");
        fs::write(&log, b"").unwrap();
        let script = dir.path().join("git");
        fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$DCTX_GIT_SPAWN_LOG\"\nprintf 'deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\\n'\nexit 0\n",
        )
        .unwrap();
        mode(&script, 0o755);
        let path = std::env::var_os("PATH");
        let mut next = std::ffi::OsString::from(dir.path());
        next.push(":");
        if let Some(old) = &path {
            next.push(old);
        }
        // SAFETY: no_git tests take `exclusive` before touching the environment,
        // and this guard restores PATH on drop before that lock is released.
        unsafe { std::env::set_var("PATH", &next) };
        unsafe { std::env::set_var("DCTX_GIT_SPAWN_LOG", &log) };
        let trap = Self {
            _dir: dir,
            log,
            path,
        };
        let mut name = String::from("gi");
        name.push('t');
        let probed = Command::new(&name).arg("rev-parse").output().unwrap();
        assert!(probed.status.success());
        assert!(!fs::read(&trap.log).unwrap().is_empty(), "trap missed git");
        fs::write(&trap.log, b"").unwrap();
        trap
    }
}

impl Drop for GitTrap {
    fn drop(&mut self) {
        unsafe {
            match &self.path {
                Some(path) => std::env::set_var("PATH", path),
                None => std::env::remove_var("PATH"),
            }
            std::env::remove_var("DCTX_GIT_SPAWN_LOG");
        }
    }
}

#[test]
fn git_directory_matches_plain_folder_and_never_spawns() {
    let _guard = exclusive();
    let trap = GitTrap::install();
    let plain = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    write_pair(plain.path());
    write_pair(repo.path());
    fs::create_dir(repo.path().join(".git")).unwrap();
    fs::write(repo.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(
        repo.path().join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n",
    )
    .unwrap();
    let plain_source = FileSource::new(plain.path()).unwrap();
    let repo_source = FileSource::new(repo.path()).unwrap();
    let plain_snapshot = plain_source.snapshot().unwrap();
    let repo_snapshot = repo_source.snapshot().unwrap();
    assert_eq!(repo_snapshot.revision, plain_snapshot.revision);
    assert_eq!(repo_snapshot.revision.len(), 64);
    assert_ne!(
        repo_snapshot.revision,
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
    );
    assert_eq!(repo_snapshot.dirty, plain_snapshot.dirty);
    assert_eq!(repo_snapshot.recent, plain_snapshot.recent);
    assert_eq!(
        repo_snapshot.files["a.rs"].hash,
        plain_snapshot.files["a.rs"].hash
    );
    assert_eq!(
        repo_snapshot.files["b.rs"].hash,
        plain_snapshot.files["b.rs"].hash
    );
    assert!(!repo_snapshot.files.keys().any(|path| path.starts_with(".git")));
    assert_eq!(plain_source.content_reads(), 2);
    assert_eq!(repo_source.content_reads(), 2);
    assert_eq!(fs::read(&trap.log).unwrap(), b"");
}
