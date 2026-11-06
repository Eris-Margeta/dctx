// SPDX-License-Identifier: Apache-2.0
//! One dctx run in a fresh directory: copy the place, set an empty environment and resource
//! limits, wait with a wall-clock timeout, read capped output, delete everything.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::grammar::File;

/// Resource limits for each run.
#[derive(Clone, Debug)]
pub struct Limits {
    pub wall: Duration,
    pub cpu_seconds: u64,
    pub address_space: u64,
    pub open_files: u64,
    pub file_size: u64,
    /// 0 leaves RLIMIT_NPROC alone (it counts every process of the user, so it only fits a container).
    pub processes: u64,
    pub output_cap: usize,
    pub stderr_cap: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            wall: Duration::from_secs(3),
            cpu_seconds: 2,
            address_space: 256 << 20,
            open_files: 64,
            file_size: 8 << 20,
            processes: 16,
            output_cap: 256 << 10,
            stderr_cap: 16 << 10,
        }
    }
}

/// What the place is built from.
pub enum Source<'a> {
    Sample { id: &'a str, dir: &'a Path },
    Files(&'a [File]),
}

/// The outcome of one run.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    /// `None` when the run was killed.
    pub exit: Option<i32>,
    pub killed: Option<&'static str>,
    pub truncated: bool,
    /// Full stdout size before any truncation.
    pub bytes: u64,
    pub ms: u64,
}

/// A run directory that removes itself.
struct RunDir(PathBuf);

impl Drop for RunDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn random_hex() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    let mut entries: Vec<_> = fs::read_dir(from)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[repr(C)]
struct Rlimit {
    cur: u64,
    max: u64,
}

extern "C" {
    fn setrlimit(resource: i32, limit: *const Rlimit) -> i32;
}

// Linux x86_64 / aarch64 resource numbers.
const RLIMIT_CPU: i32 = 0;
const RLIMIT_FSIZE: i32 = 1;
const RLIMIT_NPROC: i32 = 6;
const RLIMIT_NOFILE: i32 = 7;
const RLIMIT_AS: i32 = 9;

fn limit(resource: i32, value: u64) -> io::Result<()> {
    let rl = Rlimit { cur: value, max: value };
    // SAFETY: setrlimit reads one initialised struct and touches no Rust state.
    if unsafe { setrlimit(resource, &rl) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn read_capped(path: &Path, cap: usize) -> io::Result<(String, bool, u64)> {
    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(cap as u64).read_to_end(&mut bytes)?;
    let truncated = size > cap as u64;
    // Cut at a character boundary.
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let valid = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).unwrap_or_default()
        }
    };
    Ok((text, truncated, size))
}

/// Runs `dctx argv` on a fresh copy of the source under `work`.
pub fn run(dctx: &Path, work: &Path, source: Source<'_>, argv: &[String], stdin: Option<&str>, limits: &Limits) -> io::Result<Outcome> {
    let dir = RunDir(work.join(random_hex()?));
    fs::create_dir_all(&dir.0)?;
    let place = match source {
        Source::Sample { id, dir: sample } => {
            let place = dir.0.join(id);
            copy_tree(sample, &place)?;
            place
        }
        Source::Files(files) => {
            let place = dir.0.join("project");
            fs::create_dir_all(&place)?;
            for file in files {
                let target = place.join(&file.path);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(target, &file.content)?;
            }
            place
        }
    };
    for sub in ["cfg", "cache"] {
        fs::create_dir_all(dir.0.join(sub))?;
    }
    let out_path = dir.0.join("stdout");
    let err_path = dir.0.join("stderr");
    let bin_dir = dctx.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut command = Command::new(dctx);
    command
        .args(argv)
        .current_dir(&place)
        .env_clear()
        .env("PATH", bin_dir)
        .env("HOME", &dir.0)
        .env("XDG_CONFIG_HOME", dir.0.join("cfg"))
        .env("DCTX_CACHE_DIR", dir.0.join("cache"))
        .env("DCTX_AUTO_CLEAN", "0")
        .env("DCTX_MAX_SOURCE_BYTES", "4194304")
        .env("DCTX_MAX_ENTRIES", "1000")
        .env("DCTX_MAX_RECORDS", "20000")
        .env("LC_ALL", "C.UTF-8")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(fs::File::create(&out_path)?)
        .stderr(fs::File::create(&err_path)?);
    let rl = limits.clone();
    // SAFETY: the closure only calls setrlimit, which is async-signal-safe, between fork and exec.
    unsafe {
        command.pre_exec(move || {
            limit(RLIMIT_CPU, rl.cpu_seconds)?;
            limit(RLIMIT_AS, rl.address_space)?;
            limit(RLIMIT_NOFILE, rl.open_files)?;
            limit(RLIMIT_FSIZE, rl.file_size)?;
            if rl.processes > 0 {
                limit(RLIMIT_NPROC, rl.processes)?;
            }
            Ok(())
        });
    }
    let started = Instant::now();
    let mut child = command.spawn()?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // Small (≤ 4 MCP requests); a closed pipe just means the child exited early.
        let _ = pipe.write_all(text.as_bytes());
    }
    let mut killed = None;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= limits.wall {
            let _ = child.kill();
            killed = Some("timeout");
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(4));
    };
    let ms = started.elapsed().as_millis() as u64;
    let (stdout, cut_out, bytes) = read_capped(&out_path, limits.output_cap)?;
    let (stderr, cut_err, _) = read_capped(&err_path, limits.stderr_cap)?;
    let exit = if killed.is_some() { None } else { status.code() };
    if exit.is_none() && killed.is_none() {
        killed = Some("signal");
    }
    Ok(Outcome { stdout, stderr, exit, killed, truncated: cut_out || cut_err, bytes, ms })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dctx-play-test-{tag}-{}", random_hex().unwrap()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn limits() -> Limits {
        Limits { processes: 0, ..Limits::default() }
    }

    #[test]
    fn runs_in_an_empty_environment_and_cleans_up() {
        let tmp = scratch("env");
        let fake = script(&tmp, "dctx", "pwd; /usr/bin/env | /usr/bin/sort; echo err >&2; exit 3");
        let work = tmp.join("work");
        let files = [File { path: "src/a.rs".into(), content: "fn a() {}".into() }];
        let out = run(&fake, &work, Source::Files(&files), &[], None, &limits()).unwrap();
        assert_eq!(out.exit, Some(3));
        assert!(out.stdout.lines().next().unwrap().ends_with("/project"));
        assert!(out.stdout.contains("DCTX_AUTO_CLEAN=0\n"));
        assert!(!out.stdout.contains("CARGO"), "the parent environment does not leak");
        assert_eq!(out.stderr, "err\n");
        assert_eq!(fs::read_dir(&work).unwrap().count(), 0, "the run directory is removed");
        fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn kills_a_run_at_the_wall_clock_limit() {
        let tmp = scratch("timeout");
        let fake = script(&tmp, "dctx", "exec /bin/sleep 10");
        let limits = Limits { wall: Duration::from_millis(300), ..limits() };
        let started = Instant::now();
        let out = run(&fake, &tmp.join("work"), Source::Files(&[File { path: "a".into(), content: "x".into() }]), &[], None, &limits).unwrap();
        assert_eq!(out.exit, None);
        assert_eq!(out.killed, Some("timeout"));
        assert!(started.elapsed() < Duration::from_secs(3));
        fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn caps_output_and_reports_the_full_size() {
        let tmp = scratch("cap");
        let fake = script(&tmp, "dctx", "/usr/bin/head -c 5000 /dev/zero | /usr/bin/tr '\\0' x");
        let limits = Limits { output_cap: 1000, ..limits() };
        let out = run(&fake, &tmp.join("work"), Source::Files(&[File { path: "a".into(), content: "x".into() }]), &[], None, &limits).unwrap();
        assert_eq!(out.stdout.len(), 1000);
        assert!(out.truncated);
        assert_eq!(out.bytes, 5000);
        fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn copies_a_sample_under_its_own_name_and_feeds_stdin() {
        let tmp = scratch("sample");
        let sample = tmp.join("kiosk-src");
        fs::create_dir_all(sample.join("src")).unwrap();
        fs::write(sample.join("src/a.rs"), "fn a() {}").unwrap();
        let fake = script(&tmp, "dctx", "/usr/bin/basename \"$PWD\"; /bin/cat src/a.rs; echo; /bin/cat");
        let out = run(&fake, &tmp.join("work"), Source::Sample { id: "kiosk", dir: &sample }, &[], Some("ping\n"), &limits()).unwrap();
        assert_eq!(out.stdout, "kiosk\nfn a() {}\nping\n");
        fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn file_size_limit_stops_a_runaway_writer() {
        let tmp = scratch("fsize");
        let fake = script(&tmp, "dctx", "/usr/bin/head -c 100000 /dev/zero > big; echo $?");
        let limits = Limits { file_size: 4096, ..limits() };
        let out = run(&fake, &tmp.join("work"), Source::Files(&[File { path: "a".into(), content: "x".into() }]), &[], None, &limits).unwrap();
        assert_ne!(out.stdout.trim(), "0", "the write past RLIMIT_FSIZE fails");
        fs::remove_dir_all(tmp).unwrap();
    }
}
