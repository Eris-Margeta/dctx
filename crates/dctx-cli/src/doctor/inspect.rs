// SPDX-License-Identifier: Apache-2.0
//! Bounded version probes, streaming SHA-256 and read-only cache accounting.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use dctx_hash::Sha256;

use super::{
    discover::{same_file, Context},
    Binary,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const VERSION_BYTES: u64 = 4096;
const NOT_EXECUTED: &str = "not executed: found only in ./node_modules/.bin, VIRTUAL_ENV or a relative PATH entry, which an untrusted checkout can plant; rerun with --probe-project to run its --version";

/// Engine version and optional short build commit parsed from `dctx --version`.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Probe {
    pub version: String,
    pub commit: Option<String>,
}

pub(super) fn binaries(
    context: &Context,
    cache_base: Result<PathBuf, String>,
    probe_project: bool,
) -> Vec<Binary> {
    let mut sizes = BTreeMap::new();
    context
        .candidates()
        .into_iter()
        .map(|path| {
            let mut errors = Vec::new();
            let real_path = record(
                fs::canonicalize(&path).map_err(|error| error.to_string()),
                "path",
                &mut errors,
            );
            let running = same_file(&path, &context.running);
            let probed = if running {
                Ok(Probe {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    commit: Some(env!("DCTX_GIT_SHA").to_string()),
                })
            } else if probe_project || context.trusted_to_execute(&path) {
                probe_version(&path)
            } else {
                Err(NOT_EXECUTED.into())
            };
            let (version, commit) = match record(probed, "version", &mut errors) {
                Some(probe) => (Some(probe.version), probe.commit),
                None => (None, None),
            };
            let sha256 = record(checksum(&path), "sha256", &mut errors);
            let cache_root = match (&cache_base, &version) {
                (Ok(base), Some(version)) => {
                    namespace(version).map(|namespace| base.join(namespace))
                }
                (Err(error), _) => {
                    errors.push(format!("cache: {error}"));
                    None
                }
                _ => None,
            };
            let cache_size_bytes = cache_root.as_ref().and_then(|root| {
                let size = sizes
                    .entry(root.clone())
                    .or_insert_with(|| directory_size(root));
                record(size.clone(), "cache size", &mut errors)
            });
            Binary {
                scope: context.scope(&path),
                path,
                real_path,
                running,
                version,
                commit,
                cache_root,
                cache_size_bytes,
                sha256,
                errors,
            }
        })
        .collect()
}

fn record<T>(result: Result<T, String>, label: &str, errors: &mut Vec<String>) -> Option<T> {
    result
        .map_err(|error| errors.push(format!("{label}: {error}")))
        .ok()
}

pub(super) fn checksum(path: &Path) -> Result<String, String> {
    if !fs::metadata(path)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("not a regular file".into());
    }
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("not a regular file".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count])
            .map_err(|error| error.to_string())?;
    }
    Ok(hash
        .finalize()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Logical file bytes; links and special files are never traversed or counted.
pub(super) fn directory_size(root: &Path) -> Result<u64, String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("cache root is not a directory or is a symlink".into());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut bytes = 0u64;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                bytes = bytes
                    .checked_add(metadata.len())
                    .ok_or("cache size overflow")?;
            }
        }
    }
    Ok(bytes)
}

/// Parses only dctx's version line; never uses candidate output as a path.
pub(super) fn version_line(text: &str) -> Option<Probe> {
    let mut words = text.lines().next()?.split_whitespace();
    if words.next()? != "dctx" {
        return None;
    }
    let version = words.next()?;
    namespace(version)?;
    // `dctx X.Y.Z (abc1234)`: keep the commit only when it is plain hex, never arbitrary candidate text.
    let commit = words
        .next()
        .and_then(|word| word.strip_prefix('(')?.strip_suffix(')'))
        .filter(|sha| {
            (1..=64).contains(&sha.len()) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .map(str::to_string);
    Some(Probe {
        version: version.to_string(),
        commit,
    })
}

pub(super) fn namespace(version: &str) -> Option<String> {
    let core = version.split(['-', '+']).next()?;
    let mut numbers = core.split('.');
    let major = numbers.next()?;
    let minor = numbers.next()?;
    let patch = numbers.next()?;
    if numbers.next().is_some()
        || [major, minor, patch]
            .iter()
            .any(|number| number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()))
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
    {
        return None;
    }
    Some(format!("{major}.{minor}"))
}

pub(super) fn probe_version(path: &Path) -> Result<Probe, String> {
    let mut child = Command::new(path)
        .arg("--version")
        .env("DCTX_AUTO_CLEAN", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("version stdout unavailable".into());
    };
    // Drain a bounded amount concurrently so a noisy/broken binary cannot fill the pipe and hang doctor.
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout
            .take(VERSION_BYTES + 1)
            .read_to_end(&mut output)
            .map(|_| output);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(status)) => break Err(format!("--version exited with {status}")),
            Err(error) => break Err(error.to_string()),
            Ok(None) if Instant::now() >= deadline => break Err("--version timed out".into()),
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    };
    if outcome.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome?;
    let output = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "--version output timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if output.len() > VERSION_BYTES as usize {
        return Err("--version output exceeds 4096 bytes".into());
    }
    let text = std::str::from_utf8(&output).map_err(|error| error.to_string())?;
    version_line(text).ok_or_else(|| "unrecognized dctx --version output".into())
}
