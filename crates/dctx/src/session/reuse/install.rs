// SPDX-License-Identifier: Apache-2.0
//! Refreshes verified metadata without parsing, then admits and pins the reused graph.
use std::sync::Arc;

use super::encode_generation;
use super::verify::VerifiedFiles;
use crate::domain::{DctxError, ScanLimits};
use crate::index::Generation;
use crate::index::profile::{Elapsed, Stage};
use crate::session::{Session, SnapshotHandle};

impl Session {
    pub(super) fn install_reused(
        &mut self,
        source_key: &str,
        limits: &ScanLimits,
        retained_len: usize,
        mut generation: Arc<Generation>,
        verified: VerifiedFiles,
    ) -> Result<Option<SnapshotHandle>, DctxError> {
        let _pin = Elapsed::begin(Stage::ReusePin);
        let VerifiedFiles {
            refreshed_files,
            modified_changed,
        } = verified;
        let metadata_changed = !refreshed_files.is_empty();
        if metadata_changed {
            let generation = Arc::make_mut(&mut generation);
            for (index, modified, stamp) in refreshed_files {
                generation.files[index].modified = modified;
                generation.files[index].stamp = stamp;
            }
        }
        if modified_changed {
            // Modification time is canonical metadata. Keep the already parsed graph, but
            // refresh its identity exactly as a strict build with these live file rows would.
            Arc::make_mut(&mut generation).digest = crate::index::canonical_bytes(
                &generation.files,
                &generation.nodes,
                &generation.edges,
                &generation.text,
            )?
            .1;
        }
        // A handle already issued by this session keeps its snapshot identity. A live
        // metadata refresh can share parsed facts, but must not replace that handle's rows.
        if self
            .generations
            .get(&generation.id)
            .is_some_and(|previous| previous.digest != generation.digest)
        {
            Arc::make_mut(&mut generation).id = self.next_generation;
        }
        let refreshed = if metadata_changed {
            Some(encode_generation(
                source_key,
                &self.view,
                limits,
                &generation,
            )?)
        } else {
            None
        };
        let encoded_len = if cfg!(feature = "segments") {
            u64::try_from(refreshed.as_ref().map_or(retained_len, Vec::len)).unwrap_or(u64::MAX)
        } else {
            0
        };
        if generation.encoded_len != encoded_len {
            Arc::make_mut(&mut generation).encoded_len = encoded_len;
        }
        if cfg!(feature = "segments") {
            self.cache.segment_admit(encoded_len)?;
        }
        let now = self.clock.now().millis();
        let ticket = match match refreshed {
            Some(bytes) => self.store_refreshed_segment(&bytes),
            None => self.pin_retained(now),
        } {
            Ok(Some(ticket)) => Some(ticket),
            Ok(None) => {
                if cfg!(feature = "segments") {
                    self.cache.segment_release(encoded_len);
                }
                return Ok(None);
            }
            Err(error) => {
                if cfg!(feature = "segments") {
                    self.cache.segment_release(encoded_len);
                }
                return Err(error);
            }
        };
        Ok(Some(self.pin_generation(generation, ticket, true)))
    }
}
