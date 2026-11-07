// SPDX-License-Identifier: Apache-2.0
//! Snapshot scan: walks a `Source` into the pack tree the legacy walker and content reader build.
//! Implements PRODUCT-CONTRACTS P1 (decide before any metadata/content access, never recurse into
//! excluded directories, read only Include files under Include ancestors) and decision 077 content identity.
//! Depends on dctx-core (policy, facts, limits) and the `Source`/`Cancellation` ports. Used by [`crate::engine`].

use dctx_core::budget::PackLimits;
use dctx_core::datatypes::{DirContextTreeNode, NodeType};
use dctx_core::domain::{FileHandlingDecision, ItemContext};
use dctx_core::error::{DctxError, Resource};
use dctx_core::facts::{
    apply_content, mark_subtree_ignored, ContentDigests, Diagnostic, ReadGrant,
};
use dctx_core::filter::{has_git_segment, is_secret_shaped_name};
use dctx_core::ids::RelativePath;
use dctx_core::selection::SelectionPolicy;
use dctx_hash::Sha256;
use dctx_ports::{Cancellation, Source};
use std::cmp::Ordering;
use std::path::PathBuf;

/// What one scan produced.
#[derive(Debug, Clone)]
pub struct Scan {
    /// The selected tree, children sorted by relative path.
    pub tree: DirContextTreeNode,
    /// Own digests of every file whose content was read.
    pub digests: ContentDigests,
    /// Non-fatal observations, in walk order.
    pub diagnostics: Vec<Diagnostic>,
    /// Entries in the tree, root included.
    pub entries: u64,
    /// Content bytes read.
    pub content_bytes: u64,
}

/// Scan settings.
#[derive(Debug, Clone, Copy)]
pub struct ScanOptions<'a> {
    /// Selection policy deciding every item.
    pub policy: &'a SelectionPolicy,
    /// Show ignored items as `DebugVisible` instead of excluding them.
    pub debug_ignored: bool,
    /// Bounds on the scan.
    pub limits: PackLimits,
}

/// Walks `source` from its root. The root is always `Include`.
///
/// # Errors
/// Propagates source and cancellation errors and refuses invalid tree entries or exceeded scan limits.
pub fn scan(
    source: &dyn Source,
    cancel: &dyn Cancellation,
    options: ScanOptions<'_>,
) -> Result<Scan, DctxError> {
    cancel.checkpoint()?;
    let root_kind = source.root_kind()?;
    let mut walk = Walk {
        source,
        cancel,
        options,
        out: Scan::empty(),
    };
    let root = RelativePath::root();
    let tree = walk.node(&root, root_kind, FileHandlingDecision::Include, true)?;
    let mut out = walk.out;
    out.tree = tree;
    Ok(out)
}

impl Scan {
    fn empty() -> Self {
        Self {
            tree: DirContextTreeNode::new(NodeType::Directory, "", PathBuf::new()),
            digests: ContentDigests::new(),
            diagnostics: Vec::new(),
            entries: 0,
            content_bytes: 0,
        }
    }
}

struct Walk<'a> {
    source: &'a dyn Source,
    cancel: &'a dyn Cancellation,
    options: ScanOptions<'a>,
    out: Scan,
}

impl Walk<'_> {
    fn node(
        &mut self,
        path: &RelativePath,
        kind: NodeType,
        decision: FileHandlingDecision,
        ancestors_readable: bool,
    ) -> Result<DirContextTreeNode, DctxError> {
        self.cancel.checkpoint()?;
        self.out.entries += 1;
        self.options.limits.check_entries(self.out.entries)?;
        self.options.limits.check_depth(path.depth() as u64)?;

        let mut node = DirContextTreeNode::new(kind, path.as_str(), PathBuf::from(path.as_str()));
        node.handling_decision = decision;
        if decision.needs_metadata() {
            self.metadata(path, &mut node)?;
        }

        let readable = ancestors_readable && decision.should_read_content();
        match kind {
            NodeType::Directory => self.children(path, &mut node, readable)?,
            NodeType::File => {
                if let Some(grant) = ReadGrant::authorize(path, decision, ancestors_readable) {
                    self.content(&grant, &mut node)?;
                }
            }
        }
        Ok(node)
    }

    fn metadata(
        &mut self,
        path: &RelativePath,
        node: &mut DirContextTreeNode,
    ) -> Result<(), DctxError> {
        match self.source.metadata(path) {
            Ok(meta) => {
                node.last_modified_timestamp = meta.modified.millis();
                if node.node_type == NodeType::File {
                    node.content_size = meta.size;
                }
                Ok(())
            }
            Err(DctxError::Cancelled) => Err(DctxError::Cancelled),
            Err(_) => {
                self.out.diagnostics.push(Diagnostic::MissingMetadata {
                    path: path.to_string(),
                });
                Ok(())
            }
        }
    }

    fn children(
        &mut self,
        path: &RelativePath,
        node: &mut DirContextTreeNode,
        readable: bool,
    ) -> Result<(), DctxError> {
        let entries = match self.source.entries(path) {
            Ok(entries) => entries,
            Err(DctxError::Cancelled) => return Err(DctxError::Cancelled),
            Err(_) => {
                self.out.diagnostics.push(Diagnostic::UnlistableDirectory {
                    path: path.to_string(),
                });
                return Ok(());
            }
        };
        for entry in entries {
            let Ok(child_path) = path.child(&entry.name) else {
                self.out.diagnostics.push(Diagnostic::UnrepresentableName {
                    parent: path.to_string(),
                });
                continue;
            };
            let context = ItemContext {
                relative_path: child_path.as_str(),
                name: &entry.name,
                is_directory: entry.is_dir(),
                debug_mode: self.options.debug_ignored,
            };
            let listing_only = self.options.debug_ignored && !readable;
            let decision = withhold_sensitive(
                self.options.policy.decide(&context),
                child_path.as_str(),
                &entry.name,
                entry.is_dir(),
                listing_only,
            );
            if !decision.show_in_tree() {
                continue;
            }
            let mut child = self.node(&child_path, entry.kind, decision, readable)?;
            if decision == FileHandlingDecision::DebugVisible {
                mark_subtree_ignored(&mut child);
            }
            node.children.push(child);
        }
        node.children.sort_by(by_path);
        Ok(())
    }

    fn content(
        &mut self,
        grant: &ReadGrant,
        node: &mut DirContextTreeNode,
    ) -> Result<(), DctxError> {
        let path = grant.path().to_string();
        let limits = self.options.limits;
        if node.content_size > limits.max_file_bytes {
            self.out.diagnostics.push(Diagnostic::ContentWithheld {
                path,
                size: node.content_size,
            });
            return Ok(());
        }
        self.cancel.checkpoint()?;
        let bytes = match self.source.read(grant, limits.max_file_bytes) {
            Ok(bytes) => bytes,
            Err(DctxError::Cancelled) => return Err(DctxError::Cancelled),
            Err(DctxError::ResourceLimit {
                resource: Resource::FileBytes,
                ..
            }) => {
                self.out.diagnostics.push(Diagnostic::ContentWithheld {
                    path,
                    size: node.content_size,
                });
                return Ok(());
            }
            Err(_) => {
                self.out
                    .diagnostics
                    .push(Diagnostic::UnreadableFile { path });
                return Ok(());
            }
        };
        self.out.content_bytes = self.out.content_bytes.saturating_add(bytes.len() as u64);
        limits.check_source_bytes(self.out.content_bytes)?;
        let digest = Sha256::digest(&bytes).map_err(|_| DctxError::ResourceLimit {
            resource: Resource::FileBytes,
            limit: u64::MAX,
        })?;
        self.out.digests.insert(path, digest);
        apply_content(node, &bytes);
        Ok(())
    }
}

/// Sensitive files ([`is_secret_shaped_name`]: env, key, credential and token files) stay in the tree by name but are never read.
/// Only an `Include` verdict is downgraded; `Exclude` and `DebugVisible` already read nothing.
/// A `.git` path ([`has_git_segment`]) is excluded even when a rule such as `!.git/` negates the built-in ignore.
/// `--debug-ignored` still lists it by name: the `DebugVisible` directory and, with `listing_only` (inside an
/// unreadable debug subtree), its children. Nothing there is ever read.
fn withhold_sensitive(
    decision: FileHandlingDecision,
    path: &str,
    name: &str,
    is_dir: bool,
    listing_only: bool,
) -> FileHandlingDecision {
    if has_git_segment(path) && decision != FileHandlingDecision::DebugVisible && !listing_only {
        return FileHandlingDecision::Exclude;
    }
    if decision == FileHandlingDecision::Include && !is_dir && is_secret_shaped_name(name) {
        FileHandlingDecision::OmitContent
    } else {
        decision
    }
}

/// Children order of the legacy walker: relative path, byte-wise.
fn by_path(a: &DirContextTreeNode, b: &DirContextTreeNode) -> Ordering {
    a.relative_path.cmp(&b.relative_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_core::selection::RuleTexts;
    use dctx_ports::memory::{CancelAfter, CancelFlag, MemorySource};

    fn policy() -> SelectionPolicy {
        SelectionPolicy::from_texts(&RuleTexts {
            local_ignore: Some("secret/\n.env\n"),
            ..RuleTexts::default()
        })
    }

    fn source() -> MemorySource {
        let mut source = MemorySource::new();
        source
            .insert_file("src/main.rs", b"fn main() {}\n", 10)
            .unwrap();
        source.insert_file("Cargo.lock", b"lock", 11).unwrap();
        source.insert_file(".env", b"TOKEN=1", 12).unwrap();
        source.insert_file("secret/key.pem", b"KEY", 13).unwrap();
        source
    }

    fn run(source: &MemorySource, debug: bool, limits: PackLimits) -> Result<Scan, DctxError> {
        let policy = policy();
        let options = ScanOptions {
            policy: &policy,
            debug_ignored: debug,
            limits,
        };
        scan(source, &CancelFlag::new(), options)
    }

    #[test]
    fn included_files_are_read_and_hashed() {
        let source = source();
        let out = run(&source, false, PackLimits::unbounded()).unwrap();
        let paths: Vec<&str> = out
            .tree
            .children
            .iter()
            .map(|c| c.relative_path.as_str())
            .collect();
        assert_eq!(paths, ["Cargo.lock", "src"]);
        assert_eq!(source.reads(), ["src/main.rs"]);
        assert_eq!(out.digests.keys().collect::<Vec<_>>(), ["src/main.rs"]);
        assert_eq!(out.content_bytes, 13);
        assert_eq!(out.entries, 4);
    }

    #[test]
    fn secret_shaped_files_are_listed_but_never_read() {
        let mut source = MemorySource::new();
        source.insert_file("app.py", b"def f(): pass\n", 1).unwrap();
        for (path, at) in [
            (".env", 2),
            (".env.local", 3),
            ("settings.env", 4),
            (".envrc", 5),
            ("sub/.env.production", 6),
        ] {
            source.insert_file(path, b"K=CANARYSECRET\n", at).unwrap();
        }
        let policy = SelectionPolicy::from_texts(&RuleTexts::default());
        let options = ScanOptions {
            policy: &policy,
            debug_ignored: false,
            limits: PackLimits::unbounded(),
        };
        let out = scan(&source, &CancelFlag::new(), options).unwrap();
        assert_eq!(source.reads(), ["app.py"]);
        assert_eq!(out.digests.keys().collect::<Vec<_>>(), ["app.py"]);
        let env = out
            .tree
            .children
            .iter()
            .find(|c| c.relative_path == ".env")
            .unwrap();
        assert_eq!(env.handling_decision, FileHandlingDecision::OmitContent);
        assert!(env.file_content.is_none());
        assert!(!format!("{:?}", out.tree).contains("CANARYSECRET"));
    }

    /// v1 card 1: key, credential and token files are withheld like env files; a look-alike source file is still read.
    #[test]
    fn sensitive_key_and_credential_files_are_listed_but_never_read() {
        let mut source = MemorySource::new();
        source.insert_file("app.py", b"def f(): pass\n", 1).unwrap();
        source
            .insert_file("src/credentials.py", b"def g(): pass\n", 1)
            .unwrap();
        let sensitive = [
            "certs/server.pem",
            "id_rsa",
            "keys/id_ed25519",
            "credentials",
            "config/credentials.json",
            ".npmrc",
            "deploy/secrets.yaml",
        ];
        for path in sensitive {
            source.insert_file(path, b"K=CANARYSECRET\n", 2).unwrap();
        }
        let policy = SelectionPolicy::from_texts(&RuleTexts::default());
        let options = ScanOptions {
            policy: &policy,
            debug_ignored: false,
            limits: PackLimits::unbounded(),
        };
        let out = scan(&source, &CancelFlag::new(), options).unwrap();
        let mut reads = source.reads();
        reads.sort();
        assert_eq!(reads, ["app.py", "src/credentials.py"]);
        assert!(!format!("{:?}", out.tree).contains("CANARYSECRET"));
        let npmrc = out
            .tree
            .children
            .iter()
            .find(|c| c.relative_path == ".npmrc")
            .unwrap();
        assert_eq!(npmrc.handling_decision, FileHandlingDecision::OmitContent);
    }

    /// v1 card 1: a `!.git/` negation in the local rules cannot list or read `.git`.
    #[test]
    fn git_negations_never_list_or_read_git() {
        for rules in ["!.git/\n", "!.g*/\n", "!nested/.git/\n!nested/.git/**\n"] {
            let mut source = MemorySource::new();
            source.insert_file("keep.txt", b"keep", 1).unwrap();
            source
                .insert_file(".git/config", b"PRIVATE_SENTINEL", 2)
                .unwrap();
            source
                .insert_file("nested/.git/config", b"PRIVATE_SENTINEL", 3)
                .unwrap();
            let policy = SelectionPolicy::from_texts(&RuleTexts {
                local_ignore: Some(rules),
                ..RuleTexts::default()
            });
            let options = ScanOptions {
                policy: &policy,
                debug_ignored: false,
                limits: PackLimits::unbounded(),
            };
            let out = scan(&source, &CancelFlag::new(), options).unwrap();
            assert_eq!(source.reads(), ["keep.txt"], "{rules}");
            assert!(
                !source.touched(".git/config") && !source.touched("nested/.git/config"),
                "{rules}"
            );
            let listed = format!("{:?}", out.tree);
            assert!(
                !listed.contains(".git") && !listed.contains("PRIVATE_SENTINEL"),
                "{rules}: {listed}"
            );
        }
    }

    #[test]
    fn excluded_items_are_never_touched() {
        let source = source();
        run(&source, false, PackLimits::unbounded()).unwrap();
        assert!(!source.touched("secret"));
        assert!(!source.touched(".env"));
        assert!(!source.reads().contains(&"Cargo.lock".to_string()));
    }

    #[test]
    fn debug_visible_items_get_metadata_but_no_reads() {
        let source = source();
        let out = run(&source, true, PackLimits::unbounded()).unwrap();
        let secret = out
            .tree
            .children
            .iter()
            .find(|c| c.relative_path == "secret")
            .unwrap();
        assert!(secret.is_ignored && secret.children[0].is_ignored);
        assert_eq!(secret.children[0].content_size, 3);
        assert_eq!(source.reads(), ["src/main.rs"]);
    }

    #[test]
    fn limits_refuse_or_withhold() {
        let source = source();
        let tight = PackLimits {
            max_entries: 3,
            ..PackLimits::unbounded()
        };
        assert!(matches!(
            run(&source, false, tight),
            Err(DctxError::ResourceLimit {
                resource: Resource::Entries,
                ..
            })
        ));
        let source = self::source();
        let small = PackLimits {
            max_file_bytes: 4,
            ..PackLimits::unbounded()
        };
        let out = run(&source, false, small).unwrap();
        assert_eq!(
            source.reads(),
            [] as [std::string::String; 0] as [std::string::String; 0]
        );
        assert!(out.digests.is_empty());
        assert!(matches!(
            out.diagnostics[0],
            Diagnostic::ContentWithheld { size: 13, .. }
        ));
    }

    #[test]
    fn cancellation_stops_at_the_first_boundary_after_the_signal() {
        let source = source();
        let policy = policy();
        let options = ScanOptions {
            policy: &policy,
            debug_ignored: false,
            limits: PackLimits::unbounded(),
        };
        assert_eq!(
            scan(&source, &CancelAfter::new(0), options).unwrap_err(),
            DctxError::Cancelled
        );
        assert_eq!(source.calls(), []);
        assert_eq!(
            scan(&source, &CancelAfter::new(3), options).unwrap_err(),
            DctxError::Cancelled
        );
        assert_eq!(
            source.reads(),
            [] as [std::string::String; 0] as [std::string::String; 0]
        );
    }
}
