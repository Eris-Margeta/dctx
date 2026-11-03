//! Reserved alias words and the bundled `dctx/git` catalog row with its digests.
//! Implements PLUGINS.md §3.5 (bundled manifest from the plugin-api fixtures).
//! Depends on [`super`] catalog types; the host still opts in.

use super::*;

/// Built-in command words a plugin alias cannot take (PLUGINS.md §3.5).
pub const RESERVED_ALIASES: &[&str] = &["plugin", "pack", "map", "cache", "template", "help", "version"];

/// The bundled `dctx/git` manifest from the plugin-api fixtures. The host still opts in.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn bundled_git_plugin() -> KnownPlugin {
    KnownPlugin {
        id: "dctx/git".to_string(),
        version: "1.1.0".to_string(),
        digest: Some(hex32("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")),
        manifest_digest: Some(hex32("06103b8c8f458e406fc73a174110ea560970c4183ce930dfd642fe13013f4f44")),
        publisher_key: "fixture-key-1".to_string(),
        origin: PluginOrigin::Bundled,
        hooks: vec!["before-pack-output-written".to_string()],
        alias: "git".to_string(),
        capabilities: vec![
            "file.read:.gitignore".to_string(),
            "file.write:.gitignore".to_string(),
            "program:git.add-ignore".to_string(),
            "resource:git-admin:stage-ignore".to_string(),
        ],
        records: vec![
            Capability::FileRead { path: ".gitignore".to_string(), max_bytes: 1_048_576 },
            Capability::FileWrite {
                path: ".gitignore".to_string(),
                create: false,
                max_growth_bytes: 4096,
                max_bytes: 1_052_672,
            },
            Capability::Program {
                operation: "git.add-ignore".to_string(),
                resources: vec!["git-admin:stage-ignore".to_string()],
                max_output_bytes: 65_536,
            },
        ],
        fields: vec![
            FieldSpec {
                name: "stage".to_string(),
                kind: FieldKind::Bool,
                default_value: Some(ConfigValue::Bool(true)),
            },
            FieldSpec {
                name: "create_missing".to_string(),
                kind: FieldKind::Enum { options: vec![ConfigValue::Bool(false)] },
                default_value: Some(ConfigValue::Bool(false)),
            },
            FieldSpec {
                name: "marker".to_string(),
                kind: FieldKind::Enum { options: vec![ConfigValue::Str("host-version".to_string())] },
                default_value: Some(ConfigValue::Str("host-version".to_string())),
            },
        ],
    }
}

fn hex32(text: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    let bytes = text.as_bytes();
    for index in 0..32 {
        let hi = hex_val(bytes[index * 2]);
        let lo = hex_val(bytes[index * 2 + 1]);
        out[index] = (hi << 4) | lo;
    }
    out
}

fn hex_val(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => 0,
    }
}
