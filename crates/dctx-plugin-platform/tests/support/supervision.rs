// SPDX-License-Identifier: Apache-2.0
//! Disposable three-process parent-crash fixture for Linux PP8 evidence.
//! A dedicated subreaper owns the experiment, so no orphan/zombie escapes into the suite.
//! The confined grandchild computes forever; EOF cooperation cannot explain its death.

use dctx_plugin_platform::{NoSecretEnvironment, PlatformError, Unsupported};
use rustix::process::{Pid, Signal, WaitOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};

struct Reap(Child);

impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn(mode: &str) -> Result<Reap, PlatformError> {
    let mut command = Command::new(std::env::current_exe().map_err(|_| PlatformError::Io)?);
    NoSecretEnvironment.apply(&mut command);
    command
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.spawn().map(Reap).map_err(|_| PlatformError::Io)
}

pub(super) fn parent() -> Result<(), PlatformError> {
    rustix::process::set_parent_process_death_signal(Some(Signal::KILL))
        .map_err(|_| PlatformError::Io)?;
    let mut worker = spawn("idle")?;
    let mut line = String::new();
    let output = worker.0.stdout.take().ok_or(PlatformError::Io)?;
    BufReader::new(output)
        .take(128)
        .read_line(&mut line)
        .map_err(|_| PlatformError::Io)?;
    if line != "ready\n" {
        return Err(PlatformError::Unsupported(Unsupported::Landlock));
    }
    println!("{}", worker.0.id());
    std::io::stdout().flush().map_err(|_| PlatformError::Io)?;
    let _ = std::io::stdin().read(&mut [0_u8]);
    Ok(())
}

pub(super) fn crash_and_reap() -> Result<(), PlatformError> {
    rustix::process::set_child_subreaper(Some(rustix::process::getpid()))
        .map_err(|_| PlatformError::Io)?;
    let mut parent = spawn("supervisor")?;
    let mut line = String::new();
    let output = parent.0.stdout.take().ok_or(PlatformError::Io)?;
    BufReader::new(output)
        .take(128)
        .read_line(&mut line)
        .map_err(|_| PlatformError::Io)?;
    let pid = line
        .trim()
        .parse::<i32>()
        .ok()
        .and_then(Pid::from_raw)
        .ok_or(PlatformError::Unsupported(Unsupported::Landlock))?;
    parent.0.kill().map_err(|_| PlatformError::Io)?;
    parent.0.wait().map_err(|_| PlatformError::Io)?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some((_, status)) = rustix::process::waitpid(Some(pid), WaitOptions::NOHANG)
            .map_err(|_| PlatformError::Io)?
        {
            if status.terminating_signal() != Some(9) {
                return Err(PlatformError::Conflict);
            }
            println!("reaped-after-parent-crash");
            return Ok(());
        }
        if std::time::Instant::now() >= until {
            let _ = rustix::process::kill_process(pid, Signal::KILL);
            let _ = rustix::process::waitpid(Some(pid), WaitOptions::empty());
            return Err(PlatformError::Conflict);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
