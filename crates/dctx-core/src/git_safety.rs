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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // --- CONSTANTS TESTS ---

    #[test]
    fn test_required_patterns_not_empty() {
        assert!(!REQUIRED_PATTERNS.is_empty());
        assert!(REQUIRED_PATTERNS.contains(&".dctx"));
        assert!(REQUIRED_PATTERNS.contains(&".dctx/"));
        assert!(REQUIRED_PATTERNS.contains(&"*.llmcontext.txt"));
        assert!(REQUIRED_PATTERNS.contains(&"*.llmcontext-*-diff.txt"));
    }

    #[test]
    fn test_marker_format() {
        assert_eq!(MARKER_PREFIX, "# [dctx v");
        assert_eq!(MARKER_SUFFIX, "]");
        // Current version should be valid semver format
        assert!(!CURRENT_VERSION.is_empty());
    }

    // --- READ_EXISTING_PATTERNS TESTS ---

    #[test]
    fn test_read_existing_patterns_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "").unwrap();

        let patterns = read_existing_patterns(&gitignore_path).unwrap();
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_read_existing_patterns_with_patterns() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "node_modules\n.env\ntarget/\n").unwrap();

        let patterns = read_existing_patterns(&gitignore_path).unwrap();
        assert_eq!(patterns.len(), 3);
        assert!(patterns.contains("node_modules"));
        assert!(patterns.contains(".env"));
        assert!(patterns.contains("target/"));
    }

    #[test]
    fn test_read_existing_patterns_ignores_comments() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "# Comment\nnode_modules\n# Another comment\n.env\n").unwrap();

        let patterns = read_existing_patterns(&gitignore_path).unwrap();
        assert_eq!(patterns.len(), 2);
        assert!(patterns.contains("node_modules"));
        assert!(patterns.contains(".env"));
        assert!(!patterns.contains("# Comment"));
    }

    #[test]
    fn test_read_existing_patterns_ignores_empty_lines() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "node_modules\n\n\n.env\n  \n").unwrap();

        let patterns = read_existing_patterns(&gitignore_path).unwrap();
        assert_eq!(patterns.len(), 2);
    }

    #[test]
    fn test_read_existing_patterns_trims_whitespace() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "  node_modules  \n  .env\n").unwrap();

        let patterns = read_existing_patterns(&gitignore_path).unwrap();
        assert!(patterns.contains("node_modules"));
        assert!(patterns.contains(".env"));
    }

    // --- IS_FAST_PATH_VALID TESTS ---

    #[test]
    fn test_is_fast_path_valid_with_marker() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        let marker = format!("{}{}{}", MARKER_PREFIX, CURRENT_VERSION, MARKER_SUFFIX);
        fs::write(&gitignore_path, format!("node_modules\n{}\n", marker)).unwrap();

        assert!(is_fast_path_valid(&gitignore_path).unwrap());
    }

    #[test]
    fn test_is_fast_path_valid_without_marker() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "node_modules\n.env\n").unwrap();

        assert!(!is_fast_path_valid(&gitignore_path).unwrap());
    }

    #[test]
    fn test_is_fast_path_valid_with_old_marker() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        // Use a fake old version that won't match
        fs::write(&gitignore_path, "node_modules\n# [dctx v0.0.0]\n").unwrap();

        // Should return false since version doesn't match
        let result = is_fast_path_valid(&gitignore_path).unwrap();
        // Only true if CURRENT_VERSION is "0.0.0", which it isn't
        if CURRENT_VERSION != "0.0.0" {
            assert!(!result);
        }
    }

    #[test]
    fn test_is_fast_path_valid_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "").unwrap();

        assert!(!is_fast_path_valid(&gitignore_path).unwrap());
    }

    #[test]
    fn test_is_fast_path_valid_marker_with_whitespace() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        let marker = format!("{}{}{}", MARKER_PREFIX, CURRENT_VERSION, MARKER_SUFFIX);
        // Add whitespace around the marker
        fs::write(&gitignore_path, format!("  {}  \n", marker)).unwrap();

        // Should still match because we trim
        assert!(is_fast_path_valid(&gitignore_path).unwrap());
    }

    // --- UPDATE_GITIGNORE TESTS ---

    #[test]
    fn test_update_gitignore_adds_missing_patterns() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "node_modules\n").unwrap();

        let missing = vec![".dctx", ".dctx/"];
        update_gitignore(&gitignore_path, &missing).unwrap();

        let content = fs::read_to_string(&gitignore_path).unwrap();
        assert!(content.contains(".dctx"));
        assert!(content.contains(".dctx/"));
        assert!(content.contains("# [dctx] Automated Rules"));
        assert!(content.contains(&format!("{}{}{}", MARKER_PREFIX, CURRENT_VERSION, MARKER_SUFFIX)));
    }

    #[test]
    fn test_update_gitignore_adds_marker_when_no_missing() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "node_modules\n").unwrap();

        let missing: Vec<&str> = vec![];
        update_gitignore(&gitignore_path, &missing).unwrap();

        let content = fs::read_to_string(&gitignore_path).unwrap();
        // Should NOT have the "Automated Rules" header since no patterns added
        assert!(!content.contains("# [dctx] Automated Rules"));
        // But should have the version marker
        assert!(content.contains(&format!("{}{}{}", MARKER_PREFIX, CURRENT_VERSION, MARKER_SUFFIX)));
    }

    #[test]
    fn test_update_gitignore_preserves_existing_content() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "node_modules\n.env\n").unwrap();

        let missing = vec![".dctx"];
        update_gitignore(&gitignore_path, &missing).unwrap();

        let content = fs::read_to_string(&gitignore_path).unwrap();
        assert!(content.contains("node_modules"));
        assert!(content.contains(".env"));
        assert!(content.contains(".dctx"));
    }

    // --- ENSURE_LOCAL_GITIGNORE_IS_SAFE TESTS ---

    #[test]
    fn test_ensure_safe_no_gitignore() {
        let temp_dir = TempDir::new().unwrap();
        // No .gitignore file exists

        let result = ensure_local_gitignore_is_safe(temp_dir.path());
        assert!(result.is_ok());

        // Should not create .gitignore
        assert!(!temp_dir.path().join(".gitignore").exists());
    }

    #[test]
    fn test_ensure_safe_fast_path() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        let marker = format!("{}{}{}", MARKER_PREFIX, CURRENT_VERSION, MARKER_SUFFIX);

        // Pre-populate with all required patterns and marker
        let content = format!(
            ".dctx\n.dctx/\n*.llmcontext.txt\n*.llmcontext-*-diff.txt\n{}\n",
            marker
        );
        fs::write(&gitignore_path, &content).unwrap();

        let result = ensure_local_gitignore_is_safe(temp_dir.path());
        assert!(result.is_ok());

        // Content should be unchanged (fast path)
        let new_content = fs::read_to_string(&gitignore_path).unwrap();
        assert_eq!(new_content, content);
    }

    #[test]
    fn test_ensure_safe_adds_missing_patterns() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");
        fs::write(&gitignore_path, "node_modules\n").unwrap();

        let result = ensure_local_gitignore_is_safe(temp_dir.path());
        assert!(result.is_ok());

        let content = fs::read_to_string(&gitignore_path).unwrap();
        // Should have all required patterns
        for pattern in REQUIRED_PATTERNS {
            assert!(content.contains(pattern), "Missing pattern: {}", pattern);
        }
        // Should have version marker
        assert!(content.contains(&format!("{}{}{}", MARKER_PREFIX, CURRENT_VERSION, MARKER_SUFFIX)));
    }

    #[test]
    fn test_ensure_safe_updates_marker_when_patterns_present() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");

        // Pre-populate with all required patterns but no marker
        let content = ".dctx\n.dctx/\n*.llmcontext.txt\n*.llmcontext-*-diff.txt\n";
        fs::write(&gitignore_path, content).unwrap();

        let result = ensure_local_gitignore_is_safe(temp_dir.path());
        assert!(result.is_ok());

        let new_content = fs::read_to_string(&gitignore_path).unwrap();
        // Should have added the version marker
        assert!(new_content.contains(&format!("{}{}{}", MARKER_PREFIX, CURRENT_VERSION, MARKER_SUFFIX)));
    }

    #[test]
    fn test_ensure_safe_partial_patterns() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".gitignore");

        // Only some patterns present
        fs::write(&gitignore_path, ".dctx\nnode_modules\n").unwrap();

        let result = ensure_local_gitignore_is_safe(temp_dir.path());
        assert!(result.is_ok());

        let content = fs::read_to_string(&gitignore_path).unwrap();
        // Should have ALL required patterns now
        assert!(content.contains(".dctx/"));
        assert!(content.contains("*.llmcontext.txt"));
        assert!(content.contains("*.llmcontext-*-diff.txt"));
        // Original content preserved
        assert!(content.contains("node_modules"));
    }
}
