//! Read-only corpus discovery and the lexers.rs store acceptance helper.
//! Implements SPEC-MAP-MODE sections 5 and 6.2 for the ignored corpus sweep.
//! Uses only std filesystem reads and public parser/model records.
//! Kept separate from corpus_sweep's execution and reporting.

use dctx_map_model::{EdgeKind, NodeKind};
use dctx_map_parse::{Language, ParsedFile};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MAX_BYTES: u64 = 512 * 1024;
pub const MAX_FILES: usize = 20_000;
pub const MAX_FILES_PER_LANGUAGE: usize = 1_500;

pub struct CorpusFile {
    pub path: PathBuf,
    pub parser_path: String,
    pub language: Language,
}

fn excluded(path: &Path) -> bool {
    let mut in_node_modules = false;
    for component in path.components() {
        let name = component.as_os_str();
        if name == "target"
            || name == ".git"
            || name == "build"
            || name == "Pods"
            || (in_node_modules && name == ".cache")
        {
            return true;
        }
        in_node_modules = name == "node_modules";
    }
    false
}

pub fn collect_files(mut roots: Vec<PathBuf>) -> Vec<CorpusFile> {
    roots.sort();
    roots.dedup();
    let mut languages = BTreeMap::<String, Vec<CorpusFile>>::new();
    let mut visited = BTreeSet::new();
    for root in roots {
        match fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.is_dir() && !excluded(&root) => {}
            Ok(_) => {
                println!("skip root {root:?}: symlink, excluded path, or not a directory");
                continue;
            }
            Err(error) => {
                println!("skip root {root:?}: {error}");
                continue;
            }
        }
        println!("root {root:?}");
        // A sorted frontier gives lexical order without recursive directory walking.
        let mut pending = BTreeSet::from([root.clone()]);
        while let Some(path) = pending.pop_first() {
            if excluded(&path) || !visited.insert(path.clone()) {
                continue;
            }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    println!("skip {path:?}: {error}");
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                match fs::read_dir(&path) {
                    Ok(entries) => {
                        for entry in entries {
                            match entry {
                                Ok(entry) => {
                                    pending.insert(entry.path());
                                }
                                Err(error) => println!("skip entry in {path:?}: {error}"),
                            }
                        }
                    }
                    Err(error) => println!("skip directory {path:?}: {error}"),
                }
                continue;
            }
            if !metadata.is_file() || metadata.len() > MAX_BYTES || path.extension().is_none() {
                continue;
            }
            let relative = path.strip_prefix(&root).expect("walk stays under its root");
            let parser_path = relative.to_string_lossy().into_owned();
            let language = Language::for_path(&parser_path);
            if language != Language::Unknown {
                let files = languages.entry(format!("{language:?}")).or_default();
                if files.len() < MAX_FILES_PER_LANGUAGE {
                    files.push(CorpusFile {
                        parser_path,
                        path,
                        language,
                    });
                }
            }
        }
    }
    select_files(languages, MAX_FILES)
}

fn select_files(
    languages: BTreeMap<String, Vec<CorpusFile>>,
    total_limit: usize,
) -> Vec<CorpusFile> {
    // Discovery retains at most 1,500 candidates per language. Round-robin
    // selection keeps a late language represented even at the total safety cap.
    let mut languages = languages
        .into_values()
        .map(|files| files.into_iter().take(MAX_FILES_PER_LANGUAGE))
        .collect::<Vec<_>>();
    let mut files = Vec::new();
    'selection: loop {
        let before = files.len();
        for language in &mut languages {
            if files.len() == total_limit {
                println!("total file safety limit reached: {total_limit}");
                break 'selection;
            }
            if let Some(file) = language.next() {
                files.push(file);
            }
        }
        if files.len() == before {
            break;
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

pub fn read_source(path: &Path) -> Option<Vec<u8>> {
    let read = || -> std::io::Result<Option<Vec<u8>>> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            return Ok(None);
        }
        let mut source = Vec::new();
        // Bound the read as well as metadata: the corpus may change during a sweep.
        File::open(path)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut source)?;
        Ok((source.len() as u64 <= MAX_BYTES).then_some(source))
    };
    match read() {
        Ok(Some(source)) => Some(source),
        Ok(None) => {
            println!("skip {path:?}: no longer a regular file within {MAX_BYTES} bytes");
            None
        }
        Err(error) => {
            println!("skip {path:?}: {error}");
            None
        }
    }
}

/// Copied from tests/lexers.rs: store validation plus spans inside the file.
/// Place/Package duplication models the query indexer's inserted nodes.
pub fn assert_store_accepts(facts: &ParsedFile, source: &[u8]) {
    let file_end = source.iter().filter(|byte| **byte == b'\n').count() as u32 + 1;
    let mut nodes = facts.nodes.clone();
    let again = facts
        .nodes
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::Place | NodeKind::Package))
        .cloned()
        .collect::<Vec<_>>();
    nodes.extend(again);
    let mut ids = BTreeSet::new();
    assert!(!nodes.is_empty(), "missing file node");
    assert_eq!(
        nodes[0].kind,
        NodeKind::Module,
        "{}: first node is not a module",
        facts.path
    );
    for node in &nodes {
        let identity =
            node.id
                .as_str()
                .split_once(':')
                .map(|(_, rest)| match rest.split_once('#') {
                    Some((path, _)) => path,
                    None => rest,
                });
        assert_eq!(node.path, facts.path, "{}: wrong node path", node.name);
        assert_eq!(
            identity,
            Some(facts.path.as_str()),
            "{}: wrong identity path",
            node.id
        );
        assert!(
            node.id.as_str().starts_with(node.kind.letter()),
            "{}: wrong kind",
            node.id
        );
        assert!(
            node.lines[0] >= 1 && node.lines[1] >= node.lines[0],
            "{}: invalid span",
            node.name
        );
        assert!(
            node.lines[1] <= file_end,
            "{} spans past the file",
            node.name
        );
        assert!(
            ids.insert(node.id.as_str().to_owned()),
            "duplicate id {}",
            node.id
        );
    }
    let mut owned = BTreeSet::from([facts.nodes[0].id.as_str()]);
    loop {
        let before = owned.len();
        for edge in &facts.edges {
            assert_eq!(edge.owner_path, facts.path, "edge has wrong owner");
            assert!(edge.kind.permits(edge.conf), "edge has invalid confidence");
            assert!(
                ids.contains(edge.src.as_str()),
                "{}: missing edge source",
                edge.src
            );
            assert!(
                ids.contains(edge.dst.as_str()),
                "{}: missing edge destination",
                edge.dst
            );
            if edge.kind == EdgeKind::Contains && owned.contains(edge.src.as_str()) {
                owned.insert(edge.dst.as_str());
            }
        }
        if owned.len() == before {
            break;
        }
    }
    for node in &facts.nodes {
        assert!(
            owned.contains(node.id.as_str()),
            "{} is not owned by the file node",
            node.name
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> BTreeMap<String, Vec<CorpusFile>> {
        [
            ("Rust", "rs", MAX_FILES_PER_LANGUAGE + 1),
            ("Swift", "swift", 1),
        ]
        .into_iter()
        .map(|(language, extension, count)| {
            let files = (0..count)
                .map(|index| {
                    let parser_path = format!("{language}/{index:04}.{extension}");
                    CorpusFile {
                        path: PathBuf::from(&parser_path),
                        language: Language::for_path(&parser_path),
                        parser_path,
                    }
                })
                .collect();
            (language.to_owned(), files)
        })
        .collect()
    }

    #[test]
    fn language_caps_and_total_cap_preserve_later_languages() {
        let files = select_files(candidates(), MAX_FILES);
        assert_eq!(files.len(), MAX_FILES_PER_LANGUAGE + 1);
        assert_eq!(files.last().unwrap().language, Language::Swift);
        assert!(files.windows(2).all(|pair| pair[0].path < pair[1].path));

        let files = select_files(candidates(), 3);
        let paths = files
            .iter()
            .map(|file| file.parser_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, ["Rust/0000.rs", "Rust/0001.rs", "Swift/0000.swift"]);
        assert!(select_files(BTreeMap::new(), MAX_FILES).is_empty());
    }

    #[test]
    fn excluded_directories_are_pruned_by_component() {
        for directory in ["target", ".git", "build", "Pods", "node_modules/.cache"] {
            assert!(excluded(Path::new(&format!("root/{directory}/nested"))));
        }
        for directory in [".cache", "node_modules/package", "builder", "Podspecs"] {
            assert!(!excluded(Path::new(&format!("root/{directory}/nested"))));
        }
    }
}
