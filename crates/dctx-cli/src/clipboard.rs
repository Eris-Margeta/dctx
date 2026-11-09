// SPDX-License-Identifier: Apache-2.0
//! Platform clipboard output for explicit pack requests.

use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Clipboard tools tried in order on this platform. Empty when the platform has none.
pub(crate) fn tool_names() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["pbcopy"]
    } else if cfg!(windows) {
        &["clip.exe"]
    } else if cfg!(target_os = "linux") {
        &["wl-copy", "xclip", "xsel"]
    } else {
        &[]
    }
}

/// Resolve clipboard tools without launching a command-discovery child.
/// # Errors
/// Returns unsupported-platform/tool-not-found errors, spawn/stdin/wait failures, or a nonzero clipboard-tool exit.
pub fn copy_to_clipboard(bytes: &[u8]) -> Result<(), String> {
    copy_with(|writer| writer.write_all(bytes).map_err(|error| error.to_string()))
}

pub(crate) fn copy_with<T>(
    write: impl FnOnce(&mut dyn Write) -> Result<T, String>,
) -> Result<T, String> {
    let candidates = tool_names();
    if candidates.is_empty() {
        return Err("the clipboard is not supported on this operating system".into());
    }
    let executable = locate(candidates, env::var_os("PATH")).ok_or_else(|| {
        format!(
            "no clipboard tool found on PATH (tried {})",
            candidates.join(", ")
        )
    })?;
    let name = executable
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let mut command = Command::new(&executable);
    match name.as_str() {
        "xclip" => {
            command.args(["-selection", "clipboard"]);
        }
        "xsel" => {
            command.args(["--clipboard", "--input"]);
        }
        _ => {}
    }
    let mut child = command
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start clipboard tool {name}: {error}"))?;
    let write = match child.stdin.take() {
        Some(mut stdin) => write(&mut stdin),
        None => Err(io::Error::other("clipboard stdin missing").to_string()),
    };
    let status = child.wait().map_err(|error| error.to_string())?;
    let value = write?;
    if status.success() {
        Ok(value)
    } else {
        Err(format!("clipboard tool {name} failed ({status})"))
    }
}

/// First candidate found as an executable file in an absolute PATH entry. Empty and relative
/// entries (such as `.`) are skipped, so a repository's own `./xclip` is never run.
fn locate(candidates: &[&str], path: Option<OsString>) -> Option<PathBuf> {
    let path = path.unwrap_or_default();
    candidates.iter().find_map(|name| {
        env::split_paths(&path)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(name))
            .find(|path| executable(path))
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn relative_and_empty_path_entries_are_never_searched() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        tool(&bin, "tool");
        let relative = PathBuf::from("relative-bin");
        let joined = env::join_paths([PathBuf::new(), PathBuf::from("."), relative]).unwrap();
        assert_eq!(locate(&["tool"], Some(joined)), None);
        let joined = env::join_paths([PathBuf::from("."), bin.clone()]).unwrap();
        assert_eq!(locate(&["tool"], Some(joined)), Some(bin.join("tool")));
        assert_eq!(locate(&["tool"], None), None);
    }

    #[test]
    fn candidates_are_tried_in_order() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        tool(&bin, "second");
        let joined = env::join_paths([bin.clone()]).unwrap();
        assert_eq!(
            locate(&["first", "second"], Some(joined)),
            Some(bin.join("second"))
        );
    }
}
