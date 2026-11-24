// SPDX-License-Identifier: Apache-2.0
//! A reference belongs to a specific manifest field and its key node.
use super::{RawRef, RefForm, Tables, dir_of};
use crate::index::refs_names::inventory_join;

pub(in crate::index) fn references(
    path: &str,
    file: &dctx_lex_cfg::LexedFile,
    source: &[u8],
) -> Vec<RawRef> {
    if path.rsplit('/').next() != Some("tsconfig.json") {
        return Vec::new();
    }
    let owner = format!("s:{path}#extends");
    let mut refs: Vec<_> = file
        .references
        .iter()
        .map(|r| &r.fact)
        .filter(|r| {
            r.src.as_str() == owner
                && r.scope.is_empty()
                && (r.name.starts_with("./") || r.name.starts_with("../"))
        })
        .filter_map(|r| RawRef::new(r.src.as_str(), r.kind, &r.name, "", None))
        .collect();
    if file
        .nodes
        .iter()
        .any(|n| n.fact.name == "compilerOptions.esModuleInterop" && n.fact.signature == "true")
        && std::str::from_utf8(source).ok().and_then(|text| {
            dctx_lex_js::JsLexer::default().compiler_option(text, "esModuleInterop")
        }) == Some(true)
    {
        refs.push(RawRef::fact(RefForm::Interop, "", ""));
    }
    refs
}

impl Tables<'_> {
    pub(super) fn config_target(&self, importer: &str, r: &RawRef) -> Option<(u32, bool)> {
        let path = inventory_join(dir_of(importer), &r.name)?;
        self.module_paths.get(path.as_str()).map(|&m| (m, true))
    }
}
