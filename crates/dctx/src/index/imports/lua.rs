// SPDX-License-Identifier: Apache-2.0
//! A literal Lua require only resolves under a known template and working directory.
use super::{RawRef, RefForm, Tables};
use crate::index::refs_names::inventory_join;
impl Tables<'_> {
    pub(super) fn lua_target(&self, r: &RawRef) -> Option<(u32, bool)> {
        let RefForm::Lua(templates) = &r.form else {
            return None;
        };
        let base = self.paths.cwd?;
        let module = r.name.replace('.', "/");
        templates.iter().find_map(|tpl| {
            let path = inventory_join(base, &tpl.replace('?', &module))?;
            self.module_paths.get(path.as_str()).map(|&m| (m, true))
        })
    }
}
