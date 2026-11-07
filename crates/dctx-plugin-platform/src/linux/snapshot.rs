// SPDX-License-Identifier: Apache-2.0
//! Immutable byte snapshots for S-ARTIFACT/S-FILE using approved rustix memfd seals.
//! Hashes the actual bounded copy with dctx-hash before publishing any readable handle.
//! A sealed executable snapshot is not permission or a mechanism to launch that image.

use crate::PlatformError;
use dctx_hash::{ContentDigest, Sha256};
use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, fcntl_get_seals, memfd_create};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;

/// Absolute allocation/copy bound; callers can impose a smaller policy bound.
pub const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;

/// A digest-checked, write/grow/shrink/seal-locked anonymous file.
#[derive(Debug)]
pub struct SealedSnapshot {
    file: File,
    digest: ContentDigest,
}

impl SealedSnapshot {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "The requested copy chunk is clamped to the 16 KiB stack buffer before converting to usize."
    )]
    pub(super) fn copy_checked(
        mut source: &File,
        digest: ContentDigest,
    ) -> Result<Self, PlatformError> {
        let length = digest.byte_length();
        if length > MAX_SNAPSHOT_BYTES {
            return Err(PlatformError::ResourceLimit);
        }
        let fd = memfd_create(
            c"dctx-verified",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .map_err(|_| PlatformError::Io)?;
        let mut file = File::from(fd);
        let mut hash = Sha256::new();
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let remaining = (length + 1 - copied).min(buffer.len() as u64) as usize;
            let read = source
                .read(&mut buffer[..remaining])
                .map_err(|_| PlatformError::Io)?;
            if read == 0 {
                break;
            }
            copied += read as u64;
            if copied > length {
                return Err(PlatformError::Conflict);
            }
            hash.update(&buffer[..read])
                .map_err(|_| PlatformError::ResourceLimit)?;
            file.write_all(&buffer[..read])
                .map_err(|_| PlatformError::Io)?;
        }
        if hash.finalize() != digest {
            return Err(PlatformError::Conflict);
        }
        let seals = SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL;
        fcntl_add_seals(&file, seals).map_err(|_| PlatformError::Io)?;
        if !fcntl_get_seals(&file)
            .map_err(|_| PlatformError::Io)?
            .contains(seals)
        {
            return Err(PlatformError::Io);
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|_| PlatformError::Io)?;
        Ok(Self { file, digest })
    }

    /// Exact digest and byte count of the immutable copy.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn digest(&self) -> ContentDigest {
        self.digest
    }

    /// Reads an independently positioned bounded chunk from the sealed bytes.
    ///
    /// # Errors
    /// Returns a resource-limit error for overflow or an out-of-bounds range, or an IO error when reading the sealed file fails.
    pub fn read(&self, offset: u64, length: usize) -> Result<Vec<u8>, PlatformError> {
        let end = offset
            .checked_add(length as u64)
            .ok_or(PlatformError::ResourceLimit)?;
        if end > self.digest.byte_length() || length > 1024 * 1024 {
            return Err(PlatformError::ResourceLimit);
        }
        let mut bytes = vec![0; length];
        self.file
            .read_exact_at(&mut bytes, offset)
            .map_err(|_| PlatformError::Io)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_deny_inode_mutation_not_just_path_replacement() {
        let mut source = tempfile::tempfile().unwrap();
        source.write_all(b"approved").unwrap();
        source.rewind().unwrap();
        let digest = Sha256::digest(b"approved").unwrap();
        let snapshot = SealedSnapshot::copy_checked(&source, digest).unwrap();
        assert_eq!(snapshot.read(0, 8).unwrap(), b"approved");
        assert!(snapshot.file.write_at(b"changed!", 0).is_err());
        assert!(snapshot.file.set_len(0).is_err());
        assert!(snapshot.file.set_len(9).is_err());
        source.write_at(b"replaced", 0).unwrap();
        assert_eq!(snapshot.read(0, 8).unwrap(), b"approved");
        assert!(snapshot.read(u64::MAX, 1).is_err());
        assert!(snapshot.read(0, 9).is_err());
    }

    #[test]
    fn wrong_bytes_lengths_and_unbounded_copies_are_refused() {
        for bytes in [b"bad".as_slice(), b"good-extra", b"goo"] {
            let mut source = tempfile::tempfile().unwrap();
            source.write_all(bytes).unwrap();
            source.rewind().unwrap();
            assert!(matches!(
                SealedSnapshot::copy_checked(&source, Sha256::digest(b"good").unwrap()),
                Err(PlatformError::Conflict)
            ));
        }
        let source = tempfile::tempfile().unwrap();
        assert!(matches!(
            SealedSnapshot::copy_checked(
                &source,
                ContentDigest::sha256([0; 32], MAX_SNAPSHOT_BYTES + 1)
            ),
            Err(PlatformError::ResourceLimit)
        ));
    }
}
