// SPDX-License-Identifier: Apache-2.0
//! Linux-only adversary operations for the trusted subprocess fixture.
//! Each forbidden effect is attempted after `run_restricted`; success is a test failure.
//! Depends on safe std/rustix APIs only; never executes arbitrary input as command text.

use dctx_plugin_platform::linux::{RestrictedEntry, restrict_filesystem, run_restricted};
use dctx_plugin_platform::{PlatformError, Unsupported};
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpStream, UdpSocket};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Command;

#[path = "supervision.rs"]
mod supervision;

fn io_error(_: std::io::Error) -> PlatformError {
    PlatformError::Io
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the confinement probe effect-dispatch cases together so each forbidden syscall has one explicit test entry."
)]
pub(super) fn run() -> Result<(), PlatformError> {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).ok_or(PlatformError::InvalidPath)?.clone();
    let path = args.get(2).cloned().unwrap_or_default();
    if mode == "supervisor" {
        return supervision::parent();
    }
    if mode == "parent-crash" {
        return supervision::crash_and_reap();
    }
    if mode == "environment" {
        if !dctx_plugin_platform::NoSecretEnvironment.is_clean() {
            return Err(PlatformError::Unsupported(Unsupported::Bootstrap));
        }
        println!("clean");
        return Ok(());
    }
    if mode == "landlock" {
        let before = std::fs::read(&path).map_err(io_error)?;
        if before != b"private-fixture" {
            return Err(PlatformError::Conflict);
        }
        restrict_filesystem()?;
        match std::fs::read(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                println!("landlock-denied");
                return Ok(());
            }
            _ => return Err(PlatformError::Conflict),
        }
    }
    // This handle deliberately survives until bootstrap, which must refuse it.
    let held = if mode == "inherited" {
        Some(File::open(&path).map_err(io_error)?)
    } else {
        None
    };
    let thread = if mode == "threads-at-entry" {
        Some(std::thread::spawn(std::thread::park))
    } else {
        None
    };
    let parent =
        rustix::process::getppid().ok_or(PlatformError::Unsupported(Unsupported::Lifecycle))?;
    let parent = if mode == "wrong-parent" {
        u32::try_from(parent.as_raw_pid()).expect("positive parent PID") + 1
    } else {
        u32::try_from(parent.as_raw_pid()).expect("positive parent PID")
    };
    let entry = RestrictedEntry::new(parent, 256 * 1024 * 1024)?;
    run_restricted(entry, move || {
        let _held = held;
        let _thread = thread;
        match mode.as_str() {
            "compute" => {
                let mut input = [0; 4];
                if std::io::stdin().read_exact(&mut input).is_err() || input != *b"ping" {
                    return 91;
                }
                let bytes = vec![7_u8; 128 * 1024];
                if bytes.iter().map(|&b| u64::from(b)).sum::<u64>() != 7 * 128 * 1024 {
                    return 92;
                }
                if std::io::stdout().write_all(b"pong\n").is_err() {
                    return 93;
                }
                return 0;
            }
            "read" => {
                let _ = std::fs::read(&path);
            }
            "write" => {
                let _ = std::fs::write(&path, b"escaped");
            }
            "metadata" => {
                let _ = std::fs::metadata(&path);
            }
            "ipv4" => {
                let _ = TcpStream::connect("127.0.0.1:9");
            }
            "ipv6" => {
                let _ = TcpStream::connect("[::1]:9");
            }
            "udp" => {
                let _ = UdpSocket::bind("127.0.0.1:0");
            }
            "unix" => {
                let _ = UnixStream::connect(&path);
            }
            "socketpair" => {
                let _ = UnixStream::pair();
            }
            "spawn" => {
                let _ = Command::new("/bin/true").status();
            }
            "git-descendant" => {
                let _ = Command::new("/usr/bin/git").arg("--version").status();
            }
            "exec" => {
                let _ = Command::new("/bin/true").exec();
            }
            "detach" => {
                let _ = rustix::process::setsid();
            }
            "thread" => {
                let _ = std::thread::spawn(|| 0).join();
            }
            "cwd" => {
                let _ = std::env::current_dir();
            }
            "parent-death-reset" => {
                let _ = rustix::process::set_parent_process_death_signal(None);
            }
            "idle" => {
                let _ = std::io::stdout().write_all(b"ready\n");
                loop {
                    std::hint::spin_loop();
                }
            }
            _ => return 94,
        }
        99
    });
}
