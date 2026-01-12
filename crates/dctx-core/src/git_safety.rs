use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::Command;

// Get the version from Cargo.toml at compile time
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// The marker we look for. Example: "# [dctx v0.1.2]"
// We construct this dynamically, but the prefix helps us find old versions.
const MARKER_PREFIX: &str = "# [dctx v";
const MARKER_SUFFIX: &str = "]";

const REQUIRED_PATTERNS: &[&str] = &[
    ".dctx",
    ".dctx/",
    "*.llmcontext.txt",
    "*.llmcontext-*-diff.txt",
];

/// Ensures that the local .gitignore contains all necessary dctx patterns.
/// Uses a "Fast Path" check to avoid re-scanning if the version marker matches.
pub fn ensure_local_gitignore_is_safe(target_dir: &Path) -> io::Result<()> {
    let gitignore_path = target_dir.join(".gitignore");

    // 1. Check if .gitignore exists. If not, we do not create it.
    if !gitignore_path.exists() {
        return Ok(());
    }

    // 2. FAST PATH: Check for the exact version marker
    if is_fast_path_valid(&gitignore_path)? {
        // Marker exists and matches current version. We assume safety.
        return Ok(());
    }

    // 3. SLOW PATH: Scan, Update, and Stage
    let existing_patterns = read_existing_patterns(&gitignore_path)?;
    let mut missing_patterns = Vec::new();
    for &pattern in REQUIRED_PATTERNS {
        if !existing_patterns.contains(pattern) {
            missing_patterns.push(pattern);
        }
    }

    // If patterns are missing, we append them.
    // If NO patterns are missing (e.g., manually added), we still want to update/add the marker
    // so the next run takes the Fast Path.
    update_gitignore(&gitignore_path, &missing_patterns)?;

    if !missing_patterns.is_empty() {
        println!(
            "🛡️  Git Safety: Updated .gitignore with missing rules (v{}).",
            CURRENT_VERSION
        );
    } else {
        // We just updated the marker
        // println!("🛡️  Git Safety: Verified rules and updated version marker.");
    }

    // 4. Auto-Stage (git add .gitignore)
    run_git_add(target_dir);

    Ok(())
}

/// Checks if the .gitignore file contains the EXACT current version marker.
fn is_fast_path_valid(path: &Path) -> io::Result<bool> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let expected_marker = format!("{}{}{}", MARKER_PREFIX, CURRENT_VERSION, MARKER_SUFFIX);

    for line in reader.lines() {
        let line = line?;
        if line.trim() == expected_marker {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_existing_patterns(path: &Path) -> io::Result<HashSet<String>> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut patterns = HashSet::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            patterns.insert(trimmed.to_string());
        }
    }
    Ok(patterns)
}

fn update_gitignore(path: &Path, missing_patterns: &[&str]) -> io::Result<()> {
    let mut file = OpenOptions::new().append(true).open(path)?;

    writeln!(file)?;
    
    // If we have missing patterns, add them with a comment header
    if !missing_patterns.is_empty() {
        writeln!(file, "# [dctx] Automated Rules")?;
        for &pattern in missing_patterns {
            writeln!(file, "{}", pattern)?;
        }
    }

    // Always append the specific version marker at the end to enable Fast Path next time
    let marker = format!("{}{}{}", MARKER_PREFIX, CURRENT_VERSION, MARKER_SUFFIX);
    writeln!(file, "{}", marker)?;

    Ok(())
}

fn run_git_add(target_dir: &Path) {
    // We suppress output unless there is an error
    let _ = Command::new("git")
        .arg("add")
        .arg(".gitignore")
        .current_dir(target_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
