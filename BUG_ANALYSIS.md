# DCTX Bug Analysis: Binary Files & Ignore Hierarchy Issues

**Date:** 2026-01-25
**Severity:** HIGH - Core functionality broken

---

## Executive Summary

The dctx tool has **TWO critical bugs**:

1. **NO binary file detection** - All files (including images, PDFs, videos) are read as UTF-8 and included in output
2. **Ignore hierarchy vulnerability** - Project `.dctxignore` can override system hardcoded rules with negation patterns

---

## Bug #1: No Binary File Detection

### Current Behavior

**Every file is treated as text**, regardless of type:

```rust
// walker.rs:52 - Reads ALL file bytes
let result = match fs::read(path) {
    Ok(content) => Some((path.clone(), content)),
    ...
}

// llm_formatter.rs:275-278 - Converts ANY bytes to UTF-8
let content_str = if let Some(content) = &node.file_content {
    String::from_utf8_lossy(content)  // <-- Converts BINARY to garbage text!
} else {
    std::borrow::Cow::Borrowed("")
};
```

### Impact

When running `dctx . -c`, the output contains:
- Binary garbage from PNG, JPG, GIF files
- PDF binary streams
- Video file bytes converted to invalid UTF-8
- Executable binaries as mangled text
- **MASSIVE output files** because binary data is not filtered

### Root Cause

**No binary detection logic exists anywhere in the codebase.**

The code has:
- File walking ✅
- Ignore pattern matching ✅
- Content reading ✅
- **Binary detection ❌**
- **Content omission ❌**

---

## Bug #2: Ignore Hierarchy Vulnerability

### Current Behavior

From `ignore.rs:155-176`:

```rust
pub fn load_ignore_rules(base_dir_path: &Path) -> Vec<IgnoreRule> {
    let mut rules = Vec::new();

    // 1. HARDCODED rules (node_modules/, .git/, etc.)
    for &p in HARDCODED_IGNORE_RULES {
        if let Some(rule) = parse_ignore_pattern_line(p) {
            rules.push(rule);
        }
    }

    // 2. Global rules
    if let Some(global_path) = get_global_ignore_path() {
        if global_path.exists() {
            load_rules_from_file(&global_path, &mut rules);
        }
    }

    // 3. Project .dctxignore (LOADED LAST)
    let project_ignore_path = base_dir_path.join(DEFAULT_IGNORE_FILENAME);
    if project_ignore_path.exists() {
        load_rules_from_file(&project_ignore_path, &mut rules);
    }

    rules
}
```

From `ignore.rs:136-138`:

```rust
if matched {
    is_ignored = !rule.is_negation;  // <-- LAST MATCH WINS!
}
```

### The Problem

**Project `.dctxignore` can use negation to override hardcoded rules:**

```bash
# In project .dctxignore:
!node_modules/    # <-- This OVERRIDES hardcoded "node_modules/" ignore!
```

Because:
1. Hardcoded rule adds: `node_modules/` → ignored=true
2. Project rule adds: `!node_modules/` → ignored=false
3. **Last match wins**, so `node_modules/` is NOT ignored!

### Impact

- System-level protections (`.git/`, `node_modules/`, `.venv/`) can be bypassed
- Users may accidentally un-ignore critical directories
- **This is likely why the user is seeing `node_modules/` despite it being in hardcoded rules!**

---

## Bug #3: Missing File Type Handling

### Current Behavior

**ALL files** are treated identically - no special handling for:
- Images (PNG, JPG, GIF, WebP, AVIF, etc.)
- Videos (MP4, MOV, AVI, etc.)
- Documents (PDF, DOCX, XLSX, etc.)
- Archives (ZIP, TAR, GZ, etc.)
- Executables (EXE, DLL, SO, DYLIB, etc.)
- SVG files (should have configurable content omission)

### Expected Behavior

1. **Binary files** → Show in tree, omit content with message `[Binary file, content omitted]`
2. **SVG files** → Show in tree, optionally omit content based on config `omitSvgContent=true`
3. **Text files** → Show in tree with full content

---

## Proposed Solutions

### Solution 1: Add Binary Detection

**Location:** New file `crates/dctx-core/src/file_classifier.rs`

```rust
pub enum FileContentHandling {
    IncludeContent,           // Normal text file
    OmitBinary,               // Binary file (images, PDFs, etc.)
    OmitConfigurable(String), // Configurable (e.g., SVG)
}

pub fn classify_file(path: &Path, content: &[u8]) -> FileContentHandling {
    // 1. Check extension
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        match ext.to_lowercase().as_str() {
            // Images
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "webp" | "avif" => {
                return FileContentHandling::OmitBinary;
            }
            // SVG (configurable)
            "svg" => {
                return FileContentHandling::OmitConfigurable("svg".to_string());
            }
            // Documents
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => {
                return FileContentHandling::OmitBinary;
            }
            // Archives
            "zip" | "tar" | "gz" | "bz2" | "7z" | "rar" => {
                return FileContentHandling::OmitBinary;
            }
            // Videos
            "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" => {
                return FileContentHandling::OmitBinary;
            }
            // Audio
            "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" => {
                return FileContentHandling::OmitBinary;
            }
            // Executables
            "exe" | "dll" | "so" | "dylib" | "bin" => {
                return FileContentHandling::OmitBinary;
            }
            _ => {}
        }
    }

    // 2. Content-based detection (for extensionless files)
    if is_binary_content(content) {
        return FileContentHandling::OmitBinary;
    }

    FileContentHandling::IncludeContent
}

fn is_binary_content(content: &[u8]) -> bool {
    // Check for null bytes (common in binary files)
    if content.contains(&0) {
        return true;
    }

    // Check for high ratio of non-printable bytes
    let sample_size = content.len().min(8192); // Check first 8KB
    let non_printable = content[..sample_size]
        .iter()
        .filter(|&&b| b < 32 && b != b'\n' && b != b'\r' && b != b'\t')
        .count();

    let ratio = non_printable as f64 / sample_size as f64;
    ratio > 0.3 // More than 30% non-printable = binary
}
```

### Solution 2: Fix Ignore Hierarchy

**Location:** `crates/dctx-core/src/ignore.rs`

**Option A: Make hardcoded rules immutable**

```rust
pub fn load_ignore_rules(base_dir_path: &Path) -> Vec<IgnoreRule> {
    let mut rules = Vec::new();
    let mut hardcoded_patterns = HashSet::new();

    // 1. Load hardcoded rules and track them
    for &p in HARDCODED_IGNORE_RULES {
        if let Some(rule) = parse_ignore_pattern_line(p) {
            hardcoded_patterns.insert(rule.pattern.clone());
            rules.push(rule);
        }
    }

    // 2. Load global rules (skip if pattern is hardcoded)
    if let Some(global_path) = get_global_ignore_path() {
        load_rules_filtered(&global_path, &mut rules, &hardcoded_patterns);
    }

    // 3. Load project rules (skip if pattern is hardcoded)
    let project_ignore_path = base_dir_path.join(DEFAULT_IGNORE_FILENAME);
    if project_ignore_path.exists() {
        load_rules_filtered(&project_ignore_path, &mut rules, &hardcoded_patterns);
    }

    rules
}

fn load_rules_filtered(
    filepath: &Path,
    rules: &mut Vec<IgnoreRule>,
    hardcoded_patterns: &HashSet<String>,
) {
    if let Ok(content) = fs::read_to_string(filepath) {
        for line in content.lines() {
            if let Some(rule) = parse_ignore_pattern_line(line) {
                // Prevent overriding hardcoded patterns
                let pattern = rule.pattern.trim_start_matches('!');
                if !hardcoded_patterns.contains(pattern) {
                    rules.push(rule);
                }
            }
        }
    }
}
```

**Option B: Three-tier system with priority levels**

```rust
pub struct IgnoreRule {
    pub pattern: String,
    pub pattern_type: PatternType,
    pub is_dir_only: bool,
    pub is_negation: bool,
    pub priority: RulePriority, // NEW FIELD
}

pub enum RulePriority {
    Hardcoded = 3,  // Highest - cannot be overridden
    Global = 2,     // Medium
    Project = 1,    // Lowest
}

pub fn should_ignore_item(...) -> bool {
    let mut is_ignored = false;
    let mut highest_priority = RulePriority::Project;

    for rule in rules {
        if matched {
            // Only apply rule if priority is >= current highest
            if rule.priority as u8 >= highest_priority as u8 {
                is_ignored = !rule.is_negation;
                highest_priority = rule.priority;
            }
        }
    }
    is_ignored
}
```

### Solution 3: Add Config Option for SVG Content

**Location:** `crates/dctx-core/src/config.rs`

```rust
#[derive(Debug, Clone)]
pub struct AppConfig {
    // ... existing fields ...

    /// Controls whether SVG file content is omitted (default: true)
    pub omit_svg_content: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            // ... existing defaults ...
            omit_svg_content: true,
        }
    }
}
```

Add parsing in `parse_config_line`:

```rust
"omit_svg_content" => {
    config.omit_svg_content = val == "true";
}
```

---

## Implementation Plan

### Phase 1: Binary Detection (Critical)
1. Create `file_classifier.rs` with detection logic
2. Modify `walker.rs` to classify files during content loading
3. Modify `llm_formatter.rs` to render "[Binary file, content omitted]" for binary files
4. Add tests

### Phase 2: Ignore Hierarchy Fix (Critical)
1. Choose Option A or B (recommend Option A for simplicity)
2. Implement hardcoded pattern protection
3. Add tests for negation patterns
4. Document behavior

### Phase 3: SVG Configuration (Enhancement)
1. Add `omit_svg_content` config option
2. Update file classifier to check config
3. Add tests
4. Update documentation

### Phase 4: Testing
1. Test with real-world projects containing:
   - Images (PNG, JPG)
   - Videos (MP4)
   - Documents (PDF)
   - SVG files
   - Large node_modules
2. Verify output size reduction
3. Verify ignore hierarchy behavior

---

## Files Requiring Changes

1. `crates/dctx-core/src/file_classifier.rs` (NEW)
2. `crates/dctx-core/src/walker.rs` (MODIFY)
3. `crates/dctx-core/src/llm_formatter.rs` (MODIFY)
4. `crates/dctx-core/src/ignore.rs` (MODIFY)
5. `crates/dctx-core/src/config.rs` (MODIFY)
6. `crates/dctx-core/src/datatypes.rs` (ADD field for content handling)
7. Tests for all of the above

---

## Expected Behavior After Fix

### Before (BROKEN):
```
$ dctx . -c
🚀 dctx running on: /path/to/project
📂 Scanning directory structure...
✅ Saved: 156.7 MB (includes binary garbage)
```

### After (FIXED):
```
$ dctx . -c
🚀 dctx running on: /path/to/project
📂 Scanning directory structure...
📊 Files: 1,234 total, 45 binary (content omitted)
✅ Saved: 2.3 MB (text only, binary omitted)
```

### Example Output:

```
<DIRECTORY_TREE>
[D] ROOT (ID=ROOT, MOD=1234567890)
  [F] src/logo.png (ID=F001, MOD=1234567890, SIZE=45678)
  [F] src/main.rs (ID=F002, MOD=1234567890, SIZE=1234)
  [F] diagram.svg (ID=F003, MOD=1234567890, SIZE=5678)
</DIRECTORY_TREE>

<FILE_CONTENT_START ID=F001 PATH="src/logo.png" LINES=0>
[Binary file: PNG image, content omitted]
</FILE_CONTENT_START>

<FILE_CONTENT_START ID=F002 PATH="src/main.rs" LINES=25>
fn main() {
    println!("Hello, world!");
}
</FILE_CONTENT_START>

<FILE_CONTENT_START ID=F003 PATH="diagram.svg" LINES=0>
[SVG file, content omitted (omit_svg_content=true)]
</FILE_CONTENT_START>
```

---

## Why This Bug Wasn't Caught Earlier

1. **Testing gaps** - No tests with binary files or large projects with images
2. **Dogfooding limitations** - Developers likely used dctx on source-only projects
3. **No integration tests** - Unit tests exist, but no real-world scenario tests
4. **Silent failure** - Tool doesn't crash, just produces garbage output

---

## Recommendations

1. **Immediate**: Implement Phase 1 (Binary Detection) - This is the most critical issue
2. **High Priority**: Implement Phase 2 (Ignore Hierarchy Fix) - Security concern
3. **Medium Priority**: Implement Phase 3 (SVG Configuration) - UX improvement
4. **Long-term**: Add comprehensive integration tests with real project structures

---

## Questions for Stakeholders

1. Should hardcoded rules be **absolutely immutable**, or should there be an escape hatch?
2. What should happen if a binary file has no extension? (currently: content-based detection)
3. Should we have different omission messages for different binary types? (e.g., "[PNG image]" vs "[Binary file]")
4. Should there be a config to force-include certain binary extensions?
