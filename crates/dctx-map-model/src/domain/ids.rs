// SPDX-License-Identifier: Apache-2.0
//! Validated path/name identities and deterministic short aliases.
//! Implements SPEC-MAP-MODE section 5.2, independent of source positions.
//! The alias uses `dctx-hash`. Content identity is [`super::ContentId`], never a git object id.
//! Every graph layer consumes these ids. JSON validation is the `wire-json` feature.
use super::NodeKind;
use std::{fmt, str::FromStr};

/// Invalid stable-id components or spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidId;
impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid stable map id")
    }
}
impl std::error::Error for InvalidId {}

/// A canonical graph identity, never based on line numbers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "wire-json", derive(serde::Serialize))]
#[cfg_attr(feature = "wire-json", serde(transparent))]
pub struct StableId(String);
impl StableId {
    /// Builds a validated id. Ordinals are positive and only used for duplicates.
    ///
    /// # Errors
    /// Returns [`InvalidId`] for invalid paths or names, zero ordinals, or an ordinal without a name.
    pub fn new(
        kind: NodeKind,
        path: &str,
        name: Option<&str>,
        ordinal: Option<u32>,
    ) -> Result<Self, InvalidId> {
        if ordinal == Some(0) || (ordinal.is_some() && name.is_none()) {
            return Err(InvalidId);
        }
        let mut value = format!("{}:{path}", kind.letter());
        if let Some(name) = name {
            value.push('#');
            value.push_str(name);
        }
        if let Some(ordinal) = ordinal {
            value.push('~');
            value.push_str(&ordinal.to_string());
        }
        value.parse()
    }

    /// Canonical readable spelling.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Six SHA-256 hex digits, available only for ids longer than 120 bytes.
    /// Alias collisions must be refused by the query layer, never guessed.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn alias(&self) -> Option<String> {
        if self.0.len() <= 120 {
            return None;
        }
        let digest = super::sha256::alias_digest(self.0.as_bytes()).ok()?;
        Some(format!(
            "@{:02x}{:02x}{:02x}",
            digest[0], digest[1], digest[2]
        ))
    }
}
impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl FromStr for StableId {
    type Err = InvalidId;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (prefix, rest) = value.split_once(':').ok_or(InvalidId)?;
        if !matches!(prefix, "p" | "k" | "m" | "s" | "t" | "d")
            || value.chars().any(char::is_control)
        {
            return Err(InvalidId);
        }
        let (path, name) = match rest.split_once('#') {
            Some((path, name)) => (path, Some(name)),
            None => (rest, None),
        };
        if path.is_empty()
            || path.contains(['\\', '~', ':'])
            || path.starts_with('/')
            || (path != "."
                && path
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == ".."))
        {
            return Err(InvalidId);
        }
        if matches!(prefix, "s" | "t" | "d") != name.is_some()
            || name.is_some_and(|n| n.is_empty() || n.contains('#'))
        {
            return Err(InvalidId);
        }
        if let Some(name) = name
            && let Some((base, ordinal)) = name.split_once('~')
            && (base.is_empty()
                || ordinal.starts_with('0')
                || !ordinal.bytes().all(|b| b.is_ascii_digit())
                || ordinal.parse::<u32>().is_err())
        {
            return Err(InvalidId);
        }
        Ok(Self(value.into()))
    }
}
#[cfg(feature = "wire-json")]
impl<'de> serde::Deserialize<'de> for StableId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// A query reference, accepting either a readable id or a short SHA-256 alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdReference {
    /// A full canonical graph identity.
    Full(StableId),
    /// A validated lowercase six-hex alias, including the leading at sign.
    Alias(String),
}
impl FromStr for IdReference {
    type Err = InvalidId;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(hex) = value.strip_prefix('@') {
            if hex.len() == 6
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Ok(Self::Alias(value.into()));
            }
            return Err(InvalidId);
        }
        value.parse().map(Self::Full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_readable_and_validated_on_deserialization() {
        let id =
            StableId::new(NodeKind::Symbol, "src/lib.rs", Some("Chain::append"), None).unwrap();
        assert_eq!(id.as_str(), "s:src/lib.rs#Chain::append");
        assert!(id.alias().is_none());
        #[cfg(feature = "wire-json")]
        {
            assert_eq!(
                serde_json::from_str::<StableId>(&serde_json::to_string(&id).unwrap()).unwrap(),
                id
            );
        }
        for bad in [
            "s:../lib.rs#x",
            "m:/abs",
            "s:src/lib.rs",
            "s:x#x~0",
            "s:x#x~01",
            "m:x#y",
            "s:x#x\ny",
            "s:x#x~1~2",
        ] {
            assert!(bad.parse::<StableId>().is_err(), "{bad}");
            #[cfg(feature = "wire-json")]
            assert!(
                serde_json::from_str::<StableId>(&serde_json::to_string(bad).unwrap()).is_err()
            );
        }
        assert_eq!(
            StableId::new(NodeKind::Symbol, "x", Some("x"), Some(2))
                .unwrap()
                .as_str(),
            "s:x#x~2"
        );
        assert!(StableId::new(NodeKind::Symbol, "x", Some("x"), Some(0)).is_err());
    }
    #[test]
    fn alias_threshold_counts_utf8_bytes() {
        let short: StableId = format!("m:{}", "x".repeat(118)).parse().unwrap();
        assert!(short.alias().is_none());
        let long: StableId = format!("m:{}", "x".repeat(119)).parse().unwrap();
        assert_eq!(long.alias(), Some("@d9509f".into()));
        assert_eq!(long.alias(), long.clone().alias());
        assert!(
            format!("m:{}", "é".repeat(59))
                .parse::<StableId>()
                .unwrap()
                .alias()
                .is_none()
        );
        assert!(
            format!("m:{}", "é".repeat(60))
                .parse::<StableId>()
                .unwrap()
                .alias()
                .is_some()
        );
        assert!(matches!(
            long.alias().unwrap().parse::<IdReference>().unwrap(),
            IdReference::Alias(_)
        ));
        assert!(matches!(
            long.as_str().parse::<IdReference>().unwrap(),
            IdReference::Full(_)
        ));
        for invalid in ["@", "@ABCDEF", "@12345", "@1234567", "@zzzzzz"] {
            assert!(invalid.parse::<IdReference>().is_err());
        }
    }
}
