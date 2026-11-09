// SPDX-License-Identifier: Apache-2.0
//! Output emission for `dctx pack`: stdout, file, and clipboard destinations.
//! The page always ends in a newline. The status line is returned for stderr, never mixed into the page.

use std::path::Path;

use crate::clipboard;
use crate::exit::Failure;

use super::{clipboard_failure, output_file, pack_io_failure, Destination, PackOutcome};

pub(super) fn emit(
    destination: &Destination,
    mut body: Vec<u8>,
    files: u64,
    version: &str,
) -> Result<PackOutcome, Failure> {
    if body.last() != Some(&b'\n') {
        body.push(b'\n');
    }
    let bytes = u64::try_from(body.len()).unwrap_or(u64::MAX);
    let (wrote, clipboard) = match destination {
        Destination::Stdout => (None, false),
        Destination::File(path) => {
            write_output(path, &body)?;
            (Some(path.clone()), false)
        }
        Destination::Clipboard => {
            copy(&body).map_err(clipboard_failure)?;
            (None, true)
        }
        Destination::FileAndClipboard(path) => {
            write_output(path, &body)?;
            copy(&body).map_err(|error| {
                clipboard_failure(format!(
                    "{error}; explicit file already written at {}",
                    path.display()
                ))
            })?;
            (Some(path.clone()), true)
        }
    };
    let report_line = status_line(bytes, files, destination, version);
    Ok(PackOutcome {
        body,
        wrote,
        clipboard,
        report_line,
        files,
    })
}

/// The `pack bytes=…` line that `dctx pack` prints on stderr after the page.
pub(super) fn status_line(
    bytes: u64,
    files: u64,
    destination: &Destination,
    version: &str,
) -> String {
    let retention = match destination {
        Destination::File(path) | Destination::FileAndClipboard(path) => {
            format!("explicit path={}", path.display())
        }
        Destination::Clipboard => "clipboard".to_string(),
        Destination::Stdout => "stdout".to_string(),
    };
    format!("pack bytes={bytes} files={files} retention={retention} version={version}")
}

fn write_output(path: &Path, body: &[u8]) -> Result<(), Failure> {
    if path.as_os_str().is_empty() {
        return Err(pack_io_failure("--output path is empty".to_string()));
    }
    output_file::write_with(path, |writer| writer.write_all(body))
        .map_err(|error| pack_io_failure(format!("failed to write {}: {error}", path.display())))
}

fn copy(body: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(body);
    clipboard::copy_to_clipboard(text.as_bytes())
}
