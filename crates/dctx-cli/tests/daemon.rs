// SPDX-License-Identifier: Apache-2.0
//! End-to-end daemon lifecycle and exact CLI transport parity on Unix and Windows.
mod common;

use std::fs;
use std::io::{Read, Write};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        let result = Self(tempfile::tempdir().expect("fixture"));
        fs::create_dir(result.0.path().join("place")).expect("place");
        fs::write(result.0.path().join("place/a.rs"), "fn kept() {}\n").expect("source");
        result
    }
    fn command(&self, enabled: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.env_clear().current_dir(self.0.path().join("place"));
        common::isolated_dirs(&mut command, self.0.path());
        command
            .env("DCTX_CACHE_DIR", self.0.path().join("c"))
            .env("DCTX_AUTO_CLEAN", "0")
            .env("DCTX_DAEMON", enabled)
            .env("DCTX_DAEMON_IDLE_SECONDS", "30");
        command
    }
    fn run(&self, args: &[&str], enabled: &str) -> Output {
        self.command(enabled).args(args).output().expect("command")
    }
    fn status(&self) -> Value {
        let output = self.run(&["daemon", "status"], "0");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("status JSON")
    }
    fn pid(&self) -> u64 {
        self.status()["daemon"]["pid"]
            .as_u64()
            .expect("running daemon")
    }
    fn parity(&self, args: &[&str]) {
        // The engine reports whole-second age. Compare full bytes at an equal age;
        // retry only the clock boundary, never remove or rewrite an output field.
        for attempt in 0..4 {
            let local = self.run(args, "0");
            let remote = self.run(args, "1");
            let age = |output: &Output| {
                serde_json::from_slice::<Value>(&output.stdout)
                    .ok()
                    .and_then(|page| page.get("freshness")?.get("age_s").cloned())
            };
            if age(&local) != age(&remote) && attempt < 3 {
                continue;
            }
            assert_eq!(local.status.code(), remote.status.code(), "{args:?}");
            assert_eq!(local.stdout, remote.stdout, "stdout {args:?}");
            assert_eq!(local.stderr, remote.stderr, "stderr {args:?}");
            return;
        }
        unreachable!();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.run(&["daemon", "stop"], "0");
    }
}

#[test]
fn autostart_keeps_one_process_and_preserves_pages_errors_and_live_edits() {
    let fixture = Fixture::new();
    assert_eq!(fixture.status()["running"], false);
    fixture.parity(&["map", "find", "kept", "--json"]);
    let pid = fixture.pid();
    for args in [
        &["pack"][..],
        &["pack", "--json"],
        &["map", "find", "kept"],
        &["map", "read", "m:a.rs", "--json"],
        &["read", "a.rs", "--json"],
        &["map", "neighbors", "s:a.rs#kept", "--json"],
        &["map", "explain", "s:a.rs#kept", "--json"],
        &["map", "grep", "kept", "--json"],
        &["pack", "--for", "`kept`", "--json"],
        &["map", "read", "s:a.rs#missing", "--json"],
        &["map", "find", "kept", "--unknown"],
        &["map", "find", "kept", "--place", "missing"],
    ] {
        fixture.parity(args);
        assert_eq!(fixture.pid(), pid);
    }
    fs::write(fixture.0.path().join("place/a.rs"), "fn lost() {}\n").expect("edit");
    fixture.parity(&["map", "find", "lost", "--json"]);
    let page = fixture.run(&["map", "find", "lost", "--json"], "1");
    assert!(String::from_utf8_lossy(&page.stdout).contains("s:a.rs#lost"));
    fs::write(fixture.0.path().join("place/.dctxignore"), "a.rs\n").expect("rules");
    fixture.parity(&["map", "find", "lost", "--json"]);
    assert_eq!(fixture.pid(), pid);
    assert!(!fixture.0.path().join("place/.dctx").exists());
}

#[test]
fn configuration_enables_autostart_and_environment_can_disable_it() {
    let fixture = Fixture::new();
    assert!(fixture
        .run(&["daemon", "config", "on"], "0")
        .status
        .success());
    assert!(fixture.run(&["map", "find", "kept"], "0").status.success());
    assert_eq!(fixture.status()["running"], false);
    let output = fixture
        .command("0")
        .env_remove("DCTX_DAEMON")
        .args(["map", "find", "kept"])
        .output()
        .expect("config auto-start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fixture.status()["running"], true);
    assert!(fixture
        .run(&["daemon", "config", "off"], "0")
        .status
        .success());
}

#[test]
fn idle_exit_and_crash_restart_leave_no_stale_listener() {
    let fixture = Fixture::new();
    let mut child = fixture
        .command("1")
        .env("DCTX_DAEMON_IDLE_SECONDS", "1")
        .arg("--internal-daemon")
        .spawn()
        .expect("daemon");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.status()["running"].as_bool().expect("boolean") {
        assert!(Instant::now() < deadline, "daemon did not start");
        std::thread::sleep(Duration::from_millis(30));
    }
    // Polling status would itself reset the idle timer. Wait for the process instead.
    loop {
        if let Some(status) = child.try_wait().expect("wait") {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "idle exit timed out");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(fixture.status()["running"], false);
    let mut child = fixture
        .command("1")
        .arg("--internal-daemon")
        .spawn()
        .expect("daemon");
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.status()["running"] != true {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(30));
    }
    child.kill().expect("kill");
    child.wait().expect("reap");
    fixture.parity(&["map", "find", "kept", "--json"]);
    assert_eq!(fixture.status()["running"], true);
}

#[test]
fn environment_change_restarts_instead_of_reusing_inherited_engine_settings() {
    let fixture = Fixture::new();
    fixture.parity(&["map", "find", "kept", "--json"]);
    let pid = fixture.pid();
    let args = ["map", "find", "kept", "--json"];
    let local = fixture
        .command("0")
        .env("DCTX_MAX_RECORDS", "1")
        .args(args)
        .output()
        .expect("local cap");
    let remote = fixture
        .command("1")
        .env("DCTX_MAX_RECORDS", "1")
        .args(args)
        .output()
        .expect("daemon cap");
    assert_eq!(local.status.code(), Some(3));
    assert_eq!(local.status.code(), remote.status.code());
    assert_eq!(local.stdout, remote.stdout);
    assert_eq!(local.stderr, remote.stderr);
    assert_ne!(fixture.pid(), pid);
}

fn endpoint(fixture: &Fixture) -> std::path::PathBuf {
    let directory = fixture.0.path().join(format!(
        "c/{}/daemon",
        dctx_cli::host::engine_cache_namespace()
    ));
    let mut hash = dctx_hash::Sha256::new();
    hash.update(directory.as_os_str().as_encoded_bytes())
        .expect("hash");
    dctx_daemon_transport::endpoint(&directory, &hash.finalize().to_string())
}
fn write_frame(stream: &mut impl Write, value: Value) {
    let bytes = serde_json::to_vec(&value).expect("encode");
    stream
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .expect("length");
    stream.write_all(&bytes).expect("frame");
}
fn read_frame(stream: &mut impl Read) -> Value {
    let mut len = [0; 4];
    stream.read_exact(&mut len).expect("length");
    let len = u32::from_le_bytes(len) as usize;
    assert!(len < 1024 * 1024);
    let mut bytes = vec![0; len];
    stream.read_exact(&mut bytes).expect("frame");
    serde_json::from_slice(&bytes).expect("decode")
}

#[test]
fn mismatched_handshake_exits_and_next_cli_autostarts_matching_daemon() {
    let fixture = Fixture::new();
    fixture.parity(&["map", "find", "kept", "--json"]);
    let pid = fixture.pid();
    let mut stream = dctx_daemon_transport::connect(&endpoint(&fixture), Duration::from_secs(2))
        .expect("connect");
    write_frame(
        &mut stream,
        json!({"op":"hello", "identity":"daemon-v1-other-version", "context":"different"}),
    );
    assert_eq!(read_frame(&mut stream)["stopping"], true);
    drop(stream);
    fixture.parity(&["map", "find", "kept", "--json"]);
    assert_ne!(fixture.pid(), pid);
}

#[test]
fn unusable_transport_falls_back_without_changing_bytes() {
    let fixture = Fixture::new();
    fs::create_dir_all(
        fixture
            .0
            .path()
            .join("c")
            .join(dctx_cli::host::engine_cache_namespace()),
    )
    .expect("cache");
    fs::write(
        fixture.0.path().join(format!(
            "c/{}/daemon",
            dctx_cli::host::engine_cache_namespace()
        )),
        "occupied",
    )
    .expect("block daemon");
    fixture.parity(&["map", "find", "kept", "--json"]);
    fixture.parity(&["pack", "--for", "`kept`", "--json"]);
}

#[cfg(unix)]
#[test]
fn socket_and_directory_modes_are_private_even_with_normal_umask() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fixture.parity(&["map", "find", "kept", "--json"]);
    assert_eq!(
        fs::metadata(fixture.0.path().join(format!(
            "c/{}/daemon",
            dctx_cli::host::engine_cache_namespace()
        )))
        .expect("dir")
        .permissions()
        .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(endpoint(&fixture))
            .expect("socket")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    fixture.run(&["daemon", "stop"], "0");
    std::thread::sleep(Duration::from_millis(50));
    fs::set_permissions(
        fixture.0.path().join(format!(
            "c/{}/daemon",
            dctx_cli::host::engine_cache_namespace()
        )),
        fs::Permissions::from_mode(0o755),
    )
    .expect("unsafe directory");
    fixture.parity(&["map", "find", "kept", "--json"]);
    assert!(!endpoint(&fixture).exists());
}

#[test]
fn cli_detects_a_different_daemon_executable_and_restarts_it() {
    let fixture = Fixture::new();
    // status prepares the private directory without starting a server.
    assert_eq!(fixture.status()["running"], false);
    let mut listener =
        dctx_daemon_transport::Listener::bind(&endpoint(&fixture)).expect("old server");
    let old = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept(Duration::from_secs(2)) {
                Ok(mut stream) => {
                    assert_eq!(read_frame(&mut stream)["op"], "hello");
                    write_frame(
                        &mut stream,
                        json!({"identity":"daemon-v1-older-build", "context":"old", "stopping":true}),
                    );
                    let _ = read_frame(&mut stream);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    let output = fixture.run(&["map", "find", "kept", "--json"], "1");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    old.join().expect("old daemon exited");
    assert_eq!(fixture.status()["running"], true);
}

#[test]
fn invalid_and_abandoned_frames_do_not_poison_the_next_query() {
    let fixture = Fixture::new();
    fixture.parity(&["map", "find", "kept", "--json"]);
    let pid = fixture.pid();
    for length in [0u32, u32::MAX] {
        let mut stream =
            dctx_daemon_transport::connect(&endpoint(&fixture), Duration::from_secs(2))
                .expect("connect");
        stream.write_all(&length.to_le_bytes()).expect("bad frame");
        drop(stream);
    }
    fixture.parity(&["map", "find", "kept", "--json"]);
    assert_eq!(fixture.pid(), pid);
}

#[test]
fn maintenance_choice_tracks_each_cli_invocation() {
    let fixture = Fixture::new();
    for extra in [&[][..], &["--no-auto-clean"][..], &[][..]] {
        let mut args = vec!["map", "find", "kept", "--json"];
        args.extend_from_slice(extra);
        let local = fixture
            .command("0")
            .env_remove("DCTX_AUTO_CLEAN")
            .args(&args)
            .output()
            .expect("local");
        let remote = fixture
            .command("1")
            .env_remove("DCTX_AUTO_CLEAN")
            .args(&args)
            .output()
            .expect("daemon");
        assert!(local.status.success());
        assert_eq!(local.status.code(), remote.status.code());
        assert_eq!(local.stdout, remote.stdout);
        assert_eq!(local.stderr, remote.stderr);
        assert_eq!(fixture.status()["running"], true);
    }
}
