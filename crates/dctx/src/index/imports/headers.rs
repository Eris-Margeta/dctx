// SPDX-License-Identifier: Apache-2.0
//! Compiler search order applies only to literal header directives.
use super::{RawRef, RefForm, Tables, dir_of};
use crate::index::refs_names::inventory_join;
impl Tables<'_> {
    pub(super) fn header_target(&self, importer: &str, r: &RawRef) -> Option<(u32, bool)> {
        let local = match r.form {
            RefForm::QuotedHeader => Some(dir_of(importer)),
            RefForm::AngleHeader => None,
            _ => return None,
        };
        local
            .into_iter()
            .chain(self.paths.includes.iter().copied())
            .find_map(|dir| {
                let p = inventory_join(dir, &r.name)?;
                self.module_paths.get(p.as_str()).map(|&m| (m, true))
            })
    }
}
