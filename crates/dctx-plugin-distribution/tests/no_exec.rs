//! Structural evidence for "no code runs during install": the crate source contains no process,
//! dynamic-loading or shell primitive, and no port offers execution. PLUGINS.md §5.3, I-INTEGRITY.

const SOURCES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("../src/lib.rs")),
    ("error.rs", include_str!("../src/error.rs")),
    ("ports.rs", include_str!("../src/ports.rs")),
    ("fakes.rs", include_str!("../src/fakes.rs")),
    ("service/mod.rs", include_str!("../src/service/mod.rs")),
    ("service/download.rs", include_str!("../src/service/download.rs")),
    ("service/install.rs", include_str!("../src/service/install.rs")),
    ("service/manage.rs", include_str!("../src/service/manage.rs")),
    ("domain/mod.rs", include_str!("../src/domain/mod.rs")),
    ("domain/admission.rs", include_str!("../src/domain/admission.rs")),
    ("domain/bundle.rs", include_str!("../src/domain/bundle.rs")),
    ("domain/consent.rs", include_str!("../src/domain/consent.rs")),
    ("domain/digest.rs", include_str!("../src/domain/digest.rs")),
    ("domain/fields.rs", include_str!("../src/domain/fields.rs")),
    ("domain/index.rs", include_str!("../src/domain/index.rs")),
    ("domain/manifest.rs", include_str!("../src/domain/manifest.rs")),
    ("domain/names.rs", include_str!("../src/domain/names.rs")),
    ("domain/origin.rs", include_str!("../src/domain/origin.rs")),
    ("domain/registry.rs", include_str!("../src/domain/registry.rs")),
    ("domain/registry_codec.rs", include_str!("../src/domain/registry_codec.rs")),
    ("domain/signature.rs", include_str!("../src/domain/signature.rs")),
    ("domain/sums.rs", include_str!("../src/domain/sums.rs")),
    ("domain/trust.rs", include_str!("../src/domain/trust.rs")),
];

#[test]
fn no_execution_primitives_in_the_crate() {
    let forbidden = ["std::process", "process::Command", "Command::new", "libloading", "dlopen", "/bin/sh", "set_permissions", "PermissionsExt"];
    for (file, source) in SOURCES {
        for needle in forbidden {
            assert!(!source.contains(needle), "{file} contains {needle}");
        }
        assert!(source.starts_with("//!"), "{file} lacks its header doc");
    }
}

#[test]
fn every_source_file_is_listed() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let mut found = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    found.sort();
    let mut listed: Vec<String> = SOURCES.iter().map(|(file, _)| (*file).to_owned()).collect();
    listed.sort();
    assert_eq!(found, listed);
}
