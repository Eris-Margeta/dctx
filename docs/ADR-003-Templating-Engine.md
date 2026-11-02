# ADR-003: Extensible Templating System (DTS) & Compilation Strategy

**Status:** Accepted
**Date:** 2026-01-08
**Author:** dctx Team
**Context:** Users require granular control over the output format to support various AI models. The system must maintain high performance (avoiding expensive parsing on every run) and ensure robustness (instructions matching output).

## 1. Decision: Explicit Marker Definition
To ensure future compatibility with non-HTML-like syntax, the templating language separates "Opening" and "Closing" logic for tags into four distinct definitions:
*   `start_marker_open` (Default: `<`)
*   `start_marker_close` (Default: `>`)
*   `end_marker_open` (Default: `</`)
*   `end_marker_close` (Default: `>`)

This allows users to redefine tags completely, e.g., to `[SECTION]` ... `[/SECTION]` or `{{#SECTION}}` ... `{{/SECTION}}`.

## 2. Decision: "Ahead-of-Time" (AOT) Template Compilation
Parsing text templates, validating variable references, and checking for cycles is expensive. We will **not** do this during the standard `dctx` runtime.

### The "Baked-In" Default
The default template (defined in the ADR) is parsed at **Rust Compile Time** (or hardcoded as a static struct). This ensures `dctx` runs with zero parsing overhead out-of-the-box.

### User Custom Templates
Users can edit templates in `~/.config/dctx/templates/`. However, `dctx` does not read these text files directly during snapshot generation.
*   **Action:** User edits `.dctx_template` file.
*   **Command:** User runs `dctx template build <path>`.
*   **Process:**
    1.  `dctx` parses the text file.
    2.  Validates syntax and variable references.
    3.  Serializes the parsed structure into an optimized binary format (e.g., `bincode`).
    4.  Saves the result to `~/.cache/dctx/templates/user_custom.bin`.
*   **Runtime:** When `dctx` runs, if configured to use a custom template, it loads this pre-validated binary directly into memory.

## 3. Decision: Two-Pass Rendering Engine
The rendering engine operates in two distinct passes to separate Configuration from Data.

*   **Pass 1 (Config Resolution):** Occurs during the "Template Build" phase. Resolves all `{{VAR}}` placeholders within the `[VARIABLES]` section.
*   **Pass 2 (Data Injection):** Occurs during runtime. Injects `{{{DATA}}}` into the pre-compiled Partial structures (`[PARTIALS]` section) and assembles the `[LAYOUT]`.

## 4. Template File Structure
The file format uses a custom INI-like syntax with three specific sections:
1.  **[VARIABLES]:** Key-Value pairs for static strings.
2.  **[PARTIALS]:** Definitions for repeated elements (`TREE_ENTRY`, `FILE_BLOCK`).
3.  **[LAYOUT]:** The global document structure.

## 5. Available Data Variables (Runtime)
These variables are populated by the Rust core:
*   `{{{node.id}}}`: Optimized incremental ID (e.g., 1, 2, 10).
*   `{{{node.path}}}`: Relative path.
*   `{{{node.type_char}}}`: `[D]` or `[F]`.
*   `{{{node.mod}}}`: Unix timestamp in milliseconds. Available to custom templates. The built-in default template does not print it since 1.0.1, because a timestamp changes on a bare `touch` and breaks the byte-stable prefix of the page; `--tree-stamps=on` selects the built-in stamped template (the 1.0.0 text, `MOD:` and `SIZE:` on every tree line, `MOD` in the instructions). The default entry also leaves out `{{{node.size_part}}}`, since a size changes on every ordinary edit; `{{{node.size}}}` and `{{{node.size_part}}}` stay available to custom templates.
*   `{{{node.size}}}`: File size in bytes.
*   `{{{file.lines}}}`: Line count of the file.
*   `{{{file.content}}}`: Actual file content.



6. TEMPLATE DEFINITION V1:

# ==============================================================================
# DCTX TEMPLATE DEFINITION V1
# ==============================================================================
# This file defines how the LLM context is formatted.
# It consists of three sections: [VARIABLES], [PARTIALS], and [LAYOUT].
#
# SYNTAX GUIDE:
# - {{VAR_NAME}}      : Inserts a configuration variable defined in [VARIABLES].
# - {{{DATA_FIELD}}}  : Inserts dynamic data from the project (e.g., file content).
#
# ==============================================================================

[VARIABLES]
# --- 1. Syntactic Markers (Granular Control) ---
# Defines the exact characters used to wrap tags.
# Allows handling XML, HTML, or Custom formats safely.
start_marker_open  = "<"
start_marker_close = ">"
end_marker_open    = "</"
end_marker_close   = ">"

# --- 2. Semantic Marker Names ---
# Defines the keywords used inside the tags.
instr_marker         = "INSTRUCTIONS"
tree_start_marker    = "DIRECTORY_TREE"
tree_end_marker      = "DIRECTORY_TREE"
file_start_marker    = "FILE_CONTENT_START"
file_end_marker      = "FILE_CONTENT_END"

# --- 3. Attribute Keys ---
# Used inside tags to denote properties.
attr_id    = "ID"
attr_path  = "PATH"
attr_size  = "SIZE"
attr_lines = "LINES"
attr_mod   = "MOD"

# --- 4. Instructions Block Text ---
# This text explains the format to the LLM. It references the variables above
# to ensure the instructions always match the actual format used.
# Use '\n' for new lines.
instructions_text = "1. Manifest: The {{start_marker_open}}{{tree_start_marker}}{{start_marker_close}} section lists all files.\n   - Each entry: [TYPE] RELATIVE_PATH ({{attr_id}}:UNIQUE_ID)\n   - TYPE is [D] for directory, [F] for file.\n2. Content Access: To read a specific file:\n   - Find its UNIQUE_ID from the directory tree.\n   - Search for the marker: {{start_marker_open}}{{file_start_marker}} {{attr_id}}=\"UNIQUE_ID\"{{start_marker_close}}\n   - The content is between this marker and {{end_marker_open}}{{file_end_marker}} {{attr_id}}=\"UNIQUE_ID\"{{end_marker_close}}"

# ==============================================================================
# [PARTIALS]
# Defines the look of repeated elements (Tree items and Files).
# ==============================================================================

# --- Tree Entry Partial ---
# Context: 'node' object
# Available Data: {{{node.type_char}}}, {{{node.path}}}, {{{node.id}}}, {{{node.mod}}}, {{{node.size}}}
# The default entry leaves {{{node.mod}}} and {{{node.size_part}}} out (1.0.1); a custom template may add `, {{attr_mod}}:{{{node.mod}}}{{{node.size_part}}}`.
[PARTIAL:TREE_ENTRY]
{{{node.type_char}}} {{{node.path}}} ({{attr_id}}:{{{node.id}}})

# --- File Block Partial ---
# Context: 'file' object
# Available Data: {{{file.id}}}, {{{file.path}}}, {{{file.lines}}}, {{{file.size}}}, {{{file.content}}}
[PARTIAL:FILE_BLOCK]
{{start_marker_open}}{{file_start_marker}} {{attr_id}}="{{{file.id}}}" {{attr_path}}="{{{file.path}}}" {{attr_size}}="{{{file.size}}}" {{attr_lines}}="{{{file.lines}}}"{{start_marker_close}}
{{{file.content}}}
{{end_marker_open}}{{file_end_marker}} {{attr_id}}="{{{file.id}}}"{{end_marker_close}}

# ==============================================================================
# [LAYOUT]
# The master structure of the final output file.
# Note: The Version Header (e.g. [DCTX_SNAPSHOT_V1]) is hardcoded in the binary
# and NOT included here.
# ==============================================================================
[LAYOUT]
{{start_marker_open}}{{instr_marker}}{{start_marker_close}}
{{instructions_text}}
{{end_marker_open}}{{instr_marker}}{{end_marker_close}}

{{start_marker_open}}{{tree_start_marker}}{{start_marker_close}}
{{{comp.tree}}}
{{end_marker_open}}{{tree_end_marker}}{{end_marker_close}}

{{{comp.files}}}
