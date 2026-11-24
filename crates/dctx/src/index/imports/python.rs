// SPDX-License-Identifier: Apache-2.0
//! Python imports are rooted at the inventory or relative to the containing package.
use super::{Binding, Caller, FileRefs, RawRef, RefForm, Tables, dir_of};

impl Tables<'_> {
    fn python_module(&self, base: &str) -> Option<u32> {
        [
            format!("{base}/__init__.py"),
            format!("{base}.py"),
            format!("{base}/__init__.pyi"),
            format!("{base}.pyi"),
        ]
        .iter()
        .find_map(|p| self.module_paths.get(p.as_str()).copied())
    }

    pub(super) fn python_target(&self, importer: &str, name: &str) -> Option<(u32, bool)> {
        let (operand, member) = name
            .split_once("::")
            .map_or((name, None), |(p, m)| (p, Some(m)));
        let path = operand.trim_start_matches('.');
        let level = operand.len() - path.len();
        let mut parts = Vec::new();
        if level > 0 {
            let importer = if self.paths.python.is_empty() {
                importer
            } else {
                importer
                    .strip_prefix(self.paths.python)?
                    .strip_prefix('/')?
            };
            parts.extend(dir_of(importer).split('/').filter(|s| !s.is_empty()));
            if level > parts.len() {
                return None;
            }
            parts.truncate(parts.len() + 1 - level);
        }
        parts.extend(path.split('.').filter(|s| !s.is_empty()));
        let base = std::iter::once(self.paths.python)
            .chain(parts)
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        let module = self.python_module(&base);
        match member {
            None | Some("*") => Some((module?, true)),
            Some(member) => {
                if let Some(dst) = module.and_then(|m| self.import_symbol(m, member)) {
                    return Some(dst);
                }
                if module.is_some_and(|m| {
                    !self
                        .text
                        .get(self.nodes[m as usize].path)
                        .contains("/__init__.")
                }) {
                    return None;
                }
                self.python_module(&format!("{base}/{member}"))
                    .map(|m| (m, true))
            }
        }
    }

    pub(super) fn python_bindings<'a>(
        &self,
        caller: &mut Caller<'a>,
        r: &'a RawRef,
        dst: u32,
        module: bool,
    ) {
        let mut bind = |name: &str, dst, module| {
            caller.bindings.push(Binding {
                scope: &r.scope,
                name: name.to_owned(),
                dst,
                module,
            })
        };
        match (r.name.split_once("::"), r.alias.as_deref()) {
            (_, Some(alias)) => bind(alias, dst, module),
            (Some((_, "*")), None) => {
                for export in self
                    .facts(dst)
                    .iter()
                    .filter(|r| r.form == RefForm::PythonAll)
                {
                    if let Some((symbol, _)) = self.import_symbol(dst, &export.name) {
                        bind(&export.name, symbol, false);
                    }
                }
            }
            (Some((_, name)), None) => bind(name, dst, module),
            (None, None) => bind(&r.name, dst, module),
        }
    }

    pub(super) fn python_shadowed(&self, file: &FileRefs, scope: &str, local: &str) -> bool {
        file.refs.iter().any(|r| {
            r.form == RefForm::Parameter
                && r.name == local
                && (r.scope == scope
                    || scope
                        .strip_prefix(&r.scope)
                        .is_some_and(|s| s.starts_with("::")))
        })
    }
}
