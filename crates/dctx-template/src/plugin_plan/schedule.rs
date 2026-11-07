// SPDX-License-Identifier: Apache-2.0
//! Deterministic plugin ordering and cycle refusal.
//! Implements PLUGINS.md §3.5: locks, ceilings and sticky disable.
//! Depends on plugin groups, enabled bindings and ordered collections; used by the plugin plan compiler and loader.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::TemplateError;
use crate::plugin::{PluginBinding, PluginGroup, PluginState};

#[cfg(test)]
use crate::plugin::Resolution;

pub(super) fn schedule(
    groups: &[PluginGroup],
    plugins: &[PluginBinding],
) -> Result<Vec<String>, TemplateError> {
    let ids: Vec<String> = plugins
        .iter()
        .filter(|plugin| plugin.state == PluginState::Enabled)
        .map(|plugin| plugin.id.clone())
        .collect();
    let mut edges = Vec::new();
    for group in groups {
        for pair in group.order.windows(2) {
            if let (Some(before), Some(after)) = (
                participant(plugins, &pair[0])?,
                participant(plugins, &pair[1])?,
            ) {
                edges.push((before.to_string(), after.to_string()));
            }
        }
    }
    for plugin in plugins
        .iter()
        .filter(|plugin| plugin.state == PluginState::Enabled)
    {
        for before in &plugin.before {
            if let Some(id) = participant(plugins, before)? {
                edges.push((id.to_string(), plugin.id.clone()));
            }
        }
        for after in &plugin.after {
            if let Some(id) = participant(plugins, after)? {
                edges.push((plugin.id.clone(), id.to_string()));
            }
        }
    }
    topo(&ids, &edges)
}

fn participant<'a>(
    plugins: &'a [PluginBinding],
    id: &str,
) -> Result<Option<&'a str>, TemplateError> {
    let Some(plugin) = plugins.iter().find(|plugin| plugin.id == id) else {
        return Err(TemplateError::UnresolvedRef {
            kind: "plugin",
            name: id.to_string(),
        });
    };
    if plugin.state == PluginState::Enabled {
        Ok(Some(plugin.id.as_str()))
    } else {
        Ok(None)
    }
}

fn topo(ids: &[String], edges: &[(String, String)]) -> Result<Vec<String>, TemplateError> {
    let mut indeg: BTreeMap<String, usize> = ids.iter().map(|id| (id.clone(), 0)).collect();
    let mut outgoing: BTreeMap<String, BTreeSet<String>> =
        ids.iter().map(|id| (id.clone(), BTreeSet::new())).collect();
    for (before, after) in edges {
        if before == after {
            return Err(TemplateError::Cycle {
                kind: "plugin",
                name: before.clone(),
            });
        }
        let Some(next) = outgoing.get_mut(before) else {
            return Err(TemplateError::UnresolvedRef {
                kind: "plugin",
                name: before.clone(),
            });
        };
        if next.insert(after.clone()) {
            let Some(degree) = indeg.get_mut(after) else {
                return Err(TemplateError::UnresolvedRef {
                    kind: "plugin",
                    name: after.clone(),
                });
            };
            *degree += 1;
        }
    }
    let mut ready: BTreeSet<String> = indeg
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut order = Vec::new();
    while let Some(id) = ready.iter().next().cloned() {
        ready.remove(&id);
        order.push(id.clone());
        let nexts = outgoing.get(&id).cloned().unwrap_or_default();
        for next in nexts {
            if let Some(degree) = indeg.get_mut(&next) {
                *degree = degree.saturating_sub(1);
                if *degree == 0 {
                    ready.insert(next);
                }
            }
        }
    }
    if order.len() != ids.len() {
        let name = ids
            .iter()
            .find(|id| !order.contains(*id))
            .cloned()
            .unwrap_or_default();
        return Err(TemplateError::Cycle {
            kind: "plugin",
            name,
        });
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginOrigin;

    fn enabled(id: &str) -> PluginBinding {
        PluginBinding {
            id: id.to_string(),
            version: "1.0.0".to_string(),
            digest: None,
            manifest_digest: None,
            publisher_key: String::new(),
            origin: Some(PluginOrigin::Installed),
            resolution: Resolution::Available,
            required: false,
            state: PluginState::Enabled,
            alias: String::new(),
            hooks: Vec::new(),
            allow: Vec::new(),
            config: Vec::new(),
            before: Vec::new(),
            after: Vec::new(),
        }
    }

    #[test]
    fn tie_break_uses_canonical_id_and_a_cycle_is_refused() {
        let mut first = enabled("ex/b");
        let second = enabled("ex/a");
        let order = schedule(&[], &[first.clone(), second.clone()]).expect("tie");
        assert_eq!(order, vec!["ex/a".to_string(), "ex/b".to_string()]);
        first.after = vec!["ex/a".to_string()];
        let forced = schedule(&[], &[first.clone(), second]).expect("after");
        assert_eq!(forced, vec!["ex/b".to_string(), "ex/a".to_string()]);
        let mut looped = enabled("ex/a");
        looped.before = vec!["ex/a".to_string()];
        assert!(matches!(
            schedule(&[], &[looped]),
            Err(TemplateError::Cycle { kind: "plugin", .. })
        ));
    }
}
