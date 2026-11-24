// SPDX-License-Identifier: Apache-2.0
//! PHP namespace lookups and literal filesystem includes have different origins.
use super::{RawRef, RefForm, Tables, dir_of};
use crate::index::refs_names::inventory_join;
impl Tables<'_> {
    pub(super) fn php_target(&self, importer: &str, r: &RawRef) -> Option<(u32, bool)> {
        match r.form {
            RefForm::PhpFile | RefForm::PhpDirectory => {
                let base = if r.form == RefForm::PhpDirectory {
                    dir_of(importer)
                } else {
                    self.paths.cwd?
                };
                let path = inventory_join(base, &r.name)?;
                self.module_paths.get(path.as_str()).map(|&m| (m, true))
            }
            RefForm::PhpFunction | RefForm::PhpConst | RefForm::Namespace => {
                let name = r.name.trim_start_matches('\\').replace('\\', "::");
                self.namespace_symbol("php", &name, |k| match r.form {
                    RefForm::PhpFunction => k == "fn",
                    RefForm::PhpConst => k == "const",
                    _ => matches!(k, "class" | "interface" | "enum" | "trait"),
                })
            }
            _ => None,
        }
    }
}
