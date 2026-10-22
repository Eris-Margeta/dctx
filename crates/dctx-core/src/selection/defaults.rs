//! Built-in (layer 1) ignore and omit-content rules compiled into every build.
//! Implements the hardcoded layer of the three-layer selection policy (hardcoded → global → local).
//! Depends on nothing; plain rule text parsed by [`super::rule`].
//! Used by [`super::policy::SelectionPolicy::from_texts`] and re-exported by `crate::config`.

/// Base rules that exclude items universally unless a later rule negates them.
pub const HARDCODED_IGNORE_RULES: &[&str] = &[
    // Version Control
    ".git/",
    ".svn/",
    ".hg/",
    // Dependencies
    "node_modules/",
    "bower_components/",
    // System & IDE
    ".DS_Store",
    "Thumbs.db",
    // Build Output (Generic)
    "dist/",
    "build/",
    "target/", // Rust specific
    "bin/",
    "obj/",
    // Python
    ".venv/",
    "venv/",
    "__pycache__/",
    "*.pyc",
    // Self-referential (prevent tools scanning themselves)
    ".dctxignore",
    "*.dctx",
    "*.dircontxt", // Legacy dctx output format
    ".dctx/",      // dctx local output directory
    // AI Tool Caches (prevent scanning AI assistant state)
    ".cursor/",    // Cursor IDE cache
    ".aider/",     // Aider cache
    ".eris-code/", // Eris Code cache
    ".continue/",  // Continue.dev cache
];

/// Files that appear in the tree with their content omitted.
pub const HARDCODED_OMIT_CONTENT_RULES: &[&str] = &[
    // SVG Files (replacing deprecated omit_svg_content config)
    "*.svg",
    // Package Manager Lockfiles
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "bun.lock",
    "Cargo.lock",
    "go.sum",
    "Gemfile.lock",
    "Pipfile.lock",
    "poetry.lock",
    "pdm.lock",
    "composer.lock",
    "Package.resolved",
    "Podfile.lock",
    "Cartfile.resolved",
    "packages.lock.json",
    "paket.lock",
    "mix.lock",
    "rebar.lock",
    "stack.yaml.lock",
    "cabal.project.freeze",
    "pubspec.lock",
    "renv.lock",
    "luarocks.lock",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::rule::parse_rule_lines;

    #[test]
    fn test_hardcoded_ignore_rules_not_empty() {
        assert_ne!(HARDCODED_IGNORE_RULES, [] as [&str; 0]);
        assert!(HARDCODED_IGNORE_RULES.contains(&".git/"));
        assert!(HARDCODED_IGNORE_RULES.contains(&"node_modules/"));
        assert!(HARDCODED_IGNORE_RULES.contains(&"target/"));
    }

    #[test]
    fn every_builtin_line_parses_to_a_rule() {
        assert_eq!(parse_rule_lines(HARDCODED_IGNORE_RULES).len(), HARDCODED_IGNORE_RULES.len());
        assert_eq!(
            parse_rule_lines(HARDCODED_OMIT_CONTENT_RULES).len(),
            HARDCODED_OMIT_CONTENT_RULES.len()
        );
    }
}
