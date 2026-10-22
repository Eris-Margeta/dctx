//! The signed discovery index: verification, release selection, search and safe display.
//! Implements PLUGINS.md §5.3 discovery and distribution.md I-INDEX: 4 MiB/depth 32/4096 members,
//! index-domain signature with the pinned index key, unique IDs and case-folded repositories,
//! HTTPS locators, monotonic sequence and escaped display text. Inclusion grants nothing.
//! Pure; depends on signature, origin, fields and names. Used by the install service and CLI/app search.

use std::fmt::Write as _;
use std::collections::BTreeSet;

use dctx_plugin_protocol::{DecodeLimits, Json, decode_object};

use super::digest::{Fingerprint, Sha256Hex};
use super::fields::Closed;
use super::names::{PluginId, Version};
use super::origin::HttpsUrl;
use super::signature::{INDEX_DOMAIN, verify_detached};
use super::trust::TrustStore;
use crate::error::{DistributionError, Document};

/// Default index ceiling (4 MiB, depth 32, 4096 members).
pub const INDEX_LIMITS: DecodeLimits = DecodeLimits { max_bytes: 4 << 20, max_depth: 32, max_members: 4096 };
const PLUGIN: [&str; 5] = ["id", "repository", "summary", "publisher_key_fingerprint", "releases"];
const RELEASE: [&str; 4] = ["version", "release_sequence", "manifest_url", "manifest_sha256"];

/// One release locator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexRelease {
    /// Exact version.
    pub version: Version,
    /// Release sequence the manifest must repeat.
    pub release_sequence: u64,
    /// HTTPS manifest locator.
    pub manifest_url: HttpsUrl,
    /// Digest the fetched manifest bytes must match.
    pub manifest_sha256: Sha256Hex,
}

/// One discovery entry; its text is untrusted display data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    /// Canonical identity.
    pub id: PluginId,
    /// GitHub-style `owner/repo` locator, not an identity.
    pub repository: String,
    /// Raw summary; display only through [`escape_display`].
    pub summary: String,
    /// Curated publisher fingerprint; still verified against the manifest signer.
    pub publisher_key_fingerprint: Fingerprint,
    /// Releases.
    pub releases: Vec<IndexRelease>,
}

impl IndexEntry {
    /// The exact version, or the highest release sequence when `version` is `None`.
    /// # Errors
    /// Returns `NotFound` when the requested version, or any release for an unversioned selection, is absent.
    pub fn select(&self, version: Option<&Version>) -> Result<&IndexRelease, DistributionError> {
        match version {
            Some(version) => self.releases.iter().find(|release| release.version == *version),
            None => self.releases.iter().max_by_key(|release| release.release_sequence),
        }
        .ok_or(DistributionError::NotFound)
    }
}

/// An index whose signature verified with the pinned index key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedIndex {
    /// Index sequence; a refresh never accepts a lower one.
    pub sequence: u64,
    /// Entries in index order.
    pub entries: Vec<IndexEntry>,
}

impl VerifiedIndex {
    /// Verifies `bytes` and its detached signature, then checks the closed schema.
    /// # Errors
    /// Refuses oversized index bytes, invalid signatures, or malformed index data.
    pub fn verify(bytes: &[u8], signature: &[u8], index_key: &[u8; 32]) -> Result<Self, DistributionError> {
        if bytes.len() > INDEX_LIMITS.max_bytes {
            return Err(DistributionError::LimitExceeded { limit: INDEX_LIMITS.max_bytes as u64 });
        }
        verify_detached(INDEX_DOMAIN, index_key, bytes, signature)?;
        Self::parse(bytes)
    }

    /// Schema checks only; callers must use [`VerifiedIndex::verify`] for anything they act on.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, DistributionError> {
        let value = decode_object(bytes, INDEX_LIMITS).map_err(|refusal| refuse(refusal.reason))?;
        let top = Closed::new(&value, Document::Index, &["schema", "sequence", "plugins"], &["schema", "sequence", "plugins"])?;
        if top.uint("schema")? != 1 {
            return Err(refuse("schema"));
        }
        let mut ids = BTreeSet::new();
        let mut repositories = BTreeSet::new();
        let mut entries = Vec::new();
        for item in top.array("plugins", 0, 2048)? {
            let entry = parse_entry(item)?;
            if !ids.insert(entry.id.clone()) {
                return Err(refuse("duplicate-id"));
            }
            if !repositories.insert(entry.repository.to_ascii_lowercase()) {
                return Err(refuse("duplicate-repository"));
            }
            entries.push(entry);
        }
        Ok(Self { sequence: top.positive("sequence")?, entries })
    }

    /// Refuses a refresh that would replace a newer cached index.
    /// # Errors
    /// Refuses an index sequence below the cached sequence.
    pub fn check_newer_than(&self, cached_sequence: Option<u64>) -> Result<(), DistributionError> {
        match cached_sequence {
            Some(cached) if self.sequence < cached => Err(DistributionError::Downgrade { candidate: self.sequence, floor: cached }),
            _ => Ok(()),
        }
    }

    /// Entry by ID; refuses when the curated fingerprint contradicts an existing publisher pin.
    /// # Errors
    /// Refuses a missing plugin entry or a publisher fingerprint that conflicts with the trust pin.
    pub fn entry(&self, id: &PluginId, trust: &TrustStore) -> Result<&IndexEntry, DistributionError> {
        let entry = self.entries.iter().find(|entry| entry.id == *id).ok_or(DistributionError::NotFound)?;
        match trust.publisher_pin(id) {
            Some(pin) if *pin != entry.publisher_key_fingerprint => Err(DistributionError::SignerChanged),
            _ => Ok(entry),
        }
    }

    /// Case-insensitive search over ID, repository and summary; never fetches anything.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn search(&self, query: &str) -> Vec<&IndexEntry> {
        let query = query.to_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.id.as_str().contains(&query)
                    || entry.repository.to_lowercase().contains(&query)
                    || entry.summary.to_lowercase().contains(&query)
            })
            .collect()
    }
}

fn refuse(reason: &'static str) -> DistributionError {
    DistributionError::malformed(Document::Index, reason)
}

fn parse_entry(value: &Json) -> Result<IndexEntry, DistributionError> {
    let item = Closed::new(value, Document::Index, &PLUGIN, &PLUGIN)?;
    let repository = item.str("repository")?;
    let valid_repo = repository.len() <= 256
        && repository.split_once('/').is_some_and(|(owner, repo)| {
            [owner, repo].iter().all(|part| {
                !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
            })
        });
    if !valid_repo {
        return Err(refuse("repository"));
    }
    let summary = item.str("summary")?;
    if summary.chars().count() > 256 {
        return Err(refuse("summary"));
    }
    let mut versions = BTreeSet::new();
    let mut sequences = BTreeSet::new();
    let mut releases = Vec::new();
    for release in item.array("releases", 1, 128)? {
        let release = Closed::new(release, Document::Index, &RELEASE, &RELEASE)?;
        let parsed = IndexRelease {
            version: Version::parse(release.str("version")?).map_err(|_| refuse("version"))?,
            release_sequence: release.positive("release_sequence")?,
            manifest_url: HttpsUrl::parse(release.str("manifest_url")?)?,
            manifest_sha256: Sha256Hex::parse(release.str("manifest_sha256")?, Document::Index)?,
        };
        if !versions.insert(parsed.version.clone()) || !sequences.insert(parsed.release_sequence) {
            return Err(refuse("duplicate-release"));
        }
        releases.push(parsed);
    }
    Ok(IndexEntry {
        id: PluginId::parse(item.str("id")?).map_err(|_| refuse("id"))?,
        repository: repository.to_owned(),
        summary: summary.to_owned(),
        publisher_key_fingerprint: Sha256Hex::parse(item.str("publisher_key_fingerprint")?, Document::Index)?,
        releases,
    })
}

/// Escapes control, bidirectional and other invisible format characters for terminal display.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn escape_display(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let invisible = c.is_control()
            || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}');
        if invisible {
            let _ = write!(out, "\\u{{{:04x}}}", c as u32);
        } else if c == '\\' {
            out.push_str("\\\\");
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &[u8] = include_bytes!("../../../../docs/design/plugin-api/fixtures/valid/index.json");

    #[test]
    fn fixture_index_parses_and_selects_newest() {
        let index = VerifiedIndex::parse(INDEX).unwrap();
        assert_eq!(index.sequence, 1);
        let entry = &index.entries[0];
        assert_eq!(entry.select(None).unwrap().release_sequence, 1);
        assert_eq!(entry.select(Some(&Version::parse("9.9.9").unwrap())), Err(DistributionError::NotFound));
        assert_eq!(index.search("GIT").len(), 1);
        assert_eq!(index.search("nothing-like-this"), [] as [&crate::domain::index::IndexEntry; 0]);
    }

    #[test]
    fn stale_refresh_and_pin_conflicts_are_refused() {
        let index = VerifiedIndex::parse(INDEX).unwrap();
        assert!(index.check_newer_than(Some(1)).is_ok());
        assert_eq!(index.check_newer_than(Some(2)), Err(DistributionError::Downgrade { candidate: 1, floor: 2 }));
        let id = PluginId::parse("dctx/git").unwrap();
        let mut trust = TrustStore::new();
        assert!(index.entry(&id, &trust).is_ok());
        trust.pin_publisher(id.clone(), Sha256Hex::from_bytes([7; 32]));
        assert_eq!(index.entry(&id, &trust).map(|_| ()), Err(DistributionError::SignerChanged));
    }

    #[test]
    fn display_escaping_neutralises_terminal_controls() {
        assert_eq!(escape_display("ok text"), "ok text");
        assert_eq!(escape_display("a\u{1b}[31mb"), "a\\u{001b}[31mb");
        assert_eq!(escape_display("hidden\u{202e} text"), "hidden\\u{202e} text");
        assert_eq!(escape_display("x\\y\n"), "x\\\\y\\u{000a}");
    }
}
