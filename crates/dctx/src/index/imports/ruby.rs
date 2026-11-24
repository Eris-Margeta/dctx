// SPDX-License-Identifier: Apache-2.0
//! Relative Ruby features are anchored at the source; other loaders use ordered load paths.
use super::{RawRef, RefForm, Tables, dir_of};
use crate::index::refs_names::inventory_join;
impl Tables<'_> {
    pub(super) fn ruby_target(&self, importer: &str, r: &RawRef) -> Option<(u32, bool)> {
        let relative = [dir_of(importer)];
        let dirs = match r.form {
            RefForm::RelativeLoad => &relative[..],
            RefForm::Require | RefForm::Load => self.paths.ruby,
            _ => return None,
        };
        dirs.iter().find_map(|dir| {
            let base = inventory_join(dir, &r.name)?;
            let path = if r.form != RefForm::Load && !base.ends_with(".rb") {
                format!("{base}.rb")
            } else {
                base
            };
            self.module_paths.get(path.as_str()).map(|&m| (m, true))
        })
    }
}
