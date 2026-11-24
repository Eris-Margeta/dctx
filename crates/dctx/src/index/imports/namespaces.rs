// SPDX-License-Identifier: Apache-2.0
//! A namespace/type name identifies a declaration, independent of file placement.
use super::{RawRef, Tables};

impl Tables<'_> {
    pub(super) fn namespace_symbol(
        &self,
        family: &str,
        name: &str,
        allowed: impl Fn(&str) -> bool,
    ) -> Option<(u32, bool)> {
        let last = name.rsplit("::").next()?;
        let mut hits = self
            .bare
            .get(&(family, last))?
            .iter()
            .copied()
            .filter(|&i| {
                let node = &self.nodes[i as usize];
                node.name == name && allowed(&node.kind)
            });
        let first = hits.next()?;
        hits.next().is_none().then_some((first, false))
    }

    pub(super) fn csharp_target(&self, r: &RawRef) -> Option<(u32, bool)> {
        r.alias.as_ref()?;
        let name = r
            .name
            .strip_prefix("global::")
            .unwrap_or(&r.name)
            .replace('.', "::");
        self.namespace_symbol("csharp", &name, |k| {
            matches!(
                k,
                "class" | "struct" | "interface" | "enum" | "type" | "record"
            )
        })
    }
}
