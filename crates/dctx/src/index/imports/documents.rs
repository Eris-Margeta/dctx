// SPDX-License-Identifier: Apache-2.0
//! Local document links preserve the heading owner and the documents confidence.
use super::{RefForm, Tables, dir_of, write_node_id};
use crate::index::refs_names::inventory_join;
impl Tables<'_> {
    pub(super) fn document_target(&self, importer: &str, destination: &str) -> Option<u32> {
        let (path, anchor) = destination
            .split_once('#')
            .map_or((destination, None), |(p, a)| (p, Some(a)));
        if path.contains(':') || path.starts_with('/') {
            return None;
        }
        let target = if path.is_empty() {
            importer.into()
        } else {
            inventory_join(dir_of(importer), path)?
        };
        let module = *self.module_paths.get(target.as_str())?;
        let Some(anchor) = anchor.filter(|s| !s.is_empty()) else {
            return Some(module);
        };
        let id = &self
            .facts(module)
            .iter()
            .find(|r| r.form == RefForm::Anchor && r.name == anchor)?
            .scope;
        let file = &self.files[self.files.binary_search_by_key(&module, |f| f.first).ok()?];
        (file.first + 1..file.end).find(|&i| {
            let mut spelled = String::new();
            write_node_id(&mut spelled, self.text, &self.nodes[i as usize]);
            spelled == *id
        })
    }
}
