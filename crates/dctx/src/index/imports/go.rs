// SPDX-License-Identifier: Apache-2.0
//! Go package ownership comes from go.mod and the package clause of eligible files.
use super::{
    Binding, Caller, Confidence, EdgeKind, HashMap, RawRef, RefForm, Tables, dir_of, note,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Packages<'a> {
    dirs: HashMap<&'a str, Vec<u32>>,
}

pub(in crate::index) fn module_name(source: &str) -> Option<&str> {
    let mut lines = source.lines().filter_map(|line| {
        let mut words = line.split("//").next()?.split_whitespace();
        if words.next()? != "module" {
            return None;
        }
        let name = words.next()?;
        (words.next().is_none() && !name.contains(['"', '`'])).then_some(name)
    });
    let only = lines.next()?;
    lines.next().is_none().then_some(only)
}

impl<'a> Packages<'a> {
    pub(super) fn build(t: &Tables<'a>) -> Self {
        let mut packages = Self::default();
        for f in t.files {
            let path = t.text.get(t.nodes[f.first as usize].path);
            if path.ends_with(".go")
                && !path.ends_with("_test.go")
                && f.refs.iter().any(|r| r.form == RefForm::Package)
            {
                packages.dirs.entry(dir_of(path)).or_default().push(f.first);
            }
        }
        packages
    }
}

impl Tables<'_> {
    fn go_manifest(&self, path: &str) -> Option<(u32, &str)> {
        let mut dir = dir_of(path);
        loop {
            let manifest = if dir.is_empty() {
                "go.mod".into()
            } else {
                format!("{dir}/go.mod")
            };
            if let Some(&id) = self.module_paths.get(manifest.as_str()) {
                return Some((
                    id,
                    self.facts(id)
                        .iter()
                        .find(|r| r.form == RefForm::GoModule)?
                        .name
                        .as_str(),
                ));
            }
            if dir.is_empty() {
                return None;
            }
            dir = dir_of(dir);
        }
    }

    pub(super) fn go_import<'a>(
        &self,
        caller: &mut Caller<'a>,
        importer: &str,
        r: &'a RawRef,
        src: u32,
        found: &mut BTreeMap<(u32, EdgeKind, u32), Confidence>,
    ) {
        let Some((manifest, name)) = self.go_manifest(importer) else {
            return;
        };
        let Some(tail) = r.name.strip_prefix(name) else {
            return;
        };
        let tail = if tail.is_empty() {
            tail
        } else {
            let Some(p) = tail.strip_prefix('/') else {
                return;
            };
            p
        };
        if tail.split('/').any(|part| matches!(part, "." | "..")) {
            return;
        }
        let root = dir_of(self.text.get(self.nodes[manifest as usize].path));
        let dir = [root, tail]
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        let Some(files) = self.go.dirs.get(dir.as_str()) else {
            return;
        };
        let mut packages = files.iter().filter_map(|&m| {
            self.facts(m)
                .iter()
                .find(|r| r.form == RefForm::Package)
                .map(|r| r.name.as_str())
        });
        let Some(package) = packages.next() else {
            return;
        };
        if package == "main" || packages.any(|name| name != package) {
            return;
        }
        let local = r.alias.as_deref().unwrap_or(package);
        for &m in files {
            if self
                .go_manifest(self.text.get(self.nodes[m as usize].path))
                .map(|(id, _)| id)
                != Some(manifest)
            {
                return;
            }
        }
        for &m in files {
            note(found, src, EdgeKind::Imports, m, Confidence::Exact);
        }
        if local != "_" && local != "." {
            caller.bindings.push(Binding {
                scope: &r.scope,
                name: local.into(),
                dst: files[0],
                module: true,
            });
        }
    }

    pub(super) fn go_member(&self, module: u32, name: &str) -> Option<u32> {
        if !name.starts_with(char::is_uppercase) {
            return None;
        }
        let dir = dir_of(self.text.get(self.nodes[module as usize].path));
        let mut hits = self
            .go
            .dirs
            .get(dir)?
            .iter()
            .filter_map(|&m| self.import_symbol(m, name).map(|(i, _)| i));
        let hit = hits.next()?;
        hits.next().is_none().then_some(hit)
    }
}
