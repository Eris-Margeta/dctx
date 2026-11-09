// SPDX-License-Identifier: Apache-2.0
//! Call and import edges for the facade index.
//! Implements SPEC-MAP-MODE 5.3 and 6.3 over the lexers' raw references, using the generation's own nodes.
//! The resolver in dctx-map-resolve is not a dependency of this crate, so the same lookup order lives here.
//! The walk records one [`FileRefs`] per file, then [`link`] appends `calls` and `imports` edges once.

use std::collections::{BTreeMap, HashMap, HashSet};

use dctx_map_model::EdgeKind;

use super::decl_pair::is_declaration_only_path;
use super::extract::language_of;
use super::refs_names::{call_segments, import_segments, last_segment, module_keys};
use super::{IndexedEdge, IndexedNode, TextBlob, write_node_id};

/// One unresolved call or import exactly as a lexer reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawRef {
    /// Stable id text of the owning node (`m:` for the file itself).
    pub src: String,
    /// `calls` or `imports`.
    pub kind: EdgeKind,
    /// Referenced spelling.
    pub name: String,
    /// Lexical scope qualified with `::`. Empty means the file root.
    pub scope: String,
    /// Local name bound by an import.
    pub alias: Option<String>,
}

impl RawRef {
    /// Keeps calls and imports only. Other relations yield `None`.
    pub(crate) fn new(
        src: &str,
        kind: EdgeKind,
        name: &str,
        scope: &str,
        alias: Option<&str>,
    ) -> Option<Self> {
        if !matches!(kind, EdgeKind::Calls | EdgeKind::Imports) {
            return None;
        }
        Some(Self {
            src: src.to_string(),
            kind,
            name: name.to_string(),
            scope: scope.to_string(),
            alias: alias.map(str::to_string),
        })
    }
}

/// Node range of one indexed file and the references found in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileRefs {
    /// Index of the file's module node. Its symbols follow it up to `end`.
    pub first: u32,
    /// One past the file's last node.
    pub end: u32,
    /// Raw calls and imports in source order.
    pub refs: Vec<RawRef>,
}

/// Languages where a call spelled as a bare name never reaches a method: a method needs a receiver (`self.`, `this.`, `x.`) or a type path (`Self::`).
const RECEIVER_ONLY: &[&str] = &["rust", "python", "javascript", "typescript", "go"];

/// Language family of a language tag: calls and imports resolve only between files of one family.
///
/// Languages that share a toolchain and call each other directly form one family (JS/TS and web components,
/// the JVM languages, the BEAM languages); every other tag is its own family. A Rust `.all(..)` therefore never
/// reaches a Makefile target named `all`, and a Python call never reaches a shell function or a config key.
fn language_family(language: &str) -> &str {
    match language {
        "javascript" | "typescript" | "web" => "javascript",
        "java" | "kotlin" | "scala" | "groovy" => "jvm",
        "erlang" | "elixir" => "beam",
        other => other,
    }
}

/// Edge confidence in rank order: a lower rank wins when two references reach the same pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Exact,
    Name,
    Guess,
}

/// The calling file as the resolver sees it.
struct Caller<'b> {
    file: &'b FileRefs,
    /// Language family of the file, see [`language_family`].
    family: &'b str,
    /// Every name segment the file declares or imports (resolved or not): the types a receiver call can name.
    visible: HashSet<&'b str>,
}

/// A local name an import introduced, and what it points to.
struct Binding {
    name: String,
    dst: u32,
    module: bool,
}

/// Lookup tables over the generation, built once per [`link`].
struct Tables<'a> {
    nodes: &'a [IndexedNode],
    text: &'a TextBlob,
    files: &'a [FileRefs],
    /// (file module index, full name) to symbols.
    names: HashMap<(u32, &'a str), Vec<u32>>,
    /// (language family, last name segment) to symbols.
    bare: HashMap<(&'a str, &'a str), Vec<u32>>,
    /// Module key to (module index, key is the full path form, language family).
    modules: HashMap<String, Vec<(u32, bool, &'a str)>>,
}

/// Appends `calls` and `imports` edges for every resolvable reference in `files`.
///
/// Unresolved and ambiguous references are dropped. Edges of one file are sorted, so a repeat walk is byte-stable.
pub(crate) fn link(
    nodes: &[IndexedNode],
    edges: &mut Vec<IndexedEdge>,
    text: &mut TextBlob,
    files: &[FileRefs],
) {
    if files.iter().all(|file| file.refs.is_empty()) {
        return;
    }
    let labels = [
        text.push("calls"),
        text.push("imports"),
        text.push("exact"),
        text.push("name"),
        text.push("guess"),
    ];
    let tables = Tables::build(nodes, text, files);
    for file in files.iter().filter(|file| !file.refs.is_empty()) {
        let mut found: BTreeMap<(u32, bool, u32), Rank> = BTreeMap::new();
        tables.link_file(file, &mut found);
        for ((src, call, dst), rank) in found {
            let edge = if call { labels[0] } else { labels[1] };
            let confidence = labels[2 + rank as usize];
            edges.push(IndexedEdge {
                src,
                dst,
                edge,
                confidence,
            });
        }
    }
}

impl<'a> Tables<'a> {
    /// Indexes symbols by file and name, and modules by path key.
    fn build(nodes: &'a [IndexedNode], text: &'a TextBlob, files: &'a [FileRefs]) -> Self {
        let mut names: HashMap<(u32, &str), Vec<u32>> = HashMap::new();
        let mut bare: HashMap<(&str, &str), Vec<u32>> = HashMap::new();
        let mut modules: HashMap<String, Vec<(u32, bool, &str)>> = HashMap::new();
        for file in files {
            let Some(module) = nodes.get(file.first as usize) else {
                continue;
            };
            let family = language_family(language_of(text.get(module.path)));
            for (key, full) in module_keys(text.get(module.path)) {
                modules
                    .entry(key)
                    .or_default()
                    .push((file.first, full, family));
            }
            for index in file.first.saturating_add(1)..file.end {
                let Some(node) = nodes.get(index as usize) else {
                    break;
                };
                if node.letter != b's' || node.name.is_empty() {
                    continue;
                }
                let path = text.get(node.path);
                // A declaration-only symbol is not callable: its paired definition is the real implementation.
                if is_declaration_only_path(path, node) {
                    continue;
                }
                names
                    .entry((file.first, node.name.as_str()))
                    .or_default()
                    .push(index);
                bare.entry((family, last_segment(&node.name)))
                    .or_default()
                    .push(index);
            }
        }
        Self {
            nodes,
            text,
            files,
            names,
            bare,
            modules,
        }
    }

    /// Resolves the imports of `file` first, then its calls through the bindings they made.
    fn link_file(&self, file: &FileRefs, found: &mut BTreeMap<(u32, bool, u32), Rank>) {
        let mut ids: HashMap<String, u32> = HashMap::new();
        for index in file.first..file.end {
            let Some(node) = self.nodes.get(index as usize) else {
                break;
            };
            let mut id = String::new();
            write_node_id(&mut id, self.text, node);
            // Lexers number every copy of a duplicated name from `~1`; the index leaves the first copy bare and numbers the rest from `~2`.
            if node.ordinal == 0 && node.letter != b'm' {
                ids.entry(format!("{id}~1")).or_insert(index);
            }
            ids.insert(id, index);
        }
        let module_node = self.nodes.get(file.first as usize);
        let language = module_node.map(|module| language_of(self.text.get(module.path)));
        let family = language_family(language.unwrap_or("opaque"));
        let receiver_only = language.is_some_and(|language| RECEIVER_ONLY.contains(&language));
        let mut visible: HashSet<&str> = HashSet::new();
        for index in file.first.saturating_add(1)..file.end {
            if let Some(node) = self.nodes.get(index as usize) {
                visible.extend(node.name.split([':', '.']).filter(|part| !part.is_empty()));
            }
        }
        for reference in file
            .refs
            .iter()
            .filter(|reference| reference.kind == EdgeKind::Imports)
        {
            visible.extend(
                reference
                    .name
                    .rsplit(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '$'))
                    .find(|part| !part.is_empty()),
            );
            visible.extend(reference.alias.as_deref());
        }
        let caller = Caller {
            file,
            family,
            visible,
        };
        let mut bindings: Vec<Binding> = Vec::new();
        for reference in file
            .refs
            .iter()
            .filter(|reference| reference.kind == EdgeKind::Imports)
        {
            let Some(&src) = ids.get(&reference.src) else {
                continue;
            };
            let segments = import_segments(&reference.name);
            let Some((dst, module)) = self.import_target(&segments, family) else {
                continue;
            };
            if dst == src {
                continue;
            }
            note(found, src, false, dst, Rank::Exact);
            if let Some(alias) = &reference.alias {
                bindings.push(Binding {
                    name: alias.clone(),
                    dst,
                    module,
                });
            } else if let (Some(first), Some(last)) = (segments.first(), segments.last()) {
                bindings.push(Binding {
                    name: last.clone(),
                    dst,
                    module,
                });
                if module && first != last {
                    bindings.push(Binding {
                        name: first.clone(),
                        dst,
                        module,
                    });
                }
            }
        }
        for reference in file
            .refs
            .iter()
            .filter(|reference| reference.kind == EdgeKind::Calls)
        {
            let Some(&src) = ids.get(&reference.src) else {
                continue;
            };
            let segments = call_segments(&reference.name, language);
            let Some((callee, qualifier)) = segments.split_last() else {
                continue;
            };
            // `interpolate(..)` inside the method `Captures::interpolate` is the imported free function, not the method itself.
            let free = receiver_only
                && reference
                    .name
                    .chars()
                    .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$');
            let (targets, rank) = self.call_targets(
                &caller,
                &reference.scope,
                qualifier,
                callee,
                &bindings,
                free,
            );
            for dst in targets {
                note(found, src, true, dst, rank);
            }
        }
    }

    /// Module or symbol of `family` an import path names. `true` marks a module.
    fn import_target(&self, segments: &[String], family: &str) -> Option<(u32, bool)> {
        if let Some(module) = self.module_named(segments, family) {
            return Some((module, true));
        }
        let (symbol, path) = segments.split_last()?;
        let module = self.module_named(path, family)?;
        let hit = self.names.get(&(module, symbol.as_str()))?;
        (hit.len() == 1).then(|| (hit[0], false))
    }

    /// The one module of `family` a segment path names. A full path form beats a suffix form.
    fn module_named(&self, segments: &[String], family: &str) -> Option<u32> {
        if segments.is_empty() {
            return None;
        }
        let hits: Vec<&(u32, bool, &str)> = self
            .modules
            .get(&segments.join("::"))?
            .iter()
            .filter(|hit| hit.2 == family)
            .collect();
        let full: Vec<u32> = hits.iter().filter(|hit| hit.1).map(|hit| hit.0).collect();
        let pool: Vec<u32> = if full.is_empty() {
            hits.iter().map(|hit| hit.0).collect()
        } else {
            full
        };
        (pool.len() == 1).then(|| pool[0])
    }

    /// Targets of one call: lexical scope, then imports, then module paths, then a project-wide guess.
    ///
    /// A `free` call (a bare name in a [`RECEIVER_ONLY`] language) skips every candidate of kind `method`.
    /// Module paths and the guess only consider files of the caller's language family.
    fn call_targets(
        &self,
        caller: &Caller<'_>,
        scope: &str,
        qualifier: &[String],
        callee: &str,
        bindings: &[Binding],
        free: bool,
    ) -> (Vec<u32>, Rank) {
        let (file, family) = (caller.file, caller.family);
        let qualified = qualifier.join("::");
        let mut walk = scope;
        loop {
            let name = [walk, qualified.as_str(), callee]
                .iter()
                .filter(|part| !part.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join("::");
            if let Some(hit) = self.names.get(&(file.first, name.as_str())) {
                let hit = self.reachable(hit, free);
                if !hit.is_empty() {
                    return Self::pick(&hit);
                }
            }
            if walk.is_empty() {
                break;
            }
            walk = walk.rsplit_once("::").map_or("", |(parent, _)| parent);
        }
        for binding in bindings {
            if let Some(first) = qualifier.first() {
                if *first != binding.name {
                    continue;
                }
                let rest = qualifier[1..]
                    .iter()
                    .map(String::as_str)
                    .chain([callee])
                    .collect::<Vec<_>>()
                    .join("::");
                let (owner, name) = match self.nodes.get(binding.dst as usize) {
                    _ if binding.module => (binding.dst, rest),
                    Some(node) => (self.file_of(binding.dst), format!("{}::{rest}", node.name)),
                    None => continue,
                };
                if let Some(hit) = self.names.get(&(owner, name.as_str())) {
                    return Self::pick(hit);
                }
            } else if binding.name == callee && !binding.module {
                return (vec![binding.dst], Rank::Name);
            }
        }
        if !qualifier.is_empty()
            && let Some(module) = self.module_named(qualifier, family)
            && let Some(hit) = self.names.get(&(module, callee))
        {
            return Self::pick(hit);
        }
        (self.guess(caller, callee, qualifier, free), Rank::Guess)
    }

    /// The candidates a call can reach: all of `hits`, or for a `free` call those that are not methods.
    fn reachable(&self, hits: &[u32], free: bool) -> Vec<u32> {
        hits.iter()
            .copied()
            .filter(|&hit| {
                !free
                    || self
                        .nodes
                        .get(hit as usize)
                        .is_some_and(|node| node.kind != "method")
            })
            .collect()
    }

    /// Hits inside one file: a single one is a `name` match, overloads are guesses.
    fn pick(hit: &[u32]) -> (Vec<u32>, Rank) {
        match hit {
            [only] => (vec![*only], Rank::Name),
            many => (many.iter().copied().take(4).collect(), Rank::Guess),
        }
    }

    /// The one project-wide symbol of the caller's language family with this last segment, preferring the caller's directory when several exist.
    ///
    /// A call with a receiver (`recv.name()`) that still has several candidates resolves to the one candidate of kind `method`, so a lone method wins over same-named free functions, and a lone owned class method (`Owner::name`) wins over object-literal members. Two or more of the same standing stay ambiguous and emit nothing.
    ///
    /// A call with a receiver or qualifier (`recv.name()`, `Type::name()`, `pkg.Name()`) then keeps its pick only if the
    /// caller can see it: it is in the caller's own file, it is an owned member whose owner the caller declares, imports
    /// or names as the qualifier, or it is a free symbol whose file or directory the qualifier names. Without types the
    /// receiver is unknown, so `iter.map(..)` or `text.lines()` never guesses its way to a same-named project method.
    fn guess(
        &self,
        caller: &Caller<'_>,
        callee: &str,
        qualifier: &[String],
        free: bool,
    ) -> Vec<u32> {
        let mut picked = self.pick_guess(caller.file, caller.family, callee, qualifier, free);
        if let Some(named) = qualifier.last() {
            picked.retain(|&hit| self.visible_to(caller, named, hit));
        }
        picked
    }

    /// The guess before the visibility check: ambiguity is judged over every candidate of the family.
    fn pick_guess(
        &self,
        file: &FileRefs,
        family: &str,
        callee: &str,
        qualifier: &[String],
        free: bool,
    ) -> Vec<u32> {
        let receiver = !qualifier.is_empty();
        let Some(hits) = self.bare.get(&(family, callee)) else {
            return Vec::new();
        };
        let hits = self.reachable(hits, free);
        if let [only] = hits.as_slice() {
            return vec![*only];
        }
        let here = self
            .nodes
            .get(file.first as usize)
            .map_or("", |module| dir_of(self.text.get(module.path)));
        let near: Vec<u32> = hits
            .iter()
            .copied()
            .filter(|&hit| {
                self.nodes
                    .get(hit as usize)
                    .is_some_and(|node| dir_of(self.text.get(node.path)) == here)
            })
            .collect();
        if near.len() == 1 {
            return near;
        }
        if !receiver {
            return Vec::new();
        }
        let methods: Vec<u32> = hits
            .iter()
            .copied()
            .filter(|&hit| {
                self.nodes
                    .get(hit as usize)
                    .is_some_and(|node| node.kind == "method")
            })
            .collect();
        // Object-literal members are methods too, but carry no owner prefix; a class method (`Owner::name`) outranks them.
        let owned: Vec<u32> = methods
            .iter()
            .copied()
            .filter(|&hit| {
                self.nodes
                    .get(hit as usize)
                    .is_some_and(|node| node.name.contains("::"))
            })
            .collect();
        match (methods.as_slice(), owned.as_slice()) {
            ([only], _) | (_, [only]) => vec![*only],
            _ => Vec::new(),
        }
    }

    /// True when a call qualified by `named` (its last qualifier segment) can see candidate `hit`; see [`Self::guess`].
    fn visible_to(&self, caller: &Caller<'_>, named: &str, hit: u32) -> bool {
        if self.file_of(hit) == caller.file.first {
            return true;
        }
        let Some(node) = self.nodes.get(hit as usize) else {
            return false;
        };
        if let Some((owner, _)) = node.name.rsplit_once("::") {
            let owner = last_segment(owner);
            return owner == named || caller.visible.contains(owner);
        }
        let path = self.text.get(node.path);
        let stem = path.rsplit('/').next().map_or("", |base| {
            base.split_once('.').map_or(base, |(stem, _)| stem)
        });
        stem == named || last_segment_of_dir(path) == named
    }

    /// Module index of the file holding node `index`.
    fn file_of(&self, index: u32) -> u32 {
        let at = self.files.partition_point(|file| file.first <= index);
        at.checked_sub(1)
            .and_then(|at| self.files.get(at))
            .map_or(index, |file| file.first)
    }
}

/// Records an edge, keeping the strongest confidence for a pair.
fn note(found: &mut BTreeMap<(u32, bool, u32), Rank>, src: u32, call: bool, dst: u32, rank: Rank) {
    let slot = found.entry((src, call, dst)).or_insert(rank);
    *slot = (*slot).min(rank);
}

/// Last directory segment of a root-relative path (`a/b/c.go` is `b`).
fn last_segment_of_dir(path: &str) -> &str {
    dir_of(path).rsplit('/').next().unwrap_or("")
}

/// Directory part of a root-relative path.
fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}
