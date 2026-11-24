// SPDX-License-Identifier: Apache-2.0
//! SwiftPM target membership for literal manifests using the standard Sources layout.
use super::{Confidence, EdgeKind, RawRef, RefForm, Tables, dir_of, note};
use std::collections::BTreeMap;

impl Tables<'_> {
    pub(super) fn swift_import(
        &self,
        importer: &str,
        src: u32,
        r: &RawRef,
        found: &mut BTreeMap<(u32, EdgeKind, u32), Confidence>,
    ) {
        let mut dir = dir_of(importer);
        loop {
            let prefix = if dir.is_empty() {
                String::new()
            } else {
                format!("{dir}/")
            };
            if let Some(&manifest) = self
                .module_paths
                .get(format!("{prefix}Package.swift").as_str())
            {
                let Some((owner, _)) = importer
                    .strip_prefix(&format!("{prefix}Sources/"))
                    .and_then(|p| p.split_once('/'))
                else {
                    return;
                };
                let facts = self.facts(manifest);
                if !facts.iter().any(|f| {
                    f.form == RefForm::SwiftDependency && f.scope == owner && f.name == r.name
                }) || !facts
                    .iter()
                    .any(|f| f.form == RefForm::Package && f.name == r.name)
                {
                    return;
                }
                let base = format!("{prefix}Sources/{}/", r.name);
                for (&path, &module) in &self.module_paths {
                    if path.starts_with(&base) && path.ends_with(".swift") {
                        note(found, src, EdgeKind::Imports, module, Confidence::Exact);
                    }
                }
                return;
            }
            if dir.is_empty() {
                return;
            }
            dir = dir_of(dir);
        }
    }
}
