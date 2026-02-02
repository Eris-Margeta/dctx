// crates/dctx-cli/src/main.rs

mod platform;
mod tui;

use dctx_core::config;
use dctx_core::diff::{self, DiffReport};
use dctx_core::filter::FilterConfig;
use dctx_core::git_safety;
use dctx_core::ignore;
use dctx_core::llm_formatter;
use dctx_core::registry::ProjectRegistry;
use dctx_core::state::{self, StateLoadResult};
use dctx_core::template::{CompiledTemplate, Template};
use dctx_core::version::Version;
use dctx_core::walker;
use dctx_core::writer;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

// --- Config Structs ---

/// Represents the different commands/modes the CLI can run in.
#[derive(Debug)]
enum CliCommand {
    /// Standard snapshot operation (default)
    Snapshot(SnapshotConfig),
    /// Template subcommand
    Template(TemplateSubcommand),
    /// Show help
    Help,
    /// Show version
    Version,
}

#[derive(Debug)]
enum TemplateSubcommand {
    /// Build/compile a template file
    Build { template_path: PathBuf },
    /// List available compiled templates
    List,
}

#[derive(Debug)]
struct SnapshotConfig {
    target_path: PathBuf,
    clipboard_mode: bool,
    diff_mode: bool,
    tree_only: bool,
    force_reset: bool,
    // Filter options
    filter_extensions: Vec<String>,
    include_binary: bool,
    only_empty: bool,
    only_non_empty: bool,
    min_size_mb: Option<f64>,
    max_size_mb: Option<f64>,
    only_hidden: bool,
    debug_ignored: bool,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            target_path: PathBuf::from("."),
            clipboard_mode: false,
            diff_mode: false,
            tree_only: false,
            force_reset: false,
            filter_extensions: Vec::new(),
            include_binary: false,
            only_empty: false,
            only_non_empty: false,
            min_size_mb: None,
            max_size_mb: None,
            only_hidden: false,
            debug_ignored: false,
        }
    }
}

// CliConfig wraps SnapshotConfig for internal use
#[derive(Debug)]
struct CliConfig {
    target_path: PathBuf,
    clipboard_mode: bool,
    diff_mode: bool,
    tree_only: bool,
    force_reset: bool,
    filter_config: FilterConfig,
    debug_ignored: bool,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            target_path: PathBuf::from("."),
            clipboard_mode: false,
            diff_mode: false,
            tree_only: false,
            force_reset: false,
            filter_config: FilterConfig::default(),
            debug_ignored: false,
        }
    }
}

impl From<SnapshotConfig> for CliConfig {
    fn from(sc: SnapshotConfig) -> Self {
        let mut filter_config = FilterConfig::new();

        if !sc.filter_extensions.is_empty() {
            filter_config = filter_config.with_extensions(sc.filter_extensions);
        }
        if sc.include_binary {
            filter_config = filter_config.with_include_binary(true);
        }
        if sc.only_empty {
            filter_config = filter_config.with_only_empty(true);
        }
        if sc.only_non_empty {
            filter_config = filter_config.with_only_non_empty(true);
        }
        if let Some(min_mb) = sc.min_size_mb {
            filter_config = filter_config.with_min_size_mb(min_mb);
        }
        if let Some(max_mb) = sc.max_size_mb {
            filter_config = filter_config.with_max_size_mb(max_mb);
        }
        if sc.only_hidden {
            filter_config = filter_config.with_only_hidden(true);
        }
        if sc.debug_ignored {
            filter_config = filter_config.with_collect_ignored(true);
        }

        Self {
            target_path: sc.target_path,
            clipboard_mode: sc.clipboard_mode,
            diff_mode: sc.diff_mode,
            tree_only: sc.tree_only,
            force_reset: sc.force_reset,
            filter_config,
            debug_ignored: sc.debug_ignored,
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 1 {
        run_interactive_mode();
    } else {
        match parse_command(&args) {
            Ok(cmd) => match cmd {
                CliCommand::Help => print_help(),
                CliCommand::Version => print_version(),
                CliCommand::Snapshot(config) => run_headless_mode(config.into()),
                CliCommand::Template(subcmd) => run_template_command(subcmd),
            },
            Err(e) => {
                eprintln!("❌ Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

/// Parses command line arguments into a CliCommand.
fn parse_command(args: &[String]) -> Result<CliCommand, String> {
    let args_slice = &args[1..]; // Skip program name

    if args_slice.is_empty() {
        return Ok(CliCommand::Snapshot(SnapshotConfig::default()));
    }

    // Check for subcommands first
    match args_slice[0].as_str() {
        "template" => parse_template_subcommand(&args_slice[1..]),
        "-h" | "--help" | "help" => Ok(CliCommand::Help),
        "-v" | "--version" | "version" => Ok(CliCommand::Version),
        _ => parse_snapshot_args(args_slice),
    }
}

/// Parses template subcommand arguments.
fn parse_template_subcommand(args: &[String]) -> Result<CliCommand, String> {
    if args.is_empty() {
        return Err("template subcommand requires an action: build, list".to_string());
    }

    match args[0].as_str() {
        "build" => {
            if args.len() < 2 {
                return Err("template build requires a path to a .dctx_template file".to_string());
            }
            Ok(CliCommand::Template(TemplateSubcommand::Build {
                template_path: PathBuf::from(&args[1]),
            }))
        }
        "list" => Ok(CliCommand::Template(TemplateSubcommand::List)),
        other => Err(format!(
            "Unknown template action: {}. Use: build, list",
            other
        )),
    }
}

/// Parses standard snapshot arguments.
fn parse_snapshot_args(args: &[String]) -> Result<CliCommand, String> {
    let mut config = SnapshotConfig::default();
    let mut path_set = false;
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        match arg.as_str() {
            // Basic flags
            "-c" | "--clipboard" => config.clipboard_mode = true,
            "-d" | "--diff" => config.diff_mode = true,
            "-t" | "--tree" => config.tree_only = true,
            "-r" | "--reset" => config.force_reset = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            "-v" | "--version" => return Ok(CliCommand::Version),

            // Filter flags (no arguments)
            "--binary" => config.include_binary = true,
            "--empty" => config.only_empty = true,
            "--non-empty" => config.only_non_empty = true,
            "--hidden" => config.only_hidden = true,
            "--debug-ignored" => config.debug_ignored = true,

            // Filter flags (with arguments)
            "--ext" => {
                i += 1;
                if i >= args.len() {
                    return Err("--ext requires a comma-separated list of extensions".to_string());
                }
                let extensions_str = &args[i];
                config.filter_extensions = extensions_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if config.filter_extensions.is_empty() {
                    return Err("--ext requires at least one extension".to_string());
                }
            }
            "--large" => {
                i += 1;
                if i >= args.len() {
                    return Err(
                        "--large requires a size in megabytes (e.g., --large 1)".to_string()
                    );
                }
                let size_str = &args[i];
                config.min_size_mb = Some(
                    size_str
                        .parse::<f64>()
                        .map_err(|_| format!("Invalid size for --large: {}", size_str))?,
                );
            }
            "--small" => {
                i += 1;
                if i >= args.len() {
                    return Err(
                        "--small requires a size in megabytes (e.g., --small 0.5)".to_string()
                    );
                }
                let size_str = &args[i];
                config.max_size_mb = Some(
                    size_str
                        .parse::<f64>()
                        .map_err(|_| format!("Invalid size for --small: {}", size_str))?,
                );
            }

            // Target path or unknown flag
            val => {
                if val.starts_with('-') {
                    return Err(format!("Unknown flag: {}", val));
                }
                if path_set {
                    return Err("Multiple target paths provided.".to_string());
                }
                config.target_path = PathBuf::from(val);
                path_set = true;
            }
        }

        i += 1;
    }

    // Validate filter combinations
    if config.only_empty && config.only_non_empty {
        return Err("Cannot use both --empty and --non-empty flags".to_string());
    }

    Ok(CliCommand::Snapshot(config))
}

fn run_interactive_mode() {
    // Get current directory for context
    let current_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .display()
        .to_string();

    // Try to get last version from cache
    let last_version = get_last_version_from_cache(&current_dir);

    let context = if let Some(version) = last_version {
        tui::MenuContext::new(current_dir).with_version(version)
    } else {
        tui::MenuContext::new(current_dir)
    };

    // Main menu loop
    loop {
        let menu = tui::create_main_menu(&context);
        match menu.show() {
            Ok('1') => {
                // Snapshot current directory
                run_headless_mode(CliConfig::default());
                let _ = tui::show_message("Snapshot complete! Check .dctx/ directory.");
            }
            Ok('2') => {
                // Snapshot to clipboard
                run_headless_mode(CliConfig {
                    clipboard_mode: true,
                    ..Default::default()
                });
                let _ = tui::show_message("Snapshot copied to clipboard!");
            }
            Ok('3') => {
                // Tree-only mode
                run_headless_mode(CliConfig {
                    tree_only: true,
                    ..Default::default()
                });
                let _ = tui::show_message("Tree-only snapshot complete!");
            }
            Ok('4') => {
                // Diff mode
                run_headless_mode(CliConfig {
                    diff_mode: true,
                    clipboard_mode: true,
                    ..Default::default()
                });
                let _ = tui::show_message("Diff copied to clipboard!");
            }
            Ok('5') => {
                // Advanced options
                run_advanced_menu();
            }
            Ok('0') | Ok('q') => {
                // Exit
                println!("Goodbye!");
                std::process::exit(0);
            }
            Ok(_) => {
                let _ = tui::show_error("Invalid selection. Try again.");
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn run_advanced_menu() {
    loop {
        let menu = tui::create_advanced_menu();
        match menu.show() {
            Ok('1') => {
                // Filters submenu
                run_filter_menu();
            }
            Ok('2') => {
                // Templates submenu
                run_template_menu();
            }
            Ok('3') => {
                // Cache & Debug submenu
                run_cache_menu();
            }
            Ok('0') => {
                // Back to main menu
                return;
            }
            Ok(_) => {
                let _ = tui::show_error("Invalid selection. Try again.");
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                return;
            }
        }
    }
}

fn run_filter_menu() {
    let mut filter_config = FilterConfig::default();

    loop {
        let menu = tui::create_filter_menu();
        match menu.show() {
            Ok('1') => {
                // Filter by extension
                match tui::get_input("Enter extensions (comma-separated, e.g., rs,toml):") {
                    Ok(input) if !input.is_empty() => {
                        let extensions: Vec<String> = input
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();

                        if !extensions.is_empty() {
                            filter_config = filter_config.with_extensions(extensions.clone());
                            let _ = tui::show_message(&format!(
                                "Extension filter set: {}",
                                extensions.join(", ")
                            ));
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tui::show_error(&format!("Error: {}", e));
                    }
                }
            }
            Ok('2') => {
                // Filter by size
                match tui::get_input("Large (L) or Small (S) files?") {
                    Ok(input) => {
                        let choice = input.to_lowercase();
                        if choice == "l" || choice == "large" {
                            match tui::get_input("Minimum size in MB:") {
                                Ok(size_str) => {
                                    if let Ok(size) = size_str.parse::<f64>() {
                                        filter_config = filter_config.with_min_size_mb(size);
                                        let _ = tui::show_message(&format!(
                                            "Large file filter set: >{:.2}MB",
                                            size
                                        ));
                                    } else {
                                        let _ = tui::show_error("Invalid size number.");
                                    }
                                }
                                Err(e) => {
                                    let _ = tui::show_error(&format!("Error: {}", e));
                                }
                            }
                        } else if choice == "s" || choice == "small" {
                            match tui::get_input("Maximum size in MB:") {
                                Ok(size_str) => {
                                    if let Ok(size) = size_str.parse::<f64>() {
                                        filter_config = filter_config.with_max_size_mb(size);
                                        let _ = tui::show_message(&format!(
                                            "Small file filter set: <{:.2}MB",
                                            size
                                        ));
                                    } else {
                                        let _ = tui::show_error("Invalid size number.");
                                    }
                                }
                                Err(e) => {
                                    let _ = tui::show_error(&format!("Error: {}", e));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tui::show_error(&format!("Error: {}", e));
                    }
                }
            }
            Ok('3') => {
                // Content filters
                match tui::get_input("Empty (E) or Non-Empty (N) files?") {
                    Ok(input) => {
                        let choice = input.to_lowercase();
                        if choice == "e" || choice == "empty" {
                            filter_config = filter_config.with_only_empty(true);
                            let _ = tui::show_message("Empty files filter set.");
                        } else if choice == "n" || choice == "non-empty" {
                            filter_config = filter_config.with_only_non_empty(true);
                            let _ = tui::show_message("Non-empty files filter set.");
                        }
                    }
                    Err(e) => {
                        let _ = tui::show_error(&format!("Error: {}", e));
                    }
                }
            }
            Ok('4') => {
                // Hidden files only
                filter_config = filter_config.with_only_hidden(true);
                let _ = tui::show_message("Hidden files only filter set.");
            }
            Ok('5') => {
                // Include binary files
                filter_config = filter_config.with_include_binary(true);
                let _ = tui::show_message("Binary files will be included.");
            }
            Ok('9') => {
                // Clear all filters
                filter_config = FilterConfig::default();
                let _ = tui::show_message("All filters cleared.");
            }
            Ok('0') => {
                // Apply filters and go back
                if filter_config.has_active_filters() {
                    run_headless_mode(CliConfig {
                        filter_config,
                        ..Default::default()
                    });
                    let _ = tui::show_message("Filtered snapshot generated!");
                }
                return;
            }
            Ok(_) => {
                let _ = tui::show_error("Invalid selection. Try again.");
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                return;
            }
        }
    }
}

fn run_template_menu() {
    loop {
        let menu = tui::create_template_menu();
        match menu.show() {
            Ok('1') => {
                let _ = tui::show_message("Already using default template.");
            }
            Ok('2') => {
                // List templates
                run_template_command(TemplateSubcommand::List);
                let _ = tui::show_message("Press Enter to continue...");
            }
            Ok('3') => {
                // Build template
                match tui::get_input("Enter template file path:") {
                    Ok(path) if !path.is_empty() => {
                        run_template_command(TemplateSubcommand::Build {
                            template_path: PathBuf::from(path),
                        });
                        let _ = tui::show_message("Press Enter to continue...");
                    }
                    Ok(_) => {
                        let _ = tui::show_error("No path provided.");
                    }
                    Err(e) => {
                        let _ = tui::show_error(&format!("Error: {}", e));
                    }
                }
            }
            Ok('0') => {
                // Back
                return;
            }
            Ok(_) => {
                let _ = tui::show_error("Invalid selection. Try again.");
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                return;
            }
        }
    }
}

fn run_cache_menu() {
    loop {
        let menu = tui::create_cache_menu();
        match menu.show() {
            Ok('1') => {
                // Show ignored folders
                run_headless_mode(CliConfig {
                    debug_ignored: true,
                    tree_only: true,
                    ..Default::default()
                });
                let _ = tui::show_message("Check output above for ignored folders.");
            }
            Ok('2') => {
                // Reset project state
                run_headless_mode(CliConfig {
                    force_reset: true,
                    ..Default::default()
                });
                let _ = tui::show_message("Project state reset to V1.");
            }
            Ok('3') => {
                // Clear global cache
                match config::get_cache_dir() {
                    Some(cache_dir) => {
                        if cache_dir.exists() {
                            match fs::remove_dir_all(&cache_dir) {
                                Ok(_) => {
                                    let _ = tui::show_message("Global cache cleared successfully.");
                                }
                                Err(e) => {
                                    let _ =
                                        tui::show_error(&format!("Failed to clear cache: {}", e));
                                }
                            }
                        } else {
                            let _ = tui::show_message("Cache directory does not exist.");
                        }
                    }
                    None => {
                        let _ = tui::show_error("Could not determine cache directory.");
                    }
                }
            }
            Ok('0') => {
                // Back
                return;
            }
            Ok(_) => {
                let _ = tui::show_error("Invalid selection. Try again.");
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                return;
            }
        }
    }
}

fn get_last_version_from_cache(current_dir: &str) -> Option<String> {
    // Try to load version from cached state
    let target_dir = match std::fs::canonicalize(current_dir) {
        Ok(p) => p,
        Err(_) => return None,
    };

    let mut registry = ProjectRegistry::new().ok()?;
    let project_hash = registry.get_or_create_id(&target_dir).ok()?;

    let cache_dir = config::get_cache_dir()?;
    let bin_path = cache_dir.join(format!("{}.dctx", project_hash));

    if !bin_path.exists() {
        return None;
    }

    // Read version from binary file
    use dctx_core::reader::read_dircontxt_file;
    match read_dircontxt_file(&bin_path) {
        Ok((_tree, version)) => Some(version.to_string()),
        Err(_) => None,
    }
}

// --- HEADLESS MODE ---

fn run_headless_mode(cli_config: CliConfig) {
    let start_time = Instant::now();
    let app_config = config::load_app_config();

    // 1. Resolve Absolute Path (Canonicalize)
    let target_dir = match std::fs::canonicalize(&cli_config.target_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "❌ Error resolving path '{:?}': {}",
                cli_config.target_path, e
            );
            std::process::exit(1);
        }
    };

    let project_name = target_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // 2. Registry & Cache Lookup
    let mut registry = match ProjectRegistry::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("❌ Failed to initialize project registry: {}", e);
            std::process::exit(1);
        }
    };

    let project_hash = match registry.get_or_create_id(&target_dir) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("❌ Failed to get project ID from registry: {}", e);
            std::process::exit(1);
        }
    };

    let cache_dir = config::get_cache_dir().unwrap_or_else(|| {
        eprintln!("❌ Could not determine system cache directory.");
        std::process::exit(1);
    });

    if let Err(e) = fs::create_dir_all(&cache_dir) {
        eprintln!("❌ Failed to create cache directory {:?}: {}", cache_dir, e);
        std::process::exit(1);
    }

    let bin_filename = format!("{}.dctx", project_hash);
    let bin_path = cache_dir.join(&bin_filename);

    // 3. Output Directory Setup
    let local_output_dir = target_dir.join(".dctx");
    if !cli_config.clipboard_mode {
        if let Err(e) = fs::create_dir_all(&local_output_dir) {
            eprintln!("❌ Failed to create output directory .dctx/: {}", e);
            std::process::exit(1);
        }
        if app_config.update_local_git_repo_gitignore_rules {
            if let Err(e) = git_safety::ensure_local_gitignore_is_safe(&target_dir) {
                eprintln!("⚠️  Warning: Git safety check failed: {}", e);
            }
        }
    }

    // Always print startup info
    println!("🚀 dctx running on: {}", target_dir.display());
    println!("🔑 Project Hash: {}", project_hash);
    if cli_config.tree_only {
        println!("🌲 Tree-Only Mode Active");
    }

    // 4. Load or Reset State
    let mut old_tree_opt = None;
    let mut old_version = Version::new_v1();

    if !cli_config.force_reset && bin_path.exists() {
        println!("📝 Found existing binary state at {:?}", bin_path);
        match state::load_or_reset_state(
            &target_dir,
            &bin_path,
            Some(&local_output_dir),
            &project_name,
        ) {
            StateLoadResult::Loaded(loaded_root, loaded_version) => {
                println!("✅ State Loaded: {}", loaded_version);
                old_tree_opt = Some(loaded_root);
                old_version = loaded_version;
            }
            StateLoadResult::FreshV1 => {
                println!("⚠️ State load resulted in FreshV1 (Reset)");
            }
        }
    } else {
        if !bin_path.exists() {
            println!("ℹ️ No existing binary state found (Fresh Start).");
        }
        if cli_config.force_reset {
            println!("🔄 Force Reset enabled.");
        }
    }

    // 5. Scan Directory
    let ignore_rules = ignore::load_ignore_rules(&target_dir);
    println!("📂 Scanning directory structure...");

    let mut ignored_folders_debug = Vec::new();
    let mut new_tree = if cli_config.debug_ignored {
        match walker::walk_directory_structure_with_debug(
            &target_dir,
            &ignore_rules,
            &mut ignored_folders_debug,
            true,
        ) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("❌ Error walking directory: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        match walker::walk_directory_structure(&target_dir, &ignore_rules, false) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("❌ Error walking directory: {}", e);
                std::process::exit(1);
            }
        }
    };

    // 6. Populate Content
    walker::populate_file_contents_parallel(&mut new_tree);

    // 6.5. Display Ignored Folders (Debug)
    if cli_config.debug_ignored {
        println!();
        println!("🔍 Ignored Folders (via .gitignore / .dctxignore):");
        if ignored_folders_debug.is_empty() {
            println!("   (none)");
        } else {
            for folder in &ignored_folders_debug {
                println!("   - {}", folder);
            }
        }
        println!();
    }

    // 7. Diff & Versioning
    let mut diff_report = DiffReport::default();
    let (new_version, is_major_reset) = if let Some(old_tree) = &old_tree_opt {
        diff_report = diff::compare_trees(Some(old_tree), &new_tree);
        if diff_report.has_changes {
            let (v, reset) = old_version.calculate_next(
                diff_report.stats.files_unchanged,
                diff_report.stats.total_files_in_new_state,
                app_config.max_diff_files,
            );
            (v, reset)
        } else {
            (old_version.clone(), false)
        }
    } else {
        (Version::new_v1(), true)
    };

    // Always print stats
    if diff_report.has_changes {
        println!(
            "📊 Changes detected: +{} -{} ~{} (Version: {} -> {})",
            diff_report.stats.files_added,
            diff_report.stats.files_removed,
            diff_report.stats.files_modified,
            old_version,
            new_version
        );
    } else {
        println!("💤 No changes detected. Version remains {}.", new_version);
    }

    // --- CLEANUP (ADR-002) ---
    if is_major_reset && !cli_config.clipboard_mode && local_output_dir.exists() {
        let diff_suffix = "-diff.txt";
        if let Ok(entries) = fs::read_dir(&local_output_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.ends_with(diff_suffix) {
                        let _ = fs::remove_file(path);
                    }
                }
            }
        }
    }

    // 8. Write Binary State
    match writer::write_dircontxt_file(&bin_path, &mut new_tree, &new_version) {
        Ok(_) => {
            println!("💾 Saved binary state to cache.");
        }
        Err(e) => eprintln!("❌ Failed to save binary state: {}", e),
    }

    // 8.5. Apply Filters (if active)
    let tree_for_output = if cli_config.filter_config.has_active_filters() {
        println!("🔍 Applying filters...");
        match walker::filter_tree(&new_tree, &cli_config.filter_config) {
            Some(filtered) => {
                println!("✅ Filters applied (tree pruned)");
                filtered
            }
            None => {
                println!("⚠️ All files filtered out! No output will be generated.");
                return;
            }
        }
    } else {
        new_tree.clone()
    };

    // 9. Generate Output
    // Binary state is ALWAYS written to cache (done above).
    // Text output to .dctx/ is controlled by config.
    let should_generate_output = app_config.text_output_enabled;

    if should_generate_output {
        let mut output_buffer = Vec::new();
        let mut diff_buffer = Vec::new();

        // If user asked for -d, they want diff if available.
        // If config says generate diffs, we generate diffs if changed.
        let mut writing_diff_only = cli_config.clipboard_mode
            && cli_config.diff_mode
            && diff_report.has_changes
            && !is_major_reset;

        // Generate Main Context (Always, unless clipboard diff-only requested)
        if !writing_diff_only {
            if let Err(e) = llm_formatter::generate_llm_context_with_template(
                &mut output_buffer,
                &tree_for_output,
                &new_version,
                &project_name,
                cli_config.tree_only,
                &app_config.default_template,
                &target_dir,
            ) {
                eprintln!("❌ Generation error: {}", e);
            }
        }

        // Generate Diff Context (Logic Fix: Force diff gen if clipboard requested it)
        let mut diff_generated = false;

        // Is it possible to generate a diff?
        let can_generate_diff = diff_report.has_changes && !is_major_reset;

        // Should we?
        // 1. If Writing to Disk: check config.
        // 2. If Clipboard: check if -d flag passed.

        let should_generate_diff = if cli_config.clipboard_mode {
            // In clipboard mode, we generate diff IF user passed -d AND it's possible.
            // If they didn't pass -d, we don't need the diff buffer (we send main ctx).
            cli_config.diff_mode && can_generate_diff
        } else {
            // In File mode, check config
            can_generate_diff && app_config.text_output_mode_generates_diff_files
        };

        if should_generate_diff {
            if let Err(e) = llm_formatter::generate_diff_context_with_template(
                &mut diff_buffer,
                &tree_for_output,
                &diff_report,
                &old_version,
                &new_version,
                &app_config.default_template,
                &target_dir,
            ) {
                eprintln!("❌ Diff generation error: {}", e);
            } else {
                diff_generated = true;
                // If we successfully generated a diff for clipboard, ensure we use it
                if cli_config.clipboard_mode && cli_config.diff_mode {
                    writing_diff_only = true;
                }
            }
        }

        // --- WRITING / COPYING ---
        if cli_config.clipboard_mode {
            let content_to_copy = if writing_diff_only {
                // If user specifically asked for diff (-d) and we have one
                diff_buffer
            } else {
                // Fallback to main context if no diff (e.g. V1, or no changes)
                // If user asked for diff but there were no changes, this sends Main Context V_old.
                // We might want to warn the user? "No changes detected, copying full context."
                if cli_config.diff_mode && !diff_generated {
                    println!("⚠️ No changes detected. Copying FULL context instead of diff.");
                }
                output_buffer
            };

            let content_str = String::from_utf8_lossy(&content_to_copy);
            match platform::copy_to_clipboard(&content_str) {
                Ok(_) => println!("✨ Copied to clipboard!"),
                Err(e) => eprintln!("❌ Clipboard error: {}", e),
            }
        } else {
            // Write Main Context
            let txt_filename = format!("{}.llmcontext.txt", project_name);
            let txt_path = local_output_dir.join(&txt_filename);

            println!("📝 Writing main context: {:?}", txt_path);

            match std::fs::write(&txt_path, &output_buffer) {
                Ok(_) => println!("✅ Saved: .dctx/{}", txt_filename),
                Err(e) => eprintln!("❌ Write error for {}: {}", txt_filename, e),
            }

            // Write Diff File (if generated)
            if diff_generated {
                let diff_filename = format!("{}.llmcontext-{}-diff.txt", project_name, new_version);
                let diff_path = local_output_dir.join(&diff_filename);
                match std::fs::write(&diff_path, &diff_buffer) {
                    Ok(_) => println!("✅ Saved Diff: .dctx/{}", diff_filename),
                    Err(e) => eprintln!("❌ Diff Write error for {}: {}", diff_filename, e),
                }
            }
        }
    }

    println!("✨ Done in {:.2?}", start_time.elapsed());
}

fn print_help() {
    println!("Usage: dctx [PATH] [OPTIONS]");
    println!("       dctx template <COMMAND>");
    println!();
    println!("Snapshot Options:");
    println!("  -c, --clipboard       Copy to clipboard");
    println!("  -d, --diff            Output diff only (if available)");
    println!("  -t, --tree            Tree-Only mode (skips file content)");
    println!("  -r, --reset           Force V1 reset");
    println!("  -h, --help            Show help");
    println!("  -v, --version         Show version");
    println!();
    println!("Filter Options:");
    println!("  --ext <extensions>    Only include files with these extensions");
    println!("                        Example: --ext rs,toml");
    println!("  --binary              Include binary files (excluded by default)");
    println!("  --empty               Show only empty files (0 bytes)");
    println!("  --non-empty           Show only non-empty files");
    println!("  --large <MB>          Show only files larger than X megabytes");
    println!("                        Example: --large 1");
    println!("  --small <MB>          Show only files smaller than X megabytes");
    println!("                        Example: --small 0.5");
    println!("  --hidden              Show ONLY hidden files and folders (starting with .)");
    println!("  --debug-ignored       List ignored folders for debugging");
    println!();
    println!("Template Commands:");
    println!("  template build <PATH>  Compile a .dctx_template file");
    println!("  template list          List compiled templates");
    println!();
    println!("Examples:");
    println!("  dctx .                      Snapshot current directory");
    println!("  dctx ./project -c           Snapshot and copy to clipboard");
    println!("  dctx . --ext rs,toml        Only Rust and TOML files");
    println!("  dctx . --ext js --small 0.1 JavaScript files under 100KB");
    println!("  dctx template build my.dctx_template");
}

fn print_version() {
    println!("dctx-cli v{}", env!("CARGO_PKG_VERSION"));
}

// --- TEMPLATE COMMANDS ---

fn run_template_command(subcmd: TemplateSubcommand) {
    match subcmd {
        TemplateSubcommand::Build { template_path } => {
            run_template_build(&template_path);
        }
        TemplateSubcommand::List => {
            run_template_list();
        }
    }
}

fn run_template_build(template_path: &PathBuf) {
    println!("🔧 Building template: {:?}", template_path);

    // 1. Check if file exists
    if !template_path.exists() {
        eprintln!("❌ Template file not found: {:?}", template_path);
        std::process::exit(1);
    }

    // 2. Read template file
    let raw_content = match fs::read_to_string(template_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ Failed to read template file: {}", e);
            std::process::exit(1);
        }
    };

    // 3. Parse template
    println!("📝 Parsing template...");
    let template = Template::parse(&raw_content);

    // 4. Validate
    println!("🔍 Validating template...");
    let errors = template.validate();
    if !errors.is_empty() {
        eprintln!("❌ Template validation failed:");
        for error in &errors {
            eprintln!("   - {}", error);
        }
        std::process::exit(1);
    }
    println!("✅ Validation passed");

    // 5. Derive template name from filename
    let template_name = template_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("custom");

    // Remove .dctx_template suffix if present in stem
    let template_name = template_name.strip_suffix(".dctx").unwrap_or(template_name);

    // 6. Compile template
    println!("⚙️  Compiling template as '{}'...", template_name);
    let compiled = match template.compile(template_name) {
        Ok(c) => c,
        Err(errors) => {
            eprintln!("❌ Template compilation failed:");
            for error in &errors {
                eprintln!("   - {}", error);
            }
            std::process::exit(1);
        }
    };

    // 7. Get output path
    let templates_dir = match config::get_compiled_templates_dir() {
        Some(d) => d,
        None => {
            eprintln!("❌ Could not determine templates directory");
            std::process::exit(1);
        }
    };

    // Ensure directory exists
    if let Err(e) = fs::create_dir_all(&templates_dir) {
        eprintln!("❌ Failed to create templates directory: {}", e);
        std::process::exit(1);
    }

    let output_path = templates_dir.join(format!("{}.bin", template_name));

    // 8. Save compiled template
    println!("💾 Saving to {:?}...", output_path);
    if let Err(e) = compiled.save_to_file(&output_path) {
        eprintln!("❌ Failed to save compiled template: {}", e);
        std::process::exit(1);
    }

    // 9. Success message
    println!();
    println!("═══════════════════════════════════════════════════════════");
    println!("✅ Template built successfully!");
    println!("═══════════════════════════════════════════════════════════");
    println!();
    println!("To use this template, add to your config.toml:");
    println!();
    println!("    default_template = \"{}\"", template_name);
    println!();
    println!(
        "Config location: {:?}",
        config::get_config_file_path().unwrap_or_default()
    );
    println!();
}

fn run_template_list() {
    println!("📋 Compiled Templates");
    println!("═══════════════════════════════════════════════════════════");

    let templates_dir = match config::get_compiled_templates_dir() {
        Some(d) => d,
        None => {
            eprintln!("❌ Could not determine templates directory");
            std::process::exit(1);
        }
    };

    if !templates_dir.exists() {
        println!();
        println!("No compiled templates found.");
        println!("Use 'dctx template build <path>' to compile a template.");
        return;
    }

    let entries = match fs::read_dir(&templates_dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("❌ Failed to read templates directory: {}", e);
            std::process::exit(1);
        }
    };

    let mut found = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("bin") {
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                // Try to load and verify the template
                match CompiledTemplate::load_from_file(&path) {
                    Ok(tmpl) => {
                        println!("  ✓ {} ({})", name, tmpl.name);
                        found = true;
                    }
                    Err(e) => {
                        println!("  ✗ {} (corrupted: {})", name, e);
                        found = true;
                    }
                }
            }
        }
    }

    if !found {
        println!();
        println!("No compiled templates found.");
        println!("Use 'dctx template build <path>' to compile a template.");
    }

    println!();
    println!("Templates directory: {:?}", templates_dir);

    // Show current config
    let app_config = config::load_app_config();
    println!("Current default: {}", app_config.default_template);
}
