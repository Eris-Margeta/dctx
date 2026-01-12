mod platform;

use dctx_core::config::{self, OutputMode};
use dctx_core::diff::{self, DiffReport};
use dctx_core::ignore;
use dctx_core::llm_formatter;
use dctx_core::state::{self, StateLoadResult};
use dctx_core::version::Version;
use dctx_core::walker;
use dctx_core::writer;

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

// --- Config Struct ---
#[derive(Debug)]
struct CliConfig {
    target_path: PathBuf,
    clipboard_mode: bool,
    diff_mode: bool,
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

    // 1. Resolve Path
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

    // FIX: Extension changed to .dctx
    let bin_filename = format!("{}.dctx", project_name);
    let bin_path = target_dir.join(&bin_filename);

    // 3. Load or Reset State
    let mut old_tree_opt = None;
    let mut old_version = Version::new_v1();

    // FIX: We now load version from binary, ignoring text file completely for versioning truth.
    if !cli_config.force_reset && bin_path.exists() {
        match state::load_or_reset_state(&target_dir, &bin_path, Some(&target_dir), &project_name) {
            StateLoadResult::Loaded(loaded_root, loaded_version) => {
                old_tree_opt = Some(loaded_root);
                old_version = loaded_version; // Use version from binary
            }
            StateLoadResult::FreshV1 => { /* Reset happened */ }
        }
    }

    if !cli_config.clipboard_mode {
        println!("🚀 dctx running on: {}", target_dir.display());
    }

    // 4. Scan Directory
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

    // 5. Populate Content
    walker::populate_file_contents_parallel(&mut new_tree);

    // 6. Diff & Versioning
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

    // 7. Write Binary State (With Version!)
    match writer::write_dircontxt_file(&bin_path, &mut new_tree, &new_version) {
        Ok(_) => {}
        Err(e) => eprintln!("❌ Failed to save binary state: {}", e),
    }

    // 8. Generate Output
    let should_generate_output = match app_config.output_mode {
        OutputMode::BinaryOnly => false,
        _ => true,
    };

    if should_generate_output {
        let mut output_buffer = Vec::new();
        // Placeholder for Diff-Only logic
        let mut _writing_diff_only = false;
        if cli_config.clipboard_mode
            && cli_config.diff_mode
            && diff_report.has_changes
            && !is_major_reset
        {
            _writing_diff_only = true;
        }

        if let Err(e) = llm_formatter::generate_llm_context(
            &mut output_buffer,
            &new_tree,
            &new_version,
            &project_name,
        ) {
            eprintln!("❌ Generation error: {}", e);
        }

        if cli_config.clipboard_mode {
            let content_str = String::from_utf8_lossy(&output_buffer);
            match platform::copy_to_clipboard(&content_str) {
                Ok(_) => println!("✨ Copied to clipboard!"),
                Err(e) => eprintln!("❌ Clipboard error: {}", e),
            }
        } else {
            let txt_filename = format!("{}.llmcontext.txt", project_name);
            let txt_path = target_dir.join(&txt_filename);
            match std::fs::write(&txt_path, &output_buffer) {
                Ok(_) => println!("✅ Saved: {}", txt_filename),
                Err(e) => eprintln!("❌ Write error: {}", e),
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
    println!("  -d, --diff       Output diff only");
    println!("  -r, --reset      Force V1 reset");
    println!("  -h, --help       Show help");
    println!("  -v, --version    Show version");
}

fn print_version() {
    println!("dctx-cli v0.1.0");
}
