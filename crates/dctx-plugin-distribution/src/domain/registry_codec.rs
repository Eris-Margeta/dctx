// SPDX-License-Identifier: Apache-2.0
//! Versioned persistence of the registry as canonical JSON (state format 1).
//! Implements distribution.md "version state explicitly; no post-install/migration scripts" and
//! makes disable, consent, high-water marks and quarantine survive restarts (I-CONSENT, I-ATOMIC).
//! Pure; depends on the protocol's strict JSON and the registry types. Used by the services via the store port.
//! Unknown formats or fields are refused, never migrated silently.

use dctx_plugin_protocol::{DecodeLimits, Json, decode_object, to_canonical};

use super::consent::{CapabilitySet, ConsentRecord, ConsentScope};
use super::digest::Sha256Hex;
use super::fields::{Closed, uint};
use super::names::{PluginId, Version};
use super::registry::{
    Enablement, InstalledVersion, Origin, PluginRecord, Registry, RollbackEntry,
};
use crate::error::{DistributionError, Document};

/// Registry state format written by this crate.
pub const FORMAT: u64 = 1;
const LIMITS: DecodeLimits = DecodeLimits {
    max_bytes: 16 << 20,
    max_depth: 32,
    max_members: 1 << 20,
};
const RECORD: [&str; 9] = [
    "id",
    "origin",
    "versions",
    "active",
    "high_water",
    "enablement",
    "consent",
    "leases",
    "rollbacks",
];
const VERSION: [&str; 7] = [
    "version",
    "release_sequence",
    "digest",
    "manifest_sha256",
    "signer",
    "capabilities",
    "quarantined",
];
const CONSENT: [&str; 4] = ["digest", "signer", "capabilities", "scope"];

fn obj(members: Vec<(&str, Json)>) -> Json {
    Json::Object(
        members
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn hex(digest: &Sha256Hex) -> Json {
    Json::str(&digest.to_string())
}

fn refuse(reason: &'static str) -> DistributionError {
    DistributionError::malformed(Document::Registry, reason)
}

/// Canonical bytes of `registry`.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn encode(registry: &Registry) -> Vec<u8> {
    let plugins = registry
        .records()
        .map(|(id, record)| encode_record(id, record))
        .collect();
    to_canonical(&obj(vec![
        ("format", Json::uint(FORMAT)),
        ("plugins", Json::Array(plugins)),
    ]))
}

fn encode_record(id: &PluginId, record: &PluginRecord) -> Json {
    let origin = match record.origin {
        Origin::Installed => "installed",
        Origin::Bundled => "bundled",
        Origin::Override => "override",
    };
    let enablement = match record.enablement {
        Enablement::NeverEnabled => "never-enabled",
        Enablement::Enabled => "enabled",
        Enablement::Disabled => "disabled",
    };
    let versions = record.versions.iter().map(encode_version).collect();
    let leases = record.leases.iter().map(|(digest, count)| {
        obj(vec![
            ("digest", hex(digest)),
            ("count", Json::uint(u64::from(*count))),
        ])
    });
    let rollbacks = record.rollbacks.iter().map(|entry| {
        obj(vec![
            ("digest", hex(&entry.digest)),
            ("reason", Json::str(&entry.reason)),
        ])
    });
    obj(vec![
        ("id", Json::str(id.as_str())),
        ("origin", Json::str(origin)),
        ("versions", Json::Array(versions)),
        ("active", record.active.as_ref().map_or(Json::Null, hex)),
        ("high_water", Json::uint(record.high_water)),
        ("enablement", Json::str(enablement)),
        (
            "consent",
            record.consent.as_ref().map_or(Json::Null, encode_consent),
        ),
        ("leases", Json::Array(leases.collect())),
        ("rollbacks", Json::Array(rollbacks.collect())),
    ])
}

fn encode_version(version: &InstalledVersion) -> Json {
    obj(vec![
        ("version", Json::str(version.version.as_str())),
        ("release_sequence", Json::uint(version.release_sequence)),
        ("digest", hex(&version.digest)),
        ("manifest_sha256", hex(&version.manifest_sha256)),
        ("signer", hex(&version.signer)),
        ("capabilities", version.capabilities.to_json()),
        ("quarantined", Json::Bool(version.quarantined)),
    ])
}

fn encode_consent(consent: &ConsentRecord) -> Json {
    let scope = match &consent.scope {
        ConsentScope::User => obj(vec![("kind", Json::str("user"))]),
        ConsentScope::Project(path) => obj(vec![
            ("kind", Json::str("project")),
            ("path", Json::str(path)),
        ]),
    };
    obj(vec![
        ("digest", hex(&consent.digest)),
        ("signer", hex(&consent.signer)),
        ("capabilities", consent.capabilities.to_json()),
        ("scope", scope),
    ])
}

/// Restores a registry, refusing unknown formats and fields.
/// # Errors
/// Refuses malformed/oversized JSON, unsupported formats, duplicate IDs, or invalid registry records.
pub fn decode(bytes: &[u8]) -> Result<Registry, DistributionError> {
    let value = decode_object(bytes, LIMITS).map_err(|refusal| refuse(refusal.reason))?;
    let top = Closed::new(
        &value,
        Document::Registry,
        &["format", "plugins"],
        &["format", "plugins"],
    )?;
    if top.uint("format")? != FORMAT {
        return Err(refuse("format"));
    }
    let mut registry = Registry::default();
    for item in top.array("plugins", 0, usize::MAX)? {
        let (id, record) = decode_record(item)?;
        if registry.plugins.insert(id, record).is_some() {
            return Err(refuse("duplicate-id"));
        }
    }
    Ok(registry)
}

fn digest(value: &Json) -> Result<Sha256Hex, DistributionError> {
    Sha256Hex::parse(value.as_str().ok_or(refuse("digest"))?, Document::Registry)
}

fn decode_record(value: &Json) -> Result<(PluginId, PluginRecord), DistributionError> {
    let item = Closed::new(value, Document::Registry, &RECORD, &RECORD)?;
    let origin = match item.str("origin")? {
        "installed" => Origin::Installed,
        "bundled" => Origin::Bundled,
        "override" => Origin::Override,
        _ => return Err(refuse("origin")),
    };
    let enablement = match item.str("enablement")? {
        "never-enabled" => Enablement::NeverEnabled,
        "enabled" => Enablement::Enabled,
        "disabled" => Enablement::Disabled,
        _ => return Err(refuse("enablement")),
    };
    let active = match item.req("active")? {
        Json::Null => None,
        other => Some(digest(other)?),
    };
    let consent = match item.req("consent")? {
        Json::Null => None,
        other => Some(decode_consent(other)?),
    };
    let mut leases = std::collections::BTreeMap::new();
    for lease in item.array("leases", 0, usize::MAX)? {
        let lease = Closed::new(
            lease,
            Document::Registry,
            &["digest", "count"],
            &["digest", "count"],
        )?;
        let count = u32::try_from(lease.uint("count")?).map_err(|_| refuse("lease-count"))?;
        leases.insert(digest(lease.req("digest")?)?, count);
    }
    let mut rollbacks = Vec::new();
    for entry in item.array("rollbacks", 0, usize::MAX)? {
        let entry = Closed::new(
            entry,
            Document::Registry,
            &["digest", "reason"],
            &["digest", "reason"],
        )?;
        rollbacks.push(RollbackEntry {
            digest: digest(entry.req("digest")?)?,
            reason: entry.str("reason")?.to_owned(),
        });
    }
    let versions = item
        .array("versions", 0, usize::MAX)?
        .iter()
        .map(decode_version)
        .collect::<Result<Vec<_>, _>>()?;
    if active.is_some_and(|active| !versions.iter().any(|version| version.digest == active)) {
        return Err(refuse("active-not-installed"));
    }
    let record = PluginRecord {
        origin,
        versions,
        active,
        high_water: item.uint("high_water")?,
        enablement,
        consent,
        leases,
        rollbacks,
    };
    Ok((PluginId::parse(item.str("id")?)?, record))
}

fn decode_version(value: &Json) -> Result<InstalledVersion, DistributionError> {
    let item = Closed::new(value, Document::Registry, &VERSION, &VERSION)?;
    Ok(InstalledVersion {
        version: Version::parse(item.str("version")?)?,
        release_sequence: uint(item.req("release_sequence")?, Document::Registry)?,
        digest: digest(item.req("digest")?)?,
        manifest_sha256: digest(item.req("manifest_sha256")?)?,
        signer: digest(item.req("signer")?)?,
        capabilities: CapabilitySet::from_json(item.req("capabilities")?)?,
        quarantined: item
            .req("quarantined")?
            .as_bool()
            .ok_or(refuse("quarantined"))?,
    })
}

fn decode_consent(value: &Json) -> Result<ConsentRecord, DistributionError> {
    let item = Closed::new(value, Document::Registry, &CONSENT, &CONSENT)?;
    let scope = Closed::new(
        item.object("scope")?,
        Document::Registry,
        &["kind", "path"],
        &["kind"],
    )?;
    let scope = match (scope.str("kind")?, scope.get("path")) {
        ("user", None) => ConsentScope::User,
        ("project", Some(path)) => {
            ConsentScope::Project(path.as_str().ok_or(refuse("scope"))?.to_owned())
        }
        _ => return Err(refuse("scope")),
    };
    Ok(ConsentRecord {
        digest: digest(item.req("digest")?)?,
        signer: digest(item.req("signer")?)?,
        capabilities: CapabilitySet::from_json(item.req("capabilities")?)?,
        scope,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::registry::tests::version;

    #[test]
    fn registry_round_trips_and_refuses_unknown_formats() {
        let id = PluginId::parse("dctx/git").unwrap();
        let mut registry = Registry::default();
        registry
            .record_install(&id, Origin::Installed, version(1, 1))
            .unwrap();
        registry.disable(&id).unwrap();
        let record = registry.plugins.get_mut(&id).unwrap();
        record.active = Some(Sha256Hex::from_bytes([1; 32]));
        record.consent = Some(ConsentRecord {
            digest: Sha256Hex::from_bytes([1; 32]),
            signer: Sha256Hex::from_bytes([1; 32]),
            capabilities: CapabilitySet::default(),
            scope: ConsentScope::Project("/work/p".into()),
        });
        record.rollbacks.push(RollbackEntry {
            digest: Sha256Hex::from_bytes([1; 32]),
            reason: "r".into(),
        });
        let bytes = encode(&registry);
        assert_eq!(decode(&bytes).unwrap(), registry);
        let future = String::from_utf8(bytes.clone())
            .unwrap()
            .replace("\"format\":1", "\"format\":2");
        assert_eq!(decode(future.as_bytes()), Err(refuse("format")));
        let script = String::from_utf8(bytes)
            .unwrap()
            .replace("\"format\":1", "\"format\":1,\"migrate\":\"sh\"");
        assert_eq!(decode(script.as_bytes()), Err(refuse("unknown-field")));
    }

    #[test]
    fn active_must_name_an_installed_version() {
        let bytes = br#"{"format":1,"plugins":[{"id":"dctx/git","origin":"installed","versions":[],"active":"0101010101010101010101010101010101010101010101010101010101010101","high_water":0,"enablement":"enabled","consent":null,"leases":[],"rollbacks":[]}]}"#;
        assert_eq!(decode(bytes), Err(refuse("active-not-installed")));
    }
}
