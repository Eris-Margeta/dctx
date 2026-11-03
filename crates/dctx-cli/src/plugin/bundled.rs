// SPDX-License-Identifier: Apache-2.0
//! Bundled `dctx/git` row derived from the facade catalog.
//! Implements PLUGINS.md §5.4 and §7.3 PL-CLI: listed and enabled, capabilities disclosed,
//! unavailable when the git worker feature is off. Depends on `dctx::bundled_git_plugin` and the
//! distribution capability set. Used by list, info, enable, disable, remove and verify.

use dctx_plugin_distribution::{
    CapabilitySet, DistributionError, Document, InstalledVersion, PluginId, Sha256Hex, Version,
};
use dctx_plugin_protocol::{DecodeLimits, decode_object};

/// Public index locator. Refresh is explicit; this URL is not fetched by list or help.
pub const DEFAULT_INDEX_URL: &str = "https://example.invalid/dctx/index.json";

/// Manifest capability object for the bundled git plugin (plugin.json / PLUGINS.md §4.1).
pub const CAPABILITIES_JSON: &str = concat!(
    r#"{"required":["#,
    r#"{"kind":"file.read","scope":"project","path":".gitignore","max_bytes":1048576},"#,
    r#"{"kind":"file.write","scope":"project","path":".gitignore","create":false,"max_growth_bytes":4096,"max_bytes":1052672}"#,
    r#"],"optional":["#,
    r#"{"kind":"program.run","program":"git","operation":"git.add-ignore","resources":["git-admin:stage-ignore"],"max_output_bytes":65536}"#,
    r#"],"outputs":[],"network":[]}"#
);

/// Verbs the bundled manifest declares. Unknown verbs are refused before admission.
pub const VERBS: [&str; 2] = ["status", "protect"];

/// True only when this binary links the git worker.
pub fn worker_linked() -> bool {
    cfg!(feature = "git-worker")
}

/// Facade catalog entry for the default plugin.
pub fn catalog() -> dctx::KnownPlugin {
    dctx::bundled_git_plugin()
}

/// Canonical id `dctx/git`.
pub fn id() -> Result<PluginId, DistributionError> {
    PluginId::parse(catalog().id.as_str())
}

/// Requested capability set, canonicalized by the distribution crate.
pub fn capabilities() -> Result<CapabilitySet, DistributionError> {
    let value = decode_object(CAPABILITIES_JSON.as_bytes(), DecodeLimits::MANIFEST)
        .map_err(|_| DistributionError::Malformed { document: Document::Manifest, reason: "capabilities" })?;
    CapabilitySet::from_manifest(&value)
}

/// Registry row for the copy shipped inside this executable. No store files are published.
pub fn installed_version() -> Result<InstalledVersion, DistributionError> {
    let catalog = catalog();
    let digest = catalog.digest.ok_or(DistributionError::IntegrityFailure)?;
    let manifest = catalog.manifest_digest.ok_or(DistributionError::IntegrityFailure)?;
    Ok(InstalledVersion {
        version: Version::parse(&catalog.version)?,
        release_sequence: 1,
        digest: Sha256Hex::from_bytes(digest),
        manifest_sha256: Sha256Hex::from_bytes(manifest),
        signer: Sha256Hex::of(catalog.publisher_key.as_bytes())?,
        capabilities: capabilities()?,
        quarantined: false,
    })
}

/// Display labels from the facade catalog.
pub fn capability_labels() -> Vec<String> {
    catalog().capabilities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disclosed_capabilities_match_the_facade_catalog() {
        let labels = capability_labels();
        for expected in [
            "file.read:.gitignore",
            "file.write:.gitignore",
            "program:git.add-ignore",
            "resource:git-admin:stage-ignore",
        ] {
            assert!(labels.iter().any(|label| label == expected), "{expected}");
        }
        assert_eq!(catalog().alias, "git");
        assert_eq!(catalog().id, "dctx/git");
        assert_eq!(catalog().version, "1.1.0");
        assert!(capabilities().unwrap().len() >= 3);
    }

    #[cfg(feature = "git-worker")]
    #[test]
    fn default_build_links_the_bundled_worker() {
        assert!(worker_linked());
    }

    #[cfg(not(feature = "git-worker"))]
    #[test]
    fn strict_build_keeps_catalog_metadata_without_linking_the_worker() {
        assert!(!worker_linked());
        assert_eq!(catalog().id, "dctx/git");
        assert_eq!(installed_version().unwrap().version.as_str(), "1.1.0");
    }
}
