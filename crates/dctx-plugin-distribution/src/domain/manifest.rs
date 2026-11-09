// SPDX-License-Identifier: Apache-2.0
//! The signed release manifest as the installer reads it, kept beside its exact bytes.
//! Implements manifest.schema.json's closed top level and `artifacts` items, and distribution.md
//! "preserve exact bytes; reject invalid schema and duplicate keys even when a signature verifies".
//! Depends on the protocol's strict decoder, names, digest and consent; used by trust and the services.
//! Hook, command, config and limit semantics belong to the host (PL-H); here they are type-checked only.

use std::collections::BTreeSet;

use dctx_plugin_protocol::{DecodeLimits, Json, decode_object};

use super::consent::CapabilitySet;
use super::digest::Sha256Hex;
use super::fields::Closed;
use super::names::{KeyId, PluginId, SafeFileName, Target, Version};
use crate::error::{DistributionError, Document};

/// File names of a release that are never artifacts or runtime resources.
pub const RELEASE_FILES: [&str; 6] = [
    "plugin.json",
    "plugin.json.sig",
    "SHA256SUMS",
    "LICENSE",
    "NOTICE",
    "provenance.json",
];

const TOP: [&str; 18] = [
    "manifest_version",
    "id",
    "name",
    "version",
    "release_sequence",
    "publisher_key",
    "license",
    "protocol",
    "host_api",
    "hooks",
    "commands",
    "capabilities",
    "config",
    "limits",
    "reason_codes",
    "diagnostic_codes",
    "artifacts",
    "state_version",
];
const ARTIFACT: [&str; 8] = [
    "target",
    "file",
    "bytes",
    "sha256",
    "format",
    "min_os",
    "libc",
    "runtime_resources",
];
const PAYLOAD: [&str; 3] = ["file", "bytes", "sha256"];

/// One signed payload: name, exact length and digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payload {
    /// Safe file name beside the manifest.
    pub file: SafeFileName,
    /// Exact length in bytes.
    pub bytes: u64,
    /// SHA-256 of the exact bytes.
    pub sha256: Sha256Hex,
}

/// Per-target executable and the runtime resources it declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    /// Host target triple.
    pub target: Target,
    /// The executable payload.
    pub executable: Payload,
    /// Declared non-executable runtime resources.
    pub runtime_resources: Vec<Payload>,
}

impl Artifact {
    /// Executable first, then resources in manifest order.
    pub fn payloads(&self) -> impl Iterator<Item = &Payload> {
        std::iter::once(&self.executable).chain(self.runtime_resources.iter())
    }
}

/// A structurally valid manifest. Its authenticity is established separately by [`super::trust`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    bytes: Vec<u8>,
    /// Canonical plugin identity.
    pub id: PluginId,
    /// Exact version string.
    pub version: Version,
    /// Monotonic publisher/plugin release sequence.
    pub release_sequence: u64,
    /// Trust-record ID of the signing key.
    pub publisher_key: KeyId,
    /// Requested capability entries.
    pub capabilities: CapabilitySet,
    /// Per-target artifacts.
    pub artifacts: Vec<Artifact>,
}

impl Manifest {
    /// Decodes and checks exact manifest bytes (256 KiB, depth 32, 4096 members, no duplicate keys).
    /// # Errors
    /// Refuses malformed/oversized JSON, invalid identity/version/fields, unsupported manifest format, or duplicate artifacts.
    pub fn parse(bytes: &[u8]) -> Result<Self, DistributionError> {
        let value = decode_object(bytes, DecodeLimits::MANIFEST)
            .map_err(|refusal| DistributionError::malformed(Document::Manifest, refusal.reason))?;
        let top = Closed::new(&value, Document::Manifest, &TOP, &TOP[..17])?;
        if top.uint("manifest_version")? != 1 {
            return Err(top.refuse("manifest-version"));
        }
        if !valid_name(top.str("name")?) {
            return Err(top.refuse("name"));
        }
        if top.str("license")?.is_empty() {
            return Err(top.refuse("license"));
        }
        for list in [
            "protocol",
            "hooks",
            "commands",
            "reason_codes",
            "diagnostic_codes",
        ] {
            top.array(list, 0, 4096)?;
        }
        for object in ["host_api", "config", "limits"] {
            top.object(object)?;
        }
        if top.get("state_version").is_some() {
            top.positive("state_version")?;
        }
        let artifacts = top
            .array("artifacts", 1, 16)?
            .iter()
            .map(parse_artifact)
            .collect::<Result<Vec<_>, _>>()?;
        check_unique(&artifacts)?;
        Ok(Self {
            bytes: bytes.to_vec(),
            id: PluginId::parse(top.str("id")?).map_err(|_| top.refuse("id"))?,
            version: Version::parse(top.str("version")?)?,
            release_sequence: top.positive("release_sequence")?,
            publisher_key: KeyId::parse(top.str("publisher_key")?)?,
            capabilities: CapabilitySet::from_manifest(top.object("capabilities")?)?,
            artifacts,
        })
    }

    /// The exact bytes that were signed.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The artifact for `target`, or [`DistributionError::UnsupportedTarget`].
    /// # Errors
    /// Returns `UnsupportedTarget` if the manifest has no artifact for the requested target.
    pub fn artifact_for(&self, target: Target) -> Result<&Artifact, DistributionError> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.target == target)
            .ok_or(DistributionError::UnsupportedTarget)
    }
}

fn valid_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|&b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

fn parse_payload<'a>(
    value: &'a Json,
    allowed: &[&str],
    required: &[&str],
) -> Result<(Closed<'a>, Payload), DistributionError> {
    let item = Closed::new(value, Document::Manifest, allowed, required)?;
    let file = SafeFileName::parse(item.str("file")?).map_err(|_| item.refuse("file-name"))?;
    let payload = Payload {
        file,
        bytes: item.positive("bytes")?,
        sha256: Sha256Hex::parse(item.str("sha256")?, Document::Manifest)?,
    };
    Ok((item, payload))
}

fn parse_artifact(value: &Json) -> Result<Artifact, DistributionError> {
    let required = [
        "target",
        "file",
        "bytes",
        "sha256",
        "format",
        "min_os",
        "runtime_resources",
    ];
    let (item, executable) = parse_payload(value, &ARTIFACT, &required)?;
    if item.str("format")? != "native-executable" {
        return Err(item.refuse("format"));
    }
    let min_os = item.str("min_os")?;
    if min_os.is_empty() || min_os.len() > 64 {
        return Err(item.refuse("min-os"));
    }
    if let Some(libc) = item.get("libc") {
        let libc = Closed::new(
            libc,
            Document::Manifest,
            &["name", "min_version"],
            &["name", "min_version"],
        )?;
        if libc.str("name")? != "glibc" || libc.str("min_version")?.is_empty() {
            return Err(item.refuse("libc"));
        }
    }
    let runtime_resources = item
        .array("runtime_resources", 0, 32)?
        .iter()
        .map(|resource| parse_payload(resource, &PAYLOAD, &PAYLOAD).map(|(_, payload)| payload))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Artifact {
        target: Target::parse(item.str("target")?)?,
        executable,
        runtime_resources,
    })
}

fn check_unique(artifacts: &[Artifact]) -> Result<(), DistributionError> {
    let mut targets = BTreeSet::new();
    for artifact in artifacts {
        if !targets.insert(artifact.target) {
            return Err(DistributionError::malformed(
                Document::Manifest,
                "duplicate-target",
            ));
        }
        let mut names = BTreeSet::new();
        for payload in artifact.payloads() {
            let folded = payload.file.folded();
            if RELEASE_FILES
                .iter()
                .any(|reserved| reserved.eq_ignore_ascii_case(&folded))
                || !names.insert(folded)
            {
                return Err(DistributionError::malformed(
                    Document::Manifest,
                    "duplicate-file",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIT: &[u8] =
        include_bytes!("../../../../docs/design/plugin-api/fixtures/valid/manifest-git.json");

    fn edit(from: &str, to: &str) -> Vec<u8> {
        let text = std::str::from_utf8(GIT).unwrap();
        assert!(text.contains(from), "{from}");
        text.replacen(from, to, 1).into_bytes()
    }

    #[test]
    fn git_fixture_parses_and_keeps_exact_bytes() {
        let manifest = Manifest::parse(GIT).unwrap();
        assert_eq!(manifest.bytes(), GIT);
        assert_eq!(manifest.id.as_str(), "dctx/git");
        assert_eq!(manifest.release_sequence, 1);
        assert_eq!(manifest.publisher_key.as_str(), "fixture-key-1");
        let artifact = manifest.artifact_for(Target::AppleArm64).unwrap();
        assert_eq!(artifact.executable.file.as_str(), "abc.bin");
        assert_eq!(artifact.executable.bytes, 3);
        assert_eq!(
            manifest.artifact_for(Target::LinuxX64),
            Err(DistributionError::UnsupportedTarget)
        );
    }

    #[test]
    fn schema_failures_are_refused() {
        let cases = [
            ("\"manifest_version\": 1", "\"manifest_version\": 2"),
            ("\"id\": \"dctx/git\"", "\"id\": \"../git\""),
            ("\"release_sequence\": 1", "\"release_sequence\": 0"),
            ("\"file\": \"abc.bin\"", "\"file\": \"../abc.bin\""),
            ("\"file\": \"abc.bin\"", "\"file\": \"plugin.json\""),
            ("\"bytes\": 3", "\"bytes\": 0"),
            (
                "\"format\": \"native-executable\"",
                "\"format\": \"script\"",
            ),
            (
                "\"target\": \"aarch64-apple-darwin\"",
                "\"target\": \"riscv64\"",
            ),
            ("\"version\": \"1.3.0\"", "\"version\": \"latest\""),
            (
                "\"license\": \"Apache-2.0\",",
                "\"license\": \"Apache-2.0\", \"post_install\": \"x\",",
            ),
            (
                "\"license\": \"Apache-2.0\",",
                "\"license\": \"Apache-2.0\", \"license\": \"MIT\",",
            ),
            (
                "\"runtime_resources\": []",
                "\"runtime_resources\": [{\"file\": \"abc.bin\", \"bytes\": 1, \"sha256\": \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"}]",
            ),
        ];
        for (from, to) in cases {
            assert!(Manifest::parse(&edit(from, to)).is_err(), "{to}");
        }
    }

    #[test]
    fn oversized_manifest_is_refused_before_parsing() {
        let mut big = GIT.to_vec();
        big.extend(std::iter::repeat_n(b' ', 262_144));
        assert!(matches!(
            Manifest::parse(&big),
            Err(DistributionError::Malformed { .. })
        ));
    }
}
