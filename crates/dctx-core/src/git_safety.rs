use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::Command;

// --- CONSTANTS FROM ADR-005 ---
const DCTX_GITIGNORE_MARKER: &str =
    "# [dctx] Automated Ignore Rules - github.com/Eris-Margeta/dctx";
const REQUIRED_PATTERNS: &[&str] = &[
    ".dctx",
    ".dctx/",
    "*.llmcontext.txt",
    "*.llmcontext-*-diff.txt",
];

/// Ensures that the local .gitignore contains all necessary dctx patterns.
/// If updated, it automatically stages the file via `git add`.
pub fn ensure_local_gitignore_is_safe(target_dir: &Path) -> io::Result<()> {
    let gitignore_path = target_dir.join(".gitignore");

    // 1. Check if .gitignore exists. If not, we do not create it.
    // We assume non-git projects don't need this check.
    if !gitignore_path.exists() {
        return Ok(());
    }

    // 2. Read existing patterns
    let existing_patterns = read_existing_patterns(&gitignore_path)?;

    // 3. Identify missing patterns
    let mut missing_patterns = Vec::new();
    for &pattern in REQUIRED_PATTERNS {
        if !existing_patterns.contains(pattern) {
            missing_patterns.push(pattern);
        }
    }

    // 4. If all patterns exist, we are good.
    if missing_patterns.is_empty() {
        return Ok(());
    }

    // 5. Append missing patterns
    append_missing_rules(&gitignore_path, &missing_patterns)?;

    // 6. Log to user
    println!("🛡️  Git Safety: Updated .gitignore with missing dctx rules.");

    // 7. Auto-Stage (git add .gitignore)
    // We attempt this, but failure here shouldn't crash the program, just warn.
    match Command::new("git")
        .arg("add")
        .arg(".gitignore")
        .current_dir(target_dir)
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                println!("    -> Auto-staged .gitignore changes.");
            } else {
                eprintln!(
                    "⚠️  Warning: Failed to run 'git add .gitignore'. Check git status manually."
                );
            }
        }
        Err(_) => {
            // Git might not be installed or not in PATH
            eprintln!("⚠️  Warning: Could not run 'git'. Changes to .gitignore were not staged.");
        }
    }

    Ok(())
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

fn append_missing_rules(path: &Path, missing: &[&str]) -> io::Result<()> {
    let mut file = OpenOptions::new().append(true).open(path)?;

    // Add a newline spacer if needed (we can't easily check last char in append mode without seeking,
    // so we conservatively add one if we are appending a block).
    writeln!(file)?;
    writeln!(file, "{}", DCTX_GITIGNORE_MARKER)?;

    for &pattern in missing {
        writeln!(file, "{}", pattern)?;
    }

    Ok(())
}
