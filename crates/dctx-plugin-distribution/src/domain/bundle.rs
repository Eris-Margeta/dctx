// SPDX-License-Identifier: Apache-2.0
//! The `.dctx-plugin` v1 offline bundle: magic, bounded table and concatenated payloads.
//! Implements distribution.md §Discovery, fetch and offline bundle (layout table) and I-BUNDLE:
//! checked lengths before allocation, unique safe names without case aliases, exact totals, digests.
//! Pure; depends on fields, names and digest. Used by the install service; the table is never a trust root.

use std::collections::BTreeSet;

use dctx_plugin_protocol::{DecodeLimits, decode_object};

use super::digest::{PayloadCheck, Sha256Hex};
use super::fields::Closed;
use super::names::SafeFileName;
use crate::error::{DistributionError, Document};

/// Leading magic bytes.
pub const MAGIC: &[u8; 8] = b"DCTXPLG1";
/// Largest admitted table, in bytes.
pub const MAX_TABLE_BYTES: u32 = 262_144;
/// Largest admitted entry count.
pub const MAX_ENTRIES: usize = 128;
const ENTRY: [&str; 3] = ["name", "bytes", "sha256"];

/// One named payload of a bundle, already checked against the table digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleEntry {
    /// Safe single file name.
    pub name: SafeFileName,
    /// Exact payload bytes.
    pub bytes: Vec<u8>,
}

/// A structurally valid offline bundle; its payloads still need manifest/signature checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfflineBundle {
    entries: Vec<BundleEntry>,
}

fn refuse(reason: &'static str) -> DistributionError {
    DistributionError::malformed(Document::Bundle, reason)
}

impl OfflineBundle {
    /// Parses `bytes`, refusing any container larger than `storage_limit`.
    /// # Errors
    /// Refuses exceeded storage limits, malformed framing/table entries, duplicate names, payload length mismatches, or digest failures.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "The checked sum equals the in-memory payload length; each nonnegative entry length therefore fits usize before splitting."
    )]
    pub fn parse(bytes: &[u8], storage_limit: u64) -> Result<Self, DistributionError> {
        if bytes.len() as u64 > storage_limit {
            return Err(DistributionError::LimitExceeded {
                limit: storage_limit,
            });
        }
        let rest = bytes
            .strip_prefix(MAGIC.as_slice())
            .ok_or(refuse("magic"))?;
        let (length, rest) = rest
            .split_first_chunk::<4>()
            .ok_or(refuse("truncated-length"))?;
        let table_len = u32::from_le_bytes(*length);
        if table_len == 0 || table_len > MAX_TABLE_BYTES {
            return Err(refuse("table-length"));
        }
        let table_len = table_len as usize;
        if rest.len() < table_len {
            return Err(refuse("truncated-table"));
        }
        let (table, mut payloads) = rest.split_at(table_len);
        let limits = DecodeLimits {
            max_bytes: MAX_TABLE_BYTES as usize,
            max_depth: 32,
            max_members: 4096,
        };
        let table = decode_object(table, limits).map_err(|refusal| refuse(refusal.reason))?;
        let table = Closed::new(
            &table,
            Document::Bundle,
            &["schema", "entries"],
            &["schema", "entries"],
        )?;
        if table.uint("schema")? != 1 {
            return Err(refuse("schema"));
        }
        let declared = table.array("entries", 1, MAX_ENTRIES)?;
        let mut folded = BTreeSet::new();
        let mut total: u64 = 0;
        let mut rows = Vec::with_capacity(declared.len());
        for entry in declared {
            let entry = Closed::new(entry, Document::Bundle, &ENTRY, &ENTRY)?;
            let name = SafeFileName::parse(entry.str("name")?).map_err(|_| refuse("entry-name"))?;
            if !folded.insert(name.folded()) {
                return Err(refuse("duplicate-name"));
            }
            let length = entry.uint("bytes")?;
            total = total.checked_add(length).ok_or(refuse("length-overflow"))?;
            if total > storage_limit {
                return Err(DistributionError::LimitExceeded {
                    limit: storage_limit,
                });
            }
            rows.push((
                name,
                length,
                Sha256Hex::parse(entry.str("sha256")?, Document::Bundle)?,
            ));
        }
        if total != payloads.len() as u64 {
            return Err(refuse(if total > payloads.len() as u64 {
                "truncated-payload"
            } else {
                "trailing-bytes"
            }));
        }
        let mut entries = Vec::with_capacity(rows.len());
        for (name, length, digest) in rows {
            let (payload, next) = payloads.split_at(length as usize);
            PayloadCheck::whole(payload, length, digest)?;
            entries.push(BundleEntry {
                name,
                bytes: payload.to_vec(),
            });
            payloads = next;
        }
        Ok(Self { entries })
    }

    /// Entry by exact name.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn get(&self, name: &str) -> Option<&BundleEntry> {
        self.entries
            .iter()
            .find(|entry| entry.name.as_str() == name)
    }

    /// All entries in table order.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn entries(&self) -> &[BundleEntry] {
        &self.entries
    }

    /// Builds container bytes; used by tests and release tooling fixtures, never by install.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "This infallible fixture encoder retains the existing 32-bit wire length encoding; the parser independently enforces table limits."
    )]
    pub fn encode(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let rows: Vec<String> = entries
            .iter()
            .map(|(name, bytes)| {
                let digest = Sha256Hex::of(bytes)
                    .map(|d| d.to_string())
                    .unwrap_or_default();
                format!(
                    "{{\"name\":\"{name}\",\"bytes\":{},\"sha256\":\"{digest}\"}}",
                    bytes.len()
                )
            })
            .collect();
        let table = format!("{{\"schema\":1,\"entries\":[{}]}}", rows.join(","));
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&(table.len() as u32).to_le_bytes());
        out.extend_from_slice(table.as_bytes());
        for (_, bytes) in entries {
            out.extend_from_slice(bytes);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: u64 = 1 << 20;

    fn sample() -> Vec<u8> {
        OfflineBundle::encode(&[("plugin.json", b"{}"), ("abc.bin", b"abc"), ("NOTICE", b"")])
    }

    fn with_table(table: &str, payload: &[u8]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(
            &u32::try_from(table.len())
                .expect("fixture table length")
                .to_le_bytes(),
        );
        out.extend_from_slice(table.as_bytes());
        out.extend_from_slice(payload);
        out
    }

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn valid_bundle_round_trips() {
        let bundle = OfflineBundle::parse(&sample(), LIMIT).unwrap();
        assert_eq!(bundle.entries().len(), 3);
        assert_eq!(bundle.get("abc.bin").unwrap().bytes, b"abc");
        assert_eq!(bundle.get("NOTICE").unwrap().bytes, b"");
    }

    #[test]
    fn framing_failures_are_refused() {
        let good = sample();
        let mut magic = good.clone();
        magic[0] = b'X';
        assert_eq!(OfflineBundle::parse(&magic, LIMIT), Err(refuse("magic")));
        assert_eq!(
            OfflineBundle::parse(&good[..10], LIMIT),
            Err(refuse("truncated-length"))
        );
        assert_eq!(
            OfflineBundle::parse(&good[..20], LIMIT),
            Err(refuse("truncated-table"))
        );
        assert_eq!(
            OfflineBundle::parse(&good[..good.len() - 1], LIMIT),
            Err(refuse("truncated-payload"))
        );
        let mut trailing = good.clone();
        trailing.push(0);
        assert_eq!(
            OfflineBundle::parse(&trailing, LIMIT),
            Err(refuse("trailing-bytes"))
        );
        let mut zero = MAGIC.to_vec();
        zero.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            OfflineBundle::parse(&zero, LIMIT),
            Err(refuse("table-length"))
        );
        let mut huge = MAGIC.to_vec();
        huge.extend_from_slice(&(MAX_TABLE_BYTES + 1).to_le_bytes());
        assert_eq!(
            OfflineBundle::parse(&huge, LIMIT),
            Err(refuse("table-length"))
        );
        assert_eq!(
            OfflineBundle::parse(&good, 10),
            Err(DistributionError::LimitExceeded { limit: 10 })
        );
    }

    #[test]
    fn hostile_names_are_refused() {
        for name in [
            "../abc.bin",
            "/etc/passwd",
            "a/b",
            "C:x",
            "..",
            "con",
            "a\\\\b",
        ] {
            let table = format!(
                "{{\"schema\":1,\"entries\":[{{\"name\":\"{name}\",\"bytes\":3,\"sha256\":\"{ABC}\"}}]}}"
            );
            assert_eq!(
                OfflineBundle::parse(&with_table(&table, b"abc"), LIMIT),
                Err(refuse("entry-name")),
                "{name}"
            );
        }
        let dup = format!(
            "{{\"schema\":1,\"entries\":[{{\"name\":\"abc.bin\",\"bytes\":3,\"sha256\":\"{ABC}\"}},{{\"name\":\"ABC.bin\",\"bytes\":3,\"sha256\":\"{ABC}\"}}]}}"
        );
        assert_eq!(
            OfflineBundle::parse(&with_table(&dup, b"abcabc"), LIMIT),
            Err(refuse("duplicate-name"))
        );
    }

    #[test]
    fn table_shape_lengths_and_digests_are_checked() {
        let unknown = format!(
            "{{\"schema\":1,\"entries\":[{{\"name\":\"a\",\"bytes\":3,\"sha256\":\"{ABC}\",\"mode\":493}}]}}"
        );
        assert_eq!(
            OfflineBundle::parse(&with_table(&unknown, b"abc"), LIMIT),
            Err(refuse("unknown-field"))
        );
        let compressed = format!(
            "{{\"schema\":1,\"compression\":\"zip\",\"entries\":[{{\"name\":\"a\",\"bytes\":3,\"sha256\":\"{ABC}\"}}]}}"
        );
        assert_eq!(
            OfflineBundle::parse(&with_table(&compressed, b"abc"), LIMIT),
            Err(refuse("unknown-field"))
        );
        let empty = "{\"schema\":1,\"entries\":[]}";
        assert_eq!(
            OfflineBundle::parse(&with_table(empty, b""), LIMIT),
            Err(refuse("array-length"))
        );
        let wrong = format!(
            "{{\"schema\":1,\"entries\":[{{\"name\":\"a\",\"bytes\":3,\"sha256\":\"{ABC}\"}}]}}"
        );
        assert_eq!(
            OfflineBundle::parse(&with_table(&wrong, b"abd"), LIMIT),
            Err(DistributionError::IntegrityFailure)
        );
        let overflow = format!(
            "{{\"schema\":1,\"entries\":[{{\"name\":\"a\",\"bytes\":9007199254740991,\"sha256\":\"{ABC}\"}},{{\"name\":\"b\",\"bytes\":9007199254740991,\"sha256\":\"{ABC}\"}}]}}"
        );
        assert_eq!(
            OfflineBundle::parse(&with_table(&overflow, b"abc"), LIMIT),
            Err(DistributionError::LimitExceeded { limit: LIMIT })
        );
        let dup_key = format!(
            "{{\"schema\":1,\"schema\":1,\"entries\":[{{\"name\":\"a\",\"bytes\":3,\"sha256\":\"{ABC}\"}}]}}"
        );
        assert!(OfflineBundle::parse(&with_table(&dup_key, b"abc"), LIMIT).is_err());
    }
}
