// SPDX-License-Identifier: Apache-2.0
//! JavaScript module operands resolve relative to the importing source.
use super::{EdgeKind, RawRef, RefForm, Tables, dir_of, relative_import};
use crate::index::refs_names::{inventory_join, js_relative_candidates};

impl Tables<'_> {
    /// Returns the resolved path and the target, if any, proven by the import clause.
    pub(super) fn javascript_target(
        &self,
        importer: &str,
        reference: &RawRef,
    ) -> Option<(u32, Option<(u32, bool)>)> {
        if reference.form == RefForm::Require
            && (importer.ends_with(".mjs") || importer.ends_with(".mts"))
        {
            return None;
        }
        let (path, member) = relative_import(&reference.name)?;
        let path = if reference.form == RefForm::Require {
            path
        } else {
            path.split(['?', '#']).next()?
        };
        let module = self.relative_js_module(importer, path)?;
        let target = match member {
            Some("*") | None => Some((module, true)),
            Some("default")
                if self.js_interop(importer) && self.js_assignment(module).is_some() =>
            {
                self.js_assignment(module)
            }
            Some(symbol) => self.js_export(
                module,
                symbol,
                reference.form == RefForm::TypeOnly,
                &mut Vec::new(),
                &mut 4096,
            ),
        };
        Some((module, target))
    }

    pub(super) fn js_assignment(&self, module: u32) -> Option<(u32, bool)> {
        let fact = self
            .facts(module)
            .iter()
            .find(|r| r.form == RefForm::ExportAssignment)?;
        self.import_symbol(module, &fact.name)
    }

    fn js_interop(&self, importer: &str) -> bool {
        let mut path = importer;
        loop {
            let (dir, config) = path
                .rsplit_once('/')
                .map_or(("", String::from("tsconfig.json")), |(dir, _)| {
                    (dir, format!("{dir}/tsconfig.json"))
                });
            if let Some(&module) = self.module_paths.get(config.as_str()) {
                return self
                    .facts(module)
                    .iter()
                    .any(|r| r.form == RefForm::Interop);
            }
            if dir.is_empty() {
                return false;
            }
            path = dir;
        }
    }

    pub(super) fn js_export(
        &self,
        module: u32,
        symbol: &str,
        type_only: bool,
        visiting: &mut Vec<u32>,
        remaining: &mut usize,
    ) -> Option<(u32, bool)> {
        *remaining = remaining.checked_sub(1)?;
        if visiting.contains(&module) || visiting.len() >= 64 {
            return None;
        }
        visiting.push(module);
        let facts = self.facts(module);
        let mut result = None;
        let local_symbol = |local: &str, public_only: bool| {
            let candidates = self.names.get(&(module, local))?;
            let eligible = |i: u32| {
                let kind = self.nodes[i as usize].kind.as_str();
                !public_only
                    || facts
                        .iter()
                        .any(|r| r.form == RefForm::Public && r.name == local && r.scope == kind)
            };
            let in_type_space = |i: u32| {
                matches!(
                    self.nodes[i as usize].kind.as_str(),
                    "type" | "interface" | "class" | "enum"
                )
            };
            let typed = type_only && candidates.iter().any(|&i| eligible(i) && in_type_space(i));
            let mut hits = candidates.iter().copied().filter(|&i| {
                eligible(i)
                    && if typed {
                        in_type_space(i)
                    } else {
                        !matches!(self.nodes[i as usize].kind.as_str(), "type" | "interface")
                    }
            });
            let hit = hits.next()?;
            hits.next().is_none().then_some((hit, false))
        };
        let named_default = facts.iter().any(|r| {
            r.kind == EdgeKind::Reexports
                && r.src.starts_with("s:")
                && r.name == symbol
                && r.alias.as_deref() == Some("default")
        });
        if !named_default && let Some(hit) = local_symbol(symbol, true) {
            visiting.pop();
            return Some(hit);
        }
        for r in facts.iter().filter(|r| r.kind == EdgeKind::Reexports) {
            let found = if let Some((path, member)) = relative_import(&r.name) {
                let Some(target) =
                    self.relative_js_module(self.text.get(self.nodes[module as usize].path), path)
                else {
                    continue;
                };
                match member {
                    Some("*") if symbol != "default" => {
                        self.js_export(target, symbol, type_only, visiting, remaining)
                    }
                    Some(name) if r.alias.as_deref().unwrap_or(name) == symbol => {
                        self.js_export(target, name, type_only, visiting, remaining)
                    }
                    None if r.alias.as_deref() == Some(symbol) => Some((target, true)),
                    _ => None,
                }
            } else if r.alias.as_deref().unwrap_or(&r.name) == symbol {
                local_symbol(&r.name, false)
            } else {
                None
            };
            if found.is_some() {
                if result.is_some() && result != found {
                    result = None;
                    break;
                }
                result = found;
            }
        }
        visiting.pop();
        result
    }

    /// Joins a specifier to the importer inside the inventory, without touching the filesystem.
    fn relative_js_module(&self, importer: &str, path: &str) -> Option<u32> {
        let base = inventory_join(dir_of(importer), path)?;
        js_relative_candidates(&base, path)
            .find_map(|candidate| self.module_paths.get(candidate.as_ref()).copied())
    }
}
