//! CLI help, including the build's feature and precision limits.
//! Implements ARCHITECTURE-PROPOSAL §2 capabilities and §4 command list.
//! Depends on [`dctx::Capabilities`]. `dctx --help` and `dctx --version --verbose` print [`text`] or [`limits`].
//! Limits describe this binary. They are not a promise that a host tokenizer was linked.

use dctx::Capabilities;

/// Full help page, including snapshot compatibility and the facade commands.
pub fn text() -> String {
    let mut out = String::from(
        "\
Usage: dctx [PATH] [OPTIONS]
       dctx pack [PATH] [--output PATH] [--clipboard] [--json]
       dctx map <map|find|neighbors|explain|read> [--json]
       dctx cache status [--json] [--root PATH]
       dctx cache gc [--dry-run] [--root PATH] [--max-bytes N] [--legacy]
       dctx cache purge --root PATH [--dry-run] [--yes]
       dctx cache purge --all [--dry-run] [--yes]
       dctx template build <PATH> [--output PATH]
       dctx template list [--dir PATH]
       dctx plugin search|install|list|info|enable|disable|remove|update|verify
       dctx git <status|protect>

Explicit `dctx pack` writes stdout, the clipboard, or `--output`. It does not create a project text file.
`dctx .` remains the legacy snapshot alias and still writes its documented project text.
`dctx plugin list` and `dctx --help` do not create a project `.dctx/` directory or start a plugin.
`dctx git` routes to the bundled dctx/git plugin. It is not a Git passthrough.
Cache purge requires `--root PATH` or `--all`, and `--yes` unless `--dry-run` is set.
`dctx cache gc --legacy` reads only exports the caller already registered. It does not search the home directory.

Snapshot Options:
  -c, --clipboard       Copy to clipboard
  -d, --diff            Output diff only (if available)
  -t, --tree            Tree-Only mode (skips file content)
  -r, --reset           Force V1 reset
  -h, --help            Show help
  -v, --version         Show version
  --verbose             With --version, also show feature and precision limits

Filter Options:
  --ext <extensions>    Only include files with these extensions
  --binary              Include binary files (excluded by default)
  --empty               Show only empty files (0 bytes)
  --non-empty           Show only non-empty files
  --large <MB>          Show only files larger than X megabytes
  --small <MB>          Show only files smaller than X megabytes
  --hidden              Show ONLY hidden files and folders (starting with .)
  --debug-ignored       Show ignored files in the tree without reading them
  -V, --debug-verbose   Show ignore decision traces

Template Commands:
  template build <PATH>  Compile a template through the facade
  template list          List explicit compiled plans; no home-directory scan

Examples:
  dctx .                      Legacy snapshot of the current directory
  dctx pack .                 Pack to stdout and write no project text file
  dctx pack . --output out.txt
  dctx cache status --json
  dctx cache gc --dry-run
  dctx cache purge --root /path --dry-run
  dctx template build policy.dctx_template

",
    );
    out.push_str(&limits());
    out
}

/// Feature, storage and precision limits for this build.
pub fn limits() -> String {
    let caps = Capabilities::current();
    let mut out = String::new();
    out.push_str("Feature and precision limits:\n");
    push(&mut out, "facade_version", caps.facade_version);
    push(&mut out, "profile", caps.profile);
    out.push_str(&format!("wire_version: {}\n", caps.wire_version));
    out.push_str(&format!("segment_version: {}\n", caps.segment_version));
    push(&mut out, "storage", caps.storage);
    push(&mut out, "features", &caps.features.join(","));
    push(&mut out, "parsers", &caps.parsers.join(","));
    push(&mut out, "languages", &caps.languages.join(","));
    push(&mut out, "unsafe_boundary", caps.unsafe_boundary);
    out.push_str(&format!("native_code: {}\n", caps.native_code));
    out.push_str(&format!("git: {}\n", caps.git));
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
        assert!(page.contains("--yes"));
        assert!(page.contains("does not create a project text file"));
        assert!(page.contains("dctx ."));
        let limits = limits();
        assert!(limits.contains("precision:"));
        assert!(limits.contains("not a tokenizer guarantee"));
        assert!(limits.contains("git: false"));
        assert!(limits.contains("native_code: false"));
        assert!(limits.contains("structural"));
        assert!(limits.contains("features:"));
        assert!(page.contains(&limits));
    }
}
