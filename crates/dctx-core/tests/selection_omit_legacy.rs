//! Legacy omit-content behavior through the pure policy (D3-LEGACY-REMOVAL §3/§5 D3-07).
//! Depends on dctx-core selection and ItemContext; the core integration suite uses it.
//! All twelve legacy omit tests retain their names below; no retirements or differences.
//! Content omission is asserted as OmitContent, with Include controls beside each refusal.

#![forbid(unsafe_code)]

use dctx_core::selection::policy::RuleTexts;
use dctx_core::{FileHandlingDecision as Decision, ItemContext, SelectionPolicy};

fn builtin() -> SelectionPolicy {
    SelectionPolicy::from_texts(&RuleTexts::default())
}

fn decide(p: &SelectionPolicy, path: &str) -> Decision {
    p.decide(&ItemContext {
        relative_path: path,
        name: path.rsplit('/').next().unwrap(),
        is_directory: false,
        debug_mode: false,
    })
}

fn omitted(p: &SelectionPolicy, path: &str) {
    let decision = decide(p, path);
    assert_eq!(decision, Decision::OmitContent, "{path}");
    assert!(!decision.should_read_content());
    let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
    let control = if parent.is_empty() { "README.md".to_string() } else { format!("{parent}/README.md") };
    let decision = decide(p, &control);
    assert_eq!(decision, Decision::Include, "{control}");
    assert!(decision.should_read_content());
}

macro_rules! cases {
    ($name:ident, [$($path:literal),+ $(,)?]) => {
        #[test]
        fn $name() {
            let p = builtin();
            for path in [$($path),+] { omitted(&p, path); }
        }
    };
}

#[test]
fn test_hardcoded_omit_rules_not_empty() {
    let p = builtin();
    assert!(p.omit_layers().hardcoded > 0);
    assert_eq!(p.omit_layers().hardcoded, p.omit_rules().len());
    assert!(p.omit_rules().iter().any(|r| r.pattern == ".svg"));
    assert!(p.omit_rules().iter().any(|r| r.pattern == "package-lock.json"));
    omitted(&p, "icon.svg");
    omitted(&p, "package-lock.json");
}

cases!(test_svg_matched_by_hardcoded, ["icon.svg", "assets/logo.svg"]);
cases!(
    test_lockfiles_matched_by_hardcoded,
    ["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "Cargo.lock", "go.sum"]
);

#[test]
fn test_text_files_not_matched() {
    let p = builtin();
    for path in ["README.md", "package.json", "Cargo.toml"] {
        assert_eq!(decide(&p, path), Decision::Include, "{path}");
    }
    omitted(&p, "Cargo.lock");
}

cases!(
    test_lockfiles_at_root_level,
    [
        "Cargo.lock",
        "package-lock.json",
        "yarn.lock",
        "bun.lock",
        "go.sum",
        "Gemfile.lock",
        "poetry.lock",
        "composer.lock"
    ]
);
cases!(
    test_lockfiles_at_depth_one,
    [
        "frontend/Cargo.lock",
        "backend/package-lock.json",
        "web/pnpm-lock.yaml",
        "api/yarn.lock",
        "cli/bun.lock",
        "server/go.sum"
    ]
);
cases!(
    test_lockfiles_at_depth_three,
    [
        "apps/web/frontend/Cargo.lock",
        "packages/ui/components/package-lock.json",
        "crates/core/lib/pnpm-lock.yaml",
        "services/api/v2/yarn.lock"
    ]
);
cases!(
    test_lockfiles_at_depth_five,
    [
        "monorepo/apps/web/src/components/package-lock.json",
        "projects/backend/services/auth/lib/Cargo.lock",
        "workspace/packages/ui/design-system/tokens/yarn.lock"
    ]
);
cases!(
    test_svg_files_at_multiple_depths,
    ["logo.svg", "assets/icon.svg", "public/images/logo.svg", "src/assets/icons/social/twitter.svg"]
);

#[test]
fn test_env_files_at_multiple_depths() {
    let p = SelectionPolicy::from_texts(&RuleTexts { local_omit: Some(".env"), ..RuleTexts::default() });
    assert_eq!(p.omit_layers().local, 1);
    for path in [".env", "backend/.env", "apps/api/.env", "projects/services/auth/.env"] {
        omitted(&p, path);
        assert_eq!(decide(&builtin(), path), Decision::Include);
    }
}

#[test]
fn test_mixed_patterns_at_various_depths() {
    let p = builtin();
    for path in
        ["Cargo.lock", "frontend/package-lock.json", "apps/mobile/ios/Podfile.lock", "services/backend/api/go.sum"]
    {
        omitted(&p, path);
    }
    for path in ["frontend/package.json", "apps/api/Cargo.toml"] {
        assert_eq!(decide(&p, path), Decision::Include, "{path}");
    }
}

cases!(
    test_all_lockfile_types_at_depth_two,
    [
        "apps/web/package-lock.json",
        "apps/web/pnpm-lock.yaml",
        "apps/web/yarn.lock",
        "apps/web/bun.lockb",
        "apps/web/bun.lock",
        "crates/core/Cargo.lock",
        "services/api/go.sum",
        "backend/rails/Gemfile.lock",
        "services/ml/Pipfile.lock",
        "services/ml/poetry.lock",
        "services/ml/pdm.lock",
        "backend/laravel/composer.lock",
        "apps/ios/Podfile.lock",
        "apps/ios/Package.resolved",
        "apps/ios/Cartfile.resolved",
        "backend/dotnet/packages.lock.json",
        "backend/dotnet/paket.lock",
        "backend/phoenix/mix.lock",
        "backend/erlang/rebar.lock",
        "services/parser/stack.yaml.lock",
        "services/parser/cabal.project.freeze",
        "apps/mobile/pubspec.lock",
        "analytics/scripts/renv.lock",
        "scripts/tools/luarocks.lock"
    ]
);

#[test]
fn global_and_local_omit_negations_override_earlier_layers() {
    let p = SelectionPolicy::from_texts(&RuleTexts {
        global_omit: Some("!Cargo.lock\n*.data"),
        local_omit: Some("!keep.data\nCargo.lock"),
        ..RuleTexts::default()
    });
    assert_eq!(p.omit_layers().global, 2);
    assert_eq!(p.omit_layers().local, 2);
    omitted(&p, "Cargo.lock");
    omitted(&p, "other.data");
    assert_eq!(decide(&p, "keep.data"), Decision::Include);
    let global = SelectionPolicy::from_texts(&RuleTexts { global_omit: Some("!Cargo.lock"), ..RuleTexts::default() });
    assert_eq!(decide(&global, "Cargo.lock"), Decision::Include);
}
