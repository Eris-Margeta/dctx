// SPDX-License-Identifier: Apache-2.0
//! Call and import edges for the facade index.
//! Implements SPEC-MAP-MODE 5.3 and 6.3 over the lexers' raw references, using the generation's own nodes.
//! The resolver in dctx-map-resolve is not a dependency of this crate, so the same lookup order lives here.
//! The walk records [`FileRefs`] for files with children or references, then [`link`] appends `calls` and `imports` edges once.

#[path = "imports/config.rs"]
pub(super) mod config;
#[path = "imports/documents.rs"]
mod documents;
#[path = "imports/go.rs"]
pub(super) mod go;
#[path = "imports/headers.rs"]
mod headers;
#[path = "imports/javascript.rs"]
mod javascript;
#[path = "imports/jvm.rs"]
mod jvm;
#[path = "imports/lua.rs"]
mod lua;
#[path = "imports/namespaces.rs"]
mod namespaces;
#[path = "imports/php.rs"]
mod php;
#[path = "imports/proto.rs"]
mod proto;
#[path = "imports/python.rs"]
mod python;
#[path = "imports/ruby.rs"]
mod ruby;
#[path = "imports/rust.rs"]
mod rust;
#[path = "imports/swift.rs"]
mod swift;

use std::collections::{BTreeMap, HashMap, HashSet};

use dctx_map_model::{Confidence, EdgeKind};

use super::decl_pair::is_declaration_only_path;
use super::extract::language_of;
use super::refs_names::{
    ImportBinding, call_segments, import_binding, import_segments, last_segment, module_keys,
    module_segments, relative_import,
};
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
    pub form: RefForm,
}

/// Form information used during linking. Inventory facts never become graph edges.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum RefForm {
    #[default]
    Ordinary,
    TypeOnly,
    ImportEquals,
    ExportAssignment,
    Require,
    Public,
    Package,
    GoModule,
    SwiftDependency,
    PythonAll,
    Parameter,
    RustModule {
        path: Option<String>,
    },
    QuotedHeader,
    AngleHeader,
    Load,
    RelativeLoad,
    Namespace,
    PhpFunction,
    PhpConst,
    PhpFile,
    PhpDirectory,
    Interop,
    Anchor,
    ProtoPublic,
    ProtoWeak,
    Lua(Vec<String>),
}

impl RawRef {
    pub(crate) fn fact(form: RefForm, name: &str, scope: &str) -> Self {
        Self {
            src: String::new(),
            kind: EdgeKind::Contains,
            name: name.to_owned(),
            scope: scope.to_owned(),
            alias: None,
            form,
        }
    }

    /// Keeps calls and imports only. Other relations yield `None`.
    pub(crate) fn new(
        src: &str,
        kind: EdgeKind,
        name: &str,
        scope: &str,
        alias: Option<&str>,
    ) -> Option<Self> {
        if !matches!(
            kind,
            EdgeKind::Calls | EdgeKind::Imports | EdgeKind::Reexports | EdgeKind::Documents
        ) {
            return None;
        }
        Some(Self {
            src: src.to_string(),
            kind,
            name: name.to_string(),
            scope: scope.to_string(),
            alias: alias.map(str::to_string),
            form: RefForm::Ordinary,
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

/// Language family used for symbol resolution and module suffix lookup.
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

/// The calling file as the resolver sees it.
struct Caller<'b> {
    file: &'b FileRefs,
    /// Language family of the file, see [`language_family`].
    family: &'b str,
    /// Names declared in this file that can own a method.
    declared: HashSet<&'b str>,
    bindings: Vec<Binding<'b>>,
}

/// A local name an import introduced, and what it points to.
struct Binding<'b> {
    scope: &'b str,
    name: String,
    dst: u32,
    module: bool,
}

impl Caller<'_> {
    /// The nearest enclosing import that introduces this name.
    fn binding(&self, scope: &str, name: &str) -> Option<&Binding<'_>> {
        self.bindings
            .iter()
            .filter(|binding| {
                binding.name == name
                    && (binding.scope.is_empty()
                        || scope == binding.scope
                        || scope
                            .strip_prefix(binding.scope)
                            .is_some_and(|tail| tail.starts_with("::")))
            })
            .max_by_key(|binding| binding.scope.len())
    }
}

/// Lookup tables over the generation, built once per [`link`].
#[derive(Clone, Copy)]
pub(super) struct ImportPaths<'a> {
    pub python: &'a str,
    pub includes: &'a [&'a str],
    pub ruby: &'a [&'a str],
    pub proto: &'a [&'a str],
    pub cwd: Option<&'a str>,
}

impl Default for ImportPaths<'_> {
    fn default() -> Self {
        Self {
            python: "",
            includes: &[],
            ruby: &[],
            proto: &[""],
            cwd: Some(""),
        }
    }
}

struct Tables<'a> {
    paths: ImportPaths<'a>,
    nodes: &'a [IndexedNode],
    text: &'a TextBlob,
    files: &'a [FileRefs],
    /// (file module index, full name) to symbols.
    names: HashMap<(u32, &'a str), Vec<u32>>,
    /// (language family, last name segment) to symbols.
    bare: HashMap<(&'a str, &'a str), Vec<u32>>,
    /// Module nodes by literal inventory path, including files without symbols or references.
    go: go::Packages<'a>,
    rust: rust::ModuleTree,
    module_paths: HashMap<&'a str, u32>,
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
    link_with_paths(nodes, edges, text, files, ImportPaths::default());
}

pub(super) fn link_with_paths(
    nodes: &[IndexedNode],
    edges: &mut Vec<IndexedEdge>,
    text: &mut TextBlob,
    files: &[FileRefs],
    paths: ImportPaths<'_>,
) {
    if files.iter().all(|file| file.refs.is_empty()) {
        return;
    }
    let calls = text.push(EdgeKind::Calls.as_str());
    let imports = text.push(EdgeKind::Imports.as_str());
    let documents = text.push(EdgeKind::Documents.as_str());
    let reexports = text.push(EdgeKind::Reexports.as_str());
    let exact = text.push(Confidence::Exact.as_str());
    let name = text.push(Confidence::Name.as_str());
    let guess = text.push(Confidence::Guess.as_str());
    let tables = Tables::build(nodes, text, files, paths);
    for file in files.iter().filter(|file| !file.refs.is_empty()) {
        let mut found = BTreeMap::new();
        tables.link_file(file, &mut found);
        for ((src, kind, dst), rank) in found {
            let edge = match kind {
                EdgeKind::Calls => calls,
                EdgeKind::Imports => imports,
                EdgeKind::Reexports => reexports,
                EdgeKind::Documents => documents,
                _ => continue,
            };
            let confidence = match rank {
                Confidence::Exact => exact,
                Confidence::Name => name,
                Confidence::Guess => guess,
            };
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
    fn build(
        nodes: &'a [IndexedNode],
        text: &'a TextBlob,
        files: &'a [FileRefs],
        paths: ImportPaths<'a>,
    ) -> Self {
        let mut names: HashMap<(u32, &str), Vec<u32>> = HashMap::new();
        let mut bare: HashMap<(&str, &str), Vec<u32>> = HashMap::new();
        let mut modules: HashMap<String, Vec<(u32, bool, &str)>> = HashMap::new();
        let mut module_paths = HashMap::new();
        for (index, node) in nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.letter == b'm')
        {
            let Ok(index) = u32::try_from(index) else {
                break;
            };
            let path = text.get(node.path);
            module_paths.insert(path, index);
            let family = language_family(language_of(path));
            for (key, full) in module_keys(path) {
                modules.entry(key).or_default().push((index, full, family));
            }
        }
        for file in files {
            let Some(module) = nodes.get(file.first as usize) else {
                continue;
            };
            let family = language_family(language_of(text.get(module.path)));
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
        let mut tables = Self {
            paths,
            go: go::Packages::default(),
            rust: rust::ModuleTree::default(),
            nodes,
            text,
            files,
            names,
            bare,
            module_paths,
            modules,
        };
        tables.go = go::Packages::build(&tables);
        tables.rust = rust::ModuleTree::build(&tables);
        tables
    }

    /// Resolves the imports of `file` first, then its calls through the bindings they made.
    fn link_file(&self, file: &FileRefs, found: &mut BTreeMap<(u32, EdgeKind, u32), Confidence>) {
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
        let Some(module) = self.nodes.get(file.first as usize) else {
            return;
        };
        let importer = self.text.get(module.path);
        let language = language_of(importer);
        let family = language_family(language);
        let receiver_only = RECEIVER_ONLY.contains(&language);
        let mut declared: HashSet<&str> = HashSet::new();
        for index in file.first.saturating_add(1)..file.end {
            if let Some(node) = self.nodes.get(index as usize) {
                declared.extend(node.name.split([':', '.']).filter(|part| !part.is_empty()));
            }
        }
        let mut caller = Caller {
            file,
            family,
            declared,
            bindings: Vec::new(),
        };
        for reference in file.refs.iter().filter(|reference| {
            matches!(
                reference.kind,
                EdgeKind::Imports | EdgeKind::Reexports | EdgeKind::Documents
            )
        }) {
            let Some(&src) = ids.get(&reference.src) else {
                continue;
            };
            if reference.kind == EdgeKind::Documents {
                if family == "markdown"
                    && let Some(dst) = self.document_target(importer, &reference.name)
                    && src != dst
                {
                    note(found, src, EdgeKind::Documents, dst, Confidence::Name);
                }
                continue;
            }
            if family == "swift" {
                self.swift_import(importer, src, reference, found);
                continue;
            }
            if family == "go" {
                self.go_import(&mut caller, importer, reference, src, found);
                continue;
            }
            let segments = if family == "javascript" {
                import_segments(&reference.name)
            } else {
                module_segments(&reference.name)
            };
            let (target, js_file) = if family == "javascript" {
                let Some((file, target)) = self.javascript_target(importer, reference) else {
                    continue;
                };
                (target, Some(file))
            } else {
                (
                    self.import_target(importer, family, reference, &segments),
                    None,
                )
            };
            // Literal paths can name data files. A member must belong to the importer's family.
            let target = target.and_then(|(dst, module)| {
                let path = self.text.get(self.nodes[dst as usize].path);
                let same_family = language_family(language_of(path)) == family;
                (module || same_family).then_some((dst, module, same_family))
            });
            let Some((dst, module, same_family)) = target else {
                // The path still proves the dependency when the clause yields no binding.
                if let Some(file) = js_file
                    && reference.kind == EdgeKind::Imports
                    && src != file
                {
                    note(found, src, EdgeKind::Imports, file, Confidence::Exact);
                }
                continue;
            };
            if dst == src {
                continue;
            }
            if !same_family {
                note(found, src, reference.kind, dst, Confidence::Exact);
                continue;
            }
            let rank = if reference.form == RefForm::Require && family == "javascript" {
                Confidence::Name
            } else {
                Confidence::Exact
            };
            note(found, src, reference.kind, dst, rank);
            if matches!(family, "c" | "ruby" | "config" | "proto" | "lua")
                || matches!(reference.form, RefForm::PhpFile | RefForm::PhpDirectory)
            {
                continue;
            }
            if family == "python" {
                self.python_bindings(&mut caller, reference, dst, module);
                continue;
            }
            if reference.kind == EdgeKind::Reexports || reference.form == RefForm::TypeOnly {
                continue;
            }
            let (dst, module) =
                if matches!(reference.form, RefForm::Require | RefForm::ImportEquals) && module {
                    self.js_assignment(dst).unwrap_or((dst, true))
                } else {
                    (dst, module)
                };
            let mut bind = |name: &str| {
                caller.bindings.push(Binding {
                    scope: &reference.scope,
                    name: name.to_owned(),
                    dst,
                    module,
                })
            };
            match import_binding(family, &reference.name, reference.alias.as_deref()) {
                ImportBinding::None => {}
                ImportBinding::Named(local) => bind(local),
                ImportBinding::Path => {
                    if let (Some(first), Some(last)) = (segments.first(), segments.last()) {
                        bind(last);
                        if module && first != last {
                            bind(first);
                        }
                    }
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
            let segments = call_segments(&reference.name, Some(language));
            let Some((callee, qualifier)) = segments.split_last() else {
                continue;
            };
            if family == "python"
                && self.python_shadowed(file, &reference.scope, qualifier.first().unwrap_or(callee))
            {
                continue;
            }
            // `interpolate(..)` inside the method `Captures::interpolate` is the imported free function, not the method itself.
            let free = receiver_only
                && reference
                    .name
                    .chars()
                    .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$');
            let (targets, rank) =
                self.call_targets(&caller, &reference.scope, qualifier, callee, free);
            for dst in targets {
                note(found, src, EdgeKind::Calls, dst, rank);
            }
        }
    }

    /// Module or symbol of `family` an import path names. `true` marks a module.
    fn import_target(
        &self,
        importer: &str,
        family: &str,
        reference: &RawRef,
        segments: &[String],
    ) -> Option<(u32, bool)> {
        match family {
            "config" => return self.config_target(importer, reference),
            "proto" => return self.proto_target(&reference.name),
            "lua" => return self.lua_target(reference),
            "jvm" => return self.jvm_target(importer, &reference.name),
            "csharp" => return self.csharp_target(reference),
            "c" => return self.header_target(importer, reference),
            "ruby" => return self.ruby_target(importer, reference),
            "php" => return self.php_target(importer, reference),
            "swift" => return None,
            _ => {}
        }
        if family == "rust" {
            return self.rust_target(importer, reference);
        }
        if family == "python" {
            return self.python_target(importer, &reference.name);
        }
        if let Some(module) = self.module_named(segments, family) {
            return Some((module, true));
        }
        let (symbol, path) = segments.split_last()?;
        let module = self.module_named(path, family)?;
        self.import_symbol(module, symbol)
    }

    fn facts(&self, module: u32) -> &[RawRef] {
        self.files
            .binary_search_by_key(&module, |f| f.first)
            .ok()
            .map_or(&[], |i| self.files[i].refs.as_slice())
    }

    fn import_symbol(&self, module: u32, symbol: &str) -> Option<(u32, bool)> {
        let hit = self.names.get(&(module, symbol))?;
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

    /// Targets of one call: lexical scope, then visible import bindings, then a project-wide guess.
    ///
    /// A `free` call (a bare name in a [`RECEIVER_ONLY`] language) skips every candidate of kind `method`.
    fn call_targets(
        &self,
        caller: &Caller<'_>,
        scope: &str,
        qualifier: &[String],
        callee: &str,
        free: bool,
    ) -> (Vec<u32>, Confidence) {
        let file = caller.file;
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
        let local = qualifier.first().map_or(callee, String::as_str);
        let binding = if caller.family == "python" && !qualifier.is_empty() {
            (1..=qualifier.len()).rev().find_map(|count| {
                caller
                    .binding(scope, &qualifier[..count].join("."))
                    .map(|binding| (binding, count))
            })
        } else {
            caller.binding(scope, local).map(|binding| (binding, 1))
        };
        if let Some((binding, consumed)) = binding {
            if qualifier.is_empty() {
                if !binding.module {
                    return (vec![binding.dst], Confidence::Name);
                }
            } else {
                let rest = qualifier[consumed..]
                    .iter()
                    .map(String::as_str)
                    .chain([callee])
                    .collect::<Vec<_>>()
                    .join("::");
                let (owner, name) = if binding.module {
                    (binding.dst, rest)
                } else {
                    let node = &self.nodes[binding.dst as usize];
                    (self.file_of(binding.dst), format!("{}::{rest}", node.name))
                };
                if caller.family == "go" && binding.module {
                    return self
                        .go_member(owner, &name)
                        .map_or((Vec::new(), Confidence::Guess), |dst| {
                            (vec![dst], Confidence::Name)
                        });
                }
                if caller.family == "javascript" && binding.module {
                    return self
                        .js_export(owner, &name, false, &mut Vec::new(), &mut 4096)
                        .map_or((Vec::new(), Confidence::Guess), |(dst, _)| {
                            (vec![dst], Confidence::Name)
                        });
                }
                if let Some(hit) = self.names.get(&(owner, name.as_str())) {
                    return Self::pick(hit);
                }
            }
            return (Vec::new(), Confidence::Guess);
        }
        if caller.family == "python"
            && file.refs.iter().any(|r| {
                r.kind == EdgeKind::Imports
                    && r.name.ends_with("::*")
                    && (r.scope.is_empty()
                        || scope == r.scope
                        || scope
                            .strip_prefix(&r.scope)
                            .is_some_and(|s| s.starts_with("::")))
            })
        {
            return (Vec::new(), Confidence::Guess);
        }
        if caller.family == "javascript"
            && caller.file.refs.iter().any(|r| {
                r.kind == EdgeKind::Imports
                    && matches!(
                        import_binding("javascript", &r.name, r.alias.as_deref()),
                        ImportBinding::Named(n) if n == local
                    )
            })
        {
            return (Vec::new(), Confidence::Guess);
        }
        (
            self.guess(caller, scope, callee, qualifier, free),
            Confidence::Guess,
        )
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
    fn pick(hit: &[u32]) -> (Vec<u32>, Confidence) {
        match hit {
            [only] => (vec![*only], Confidence::Name),
            many => (many.iter().copied().take(4).collect(), Confidence::Guess),
        }
    }

    /// The one project-wide symbol of the caller's language family with this last segment, preferring the caller's directory when several exist.
    ///
    /// A call with a receiver (`recv.name()`) that still has several candidates resolves to the one candidate of kind `method`, so a lone method wins over same-named free functions, and a lone owned class method (`Owner::name`) wins over object-literal members. Two or more of the same standing stay ambiguous and emit nothing.
    ///
    /// A call with a receiver or qualifier (`recv.name()`, `Type::name()`, `pkg.Name()`) then keeps its pick only if the
    /// caller can see it in the same file or can identify its owner by declaration, import or qualifier.
    /// Without types the receiver is unknown, so `iter.map(..)` or `text.lines()` never guesses its way to a same-named project method.
    fn guess(
        &self,
        caller: &Caller<'_>,
        scope: &str,
        callee: &str,
        qualifier: &[String],
        free: bool,
    ) -> Vec<u32> {
        let mut picked = self.pick_guess(caller.file, caller.family, callee, qualifier, free);
        if let Some(named) = qualifier.last() {
            picked.retain(|&hit| self.visible_to(caller, scope, named, hit));
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
    fn visible_to(&self, caller: &Caller<'_>, scope: &str, named: &str, hit: u32) -> bool {
        if self.file_of(hit) == caller.file.first {
            return true;
        }
        let Some(node) = self.nodes.get(hit as usize) else {
            return false;
        };
        if let Some((owner, _)) = node.name.rsplit_once("::") {
            let owner = last_segment(owner);
            return owner == named
                || caller.declared.contains(owner)
                || caller.binding(scope, owner).is_some();
        }
        false
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
fn note(
    found: &mut BTreeMap<(u32, EdgeKind, u32), Confidence>,
    src: u32,
    kind: EdgeKind,
    dst: u32,
    rank: Confidence,
) {
    if !kind.permits(rank) {
        return;
    }
    let slot = found.entry((src, kind, dst)).or_insert(rank);
    *slot = (*slot).min(rank);
}

/// Directory part of a root-relative path.
fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}
