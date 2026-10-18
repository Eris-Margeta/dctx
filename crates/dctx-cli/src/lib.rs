//! CLI application operations shared by the binary, TUI and integration tests.
//! Implements D3-LEGACY-REMOVAL §5 D3-04 without legacy core IO dependencies.
//! Uses the facade filesystem source, pure pack scan/diff/render and explicit compatibility IO.
//! The alias retains its separate registry, state and project exports; it never auto-cleans.

#![forbid(unsafe_code)]
#![allow(dead_code)]

pub mod cache;
pub mod compat;
pub mod help;
pub mod host;
pub mod pack;
pub mod template;
pub mod tui;

pub use cache::run_in;

use dctx::memory::CancelFlag;
use dctx_core::{
    budget::PackLimits,
    datatypes::{DirContextTreeNode, NodeType},
    diff::{self, DiffReport},
    filter::FilterConfig,
    version::Version,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

/// Settings shared by the alias and the interactive snapshot actions.
#[derive(Debug)]
pub struct SnapshotConfig {
    /// Directory to snapshot.
    pub target_path: PathBuf,
    /// Copy output instead of creating project text exports.
    pub clipboard_mode: bool,
    /// Prefer a diff when copying and a minor-version diff exists.
    pub diff_mode: bool,
    /// Omit content blocks in the main export.
    pub tree_only: bool,
    /// Start at V1 while retaining existing exports.
    pub force_reset: bool,
    /// Pure presentation filters; binary state is saved before filtering.
    pub filter_config: FilterConfig,
    /// Show ignored metadata without reading ignored content.
    pub debug_ignored: bool,
    /// Report selection diagnostics.
    pub debug_verbose: bool,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            target_path: PathBuf::from("."),
            clipboard_mode: false,
            diff_mode: false,
            tree_only: false,
            force_reset: false,
            filter_config: FilterConfig::default(),
            debug_ignored: false,
            debug_verbose: false,
        }
    }
}

/// The binary and TUI retain the alias's fatal setup-error behavior.
pub fn run_headless_mode(config: SnapshotConfig) {
    if let Err(error) = snapshot(&config) {
        eprintln!("❌ {error}");
        std::process::exit(1);
    }
}

/// Scan, version, save and export a legacy snapshot without starting plugins or Git.
pub fn snapshot(config: &SnapshotConfig) -> Result<(), String> {
    let started = Instant::now();
    let settings = compat::app_config();
    let root = fs::canonicalize(&config.target_path)
        .map_err(|error| format!("Error resolving path {:?}: {error}", config.target_path))?;
    let name = root.file_name().unwrap_or_default().to_string_lossy();
    let cache = compat::cache_dir().ok_or("Could not determine system cache directory.")?;
    let id =
        compat::project_id(&cache, &root).map_err(|error| format!("Failed to initialize project registry: {error}"))?;
    let binary = cache.join(format!("{id}.dctx"));
    let output = root.join(".dctx");
    if !config.clipboard_mode {
        fs::create_dir_all(&output).map_err(|error| format!("Failed to create output directory .dctx/: {error}"))?;
    }
    println!("🚀 dctx running on: {}\n🔑 Project Hash: {id}", root.display());
    if config.tree_only {
        println!("🌲 Tree-Only Mode Active");
    }
    let old = compat::load_state(&root, &binary, config.force_reset);
    let policy = compat::scan_policy(&root, config.debug_verbose)?;
    println!("📂 Scanning directory structure...");
    let source = dctx::FsSource::open(&root).map_err(|error| error.to_string())?;
    let scan = dctx_pack::scan(
        &source,
        &CancelFlag::new(),
        dctx_pack::ScanOptions {
            policy: &policy,
            debug_ignored: config.debug_ignored,
            limits: PackLimits::unbounded(),
        },
    )
    .map_err(|error| error.to_string())?;
    let mut tree = scan.tree;
    set_disk_paths(&mut tree, &root);
    let mut changes = DiffReport::default();
    let mut previous = Version::new_v1();
    let (version, major_reset) = if let Some((old_tree, old_version)) = old {
        changes = diff::compare_trees(Some(&old_tree), &tree);
        previous = old_version;
        if changes.has_changes {
            previous.calculate_next(
                changes.stats.files_unchanged,
                changes.stats.total_files_in_new_state,
                settings.max_diff_files,
            )
        } else {
            (previous.clone(), false)
        }
    } else {
        (Version::new_v1(), true)
    };
    if changes.has_changes {
        println!(
            "📊 Changes detected: +{} -{} ~{} (Version: {} -> {})",
            changes.stats.files_added, changes.stats.files_removed, changes.stats.files_modified, previous, version
        );
    } else {
        println!("💤 No changes detected. Version remains {version}.");
    }
    match compat::save_state(&binary, &tree, &version) {
        Ok(()) => println!("💾 Saved binary state to cache."),
        Err(error) => eprintln!("❌ Failed to save binary state: {error}"),
    }
    let Some(filtered) = filter_tree(&tree, &config.filter_config) else {
        println!("⚠️ All files filtered out! No output will be generated.");
        return Ok(());
    };
    if settings.text_output_enabled {
        let markers = pack::legacy_markers(&filtered, &compat::output_policy(&root));
        let can_diff = changes.has_changes && !major_reset;
        let diff_only = config.clipboard_mode && config.diff_mode && can_diff;
        let mut body = Vec::new();
        if !diff_only {
            match template::legacy_template(&settings.default_template) {
                Ok(mut template) => {
                    template.insert_variable("project", &name);
                    body = dctx_template::render_legacy_snapshot(
                        &template,
                        &markers,
                        &version,
                        config.tree_only,
                        settings.omit_svg_content,
                    )
                    .into_bytes();
                }
                Err(error) => eprintln!("❌ Generation error: {error}"),
            }
        }
        let generate_diff = can_diff
            && if config.clipboard_mode { config.diff_mode } else { settings.text_output_mode_generates_diff_files };
        let diff_body = if generate_diff {
            match template::legacy_template(&settings.default_template) {
                Ok(template) => Some(
                    dctx_template::render_legacy_diff(
                        &template,
                        &markers,
                        &changes,
                        &previous,
                        &version,
                        settings.omit_svg_content,
                    )
                    .into_bytes(),
                ),
                Err(error) => {
                    eprintln!("❌ Diff generation error: {error}");
                    None
                }
            }
        } else {
            None
        };
        if config.clipboard_mode {
            if config.diff_mode && diff_body.is_none() && !diff_only {
                println!("⚠️ No changes detected. Copying FULL context instead of diff.");
            }
            let bytes = if diff_only { diff_body.as_deref().unwrap_or_default() } else { &body };
            match compat::copy_to_clipboard(bytes) {
                Ok(()) => println!("✨ Copied to clipboard!\nLegacy compatibility alias copied {} bytes and wrote no project text file. Explicit pack: dctx pack", bytes.len()),
                Err(error) => eprintln!("❌ Clipboard error: {error}"),
            }
        } else {
            compat::export(&output.join(format!("{name}.llmcontext.txt")), &body, false);
            if let Some(bytes) = diff_body {
                compat::export(&output.join(format!("{name}.llmcontext-{version}-diff.txt")), &bytes, true);
            }
        }
    }
    println!("✨ Done in {:.2?}", started.elapsed());
    Ok(())
}

fn set_disk_paths(node: &mut DirContextTreeNode, root: &Path) {
    node.disk_path = root.join(&node.relative_path);
    for child in &mut node.children {
        set_disk_paths(child, root);
    }
}

fn filter_tree(tree: &DirContextTreeNode, filter: &FilterConfig) -> Option<DirContextTreeNode> {
    if !filter.has_active_filters() {
        return Some(tree.clone());
    }
    let name = tree.disk_path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    if tree.node_type == NodeType::File {
        return filter
            .should_include_file(name, tree.content_size, tree.is_binary_file, &tree.disk_path)
            .then(|| tree.clone());
    }
    if !tree.relative_path.is_empty() && !filter.should_include_directory(name) {
        return None;
    }
    let children: Vec<_> = tree.children.iter().filter_map(|child| filter_tree(child, filter)).collect();
    if children.is_empty() && !tree.relative_path.is_empty() {
        return None;
    }
    let mut filtered = tree.clone();
    filtered.children = children;
    Some(filtered)
}
