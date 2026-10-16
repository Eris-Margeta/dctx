//! Built-in ignore and omission patterns.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-014/015b.
//! Uses standard-library types; consumed by scan clients.

pub(crate) const HARDCODED_IGNORE_RULES: &[&str] = &[
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
    "target/", // Rust specific
    "bin/",
    "obj/",
    ".venv/",
    "venv/",
    "__pycache__/",
    "*.pyc",
    ".dctxignore",
    "*.dctx",
    "*.dircontxt", // Legacy dctx output format
    ".dctx/",      // dctx local output directory
    ".cursor/",    // Cursor IDE cache
    ".aider/",     // Aider cache
    ".eris-code/", // Eris Code cache
    ".continue/",  // Continue.dev cache
];

pub(crate) const HARDCODED_OMIT_CONTENT_RULES: &[&str] = &[
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
