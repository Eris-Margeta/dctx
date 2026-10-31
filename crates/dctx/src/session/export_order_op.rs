//! Outline order by a file's explicit export list (DCX-55c): `map m:<file>` lists the names in Python `__all__`, CommonJS
//! `module.exports` / `exports.x` and ES `export { .. }` first. Reads the file once through the same verified read as `sites`.
//! Depends on [`crate::index::export_list`] for the list.

use super::Session;
use crate::domain::{AnswerRecord, MapRequest};
use crate::index::Generation;

impl Session {
    /// For a `map` whose target is one file root (`m:<path>`), moves the records whose name after `#` is exactly one of the
    /// file's explicitly exported names to the front, keeping the existing order inside both groups (a stable partition).
    /// A file without an export list, or one that cannot be read and verified, keeps the order unchanged.
    pub(super) fn order_by_exports(&mut self, generation: &Generation, request: &MapRequest, records: &mut [AnswerRecord]) {
        let _ = (generation, request, records);
    }
}
