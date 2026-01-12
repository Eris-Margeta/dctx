mod platform;

use dctx_core::ignore;
use dctx_core::llm_formatter;
use dctx_core::version::Version;
use dctx_core::walker;
use dctx_core::writer;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

// --- Configuration Struct ---

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

// --- Main Entry Point ---

fn main() {
    let args: Vec<String> = env::args().collect();

    // MODE SWITCHING LOGIC:
    // 1. No arguments provided -> Interactive Mode
    // 2. Arguments provided -> Headless Mode
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
                eprintln!("Try 'dctx --help' for usage information.");
                std::process::exit(1);
            }
        }
    }
}

// --- Argument Parsing (Zero Dep) ---

fn parse_arguments(args: &[String]) -> Result<CliConfig, String> {
    let mut config = CliConfig::default();
    let mut path_set = false;

    // Skip the first argument (executable name)
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
                    return Err(
                        "Multiple target paths provided. Only one path is allowed.".to_string()
                    );
                }
                config.target_path = PathBuf::from(val);
                path_set = true;
            }
        }
    }

    Ok(config)
}

// --- Modes ---

fn run_interactive_mode() {
    println!("========================================");
    println!("      dctx - Intelligent Context        ");
    println!("========================================");
    println!("No arguments provided. Select an action:");
    println!();
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
        "1" => {
            let config = CliConfig::default();
            run_headless_mode(config);
        }
        "2" => {
            let mut config = CliConfig::default();
            config.clipboard_mode = true;
            run_headless_mode(config);
        }
        "3" => print_help(),
        "4" | "q" | "exit" => std::process::exit(0),
        _ => println!("Invalid selection."),
    }
}

fn run_headless_mode(config: CliConfig) {
    let start_time = Instant::now();
    let target_dir = match std::fs::canonicalize(&config.target_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("❌ Error resolving path '{:?}': {}", config.target_path, e);
            std::process::exit(1);
        }
    };

    let project_name = target_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    if !config.clipboard_mode {
        println!("🚀 dctx running on: {}", target_dir.display());
    }

    // 1. Load Ignore Rules
    let rules = ignore::load_ignore_rules(&target_dir);

    // 2. Walk Structure
    if !config.clipboard_mode {
        println!("📂 Scanning directory structure...");
    }
    let mut root = match walker::walk_directory_structure(&target_dir, &rules) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("❌ Error walking directory: {}", e);
            std::process::exit(1);
        }
    };

    // 3. Populate Content
    walker::populate_file_contents_parallel(&mut root);

    // 4. Save Binary State (.dctx)
    // NOTE: In the future, this will be conditional based on Config/Diff logic.
    // For now, we always save it locally to maintain current behavior, unless we want strict clipboard purity.
    // However, the Legacy C utility DOES update state even in clipboard mode (implied by design).
    let bin_filename = format!("{}.dctx", project_name);
    let bin_path = target_dir.join(&bin_filename);

    match writer::write_dircontxt_file(&bin_path, &mut root) {
        Ok(_) => {
            if !config.clipboard_mode {
                println!("✅ Binary state saved to: {}", bin_filename)
            }
        }
        Err(e) => eprintln!("❌ Failed to write binary: {}", e),
    }

    // 5. Generate Output (File or Clipboard)
    // Mock versioning for Phase 1
    let version = Version::new_v1();

    // Use a Vec<u8> buffer to capture output in memory first
    let mut output_buffer = Vec::new();

    match llm_formatter::generate_llm_context(&mut output_buffer, &root, &version, &project_name) {
        Ok(_) => {
            if config.clipboard_mode {
                // Clipboard Mode
                let content_str = String::from_utf8_lossy(&output_buffer);
                match platform::copy_to_clipboard(&content_str) {
                    Ok(_) => println!(
                        "✨ Context copied to clipboard! ({} bytes)",
                        output_buffer.len()
                    ),
                    Err(e) => eprintln!("❌ Clipboard error: {}", e),
                }
            } else {
                // File Mode
                let txt_filename = format!("{}.llmcontext.txt", project_name);
                let txt_path = target_dir.join(&txt_filename);
                match std::fs::write(&txt_path, &output_buffer) {
                    Ok(_) => println!("✅ LLM Context saved to: {}", txt_filename),
                    Err(e) => eprintln!("❌ Failed to write text file: {}", e),
                }
            }
        }
        Err(e) => eprintln!("❌ Failed to generate context: {}", e),
    }

    if !config.clipboard_mode {
        let duration = start_time.elapsed();
        println!("✨ Done in {:.2?}", duration);
    }
}

// --- Helpers ---

fn print_help() {
    println!("Usage: dctx [PATH] [OPTIONS]");
    println!();
    println!("Arguments:");
    println!("  [PATH]           Target directory (default: current directory)");
    println!();
    println!("Options:");
    println!("  -c, --clipboard  Copy output to clipboard instead of creating text files");
    println!("  -d, --diff       Generate a diff file (requires existing history)");
    println!("  -r, --reset      Force a reset of the version history to V1");
    println!("  -h, --help       Show this help message");
    println!("  -v, --version    Show version information");
    println!();
    println!("Interactive Mode:");
    println!("  Run 'dctx' without arguments to launch the menu.");
}

fn print_version() {
    println!("dctx-cli v0.1.0");
}
