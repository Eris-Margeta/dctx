// SPDX-License-Identifier: Apache-2.0
//! Platform clipboard output for explicit pack requests.

use std::{
    env, fs,
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
};

/// Resolve clipboard tools without launching a command-discovery child.
/// # Errors
/// Returns unsupported-platform/tool-not-found errors, spawn/stdin/wait failures, or a nonzero clipboard-tool exit.
pub fn copy_to_clipboard(bytes: &[u8]) -> Result<(), String> {
    copy_with(|writer| writer.write_all(bytes).map_err(|error| error.to_string()))
}

pub(crate) fn copy_with<T>(
    write: impl FnOnce(&mut dyn Write) -> Result<T, String>,
) -> Result<T, String> {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &["pbcopy"]
    } else if cfg!(windows) {
        &["clip.exe"]
    } else if cfg!(target_os = "linux") {
        &["wl-copy", "xclip"]
    } else {
        return Err("Clipboard not supported on this OS.".into());
    };
    let path = env::var_os("PATH").unwrap_or_default();
    let executable = candidates
        .iter()
        .find_map(|name| {
            env::split_paths(&path)
                .map(|dir| dir.join(name))
                .find(|path| executable(path))
        })
        .ok_or("Clipboard utility not found. Please install a clipboard tool.")?;
    let mut command = Command::new(&executable);
    if executable.file_name().is_some_and(|name| name == "xclip") {
        command.args(["-selection", "clipboard"]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to spawn clipboard command: {error}"))?;
    let write = match child.stdin.take() {
        Some(mut stdin) => write(&mut stdin),
        None => Err(io::Error::other("clipboard stdin missing").to_string()),
    };
    let status = child.wait().map_err(|error| error.to_string())?;
    let value = write?;
    if status.success() {
        Ok(value)
    } else {
        Err("Clipboard command returned non-zero exit code.".into())
    }
}

fn executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
