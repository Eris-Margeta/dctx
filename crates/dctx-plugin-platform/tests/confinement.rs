//! Native subprocess adversaries for Linux Landlock/seccomp and bootstrap refusal.
//! PP4/PP8: successful restricted computation precedes escape checks; no fake provider claims.
//! Fixtures and markers are private and disposable; this never reads the real user home.

#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]

use dctx_plugin_platform::NoSecretEnvironment;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn command(mode: &str, path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-platform-probe"));
    NoSecretEnvironment.apply(&mut command);
    command
        .arg(mode)
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run(mut command: Command) -> Output {
    let mut child = command.spawn().unwrap();
    if let Some(mut input) = child.stdin.take() {
        input.write_all(b"ping").ok();
    }
    // A watchdog bounds broken fixtures, not worker progress policy or performance.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if std::time::Instant::now() >= until {
            child.kill().ok();
            let output = child.wait_with_output().unwrap();
            panic!("probe watchdog: {output:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn restricted_computation_and_every_escape_run_in_distinct_processes() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let source = dir.path().join("source");
    let home = dir.path().join("fake-home");
    let cache = dir.path().join("fake-cache");
    for path in [&source, &home, &cache] {
        std::fs::write(path, b"private-fixture").unwrap();
    }

    let control = run(command("compute", &source));
    if control.status.code() == Some(125) {
        let error = String::from_utf8_lossy(&control.stderr);
        assert!(
            error.contains("Landlock") || error.contains("Seccomp"),
            "unexpected bootstrap failure: {error}"
        );
        writeln!(
            std::io::stdout(),
            "PL-Ab UNQUALIFIED KERNEL: {error}; no native confinement evidence claimed"
        )
        .unwrap();
        return;
    }
    assert!(
        control.status.success(),
        "positive control failed: {control:?}"
    );
    assert_eq!(control.stdout, b"pong\n");
    for path in [&source, &home, &cache] {
        let result = run(command("read", path));
        assert_eq!(result.status.signal(), Some(31), "direct read: {result:?}");
        assert_eq!(result.stdout, []);
    }
    let marker = dir.path().join("escape-marker");
    for mode in [
        "write",
        "metadata",
        "ipv4",
        "ipv6",
        "udp",
        "unix",
        "socketpair",
        "spawn",
        "git-descendant",
        "exec",
        "detach",
        "thread",
        "cwd",
        "parent-death-reset",
    ] {
        let result = run(command(mode, &marker));
        assert_eq!(result.status.signal(), Some(31), "{mode}: {result:?}");
        assert!(result.stdout.is_empty(), "{mode} wrote before termination");
        assert!(!marker.exists(), "{mode} escaped");
    }
    for path in [&source, &home, &cache] {
        assert_eq!(std::fs::read(path).unwrap(), b"private-fixture");
    }
    let reaped = run(command("parent-crash", &source));
    assert!(reaped.status.success(), "parent crash: {reaped:?}");
    assert_eq!(reaped.stdout, b"reaped-after-parent-crash\n");
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
    writeln!(std::io::stdout(), "PL-Ab native evidence: computation, 3 direct reads, 14 syscall adversaries, parent crash/reap; arch={} kernel={}",
        std::env::consts::ARCH, kernel.trim()).unwrap();
}

#[test]
fn landlock_denial_is_observed_independently_of_the_syscall_filter() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let fixture = dir.path().join("private");
    std::fs::write(&fixture, b"private-fixture").unwrap();
    let result = run(command("landlock", &fixture));
    if result.status.code() == Some(125) {
        assert!(String::from_utf8_lossy(&result.stderr).contains("Landlock"));
        writeln!(
            std::io::stdout(),
            "PL-Ab UNQUALIFIED KERNEL: Landlock unavailable"
        )
        .unwrap();
    } else {
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, b"landlock-denied\n");
        writeln!(
            std::io::stdout(),
            "PL-Ab Landlock ABI4 denial observed independently of seccomp"
        )
        .unwrap();
    }
}

#[test]
fn bootstrap_never_calls_body_with_secrets_files_or_wrong_parent() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let fixture = dir.path().join("secret-fixture");
    std::fs::write(&fixture, b"private-fixture").unwrap();
    let clean = run(command("environment", &fixture));
    assert!(clean.status.success());
    assert_eq!(clean.stdout, b"clean\n");
    let mut dirty = command("compute", &fixture);
    dirty.env("TOKEN", "fixture-secret");
    let mut file_stdio = command("compute", &fixture);
    file_stdio.stdin(std::fs::File::open(&fixture).unwrap());
    for request in [
        dirty,
        command("inherited", &fixture),
        command("wrong-parent", &fixture),
        command("threads-at-entry", &fixture),
        file_stdio,
    ] {
        let result = run(request);
        assert_eq!(result.status.code(), Some(125), "{result:?}");
        assert_eq!(result.stdout, []);
        assert!(!String::from_utf8_lossy(&result.stderr).contains("fixture-secret"));
    }
}
