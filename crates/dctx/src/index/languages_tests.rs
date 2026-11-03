//! Locks [`crate::languages`] to the extension dispatch of [`super::extract`], to the heuristic families of
//! [`super::generic`] (confidence `guess`) and to the document formats (tier `docs`).

use super::extract::{language_of, EXTENSIONS, INFRA_BASENAMES, RUBY_BASENAMES};
use crate::languages::{by_tag, Confidence, Language, Tier, LANGUAGES, TAGS};

/// The registry entry for `tag`; panics with the tag when it is missing.
fn entry(tag: &str) -> &'static Language {
    by_tag(tag).unwrap_or_else(|| panic!("no registry entry for tag {tag}"))
}

#[test]
fn every_dispatch_extension_and_file_name_is_in_the_registry_under_its_tag() {
    for (extension, tag) in EXTENSIONS {
        assert!(entry(tag).extensions.contains(extension), "{extension} -> {tag}");
    }
    for name in RUBY_BASENAMES {
        assert!(entry("ruby").files.contains(name), "{name} -> ruby");
    }
    for (name, tag) in INFRA_BASENAMES {
        assert!(entry(tag).files.contains(name), "{name} -> {tag}");
    }
}

#[test]
fn every_registry_extension_and_file_name_dispatches_to_its_tag() {
    let mut extensions = 0usize;
    for language in LANGUAGES {
        assert!(!language.extensions.is_empty() || !language.files.is_empty(), "{} selects no file", language.tag);
        for extension in language.extensions {
            assert_eq!(language_of(&format!("a.{extension}")), language.tag, "{extension}");
            extensions += 1;
        }
        for file in language.files {
            assert_eq!(language_of(&file.replacen('*', "a", 1)), language.tag, "{file}");
        }
    }
    assert_eq!(extensions, EXTENSIONS.len(), "an extension is listed twice in the registry");
}

#[test]
fn tags_are_unique_and_the_tag_list_follows_the_entries() {
    let tags: Vec<&str> = LANGUAGES.iter().map(|language| language.tag).collect();
    assert_eq!(tags, TAGS);
    let mut unique = tags.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), tags.len(), "duplicate tag");
    assert_eq!(language_of("a.unknown"), "opaque");
    assert!(by_tag("opaque").is_none());
}

#[test]
fn guess_confidence_is_exactly_the_heuristic_families() {
    for language in LANGUAGES {
        assert_eq!(language.confidence == Confidence::Guess, super::generic::handles(language.tag), "{}", language.tag);
    }
}

#[test]
fn docs_tier_is_exactly_markdown_and_the_text_formats() {
    for language in LANGUAGES {
        let docs = language.tag == "markdown" || super::text_docs::handles(language.tag);
        assert_eq!(language.tier == Tier::Docs, docs, "{}", language.tag);
    }
}

#[test]
fn promised_tiers_of_the_benchmark_languages() {
    // Shell has same-file command-word call guesses, but the benchmark probe found no shell call edges: v1 promises symbols only.
    assert_eq!(entry("shell").tier, Tier::Outline);
    assert_eq!(entry("markdown").tier, Tier::Docs);
for tag in ["rust", "python", "javascript", "typescript", "go", "erlang", "powershell", "perl", "verilog"] {
        assert_eq!(entry(tag).tier, Tier::Full, "{tag}");
    }
    for tag in ["config", "make", "docker", "proto", "pascal", "graphql", "nix"] {
        assert_eq!(entry(tag).tier, Tier::Outline, "{tag}");
    }
    assert!(by_tag("fortran").is_none(), "fortran is not in v1");
    assert!(by_tag("verilog").is_some(), "verilog is present");
    let tiers: Vec<&str> = Tier::ALL.iter().map(|tier| tier.as_str()).collect();
    assert_eq!(tiers, ["full", "outline", "docs"]);
    assert!(LANGUAGES.iter().all(|language| Tier::ALL.contains(&language.tier)));
}

#[test]
fn capabilities_list_the_registry_tags() {
    assert_eq!(crate::Capabilities::current().languages, TAGS);
}
