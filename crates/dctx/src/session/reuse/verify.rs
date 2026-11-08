// SPDX-License-Identifier: Apache-2.0
//! Live inventory equality and content verification, before any cached metadata is changed.
use dctx_core::facts::EntryStamp;

use super::keys::file_inventory;
use super::stamp_pass_enabled;
use crate::domain::{DctxError, ScanLimits};
use crate::index::profile::{Elapsed, Stage};
use crate::index::{Generation, Walked};
use crate::session::Session;

pub(super) struct VerifiedFiles {
    pub refreshed_files: Vec<(usize, u64, Option<EntryStamp>)>,
    pub modified_changed: bool,
}

impl Session {
    pub(super) fn verify_files(
        &mut self,
        generation: &Generation,
        walked: &Walked,
        limits: &ScanLimits,
    ) -> Result<Option<VerifiedFiles>, DctxError> {
        let check = Elapsed::begin(Stage::ReuseCheck);
        let Some(stored_files) = file_inventory(&generation.files) else {
            return Ok(None);
        };
        let Some(live_files) = file_inventory(&walked.files) else {
            return Ok(None);
        };
        if generation.text.partial_dirs != walked.text.partial_dirs
            || stored_files.len() != live_files.len()
            || stored_files.iter().any(|(path, stored)| {
                live_files.get(path).is_none_or(|live| {
                    live.partial || stored.decision != live.decision || stored.size != live.size
                })
            })
        {
            return Ok(None);
        }
        drop(stored_files);
        drop(check);
        let trust_stamps = stamp_pass_enabled()?;
        let mut refreshed_files = Vec::new();
        let mut modified_changed = false;
        let mut confirmed = 0u64;
        let mut skipped = 0u64;
        let mut verified = 0u64;
        for (index, file) in generation.files.iter().enumerate() {
            // Inventory equality established this lookup, including duplicate-path rejection.
            let live = live_files[file.path.as_str()];
            if file.modified != live.modified || file.stamp != live.stamp {
                refreshed_files.push((index, live.modified, live.stamp));
            }
            modified_changed |= file.modified != live.modified;
            let stored_stamp = file.stamp;
            let Some(digest) = file.digest else {
                // An earlier failed stat/read cannot establish content equality. Oversized
                // files remain metadata-only under the unchanged file-byte limit.
                if file.partial && file.size <= limits.file_bytes {
                    return Ok(None);
                }
                continue;
            };
            // Equal stat stamp of a settled file: the bytes are not read. Any difference, a missing live
            // stamp (racy window, new inode source) or `strict_live` falls through to the digest.
            if trust_stamps
                && let (Some(stored), Some(live)) = (stored_stamp, live.stamp)
                && stored == live
            {
                skipped += 1;
                continue;
            }
            verified += 1;
            match self.digest_matches_source(&file.path, digest, limits.file_bytes) {
                Ok(true) => confirmed += 1,
                Err(
                    error @ (DctxError::Cancelled
                    | DctxError::ResourceLimit { .. }
                    | DctxError::PolicyDenied),
                ) => {
                    return Err(error);
                }
                Ok(false) | Err(_) => {
                    self.content_reads += verified;
                    return Ok(None);
                }
            }
        }
        self.stamp_skipped_reads += skipped;
        self.cached_content_reads += confirmed;
        Ok(Some(VerifiedFiles {
            refreshed_files,
            modified_changed,
        }))
    }
}
