//! The grep verb's file access: each indexed file is re-read and re-hashed before it is searched.
//! Implements `dctx map grep` over the pinned generation. Matching and grouping live in the index.
//! A file the policy no longer admits is skipped; a file whose bytes changed refuses the query.

use super::Session;
use crate::domain::{AnswerRecord, DctxError, MapRequest};
use crate::index::{grep_records, Generation};

impl Session {
    /// Hit groups for `request`, one per enclosing symbol, from verified bytes only.
    pub(super) fn grep_hits(&mut self, generation: &Generation, request: &MapRequest) -> Result<Vec<AnswerRecord>, DctxError> {
        grep_records(generation, request, |path, digest| {
            if self.authorize(path).is_err() {
                return Ok(None);
            }
            self.read_verified(path, digest).map(Some)
        })
    }
}
