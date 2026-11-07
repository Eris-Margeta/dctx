// SPDX-License-Identifier: Apache-2.0
//! Same-volume, write-through name publication. Callers confine paths under the store root.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let mut wide: Vec<_> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"));
    }
    wide.push(0);
    Ok(wide)
}

pub(super) fn rename(from: &Path, to: &Path, replace: bool) -> io::Result<()> {
    let from = wide_path(from)?;
    let to = wide_path(to)?;
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    // No COPY_ALLOWED: a cross-volume copy/delete would break atomic visibility.
    // SAFETY: both pointers refer to owned, NUL-terminated UTF-16 buffers that
    // remain alive for the call. The API does not retain or mutate them.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), flags) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_rename_refuses_an_existing_name_unless_replacement_is_requested() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("source-🦀");
        let to = dir.path().join("destination-λ");
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&to, b"old").unwrap();
        // Exercise the native no-replace flag, without StdFs's earlier check.
        assert!(rename(&from, &to, false).is_err());
        assert_eq!(std::fs::read(&from).unwrap(), b"new");
        assert_eq!(std::fs::read(&to).unwrap(), b"old");
        rename(&from, &to, true).unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read(&to).unwrap(), b"new");
    }

    #[test]
    fn paths_preserve_utf16_and_reject_embedded_nul() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        let path = OsString::from_wide(&[0x61, 0xd800, 0x62]);
        assert_eq!(
            wide_path(Path::new(&path)).unwrap(),
            [0x61, 0xd800, 0x62, 0]
        );
        assert_eq!(
            wide_path(Path::new("a\0b")).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
