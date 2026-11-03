// SPDX-License-Identifier: Apache-2.0
//! Output emission for `dctx pack`: stdout, file, and clipboard destinations.

use std::path::Path;

use crate::compat;

use super::{Destination, PackOutcome};

pub(super) fn emit(
    destination: &Destination,
    body: &[u8],
    files: u64,
    version: &str,
) -> Result<PackOutcome, String> {
    let bytes = u64::try_from(body.len()).unwrap_or(u64::MAX);
    match destination {
        Destination::Stdout => Ok(PackOutcome {
            body: body.to_vec(),
            wrote: None,
            clipboard: false,
            report_line: format!(
                "pack bytes={bytes} files={files} retention=stdout version={version}"
            ),
            files,
        }),
        Destination::File(path) => {
            write_output(path, body)?;
            Ok(PackOutcome {
                body: body.to_vec(),
                wrote: Some(path.clone()),
                clipboard: false,
                report_line: format!(
                    "pack bytes={bytes} files={files} retention=explicit path={} version={version}",
                    path.display()
                ),
                files,
            })
        }
        Destination::Clipboard => {
            copy(body)?;
            Ok(PackOutcome {
                body: body.to_vec(),
                wrote: None,
                clipboard: true,
                report_line: format!(
                    "pack bytes={bytes} files={files} retention=clipboard version={version}"
                ),
                files,
            })
        }
        Destination::FileAndClipboard(path) => {
            write_output(path, body)?;
            if let Err(error) = copy(body) {
                return Err(format!(
                    "{error}; explicit file already written at {}",
                    path.display()
                ));
            }
            Ok(PackOutcome {
                body: body.to_vec(),
                wrote: Some(path.clone()),
                clipboard: true,
                report_line: format!(
                    "pack bytes={bytes} files={files} retention=explicit path={} version={version}",
                    path.display()
                ),
                files,
            })
        }
    }
}

fn write_output(path: &Path, body: &[u8]) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("--output path is empty".to_string());
    }
    std::fs::write(path, body)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn copy(body: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(body);
    compat::copy_to_clipboard(text.as_bytes())
}
