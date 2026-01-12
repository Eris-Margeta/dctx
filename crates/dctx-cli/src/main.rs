mod platform;

use dctx_core::config::{self, OutputMode};
use dctx_core::diff::{self, DiffReport};
use dctx_core::git_safety;
use dctx_core::ignore;
use dctx_core::llm_formatter;
use dctx_core::registry::ProjectRegistry;
use dctx_core::state::{self, StateLoadResult};
use dctx_core::version::Version;
use dctx_core::walker;
use dctx_core::writer;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

// --- Config Struct ---
#[derive(Debug)]
struct CliConfig {
    target_path: PathBuf,
    clipboard_mode: bool,
    diff_mode: bool,
    tree_only: bool, // Added Phase 5.3
    force_reset: bool,
    show_help: bool,
    show_version: bool,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            target_path: PathBuf::from("."),
            clipboard_mode: false,
            diff_mode: false,
            tree_only: false,
            force_reset: false,
            show_help: false,
            show_version: false,
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 1 {
        run_interactive_mode();
    } else {
        match parse_arguments(&args) {
            Ok(config) => {
                if config.show_help {
                    print_help();
                } else if config.show_version {
                    print_version();
                } else {
                    run_headless_mode(config);
                }
            }
            Err(e) => {
                eprintln!("❌ Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn parse_arguments(args: &[String]) -> Result<CliConfig, String> {
    let mut config = CliConfig::default();
    let mut path_set = false;
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-c" | "--clipboard" => config.clipboard_mode = true,
            "-d" | "--diff" => config.diff_mode = true,
            "-t" | "--tree" => config.tree_only = true, // Added Phase 5.3
            "-r" | "--reset" => config.force_reset = true,
            "-h" | "--help" => config.show_help = true,
            "-v" | "--version" => config.show_version = true,
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
    }
    Ok(config)
}

fn run_interactive_mode() {
    println!("========================================");
    println!("      dctx - Intelligent Context        ");
    println!("========================================");
    println!("  [1] Snapshot Current Directory (.)");
    println!("  [2] Snapshot Current Directory to Clipboard (. -c)");
    println!("  [3] Help");
    println!("  [4] Exit");
    println!();
    print!("> ");
    io::stdout().flush().unwrap();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return;
    }
    match input.trim() {
        "1" => run_headless_mode(CliConfig::default()),
        "2" => {
            let mut c = CliConfig::default();
            c.clipboard_mode = true;
            run_headless_mode(c);
        }
        "3" => print_help(),
        "4" | "q" | "exit" => std::process::exit(0),
        _ => println!("Invalid selection."),
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

    if !cli_config.clipboard_mode {
        println!("🚀 dctx running on: {}", target_dir.display());
        println!("🔑 Project Hash: {}", project_hash);
        if cli_config.tree_only {
            println!("🌲 Tree-Only Mode Active");
        }
    }

    // 4. Load or Reset State
    let mut old_tree_opt = None;
    let mut old_version = Version::new_v1();

    if !cli_config.force_reset && bin_path.exists() {
        match state::load_or_reset_state(
            &target_dir,
            &bin_path,
            Some(&local_output_dir),
            &project_name,
        ) {
            StateLoadResult::Loaded(loaded_root, loaded_version) => {
                old_tree_opt = Some(loaded_root);
                old_version = loaded_version;
            }
            StateLoadResult::FreshV1 => { /* Reset happened */ }
        }
    }

    // 5. Scan Directory
    let ignore_rules = ignore::load_ignore_rules(&target_dir);
    if !cli_config.clipboard_mode {
        println!("📂 Scanning directory structure...");
    }

    let mut new_tree = match walker::walk_directory_structure(&target_dir, &ignore_rules) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("❌ Error walking directory: {}", e);
            std::process::exit(1);
        }
    };

    // 6. Populate Content
    walker::populate_file_contents_parallel(&mut new_tree);

    // 7. Diff & Versioning
    let mut diff_report = DiffReport::default();
    let (new_version, is_major_reset) = if let Some(old_tree) = &old_tree_opt {
        diff_report = diff::compare_trees(Some(old_tree), &new_tree);
        if diff_report.has_changes {
            old_version.calculate_next(
                diff_report.stats.files_unchanged,
                diff_report.stats.total_files_in_new_state,
                app_config.max_diff_files,
            )
        } else {
            (old_version.clone(), false)
        }
    } else {
        (Version::new_v1(), true)
    };

    if !cli_config.clipboard_mode {
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
    }

    // --- CLEANUP (ADR-002) ---
    if is_major_reset && !cli_config.clipboard_mode {
        if local_output_dir.exists() {
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
    }

    // 8. Write Binary State
    match writer::write_dircontxt_file(&bin_path, &mut new_tree, &new_version) {
        Ok(_) => {}
        Err(e) => eprintln!("❌ Failed to save binary state: {}", e),
    }

    // 9. Generate Output
    let should_generate_output = match app_config.output_mode {
        OutputMode::BinaryOnly => false,
        _ => true,
    };

    if should_generate_output {
        let mut output_buffer = Vec::new();
        let mut diff_buffer = Vec::new();

        let writing_diff_only = cli_config.clipboard_mode
            && cli_config.diff_mode
            && diff_report.has_changes
            && !is_major_reset;

        // Generate Main Context (Always, unless clipboard diff-only requested)
        if !writing_diff_only {
            if let Err(e) = llm_formatter::generate_llm_context(
                &mut output_buffer,
                &new_tree,
                &new_version,
                &project_name,
                cli_config.tree_only, // Pass tree_only flag
            ) {
                eprintln!("❌ Generation error: {}", e);
            }
        }

        // Generate Diff Context (If changes exist, not a major reset, and Diffs ENABLED or requested via CLI)
        // Phase 5.2: Check config.text_output_mode_generates_diff_files
        let mut diff_generated = false;

        let should_generate_diff = diff_report.has_changes
            && !is_major_reset
            && (app_config.text_output_mode_generates_diff_files || writing_diff_only);

        if should_generate_diff {
            if let Err(e) = llm_formatter::generate_diff_context(
                &mut diff_buffer,
                &new_tree,
                &diff_report,
                &old_version,
                &new_version,
            ) {
                eprintln!("❌ Diff generation error: {}", e);
            } else {
                diff_generated = true;
            }
        }

        // --- WRITING / COPYING ---
        if cli_config.clipboard_mode {
            let content_to_copy = if writing_diff_only {
                // If user specifically asked for diff (-d) and we have one
                if diff_generated {
                    diff_buffer
                } else {
                    // Fallback to main context if no diff (e.g. V1)
                    output_buffer
                }
            } else {
                // Standard behavior: Copy main context
                output_buffer
            };

            let content_str = String::from_utf8_lossy(&content_to_copy);
            match platform::copy_to_clipboard(&content_str) {
                Ok(_) => println!("✨ Copied to clipboard!"),
                Err(e) => eprintln!("❌ Clipboard error: {}", e),
            }
        } else {
            // Write Main Context
            // Phase 5.1: Added logging to trace execution
            let txt_filename = format!("{}.llmcontext.txt", project_name);
            let txt_path = local_output_dir.join(&txt_filename);

            println!("📝 Attempting to write main context: {:?}", txt_path);

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

    if !cli_config.clipboard_mode {
        println!("✨ Done in {:.2?}", start_time.elapsed());
    }
}

fn print_help() {
    println!("Usage: dctx [PATH] [OPTIONS]");
    println!("Options:");
    println!("  -c, --clipboard  Copy to clipboard");
    println!("  -d, --diff       Output diff only (if available)");
    println!("  -t, --tree       Tree-Only mode (skips file content)");
    println!("  -r, --reset      Force V1 reset");
    println!("  -h, --help       Show help");
    println!("  -v, --version    Show version");
}

fn print_version() {
    println!("dctx-cli v{}", env!("CARGO_PKG_VERSION"));
}
