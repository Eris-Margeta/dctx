//! Host and process-RSS evidence for one DX-A1 measurement process.
//! Implements ARCHITECTURE-PROPOSAL §3 resource accounting for Stage 0.
//! Depends on the stats parsers; the measure adapter records the result.

use crate::error::BenchError;
use crate::stats::{rss_from_proc_status, rss_from_ps};

/// Compiler and machine facts observed for this process.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostInfo {
    /// `std::env::consts::OS`.
    pub os: String,
    /// `std::env::consts::ARCH`.
    pub arch: String,
    /// `available_parallelism`, or 1 when the platform refuses.
    pub parallelism: usize,
    /// `rustc --version`, or `unavailable`.
    pub rustc: String,
    /// `release` or `debug`, from `debug_assertions`.
    pub profile: String,
}

/// Resident set size and the source that produced it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Resident {
    /// Bytes.
    pub bytes: u64,
    /// `proc-vmhwm` or `ps-rss`.
    pub source: String,
}

/// Captures host facts. Does not sample RSS.
pub fn host() -> HostInfo {
    HostInfo {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        parallelism: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        rustc: command_stdout("rustc", &["--version"]).unwrap_or_else(|| "unavailable".into()),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .to_owned(),
    }
}

/// Reads the high-water RSS. Refuses when neither proc nor `ps` can answer.
pub fn resident() -> Result<Resident, BenchError> {
    if let Ok(text) = std::fs::read_to_string("/proc/self/status")
        && let Some(bytes) = rss_from_proc_status(&text)
    {
        return Ok(Resident {
            bytes,
            source: "proc-vmhwm".into(),
        });
    }
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .map_err(|error| BenchError::Message(format!("ps failed: {error}")))?;
    if !output.status.success() {
        return Err(BenchError::Message(format!(
            "ps exited {}",
            output.status
        )));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| BenchError::Message("ps RSS was not utf-8".into()))?;
    let bytes = rss_from_ps(&text)
        .ok_or_else(|| BenchError::Message(format!("ps RSS was not a number: {text}")))?;
    Ok(Resident {
        bytes,
        source: "ps-rss".into(),
    })
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}
