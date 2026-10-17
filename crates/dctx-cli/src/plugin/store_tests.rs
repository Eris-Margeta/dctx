//! Native filesystem controls for PLUGINS.md §5.4 / PP7 and PP9.
//! Exercises allocation ceilings, publication crash cuts, interprocess ownership and root swaps.
//! Uses only std and the parent store; subprocesses exit without destructors at real IO cuts.
#![forbid(unsafe_code)]

use super::*;
#[cfg(unix)]
use std::process::{Command, Stdio};

#[cfg(unix)]
struct Fixture(PathBuf);
#[cfg(unix)]
impl Fixture {
    fn new() -> Self {
        // Canonicalize the system temp parent: macOS /var itself is a symlink.
        let parent = fs::canonicalize(std::env::temp_dir()).unwrap();
        let path = parent.join(format!("dctx-store-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> FsStore { FsStore::new(self.0.join("plugins")) }
}
#[cfg(unix)]
impl Drop for Fixture { fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); } }

pub(super) fn crash_cut(phase: &str) {
    if std::env::var("DCTX_STORE_CRASH").as_deref() == Ok(phase) { std::process::exit(77); }
}
#[cfg(unix)]
fn key() -> InstallKey { install_key("test/plugin", "1.0.0", &"ab".repeat(32)).unwrap() }
#[cfg(unix)]
fn payload() -> SafeFileName { SafeFileName::parse("worker").unwrap() }
#[cfg(unix)]
fn child(root: &Path, mode: &str, crash: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "plugin::disk::store_tests::subprocess_client", "--nocapture"])
        .env("DCTX_STORE_TEST_ROOT", root).env("DCTX_STORE_TEST_MODE", mode)
        .env("DCTX_STORE_CRASH", crash).stdout(Stdio::null()).stderr(Stdio::inherit());
    command
}

#[test]
#[cfg(unix)]
fn missing_root_read_is_side_effect_free() {
    let fixture = Fixture::new();
    let store = fixture.store();
    assert_eq!(store.load_registry().unwrap(), None);
    assert!(store.staged().unwrap().is_empty());
    assert!(!store.root.exists());
    let lock = store.try_lock().unwrap();
    store.save_registry(lock, b"{}").unwrap();
    assert_eq!(store.load_registry().unwrap(), Some(b"{}".to_vec()));
    store.unlock(lock);
}

#[test]
#[cfg(unix)]
fn sparse_oversize_files_are_refused_before_allocation() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lock = store.try_lock().unwrap();
    let valid = dctx_plugin_distribution::domain::registry_codec::encode(&dctx_plugin_distribution::Registry::default());
    store.save_registry(lock, &valid).unwrap();
    assert_eq!(store.load_registry().unwrap(), Some(valid));
    let registry = store.root.join("registry.json");
    File::options().write(true).open(&registry).unwrap().set_len(REGISTRY_LIMIT + 1).unwrap();
    assert_eq!(store.load_registry(), Err(DistributionError::LimitExceeded { limit: REGISTRY_LIMIT }));
    // A huge sparse length must not be converted into an allocation request.
    File::options().write(true).open(registry).unwrap().set_len(1 << 40).unwrap();
    assert_eq!(store.load_registry(), Err(DistributionError::LimitExceeded { limit: REGISTRY_LIMIT }));
    store.stage_append(lock, &key(), &payload(), b"complete").unwrap();
    store.publish(lock, &key()).unwrap();
    assert_eq!(store.read_installed(&key(), &payload()).unwrap(), b"complete");
    let installed = store.root.join("installed").join(key().components().iter().collect::<PathBuf>()).join("worker");
    File::options().write(true).open(installed).unwrap().set_len(FILE_LIMIT + 1).unwrap();
    assert_eq!(store.read_installed(&key(), &payload()), Err(DistributionError::LimitExceeded { limit: FILE_LIMIT }));
    store.unlock(lock);
}

#[test]
#[cfg(unix)]
fn crash_cuts_preserve_complete_registry_and_installations() {
    for phase in ["registry-synced", "registry-renamed", "install-synced", "install-renamed", "complete"] {
        let fixture = Fixture::new();
        let store = fixture.store();
        let lock = store.try_lock().unwrap();
        store.save_registry(lock, b"{\"active\":\"old\"}").unwrap();
        store.unlock(lock);
        let status = child(&store.root, "publish", phase).status().unwrap();
        assert_eq!(status.code(), Some(if phase == "complete" { 0 } else { 77 }));
        let registry = store.load_registry().unwrap().unwrap();
        let expected = if matches!(phase, "registry-renamed" | "complete") { b"{\"active\":\"new\"}" } else { b"{\"active\":\"old\"}" };
        assert_eq!(registry, expected, "{phase}");
        if phase == "install-synced" {
            assert!(store.installed().unwrap().is_empty());
            assert_eq!(store.staged().unwrap(), [key()]);
        } else {
            assert_eq!(store.installed().unwrap(), [key()]);
            assert_eq!(store.read_installed(&key(), &payload()).unwrap(), b"complete");
        }
        assert_eq!(store.try_lock(), Err(DistributionError::Busy));
        // Every child has exited: the explicit offline takeover rule is now satisfied.
        fs::remove_file(store.root.join("registry.lock")).unwrap();
        let temporary = store.root.join("registry.json.tmp");
        if temporary.exists() { fs::remove_file(temporary).unwrap(); }
        let lock = store.try_lock().unwrap();
        store.save_registry(lock, &registry).unwrap();
        assert_eq!(store.load_registry().unwrap().unwrap(), registry);
        store.unlock(lock);
    }
}

#[test]
#[cfg(unix)]
fn subprocess_client() {
    let Some(root) = std::env::var_os("DCTX_STORE_TEST_ROOT") else { return };
    let store = FsStore::new(root);
    let mode = std::env::var("DCTX_STORE_TEST_MODE").unwrap();
    if mode == "compete" {
        // Both clients wait on stdin; the winner deliberately retains ownership after exit.
        std::io::stdin().read_exact(&mut [0]).unwrap();
        match store.try_lock() {
            Ok(lock) => { store.save_registry(lock, b"{\"active\":\"winner\"}").unwrap(); std::process::exit(0); }
            Err(DistributionError::Busy) => std::process::exit(3),
            other => panic!("unexpected lock result: {other:?}"),
        }
    }
    let lock = store.try_lock().unwrap();
    store.stage_append(lock, &key(), &payload(), b"complete").unwrap();
    store.publish(lock, &key()).unwrap();
    store.save_registry(lock, b"{\"active\":\"new\"}").unwrap();
    std::process::exit(0);
}

#[test]
#[cfg(unix)]
fn concurrent_clients_cannot_both_activate() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut first = child(&store.root, "compete", "").stdin(Stdio::piped()).spawn().unwrap();
    let mut second = child(&store.root, "compete", "").stdin(Stdio::piped()).spawn().unwrap();
    first.stdin.take().unwrap().write_all(b"x").unwrap();
    second.stdin.take().unwrap().write_all(b"x").unwrap();
    let mut codes = [first.wait().unwrap().code().unwrap(), second.wait().unwrap().code().unwrap()];
    codes.sort();
    assert_eq!(codes, [0, 3]);
    assert_eq!(store.load_registry().unwrap().unwrap(), b"{\"active\":\"winner\"}");
    assert_eq!(store.try_lock(), Err(DistributionError::Busy));
}

#[test]
#[cfg(unix)]
fn foreign_tokens_and_changed_ownership_are_refused() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lock = store.try_lock().unwrap();
    let other = fixture.store();
    assert_eq!(other.save_registry(lock, b"{}"), Err(DistributionError::Busy));
    other.unlock(lock);
    assert_eq!(other.try_lock(), Err(DistributionError::Busy));
    store.save_registry(lock, b"{}").unwrap();
    fs::write(store.root.join("registry.lock"), b"replacement owner").unwrap();
    assert_eq!(store.save_registry(lock, b"bad"), Err(DistributionError::Busy));
    store.unlock(lock);
    assert_eq!(fs::read(store.root.join("registry.lock")).unwrap(), b"replacement owner");
    assert_eq!(store.load_registry().unwrap().unwrap(), b"{}");
}

#[test]
#[cfg(unix)]
fn symlinked_roots_children_and_registry_are_refused() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = fixture.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("registry.json"), b"outside").unwrap();
    let store = fixture.store();
    symlink(&outside, &store.root).unwrap();
    assert_eq!(store.load_registry(), Err(DistributionError::Interrupted));
    assert_eq!(store.try_lock(), Err(DistributionError::Interrupted));
    assert_eq!(FsStore::new(store.root.join("nested")).try_lock(), Err(DistributionError::Interrupted));
    assert!(!outside.join("registry.lock").exists());
    fs::remove_file(&store.root).unwrap();
    let lock = store.try_lock().unwrap();
    symlink(&outside, store.root.join("staging")).unwrap();
    assert_eq!(store.stage_append(lock, &key(), &payload(), b"bad"), Err(DistributionError::Interrupted));
    fs::remove_file(store.root.join("staging")).unwrap();
    symlink(outside.join("registry.json"), store.root.join("registry.json")).unwrap();
    assert_eq!(store.load_registry(), Err(DistributionError::Interrupted));
    store.save_registry(lock, b"{}").unwrap();
    assert_eq!(fs::read(outside.join("registry.json")).unwrap(), b"outside");
    store.stage_append(lock, &key(), &payload(), b"complete").unwrap();
    store.publish(lock, &key()).unwrap();
    assert_eq!(store.read_installed(&key(), &payload()).unwrap(), b"complete");
    store.unlock(lock);
}

#[test]
#[cfg(unix)]
fn retained_directory_handle_survives_a_root_symlink_swap() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let store = fixture.store();
    let lock = store.try_lock().unwrap();
    let root = store.root(false).unwrap();
    let moved = fixture.0.join("moved");
    let outside = fixture.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::rename(&store.root, &moved).unwrap();
    symlink(&outside, &store.root).unwrap();
    assert!(store.root(false).is_err());
    // An operation already holding its root handle remains in the original directory.
    let mut file = private_options().create_new(true).open(entry(&root, "probe").unwrap()).unwrap();
    file.write_all(b"pinned").unwrap();
    assert_eq!(fs::read(moved.join("probe")).unwrap(), b"pinned");
    assert!(!outside.join("probe").exists());
    assert_eq!(store.save_registry(lock, b"bad"), Err(DistributionError::Interrupted));
}

#[test]
#[cfg(unix)]
fn incomplete_temporary_and_duplicate_publication_preserve_previous_state() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lock = store.try_lock().unwrap();
    store.save_registry(lock, b"old").unwrap();
    fs::write(store.root.join("registry.json.tmp"), b"partial").unwrap();
    assert_eq!(store.save_registry(lock, b"new"), Err(DistributionError::Interrupted));
    assert_eq!(store.load_registry().unwrap().unwrap(), b"old");
    fs::remove_file(store.root.join("registry.json.tmp")).unwrap();
    store.save_registry(lock, b"new").unwrap();
    assert_eq!(store.load_registry().unwrap().unwrap(), b"new");
    store.stage_append(lock, &key(), &payload(), b"complete").unwrap();
    store.publish(lock, &key()).unwrap();
    store.stage_append(lock, &key(), &payload(), b"replacement").unwrap();
    assert_eq!(store.publish(lock, &key()), Err(DistributionError::Interrupted));
    assert_eq!(store.read_installed(&key(), &payload()).unwrap(), b"complete");
    store.discard_staging(lock, &key()).unwrap();
    assert!(store.staged().unwrap().is_empty());
    store.remove_installed(lock, &key()).unwrap();
    assert!(store.installed().unwrap().is_empty());
    store.unlock(lock);
}

#[test]
#[cfg(unix)]
fn bounded_reader_exact_limit_and_hardlink_refusal() {
    let fixture = Fixture::new();
    let path = fixture.0.join("bytes");
    fs::write(&path, b"12345678").unwrap();
    assert_eq!(bounded_read(&path, 8).unwrap(), b"12345678");
    assert_eq!(bounded_read(&path, 7), Err(DistributionError::LimitExceeded { limit: 7 }));
    fs::hard_link(&path, fixture.0.join("alias")).unwrap();
    assert_eq!(bounded_read(&path, 8), Err(DistributionError::Interrupted));
    fs::remove_file(fixture.0.join("alias")).unwrap();
    assert_eq!(bounded_read(&path, 8).unwrap(), b"12345678");
}

#[test]
#[ignore = "TODO: execute on Windows with native atomic replacement, directory flush and owner-only ACL assertions"]
fn windows_replace_and_acl_semantics() {
    // TODO: qualify old/new complete replacement and inherited/explicit ACL refusal on Windows.
    // This test must gain native assertions before Windows store operations are enabled.
    assert!(cfg!(windows), "requires a Windows executor");
}
