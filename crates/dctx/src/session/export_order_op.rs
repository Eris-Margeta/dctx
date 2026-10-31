//! Outline order by a file's explicit export list (DCX-55c): `map m:<file>` lists the names in Python `__all__`, CommonJS
//! `module.exports` / `exports.x` and ES `export { .. }` first. Reads the file once through the same verified read as `sites`.
//! Depends on [`crate::index::export_list`] for the list.

use std::collections::BTreeSet;

use super::Session;
use crate::domain::{AnswerRecord, MapRequest};
use crate::index::{export_list, Generation};

impl Session {
    /// For a `map` whose target is one file root (`m:<path>`), moves the records whose name after `#` is exactly one of the
    /// file's explicitly exported names to the front, keeping the existing order inside both groups (a stable partition).
    /// A file without an export list, or one that cannot be read and verified, keeps the order unchanged.
    pub(super) fn order_by_exports(&mut self, generation: &Generation, request: &MapRequest, records: &mut [AnswerRecord]) {
        let Some(path) = request.target.strip_prefix("m:") else {
            return;
        };
        let Some(language) = export_language(path) else {
            return;
        };
        let Some(expected) = generation.files.iter().find(|file| file.path == path).and_then(|file| file.digest) else {
            return;
        };
        if self.authorize(path).is_err() {
            return;
        }
        let Ok(bytes) = self.read_verified(path, expected) else {
            return;
        };
        let text = String::from_utf8_lossy(&bytes);
        if let Some(names) = export_list::names(language, &text) {
            exports_first(records, &names);
        }
    }
}

/// The export-list language of `path` by its extension (`python`, `javascript`, `typescript`, the tags of
/// `index::extract::language_of`, which the session cannot call), or `None` for a file that has no export list.
fn export_language(path: &str) -> Option<&'static str> {
    let base = path.rsplit('/').next().unwrap_or(path);
    let (_, extension) = base.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "py" | "pyi" => Some("python"),
        "js" | "mjs" | "cjs" | "jsx" => Some("javascript"),
        "ts" | "tsx" | "mts" | "cts" => Some("typescript"),
        _ => None,
    }
}

/// The name part of a stable id: the text after its last `#`, or the whole id when it has none (a module id).
fn after_hash(id: &str) -> &str {
    id.rsplit_once('#').map_or(id, |(_, name)| name)
}

/// Stable partition: records whose [`after_hash`] name is in `names` first, the rest after, each group in its old order.
fn exports_first(records: &mut [AnswerRecord], names: &BTreeSet<String>) {
    records.sort_by_key(|record| !names.contains(after_hash(&record.id)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_names_move_first_and_both_groups_keep_their_order() {
        let ids = ["m:a.py", "s:a.py#alpha", "s:a.py#beta", "s:a.py#Gamma", "s:a.py#Gamma.beta"];
        let mut records: Vec<AnswerRecord> = ids.iter().map(|id| AnswerRecord::basic(*id, "fn", "a.py")).collect();
        let names: BTreeSet<String> = ["Gamma", "beta"].iter().map(|name| (*name).to_string()).collect();
        exports_first(&mut records, &names);
        let order: Vec<&str> = records.iter().map(|record| record.id.as_str()).collect();
        assert_eq!(order, ["s:a.py#beta", "s:a.py#Gamma", "m:a.py", "s:a.py#alpha", "s:a.py#Gamma.beta"]);
    }

    #[test]
    fn export_languages_follow_the_extension() {
        assert_eq!(export_language("pkg/mod.py"), Some("python"));
        assert_eq!(export_language("stubs/mod.PYI"), Some("python"));
        assert_eq!(export_language("lib.cjs"), Some("javascript"));
        assert_eq!(export_language("src/api.tsx"), Some("typescript"));
        assert_eq!(export_language("src/main.rs"), None);
        assert_eq!(export_language("Makefile"), None);
        assert_eq!(after_hash("m:a.py"), "m:a.py");
        assert_eq!(after_hash("s:a.py#Gamma.beta"), "Gamma.beta");
    }
}
