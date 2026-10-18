//! Content pins for the dctx worktree and the generated fixture.
//! Implements ARCHITECTURE-PROPOSAL §7 Stage 0 reproducible corpus hashes.
//! Depends on dctx-hash and the manifest skip lists; the measure adapter stores the digest.

use crate::error::BenchError;
use crate::manifest::DctxSpec;
use dctx_hash::Sha256;
use std::{fs, path::Path};

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Aggregate hash: for each sorted path, absorb `path`, NUL, file sha256 hex, NUL.
///
/// # Errors
/// Returns the first-party hasher's typed length refusal. Corpus files fit.
pub fn hash_sequences(files: &[(&str, &[u8])]) -> Result<String, BenchError> {
    let mut total = Sha256::new();
    for (path, bytes) in files {
        absorb(&mut total, path, bytes)?;
    }
    Ok(hex(total.finalize().as_bytes()))
}

fn absorb(total: &mut Sha256, path: &str, bytes: &[u8]) -> Result<(), BenchError> {
    let mut file = Sha256::new();
    file.update(bytes).map_err(hash_error)?;
    let file_hex = hex(file.finalize().as_bytes());
    total.update(path.as_bytes()).map_err(hash_error)?;
    total.update(&[0]).map_err(hash_error)?;
    total.update(file_hex.as_bytes()).map_err(hash_error)?;
    total.update(&[0]).map_err(hash_error)?;
    Ok(())
}

fn hash_error(error: dctx_hash::HashError) -> BenchError {
    BenchError::Message(error.to_string())
}

/// Walks `root`, skipping the manifest's directories and prefixes, without following symlinks.
pub fn hash_tree(root: &Path, spec: &DctxSpec) -> Result<TreePin, BenchError> {
    let mut paths = Vec::new();
    collect(root, root, spec, &mut paths)?;
    paths.sort();
    let mut total = Sha256::new();
    let mut file_count = 0_u64;
    let mut utf8_bytes = 0_u64;
    for path in paths {
        let rel = path.strip_prefix(root).map_err(|_| {
            BenchError::Message(format!("path {} left the pin root", path.display()))
        })?;
        let rel = rel.to_str().ok_or_else(|| {
            BenchError::Message(format!("path {} is not utf-8", path.display()))
        })?;
        let bytes = fs::read(&path)?;
        utf8_bytes = utf8_bytes.saturating_add(bytes.len() as u64);
        file_count = file_count.saturating_add(1);
        absorb(&mut total, rel, &bytes)?;
    }
    Ok(TreePin {
        content_sha256: hex(total.finalize().as_bytes()),
        file_count,
        byte_count: utf8_bytes,
    })
}

/// Result of hashing a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreePin {
    /// Aggregate content digest.
    pub content_sha256: String,
    /// Files included in the digest.
    pub file_count: u64,
    /// Raw file bytes included in the digest.
    pub byte_count: u64,
}

fn collect(
    root: &Path,
    dir: &Path,
    spec: &DctxSpec,
    out: &mut Vec<std::path::PathBuf>,
) -> Result<(), BenchError> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let rel = match path.strip_prefix(root) {
            Ok(rel) => rel.to_str().ok_or_else(|| {
                BenchError::Message(format!("path {} is not utf-8", path.display()))
            })?,
            Err(_) => {
                return Err(BenchError::Message(format!(
                    "path {} left the pin root",
                    path.display()
                )))
            }
        };
        if skipped(rel, spec) {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect(root, &path, spec, out)?;
        } else if kind.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn sensitive_name(name: &str) -> bool {
    name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with(".env.")
}

fn skipped(rel: &str, spec: &DctxSpec) -> bool {
    if rel.is_empty() {
        return false;
    }
    if rel.split('/').any(|part| {
        spec.skip_dir_names.iter().any(|name| name == part) || sensitive_name(part)
    }) {
        return true;
    }
    spec.skip_prefixes.iter().any(|prefix| {
        let trimmed = prefix.trim_end_matches('/');
        rel == trimmed || rel.starts_with(prefix)
    })
}

/// Caller provenance from DCTX_BENCH_REVISION, without inspecting repository metadata.
pub fn revision_label() -> String {
    revision_or_unversioned(std::env::var("DCTX_BENCH_REVISION").ok())
}

fn revision_or_unversioned(label: Option<String>) -> String {
    label.unwrap_or_else(|| "unversioned".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of no bytes. Fixed empty-corpus control (FIPS 180-4).
    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn revision_is_caller_provenance_with_an_explicit_default() {
        assert_eq!(revision_or_unversioned(None), "unversioned");
        assert_eq!(
            revision_or_unversioned(Some("release/next".into())),
            "release/next"
        );
        assert_eq!(revision_or_unversioned(Some("".into())), "");
    }

    #[test]
    fn empty_hash_is_sha256_of_nothing_and_order_changes_the_digest() {
        assert_eq!(hash_sequences(&[]).expect("empty"), EMPTY_SHA256);
        let forward = hash_sequences(&[("a", b"a"), ("b", b"b")]).expect("forward");
        let reverse = hash_sequences(&[("b", b"b"), ("a", b"a")]).expect("reverse");
        assert_ne!(forward, reverse);
        assert_eq!(
            hash_sequences(&[("a", b"a")]).expect("left"),
            hash_sequences(&[("a", b"a")]).expect("right")
        );
    }

    /// F9: production hashing is `dctx_hash::Sha256`. Empty and NIST "abc" stay fixed.
    /// Positive control: those digests match. `sha2` is not a normal dependency.
    #[test]
    fn production_hash_uses_dctx_hash_and_keeps_known_vectors() {
        let manifest = include_str!("../Cargo.toml");
        let deps = manifest
            .split("[dependencies]")
            .nth(1)
            .expect("dependencies")
            .split("\n[")
            .next()
            .expect("dependency block");
        assert!(
            deps.contains("dctx-hash"),
            "pin must path-depend on dctx-hash"
        );
        assert!(
            !deps.contains("sha2"),
            "sha2 must not be a production dependency"
        );
        let digest = dctx_hash::Sha256::digest(b"abc").expect("abc");
        assert_eq!(
            hex(digest.as_bytes()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(digest.byte_length(), 3);
        assert_eq!(hash_sequences(&[]).expect("empty"), EMPTY_SHA256);
        let pinned = hash_sequences(&[("src/a.txt", b"abc")]).expect("pin");
        assert_eq!(
            pinned,
            hash_sequences(&[("src/a.txt", b"abc")]).expect("same pin")
        );
        assert_eq!(pinned.len(), 64);
        assert_ne!(
            pinned,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn skip_rules_match_names_and_prefixes_and_keep_the_root() {
        let spec = DctxSpec {
            id: "dctx".into(),
            skip_dir_names: vec![".git".into(), "target".into()],
            skip_prefixes: vec!["tools/architecture-bench/scratch/".into()],
        };
        assert!(!skipped("", &spec));
        assert!(skipped(".git", &spec));
        assert!(skipped("crates/foo/target/debug/dctx", &spec));
        assert!(skipped("tools/architecture-bench/scratch", &spec));
        assert!(skipped("tools/architecture-bench/scratch/fixture/a.rs", &spec));
        assert!(!skipped("tools/architecture-bench/src/lib.rs", &spec));
        assert!(skipped("secrets/id_ed25519", &spec));
        assert!(skipped("config/.env.local", &spec));
        assert!(!skipped("src/lib.rs", &spec));
    }
}
