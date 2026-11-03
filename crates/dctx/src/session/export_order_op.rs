// SPDX-License-Identifier: Apache-2.0
//! Outline order by a file's explicit export list (DCX-55c): `map m:<file>` lists the names in Python `__all__`, CommonJS
//! `module.exports` / `exports.x` and ES `export { .. }` first. The list is read at index time and kept on the module node
//! (`index::outline_order::export_signature`), so the outline never rereads the file.

use std::collections::BTreeSet;

use super::Session;
use crate::domain::{AnswerRecord, MapRequest};
use crate::index::Generation;

impl Session {
    /// For a `map` whose target is one file root (`m:<path>`), moves the records whose name after `#` is exactly one of the
    /// file's explicitly exported names to the front, keeping the existing order inside both groups (a stable partition).
    /// A file without an export list keeps the order unchanged.
    pub(super) fn order_by_exports(&self, generation: &Generation, request: &MapRequest, records: &mut [AnswerRecord]) {
        let Some(path) = request.target.strip_prefix("m:") else {
            return;
        };
        let module = generation.nodes.iter().find(|node| node.letter == b'm' && generation.text.get(node.path) == path);
        if let Some(module) = module.filter(|module| !module.signature.is_empty()) {
            exports_first(records, &module.signature.split(' ').collect());
        }
    }
}

/// The name part of a stable id: the text after its last `#`, or the whole id when it has none (a module id).
fn after_hash(id: &str) -> &str {
    id.rsplit_once('#').map_or(id, |(_, name)| name)
}

/// Stable partition: records whose [`after_hash`] name is in `names` first, the rest after, each group in its old order.
fn exports_first(records: &mut [AnswerRecord], names: &BTreeSet<&str>) {
    records.sort_by_key(|record| !names.contains(after_hash(&record.id)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_names_move_first_and_both_groups_keep_their_order() {
        let ids = ["m:a.py", "s:a.py#alpha", "s:a.py#beta", "s:a.py#Gamma", "s:a.py#Gamma.beta"];
        let mut records: Vec<AnswerRecord> = ids.iter().map(|id| AnswerRecord::basic(*id, "fn", "a.py")).collect();
        let names: BTreeSet<&str> = ["Gamma", "beta"].into_iter().collect();
        exports_first(&mut records, &names);
        let order: Vec<&str> = records.iter().map(|record| record.id.as_str()).collect();
        assert_eq!(order, ["s:a.py#beta", "s:a.py#Gamma", "m:a.py", "s:a.py#alpha", "s:a.py#Gamma.beta"]);
    }

    #[test]
    fn module_ids_keep_their_whole_text() {
        assert_eq!(after_hash("m:a.py"), "m:a.py");
        assert_eq!(after_hash("s:a.py#Gamma.beta"), "Gamma.beta");
    }
}
