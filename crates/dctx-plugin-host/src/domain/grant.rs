//! Grant calculation `G(t) = M ∩ U ∩ O ∩ H ∩ S(t) ∩ A` with explicit denies and quota minima.
//! Implements PLUGINS.md §4.1 and Authority.tla `Effective`: semantic containment per capability kind,
//! the minimum of numeric quotas, and no cross-kind implication (write never implies read or spawn).
//! Depends on protocol capability records and selection. Used by authority admission and commit checks.

use dctx_plugin_protocol::records::capability::{Capability, OutputOperation};
use dctx_plugin_protocol::records::scalars::{ProjectPath, Role};

use crate::domain::selection::Selection;

/// One concrete need of a broker action, checked for containment in the effective grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Need {
    /// Read `bytes` total of a named file.
    FileRead {
        /// Named file.
        path: ProjectPath,
        /// Cumulative bytes this read reaches.
        bytes: u64,
    },
    /// Replace a named file.
    FileWrite {
        /// Named file.
        path: ProjectPath,
        /// Resulting size.
        size: u64,
        /// Growth over the precondition size.
        growth: u64,
        /// Whether the file is created.
        create: bool,
    },
    /// The `git.add-ignore` recipe on the ignore file.
    GitAddIgnore {
        /// Ignore file the recipe stages.
        path: ProjectPath,
    },
    /// Read an output of `role` from `operation`.
    OutputRead {
        /// Output role.
        role: Role,
        /// Producing operation.
        operation: OutputOperation,
        /// Cumulative bytes this read reaches.
        bytes: u64,
    },
}

/// An explicit deny rule; denies win over every grant layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DenyRule {
    /// Deny every file read of `path`.
    FileRead(ProjectPath),
    /// Deny every file write of `path`.
    FileWrite(ProjectPath),
    /// Deny every program recipe.
    Programs,
    /// Deny every output read.
    Outputs,
}

/// The six grant layers plus explicit denies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrantLayers {
    /// `M`: manifest request after configuration.
    pub manifest: Vec<Capability>,
    /// `U`: user consent record.
    pub consent: Vec<Capability>,
    /// `O`: compiled organization ceiling.
    pub organization: Vec<Capability>,
    /// `H`: embedding host/caller ceiling.
    pub host: Vec<Capability>,
    /// `A`: enforceable adapter operations.
    pub adapter: Vec<Capability>,
    /// Explicit denies.
    pub denies: Vec<DenyRule>,
}

impl GrantLayers {
    /// `G(t)`: the intersection of all layers under `selection`, minus explicit denies.
    pub fn effective(&self, selection: &Selection) -> Vec<Capability> {
        let mut grant = self.manifest.clone();
        for layer in [&self.consent, &self.organization, &self.host, &self.adapter] {
            grant = intersect_sets(&grant, layer);
        }
        grant.retain(|cap| selected(cap, selection) && !self.denies.iter().any(|deny| denies(deny, cap)));
        grant
    }
}

/// Pairwise intersection of two capability sets, without duplicates.
pub fn intersect_sets(left: &[Capability], right: &[Capability]) -> Vec<Capability> {
    let mut out: Vec<Capability> = Vec::new();
    for a in left {
        for b in right {
            if let Some(cap) = intersect(a, b)
                && !out.contains(&cap)
            {
                out.push(cap);
            }
        }
    }
    out
}

/// Semantic intersection of two capabilities; `None` when they share no operation.
pub fn intersect(a: &Capability, b: &Capability) -> Option<Capability> {
    use Capability as C;
    match (a, b) {
        (C::FileRead { path, max_bytes }, C::FileRead { path: p2, max_bytes: m2 }) if path == p2 => {
            Some(C::FileRead { path: path.clone(), max_bytes: (*max_bytes).min(*m2) })
        }
        (
            C::FileWrite { path, create, max_growth_bytes, max_bytes },
            C::FileWrite { path: p2, create: c2, max_growth_bytes: g2, max_bytes: m2 },
        ) if path == p2 => Some(C::FileWrite {
            path: path.clone(),
            create: *create && *c2,
            max_growth_bytes: (*max_growth_bytes).min(*g2),
            max_bytes: (*max_bytes).min(*m2),
        }),
        (C::GitAddIgnore { max_output_bytes }, C::GitAddIgnore { max_output_bytes: m2 }) => {
            Some(C::GitAddIgnore { max_output_bytes: (*max_output_bytes).min(*m2) })
        }
        (
            C::OutputsRead { roles, operations, max_bytes },
            C::OutputsRead { roles: r2, operations: o2, max_bytes: m2 },
        ) => {
            let roles: Vec<Role> = roles.iter().filter(|r| r2.contains(r)).cloned().collect();
            let operations: Vec<OutputOperation> = operations.iter().filter(|o| o2.contains(o)).copied().collect();
            (!roles.is_empty() && !operations.is_empty())
                .then(|| C::OutputsRead { roles, operations, max_bytes: (*max_bytes).min(*m2) })
        }
        (
            C::State { read, write, reclaim, max_bytes, source_content },
            C::State { read: r2, write: w2, reclaim: c2, max_bytes: m2, source_content: s2 },
        ) => {
            let (read, write, reclaim) = (*read && *r2, *write && *w2, *reclaim && *c2);
            (read || write || reclaim).then(|| C::State {
                read,
                write,
                reclaim,
                max_bytes: (*max_bytes).min(*m2),
                source_content: *source_content && *s2,
            })
        }
        _ => None,
    }
}

/// Whether `cap` contains `need` (same kind, scope and arguments; quotas within the minimum).
pub fn covers(cap: &Capability, need: &Need) -> bool {
    use Capability as C;
    match (cap, need) {
        (C::FileRead { path, max_bytes }, Need::FileRead { path: p, bytes }) => path == p && bytes <= max_bytes,
        (C::FileWrite { path, create, max_growth_bytes, max_bytes }, Need::FileWrite { path: p, size, growth, create: c }) => {
            path == p && size <= max_bytes && growth <= max_growth_bytes && (!*c || *create)
        }
        (C::GitAddIgnore { .. }, Need::GitAddIgnore { .. }) => true,
        (C::OutputsRead { roles, operations, max_bytes }, Need::OutputRead { role, operation, bytes }) => {
            roles.contains(role) && operations.contains(operation) && bytes <= max_bytes
        }
        _ => false,
    }
}

fn selected(cap: &Capability, selection: &Selection) -> bool {
    match cap {
        Capability::FileRead { path, .. } | Capability::FileWrite { path, .. } => selection.authorizes(path),
        _ => true,
    }
}

fn denies(rule: &DenyRule, cap: &Capability) -> bool {
    match (rule, cap) {
        (DenyRule::FileRead(p), Capability::FileRead { path, .. }) => p == path,
        (DenyRule::FileWrite(p), Capability::FileWrite { path, .. }) => p == path,
        (DenyRule::Programs, Capability::GitAddIgnore { .. }) => true,
        (DenyRule::Outputs, Capability::OutputsRead { .. }) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::selection::Decision;

    fn path(text: &str) -> ProjectPath {
        ProjectPath::new(text).expect("path")
    }

    fn read(max: u64) -> Capability {
        Capability::FileRead { path: path(".gitignore"), max_bytes: max }
    }

    fn write() -> Capability {
        Capability::FileWrite { path: path(".gitignore"), create: false, max_growth_bytes: 4096, max_bytes: 1 << 20 }
    }

    fn layers(org: Vec<Capability>) -> GrantLayers {
        let all = vec![read(1 << 20), write(), Capability::GitAddIgnore { max_output_bytes: 65_536 }];
        GrantLayers { manifest: all.clone(), consent: all.clone(), organization: org, host: all.clone(), adapter: all, denies: vec![] }
    }

    fn include() -> Selection {
        Selection::new([(path(".gitignore"), Decision::Include)])
    }

    #[test]
    fn intersection_takes_quota_minimum_and_never_crosses_kinds() {
        assert_eq!(intersect(&read(10), &read(4)), Some(read(4)));
        assert_eq!(intersect(&read(10), &write()), None);
        assert_eq!(intersect(&write(), &Capability::GitAddIgnore { max_output_bytes: 1 }), None);
        let other = Capability::FileRead { path: path("other"), max_bytes: 10 };
        assert_eq!(intersect(&read(10), &other), None);
    }

    #[test]
    fn organization_ceiling_narrows_with_positive_control() {
        let granted = layers(vec![read(100)]).effective(&include());
        assert_eq!(granted, vec![read(100)]);
        assert!(granted.iter().any(|c| covers(c, &Need::FileRead { path: path(".gitignore"), bytes: 100 })));
        assert!(!granted.iter().any(|c| covers(c, &Need::FileRead { path: path(".gitignore"), bytes: 101 })));
        let need = Need::FileWrite { path: path(".gitignore"), size: 10, growth: 1, create: false };
        assert!(!granted.iter().any(|c| covers(c, &need)));
        let full = layers(vec![read(1 << 20), write()]).effective(&include());
        assert!(full.iter().any(|c| covers(c, &need)));
    }

    #[test]
    fn selection_and_denies_remove_grants_with_positive_control() {
        let all = layers(vec![read(1 << 20), write()]);
        assert_eq!(all.effective(&include()).len(), 2);
        for decision in [Decision::Exclude, Decision::OmitContent, Decision::DebugVisible] {
            let selection = Selection::new([(path(".gitignore"), decision)]);
            assert!(all.effective(&selection).is_empty(), "{decision:?}");
        }
        let mut denied = all.clone();
        denied.denies.push(DenyRule::FileWrite(path(".gitignore")));
        assert_eq!(denied.effective(&include()), vec![read(1 << 20)]);
    }

    #[test]
    fn create_needs_its_own_grant() {
        let need = Need::FileWrite { path: path(".gitignore"), size: 1, growth: 1, create: true };
        assert!(!covers(&write(), &need));
        let creating = Capability::FileWrite { path: path(".gitignore"), create: true, max_growth_bytes: 4096, max_bytes: 1 << 20 };
        assert!(covers(&creating, &need));
    }
}
