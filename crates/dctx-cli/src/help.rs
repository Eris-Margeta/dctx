// SPDX-License-Identifier: Apache-2.0
//! CLI help, including the build's feature and precision limits.
//! Implements ARCHITECTURE-PROPOSAL §2 capabilities and §4 command list.
//! Depends on [`dctx::Capabilities`]. `dctx --help` and `dctx --version --verbose` print [`text`] or [`limits`].
//! Limits describe this binary. They are not a promise that a host tokenizer was linked.

use crate::tui::{Menu, MenuContext};
use dctx::Capabilities;
use std::fmt::Write as _;

/// Build the main menu
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn create_main_menu(context: &MenuContext) -> Menu {
    let mut menu = Menu::new("dctx - Directory Context Generator");

    menu.add_item(
        '1',
        "Pack Current Directory",
        "Write source context to stdout",
    )
    .add_item('2', "Pack to Clipboard", "Copy source context to clipboard")
    .add_item('3', "Templates →", "Build and list templates")
    .add_item('4', "Cache →", "Inspect and collect the managed cache")
    .add_item('0', "Exit", "")
    .set_footer(&context.format());

    menu
}

/// Build the template submenu
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn create_template_menu() -> Menu {
    let mut menu = Menu::new("Template Management");

    menu.add_item('1', "Use Default Template", "Standard dctx output format")
        .add_item('2', "List Available Templates", "Show compiled templates")
        .add_item('3', "Build New Template", "Compile a .dctx_template file")
        .add_item('0', "← Back", "")
        .set_footer("Templates customize output format");

    menu
}

/// Build the managed cache submenu
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn create_cache_menu() -> Menu {
    let mut menu = Menu::new("Cache");

    menu.add_item('1', "Status", "Show managed cache usage")
        .add_item(
            '2',
            "Preview Collection",
            "Show a dry run of cache collection",
        )
        .add_item('3', "Collect Cache", "Collect unused managed cache entries")
        .add_item('0', "← Back", "")
        .set_footer("Manage cached state and diagnostics");

    menu
}

/// Full help page for the explicit facade commands.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn text() -> String {
    let mut out = String::from(
        "\
Usage: dctx <COMMAND> [OPTIONS]
       dctx pack [PATH] [--output PATH] [--clipboard] [--json]
       dctx pack [PATH] --for <TEXT> [--budget TOKENS]
       dctx map <map|find|neighbors|explain|read|skeleton|grep|hubs|path> [--json]
       dctx context TARGET [--place PATH] [--budget N] [--json] [--session ID --dedupe]
       dctx rpc
       dctx mcp [PATH]
       dctx serve [--mcp] [PATH]
       dctx read <TARGET> [OPTIONS]
       dctx languages [--json|--markdown]
       dctx cache status [--json] [--root PATH]
       dctx cache gc [--dry-run] [--root PATH] [--max-bytes N]
       dctx cache purge --root PATH [--dry-run] [--yes]
       dctx cache purge --all [--dry-run] [--yes]
       dctx cache config [auto on|off] [--max-bytes N] [--ttl-days N]
       dctx cache auto [--json]
       dctx template build <PATH> [--output PATH]
       dctx template list [--dir PATH]
       dctx plugin search|install|list|info|enable|disable|remove|update|verify
       dctx git <status|protect>

`dctx languages` lists every supported language with its tier: full (symbols and call edges), outline (symbols only) or docs (sections).
`dctx rpc` serves the versioned engine protocol for language packages (send hello first; docs/protocol/README.md).
`dctx mcp` serves the map verbs as MCP tools over stdio (JSON-RPC 2.0, one message per line) for Claude Code, Cursor and Codex.
`dctx serve` and `dctx serve --mcp` use that same server; `dctx read` uses `dctx map read`.
Explicit `dctx pack` writes stdout, the clipboard, or `--output`. It does not create a project text file.
`dctx pack --for <TEXT>` prints task context for an agent card instead: map find, neighbors and read of the names in TEXT, cut to `--budget` estimated tokens (default 3000).
`dctx plugin list` and `dctx --help` do not create a project `.dctx/` directory or start a plugin.
`dctx git` routes to the bundled dctx/git plugin. It is not a Git passthrough.
Cache purge requires `--root PATH` or `--all`, and `--yes` unless `--dry-run` is set.
`dctx cache config` shows the persisted policy. `auto on|off` enables auto-clean, `--max-bytes` tightens the system cap, and `--ttl-days` sets idle overlay and preview lifetime.
`dctx cache auto` prints the last auto-clean time, the bytes it reclaimed and the next due time.
pack, map, template, plugin and the TUI run one bounded auto-clean when it is due. They print nothing unless bytes were reclaimed.
`--no-auto-clean` and `DCTX_AUTO_CLEAN=0` disable that slice. `DCTX_CACHE_DIR` selects the cache root.

General Options:
  -h, --help            Show help
  -v, --version         Show version
  --verbose            With --version, also show feature and precision limits

Run `dctx pack --help` for pack options and filters.

Template Commands:
  template build <PATH>  Compile a template through the facade
  template list          List explicit compiled plans; no home-directory scan

Examples:
  dctx pack .                 Pack to stdout and write no project text file
  dctx pack . --output out.txt
  dctx languages --json
  dctx cache status --json
  dctx cache gc --dry-run
  dctx cache purge --root /path --dry-run
  dctx cache config auto off --max-bytes 1000000 --ttl-days 7
  dctx cache auto
  dctx template build policy.dctx_template

",
    );
    out.push_str(&limits());
    out
}

/// Feature, storage and precision limits for this build.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn limits() -> String {
    let caps = Capabilities::current();
    let mut out = String::new();
    out.push_str("Feature and precision limits:\n");
    push(&mut out, "facade_version", caps.facade_version);
    push(&mut out, "profile", caps.profile);
    let _ = writeln!(out, "wire_version: {}", caps.wire_version);
    let _ = writeln!(out, "segment_version: {}", caps.segment_version);
    push(&mut out, "storage", caps.storage);
    push(&mut out, "features", &caps.features.join(","));
    push(&mut out, "parsers", &caps.parsers.join(","));
    // The facade registry (`dctx::LANGUAGES`) is the source of this list and of `dctx languages`.
    push(&mut out, "languages", &caps.languages.join(","));
    push(&mut out, "unsafe_boundary", caps.unsafe_boundary);
    let _ = writeln!(out, "native_code: {}", caps.native_code);
    let _ = writeln!(out, "git: {}", caps.git);
    push(&mut out, "subprocess", caps.subprocess);
    out.push_str(
        "\
precision: the structural extractor reports declarations, imports, headings and conservative calls.
It does not promise exact semantic calls, name or type resolution, or macro expansion.
A syntax-rust build adds syn for supported Rust syntax only, still without type checking.
token estimate: ceil(2 * UTF-8 bytes / 7) is an estimate, not a tokenizer guarantee.
cache cap: 2 GiB hard, 1.5 GiB soft watermark, 1.2 GiB eviction target.
",
    );
    let _ =
        writeln!(out,
        "cache ttl: idle overlays and previews {} days, unpinned roots {} days, staging {} day.",
        dctx_adapter_cache::OVERLAY_TTL_MS / dctx_adapter_cache::DAY_MS,
        dctx_adapter_cache::ROOT_TTL_MS / dctx_adapter_cache::DAY_MS,
        dctx_adapter_cache::STAGING_TTL_MS / dctx_adapter_cache::DAY_MS
    );
    out.push_str("auto-clean interval: 1 day.\n");
    out
}

fn push(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_names_commands_and_the_precision_limit() {
        let page = text();
        assert!(page.contains("dctx pack"));
        assert!(page.contains("dctx cache purge"));
        assert!(page.contains("dctx cache config"));
        assert!(page.contains("dctx cache auto"));
        assert!(page.contains("--ttl-days"));
        assert!(page.contains("--no-auto-clean"));
        assert!(page.contains("DCTX_AUTO_CLEAN=0"));
        assert!(page.contains("--yes"));
        assert!(page.contains("does not create a project text file"));
        assert!(!page.contains("dctx ."));
        assert!(!page.contains("legacy"));
        assert!(!page.contains("--diff"));
        assert!(!page.contains("--reset"));
        let limits = limits();
        assert!(limits.contains("precision:"));
        assert!(limits.contains("not a tokenizer guarantee"));
        assert!(limits.contains("git: false"));
        assert!(limits.contains("native_code: false"));
        assert!(limits.contains("structural"));
        assert!(limits.contains("features:"));
        assert!(limits.contains("cache ttl:"));
        assert!(limits.contains("7 days"));
        assert!(limits.contains("30 days"));
        assert!(limits.contains("auto-clean interval: 1 day"));
        assert!(page.contains(&limits));
    }

    #[test]
    fn capability_languages_come_from_the_registry() {
        let tags: Vec<&str> = dctx::LANGUAGES
            .iter()
            .map(|language| language.tag)
            .collect();
        assert!(limits().contains(&format!("languages: {}\n", tags.join(","))));
        assert!(text().contains("dctx languages [--json|--markdown]"));
    }
}
