//! Publishes a complete query generation after file-owned store transactions.
//! Implements SPEC-MAP-MODE sections 7 and 8 without rewriting unchanged sqlite rows
//! when the comparison store already matches. The segment journal is the generation.
use super::Index;
use crate::Result;

impl Index {
    pub(super) fn persist(&mut self) -> Result<()> {
        super::segment::persist(self)
    }
}
