// crates/dctx-cli/src/main.rs

mod platform;

use dctx_core::config;
use dctx_core::diff::{self, DiffReport};
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
use std::io::{self, Write};
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
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            target_path: PathBuf::from("."),
            clipboard_mode: false,
            diff_mode: false,
            tree_only: false,
            force_reset: false,
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
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            target_path: PathBuf::from("."),
            clipboard_mode: false,
            diff_mode: false,
            tree_only: false,
            force_reset: false,
        }
    }
}

impl From<SnapshotConfig> for CliConfig {
    fn from(sc: SnapshotConfig) -> Self {
        Self {
            target_path: sc.target_path,
            clipboard_mode: sc.clipboard_mode,
            diff_mode: sc.diff_mode,
            tree_only: sc.tree_only,
            force_reset: sc.force_reset,
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

    for arg in args {
        match arg.as_str() {
            "-c" | "--clipboard" => config.clipboard_mode = true,
            "-d" | "--diff" => config.diff_mode = true,
            "-t" | "--tree" => config.tree_only = true,
            "-r" | "--reset" => config.force_reset = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            "-v" | "--version" => return Ok(CliCommand::Version),
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

    Ok(CliCommand::Snapshot(config))
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
            run_headless_mode(CliConfig {
                clipboard_mode: true,
                ..Default::default()
            });
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
                &new_tree,
                &new_version,
                &project_name,
                cli_config.tree_only,
                &app_config.default_template,
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
                &new_tree,
                &diff_report,
                &old_version,
                &new_version,
                &app_config.default_template,
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
    println!("  -c, --clipboard  Copy to clipboard");
    println!("  -d, --diff       Output diff only (if available)");
    println!("  -t, --tree       Tree-Only mode (skips file content)");
    println!("  -r, --reset      Force V1 reset");
    println!("  -h, --help       Show help");
    println!("  -v, --version    Show version");
    println!();
    println!("Template Commands:");
    println!("  template build <PATH>  Compile a .dctx_template file");
    println!("  template list          List compiled templates");
    println!();
    println!("Examples:");
    println!("  dctx .                 Snapshot current directory");
    println!("  dctx ./project -c      Snapshot and copy to clipboard");
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
