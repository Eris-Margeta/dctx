// SPDX-License-Identifier: Apache-2.0
//! Built-in plain ignore and omit lines.
//! Implements the product privacy defaults used by DX-B3 rule loading.
//! Depends on [`super::rule`]. These are text patterns, not VCS metadata reads.

use super::rule::{Rule, parse_rule};

const HARDCODED_IGNORE: &[&str] = &[
    ".git/",
    ".turbo/",
    ".pnpm-store/",
    ".localstack/",
    ".svn/",
    ".hg/",
    "node_modules/",
    "bower_components/",
    ".DS_Store",
    "Thumbs.db",
    "dist/",
    "build/",
    "target/",
    "bin/",
    "obj/",
    ".venv/",
    "venv/",
    "__pycache__/",
    "*.pyc",
    ".dctxignore",
    "*.dctx",
    "*.dircontxt",
    ".dctx/",
    ".cursor/",
    ".aider/",
    ".eris-code/",
    ".continue/",
];

const HARDCODED_OMIT: &[&str] = &[
    "*.svg",
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

pub(crate) fn builtin_ignore() -> Vec<Rule> {
    compile(HARDCODED_IGNORE)
}

pub(crate) fn builtin_omit() -> Vec<Rule> {
    compile(HARDCODED_OMIT)
}

fn compile(lines: &[&str]) -> Vec<Rule> {
    lines.iter().filter_map(|line| parse_rule(line)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SelectionPolicy;
    use crate::domain::selection::{FileHandlingDecision, ItemContext};

    #[test]
    fn git_directory_is_a_plain_exclude_pattern() {
        let policy = SelectionPolicy::from_rules(builtin_ignore(), builtin_omit());
        let git = policy.decide(&ItemContext {
            relative_path: ".git",
            name: ".git",
            is_directory: true,
            debug_mode: false,
        });
        assert_eq!(git, FileHandlingDecision::Exclude);
        let source = policy.decide(&ItemContext {
            relative_path: "src/main.rs",
            name: "main.rs",
            is_directory: false,
            debug_mode: false,
        });
        assert_eq!(source, FileHandlingDecision::Include);
        let lock = policy.decide(&ItemContext {
            relative_path: "Cargo.lock",
            name: "Cargo.lock",
            is_directory: false,
            debug_mode: false,
        });
        assert_eq!(lock, FileHandlingDecision::OmitContent);
    }
}
