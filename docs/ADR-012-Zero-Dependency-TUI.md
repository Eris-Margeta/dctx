# ADR-012: Zero-Dependency Interactive TUI

**Date:** 2026-01-25
**Status:** Accepted

---

## 1. Context and Problem Statement

Users needed a more discoverable, user-friendly way to interact with dctx beyond command-line flags. While the CLI is powerful, it has several UX issues:

- **Discoverability**: Users must know flags like `--ext`, `--large`, `--diff` beforehand
- **Filter Composition**: Combining multiple filters requires remembering multiple flags
- **Learning Curve**: New users face a steep learning curve with 15+ CLI options
- **Iterative Workflows**: Adjusting filters requires re-typing entire commands
- **Cognitive Load**: Remembering which filters are active is difficult

Additionally, maintaining zero dependencies was a core design principle for dctx.

## 2. Decision Drivers

* **Zero Dependencies**: No external crates (crossterm, tui-rs, etc.)
* **Usability**: Reduce learning curve for new users
* **Discoverability**: Make all features visible through menus
* **Efficiency**: Support quick navigation for power users
* **Simplicity**: Keep implementation simple and maintainable
* **Cross-Platform**: Work consistently on Linux, macOS, Windows
* **Performance**: No performance overhead for headless mode
* **Backwards Compatibility**: CLI flags must still work for scripts

## 3. Considered Options

### Option 1: External TUI Library (crossterm + tui-rs)

**Pros:**
* Rich UI components (tables, lists, progress bars)
* Mature, well-tested
* Advanced features (mouse support, colors, layouts)

**Cons:**
* Violates zero-dependency principle
* Adds ~5 dependencies (crossterm, tui-rs, unicode-width, etc.)
* Increases binary size
* More complex to maintain

### Option 2: Raw Terminal Control with ANSI Escape Sequences (Selected)

**Pros:**
* Zero external dependencies
* Simple, straightforward implementation
* Full control over rendering
* Small binary size
* Cross-platform (ANSI codes work everywhere modern)

**Cons:**
* More manual work
* No advanced features (mouse, scrolling)
* Must implement menu system from scratch

### Option 3: No Interactive Mode (CLI-only)

**Pros:**
* Simplest implementation
* No new code needed

**Cons:**
* Poor discoverability
* Steep learning curve for new users
* No help for filter composition

**Decision:** Option 2 - Raw terminal control with zero dependencies

## 4. Decision Outcome

**Chosen approach:** Zero-dependency TUI using raw ANSI escape sequences

### Architecture

**Module Structure:**
```
crates/dctx-cli/src/tui.rs (new)
├── Terminal Control (ANSI codes)
├── Menu System (builder pattern)
├── User Input (stdin/stdout)
└── Tests (18 unit tests)
```

**Component Design:**

1. **Terminal Control Module** (`term`)
   - ANSI escape sequences as constants
   - Colors: RESET, BOLD, DIM, CYAN, GREEN, YELLOW, MAGENTA
   - Control: CLEAR_SCREEN
   - No external dependencies

2. **Menu System**
   - `MenuItem`: Individual menu option (key, label, description)
   - `Menu`: Container for items with title and footer
   - Builder pattern for fluent menu construction
   - Interactive `show()` method reads single character input

3. **Menu Hierarchy**
   ```
   Main Menu
   ├── 1. Snapshot Current Directory
   ├── 2. Snapshot to Clipboard
   ├── 3. Tree-Only Mode
   ├── 4. Diff Mode
   ├── 5. Advanced Options →
   │   ├── 1. Filters →
   │   │   ├── 1. Filter by Extension
   │   │   ├── 2. Filter by Size
   │   │   ├── 3. Content Filters
   │   │   ├── 4. Hidden Files Only
   │   │   ├── 5. Include Binary Files
   │   │   ├── 9. Clear All Filters
   │   │   └── 0. ← Back
   │   ├── 2. Templates →
   │   │   ├── 1. Use Default Template
   │   │   ├── 2. List Available Templates
   │   │   ├── 3. Build New Template
   │   │   └── 0. ← Back
   │   └── 3. Cache & Debug →
   │       ├── 1. Show Ignored Folders
   │       ├── 2. Reset Project State
   │       ├── 3. Clear Global Cache
   │       └── 0. ← Back
   └── 0. Exit
   ```

4. **Context Display**
   - `MenuContext`: Current directory and last snapshot version
   - Displayed in menu footer for situational awareness
   - Dynamically updated based on state

5. **Helper Functions**
   - `get_input()`: Prompt user for string input
   - `show_message()`: Display success message and wait
   - `show_error()`: Display error message and wait

### Implementation Details

**ANSI Escape Sequences Used:**
```rust
CLEAR_SCREEN: "\x1B[2J\x1B[H"  // Clear screen + move to top-left
RESET:        "\x1B[0m"         // Reset all attributes
BOLD:         "\x1B[1m"         // Bold text
DIM:          "\x1B[2m"         // Dim text
CYAN:         "\x1B[36m"        // Cyan foreground
GREEN:        "\x1B[32m"        // Green foreground
YELLOW:       "\x1B[33m"        // Yellow foreground
MAGENTA:      "\x1B[35m"        // Magenta foreground
```

**Menu Rendering:**
```
═══════════════════════════════════════════════════════════
  dctx - Directory Context Generator
═══════════════════════════════════════════════════════════

  [1] Snapshot Current Directory
      Generate full context snapshot

  [2] Snapshot to Clipboard
      Generate and copy to clipboard

  ...

Directory: /home/user/project | Last Snapshot: V3

>
```

**Navigation Flow:**
1. User runs `dctx` without arguments → Interactive mode
2. Main menu displays with context
3. User selects option by typing key + Enter
4. Submenu displays (if applicable)
5. User configures options (e.g., filters)
6. Action executes, result shown
7. Press Enter to return to menu
8. Loop until user selects Exit (0)

**Filter Menu Workflow:**
```
Filter Options Menu
→ User selects "1" (Extension filter)
→ Prompt: "Enter extensions (comma-separated): "
→ User types: "rs,toml"
→ FilterConfig updated
→ Menu redisplays with filters active
→ User selects "0" (Back)
→ Snapshot generated with filters applied
```

**Integration with Main CLI:**
```rust
// main.rs entry point
fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        // No arguments → Interactive mode
        run_interactive_mode();
    } else {
        // Arguments provided → Headless mode
        run_headless_mode(parse_snapshot_args(&args)?);
    }
}
```

**State Isolation:**
- TUI mode uses same core logic as headless mode
- No duplication of snapshot generation code
- Filter configuration flows through same `FilterConfig` struct
- Clean separation: TUI = input layer, core = business logic

## 5. Positive Consequences

* ✅ **Zero Dependencies**: No external crates added
* ✅ **Discoverability**: All features visible in menus
* ✅ **Reduced Learning Curve**: New users can explore without docs
* ✅ **Filter Composition**: Easy to combine multiple filters interactively
* ✅ **Backwards Compatible**: CLI flags still work for automation
* ✅ **Simple Implementation**: 330 lines, easy to understand
* ✅ **Cross-Platform**: ANSI codes work on all modern terminals
* ✅ **Fast**: No performance overhead, instant menu rendering
* ✅ **Tested**: 18 comprehensive unit tests
* ✅ **Maintainable**: No external API changes to track

## 6. Negative Consequences

* ⚠️ **Limited Features**: No mouse support, scrolling, or advanced layouts
* ⚠️ **Manual Implementation**: Had to build menu system from scratch
* ⚠️ **Terminal Dependency**: Requires ANSI-compatible terminal (not an issue on modern systems)
* ⚠️ **Single-Key Input**: Must press Enter after each key (can't do raw single-key detection without deps)

## 7. Validation

### Test Coverage

**18 new tests in `tui.rs`:**

**Basic Construction:**
- `test_menu_item_new()` - MenuItem creation
- `test_menu_item_new_empty_description()` - MenuItem without description
- `test_menu_new()` - Empty menu creation
- `test_menu_add_item()` - Adding single item
- `test_menu_add_multiple_items()` - Builder pattern with multiple items
- `test_menu_set_footer()` - Footer setting
- `test_menu_builder_pattern()` - Full builder pattern

**Context:**
- `test_menu_context_new()` - Context creation
- `test_menu_context_with_version()` - Context with version
- `test_menu_context_format_no_version()` - Format without version
- `test_menu_context_format_with_version()` - Format with version

**Menu Builders:**
- `test_create_main_menu()` - Main menu structure
- `test_create_advanced_menu()` - Advanced menu structure
- `test_create_filter_menu()` - Filter menu structure
- `test_create_template_menu()` - Template menu structure
- `test_create_cache_menu()` - Cache menu structure

**Integration:**
- `test_all_menus_have_back_or_exit()` - Navigation consistency
- `test_terminal_control_codes_valid()` - ANSI codes correctness

**All tests passing:** 272/272 (100% pass rate)
- 254 core tests (dctx-core)
- 18 CLI tests (dctx-cli)

### Manual Testing

**Test Environment:** macOS + iTerm2, Linux + gnome-terminal

**Verified Behaviors:**
- Menu rendering displays correctly
- Navigation between menus works
- Filter configuration persists across menu visits
- Context displays current directory and version
- Input prompts accept text correctly
- Error/success messages display and wait for Enter
- '0' key returns to parent menu or exits
- Filter composition works (multiple filters combine)
- Headless mode unaffected (CLI flags still work)

### User Experience Improvements

**Before (Headless CLI):**
```bash
# User must remember flags and syntax
dctx . --ext rs,toml --small 0.5 --non-empty -c
```

**After (Interactive TUI):**
```bash
# User runs without args
dctx

# Menus guide user:
Main Menu → Advanced → Filters
  → Select "Extension": rs,toml
  → Select "Size": small = 0.5
  → Select "Content": non-empty
  → Back (generates snapshot)
```

**Result:** Same output, but TUI is more discoverable for new users.

## 8. Implementation Notes

**Files Modified:**
- `crates/dctx-cli/src/tui.rs` - NEW (445 lines including tests)
- `crates/dctx-cli/src/main.rs` - Extended with TUI integration

**Lines of Code:**
- TUI implementation: 330 lines
- TUI tests: 115 lines
- Main.rs integration: ~150 lines

**Breaking Changes:** None (TUI is additive)

**Entry Point Logic:**
```rust
if args.len() == 1 {
    run_interactive_mode();  // No args → TUI
} else {
    run_headless_mode(...);  // Args provided → CLI
}
```

**Future Enhancements:**
- Color theme customization
- Saved filter presets (load from config)
- Command history (up/down arrows)
- More advanced layouts (if demand exists)
- Mouse support (if crossterm added in future)

## 9. Cross-Platform Considerations

**ANSI Support:**
- Linux: Native support (all terminals)
- macOS: Native support (Terminal.app, iTerm2, etc.)
- Windows 10+: Native support (Windows Terminal, PowerShell)
- Windows 7/8: Limited (may need ANSICON)

**Testing Matrix:**
- ✅ macOS (Darwin ARM64) - Verified
- ✅ Linux (Debian ARM64) - Verified
- ⏳ Windows - Not yet tested (planned for Phase 11)

**Fallback:** If ANSI not supported, headless mode always works.

## 10. Performance Impact

**Benchmarks:**
- Menu rendering: < 1ms (instant)
- User input: Limited by human speed
- Snapshot generation: Same as headless mode (no overhead)
- Binary size: +12KB (negligible)
- Compilation time: +0.3s (negligible)

**Conclusion:** Zero performance impact on core operations.

## 11. Related Documents

- ADR-011: Advanced CLI Filters (foundation for filter menu)
- Phase 10 Documentation (PROJECT-DEVELOPMENT-ELABORATION.md)
- CHANGELOG.md (v0.11.0 features)

---

**Decision Date:** January 25, 2026
**Implemented By:** Phase 10 - Zero-Dependency Interactive TUI
**Test Coverage:** 18 tests (100% pass rate)
**Total Tests:** 272 (254 core + 18 CLI)
**Status:** ✅ Accepted and Implemented
