// SPDX-License-Identifier: Apache-2.0
//! Protobuf file operands are relative to the ordered compiler import roots.
use super::Tables;
use crate::index::refs_names::inventory_join;
impl Tables<'_> {
    pub(super) fn proto_target(&self, path: &str) -> Option<(u32, bool)> {
        if path.split('/').any(|s| matches!(s, "" | "." | "..")) || path.contains('\\') {
            return None;
        }
        self.paths.proto.iter().find_map(|dir| {
            let full = inventory_join(dir, path)?;
            self.module_paths.get(full.as_str()).map(|&m| (m, true))
        })
    }
}
