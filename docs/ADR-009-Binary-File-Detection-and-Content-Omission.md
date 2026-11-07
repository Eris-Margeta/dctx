<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-009: Binary File Detection and Content Omission

**Date:** `2026-01-25`
**Status:** current — architectural principle retained; historical examples describe their original release.

For shipped behavior and limitations, see the [README](../README.md), [testing guide](TESTING.md) and [specification](SPEC.md).

---

## 1. Context and Problem Statement

Prior to this decision, dctx attempted to read all files as UTF-8 text and include their contents in generated context files. This caused critical issues when encountering binary files (images, PDFs, videos, executables, etc.):

1. **Corrupted Output**: Binary data converted to garbage UTF-8 characters, producing unreadable content
2. **Massive File Sizes**: A single PNG image could produce 4,000+ lines of garbage, inflating output from ~500 KB to 4+ MB
3. **Memory Waste**: Binary content consumed memory during processing despite being unusable
4. **Poor User Experience**: LLM context files became polluted with binary garbage

**Example:** A project with a single screenshot produced a 4.3 MB context file with 4,619 lines of corrupted characters like `�PNG IHDR � 0 ��&Z MiCCPICC...`.

The system needed to:
- Detect binary files automatically
- Show files in directory tree (for structural awareness)
- Omit binary content from output (avoid garbage)
- Provide clean omission messages with file type metadata

## 2. Decision Drivers

* **Correctness**: Binary files should never be treated as text
* **Output Quality**: No corrupted UTF-8 characters in generated context
* **Performance**: Dramatic reduction in output file size for media-heavy projects
* **User Experience**: Clean, informative messages instead of binary garbage
* **Extensibility**: Easy to add new binary file types
* **Robustness**: Fallback detection for files without recognized extensions
* **Backward Compatibility**: Text files should remain unaffected

## 3. Considered Options

### Option 1: Ignore Binary Files Entirely
Add binary file extensions to `.dctxignore` patterns, excluding them from directory tree.

*   **Pros:**
    *   Simple implementation
    *   No processing overhead for binary files
*   **Cons:**
    *   Files disappear from directory tree (loss of structural context)
    *   LLMs can't see that assets exist in the project
    *   Users must manually configure ignore patterns for each file type
    *   No distinction between "deliberately ignored" and "binary"

### Option 2: Extension-Based Detection Only
Maintain a hardcoded list of binary file extensions and omit content for matches.

*   **Pros:**
    *   Fast and deterministic
    *   Works for 99% of cases
    *   Easy to extend with new extensions
*   **Cons:**
    *   Fails for files without extensions (e.g., `Makefile`, custom binaries)
    *   Fails for misnamed files (e.g., `.jpg` file with wrong extension)
    *   No fallback for unknown binary types

### Option 3: Content-Based Detection with Extension Hints
Combine extension-based detection with content analysis (null byte checks, non-printable character ratio).

*   **Pros:**
    *   Robust: Detects binaries even without recognized extensions
    *   Accurate: Content analysis confirms binary nature
    *   Comprehensive: Covers both known and unknown binary types
    *   Files remain in directory tree with clean omission messages
*   **Cons:**
    *   Slightly more processing overhead (mitigated by early exit on extension match)
    *   Requires defining content analysis heuristics

## 4. Decision Outcome

**Chosen option:** Content-Based Detection with Extension Hints (Option 3)

This approach provides maximum robustness while maintaining performance through a two-stage detection strategy:

1. **Fast Path**: Check file extension against 42 known binary types
2. **Fallback**: If extension unrecognized, analyze file content for binary indicators

### Supported Binary File Types (42 extensions)

| Category | Extensions | Type Metadata |
|----------|-----------|---------------|
| **Images** | PNG, JPG, GIF, BMP, ICO, WebP, AVIF, TIFF | "PNG image", "JPEG image", etc. |
| **Documents** | PDF, DOC, DOCX, XLS, XLSX, PPT, PPTX, ODT, ODS, ODP | "PDF document", "Word document", etc. |
| **Archives** | ZIP, TAR, GZ, BZ2, XZ, 7Z, RAR | "ZIP archive", "TAR archive", etc. |
| **Videos** | MP4, MOV, AVI, MKV, WebM, FLV, WMV, M4V | "MP4 video", "MOV video", etc. |
| **Audio** | MP3, WAV, OGG, FLAC, AAC, M4A, WMA | "MP3 audio", "WAV audio", etc. |
| **Executables** | EXE, DLL, SO, DYLIB, BIN, APP | "Windows executable", "Linux shared library", etc. |
| **Fonts** | TTF, OTF, WOFF, WOFF2, EOT | "TrueType font", "OpenType font", etc. |
| **Database** | DB, SQLite, SQLite3 | "Database file", "SQLite database" |
| **Compiled** | O, OBJ, A, LIB, PYC, CLASS | "Object file", "Python bytecode", etc. |

### Content-Based Detection Algorithm

For files without recognized extensions:
1. **Null Byte Check**: If file contains `\0` (null byte), classify as binary
2. **Non-Printable Ratio**: Calculate ratio of non-printable characters
   - If >30% non-printable → binary
   - Excludes whitespace (tabs, newlines, spaces) from calculation
3. **Type Metadata**: Label as "binary file" (generic)

### Output Format

Binary files show in tree:
```
[F] logo.png (ID:F001, SIZE:45678)
```

Content section shows clean omission:
```
<FILE_CONTENT_START ID="F001" PATH="logo.png" LINES="0">
[Binary file: PNG image, content omitted]
</FILE_CONTENT_END ID="F001">
```

### Positive Consequences
*   **Dramatic Size Reduction**: Example project dropped from 4.3 MB to 844 KB (80% reduction)
*   **Clean Output**: No more corrupted UTF-8 characters or binary garbage
*   **Structural Awareness**: LLMs still see that assets exist in directory tree
*   **Informative Messages**: File type metadata helps LLMs understand project composition
*   **Memory Efficiency**: Binary content never stored in memory unnecessarily
*   **Robust Detection**: Catches binaries even without standard extensions
*   **Extensible**: Easy to add new binary types to the extension list

### Negative Consequences
*   **Processing Overhead**: Content analysis required for unrecognized extensions (minimal impact, <1% of files)
*   **False Positives**: Extremely rare edge case where text file has >30% non-printable characters (mitigated by whitespace exclusion)
*   **Maintenance**: Extension list must be updated as new binary formats emerge (infrequent)

## 5. Implementation Plan

1. **Create new module**: `crates/dctx-core/src/file_classifier.rs` (378 lines)
   - Define `BINARY_EXTENSIONS` constant with 42 file types
   - Implement `classify_file()` with two-stage detection
   - Implement `is_binary_by_content()` with null byte + ratio checks
   - Implement `get_binary_file_type()` for type metadata

2. **Update datatypes.rs**:
   - Add `is_binary_file: bool` field to `DirContextTreeNode`
   - Add `binary_file_type: Option<String>` field for type metadata

3. **Update walker.rs**:
   - Call `classify_file()` after loading file content
   - Set `is_binary_file` and `binary_file_type` fields
   - Store original content only if not binary

4. **Update llm_formatter.rs**:
   - Check `is_binary_file` flag before outputting content
   - If binary, output: `[Binary file: {type}, content omitted]`
   - Set `LINES="0"` in header for binary files

5. **Update config.rs**:
   - Add `omit_svg_content` boolean option (default: `true`)
   - SVGs are text but often large/unreadable
   - Allows users to include SVG content if needed

6. **Add comprehensive tests** (15 new tests):
   - Extension-based detection for all 42 types
   - Content-based detection (null bytes, high non-printable ratio)
   - SVG special handling
   - Empty files
   - Whitespace-only files
   - Edge cases (mixed content)

## 6. Validation

Success criteria:
*   All 234 tests pass (219 existing + 15 new in `file_classifier.rs`)
*   Binary files show clean omission messages in output
*   Output file size reduced by 80-90% for media-heavy projects
*   No false positives (text files still processed as text)
*   SVG content omitted by default but configurable

**Metrics:**
- Test coverage: 15 unit tests for file classification
- Output size reduction: 80% (4.3 MB → 844 KB on example project)
- Performance impact: <1% overhead for content analysis on unrecognized extensions
- False positive rate: 0% in test suite (all text files correctly identified)

**Real-World Validation:**
```bash
# Before (v0.9.0)
dctx /path/to/project
# Output: 4.3 MB with binary garbage

# After (v0.10.0)
dctx /path/to/project
# Output: 844 KB with clean omissions
```

---
