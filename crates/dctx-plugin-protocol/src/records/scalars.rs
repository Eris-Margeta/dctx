// SPDX-License-Identifier: Apache-2.0
//! Validated string newtypes: tokens, request IDs, plugin IDs, names, roles, aliases, `SemVer`, digests, paths, text.
//! Implements common.schema.json string `$defs` with byte-length bounds and whole-string matching (S-BOUNDS),
//! plus the plugin-ID component and `SemVer` prerelease rules of S-MANIFEST. Depends on json and refusal.
//! Used by every record; a value of these types is always valid by construction.

use crate::json::fields::string;
use crate::json::value::Json;
use crate::refusal::Refusal;

macro_rules! text_type {
    ($(#[$meta:meta])* $name:ident, $check:ident, $reason:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Validates `value` against the contract pattern and bounds.
            /// # Errors
            /// Refuses text outside this scalar's contract pattern or length bounds.
            pub fn new(value: &str) -> Result<Self, Refusal> {
                if $check(value) { Ok(Self(value.to_owned())) } else { Err(Refusal::schema($reason)) }
            }

            /// The validated text.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
                Self::new(string(value)?)
            }

            pub(crate) fn encode(&self) -> Json {
                Json::Str(self.0.clone())
            }
        }
    };
}

text_type!(
    /// Opaque host or plugin token (handles, sessions, receipts, preconditions).
    Token, is_token, "invalid-token"
);
text_type!(
    /// Session request ID: `h:` for host-sent and `p:` for plugin-sent requests.
    RequestId, is_request_id, "invalid-request-id"
);
text_type!(
    /// Lowercase `publisher/name` plugin ID.
    PluginId, is_plugin_id, "invalid-plugin-id"
);
text_type!(
    /// Lowercase identifier (`name`): APIs, config keys, reasons, diagnostic codes, phases.
    Name, is_name, "invalid-name"
);
text_type!(
    /// Output or resource role.
    Role, is_role, "invalid-role"
);
text_type!(
    /// Command alias that does not shadow a built-in command.
    Alias, is_alias, "invalid-alias"
);
text_type!(
    /// `SemVer` 2.0 version string.
    SemVer, is_semver, "invalid-semver"
);
text_type!(
    /// `sha256:` followed by 64 lowercase hex digits.
    Digest, is_digest, "invalid-digest"
);
text_type!(
    /// Literal project-relative resource path (never a glob or traversal).
    ProjectPath, is_project_path, "invalid-path"
);
text_type!(
    /// Bounded human text (at most 4096 UTF-8 bytes).
    Text, is_text, "invalid-text"
);

/// Which side sent a session request, from its ID prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sender {
    /// The dctx host (`h:` IDs).
    Host,
    /// The plugin worker (`p:` IDs).
    Plugin,
}

impl Sender {
    /// The other side of the session.
    #[allow(clippy::must_use_candidate, clippy::return_self_not_must_use, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn peer(self) -> Self {
        match self {
            Self::Host => Self::Plugin,
            Self::Plugin => Self::Host,
        }
    }
}

impl RequestId {
    /// Side that owns this ID.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn sender(&self) -> Sender {
        if self.0.starts_with("h:") { Sender::Host } else { Sender::Plugin }
    }
}

const RESERVED_ALIASES: [&str; 7] = ["plugin", "pack", "map", "cache", "template", "help", "version"];
const WINDOWS_DEVICES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1",
    "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn bounded(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.len())
}

fn is_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    bounded(value, 1, 128)
        && bytes[0].is_ascii_alphanumeric()
        && bytes.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'.' | b'_' | b'-'))
}

fn is_request_id(value: &str) -> bool {
    let rest = value.strip_prefix("h:").or_else(|| value.strip_prefix("p:"));
    bounded(value, 1, 128)
        && rest.is_some_and(|rest| {
            !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

fn is_id_component(part: &str) -> bool {
    let bytes = part.as_bytes();
    !bytes.is_empty()
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
        && !part.ends_with('.')
        && !WINDOWS_DEVICES.iter().any(|device| part == *device || part.starts_with(&format!("{device}.")))
}

fn is_plugin_id(value: &str) -> bool {
    bounded(value, 1, 128)
        && value.split_once('/').is_some_and(|(publisher, name)| is_id_component(publisher) && is_id_component(name))
}

fn is_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    bounded(value, 1, 128)
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

fn is_role(value: &str) -> bool {
    let bytes = value.as_bytes();
    bounded(value, 1, 64)
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn is_alias(value: &str) -> bool {
    is_role(value) && !RESERVED_ALIASES.contains(&value)
}

fn is_numeric_identifier(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) && (part == "0" || !part.starts_with('0'))
}

fn is_dot_identifiers(text: &str, numeric_no_leading_zero: bool) -> bool {
    text.split('.').all(|part| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && (!numeric_no_leading_zero || !part.bytes().all(|b| b.is_ascii_digit()) || is_numeric_identifier(part))
    })
}

fn is_semver(value: &str) -> bool {
    if !bounded(value, 1, 128) {
        return false;
    }
    let (rest, build) = match value.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (value, None),
    };
    let (core, prerelease) = match rest.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (rest, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| is_numeric_identifier(part))
        && prerelease.is_none_or(|pre| is_dot_identifiers(pre, true))
        && build.is_none_or(|build| is_dot_identifiers(build, false))
}

fn is_digest(value: &str) -> bool {
    value.len() == 71
        && value
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

fn is_project_path(value: &str) -> bool {
    bounded(value, 1, 256)
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-'))
        && value.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

fn is_text(value: &str) -> bool {
    value.len() <= 4096
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_request_ids_match_whole_strings() {
        assert!(Token::new("file:ignore").is_ok());
        assert!(Token::new("../private").is_err());
        assert!(Token::new("a\n").is_err());
        assert!(Token::new(&"a".repeat(129)).is_err());
        assert!(Token::new("").is_err());
        let id = RequestId::new("p:1").expect("valid");
        assert_eq!(id.sender(), Sender::Plugin);
        assert_eq!(RequestId::new("h:hello").map(|id| id.sender()), Ok(Sender::Host));
        assert!(RequestId::new("x:1").is_err());
        assert!(RequestId::new("p:").is_err());
        assert!(RequestId::new("p:a:b").is_err());
        assert_eq!(Sender::Host.peer(), Sender::Plugin);
    }

    #[test]
    fn plugin_ids_refuse_dot_device_and_case_components() {
        assert!(PluginId::new("dctx/git").is_ok());
        assert!(PluginId::new("Dctx/git").is_err());
        assert!(PluginId::new("dctx/con").is_err());
        assert!(PluginId::new("dctx/nul.txt").is_err());
        assert!(PluginId::new("dctx/git.").is_err());
        assert!(PluginId::new("dctx/git/x").is_err());
        assert!(PluginId::new("dctx").is_err());
    }

    #[test]
    fn names_roles_and_aliases() {
        assert!(Name::new("read-ignore").is_ok());
        assert!(Name::new("1abc").is_err());
        assert!(Role::new("pack-text").is_ok());
        assert!(Role::new("pack.text").is_err());
        assert!(Alias::new("git").is_ok());
        assert!(Alias::new("pack").is_err());
    }

    #[test]
    fn semver_follows_2_0() {
        assert!(SemVer::new("1.0.0").is_ok());
        assert!(SemVer::new("0.26.0-rc.1+build.07").is_ok());
        assert!(SemVer::new("1.0").is_err());
        assert!(SemVer::new("01.0.0").is_err());
        assert!(SemVer::new("1.0.0-01").is_err());
        assert!(SemVer::new("1.0.0-").is_err());
        assert!(SemVer::new("1.0.0-a..b").is_err());
    }

    #[test]
    fn digests_paths_and_text() {
        let hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(Digest::new(&format!("sha256:{hex}")).is_ok());
        assert!(Digest::new(&format!("sha256:{}", hex.to_uppercase())).is_err());
        assert!(Digest::new(hex).is_err());
        assert!(ProjectPath::new(".gitignore").is_ok());
        assert!(ProjectPath::new("a/b.txt").is_ok());
        for bad in ["/etc", "a/../b", "./a", "a//b", "a/", "a*"] {
            assert!(ProjectPath::new(bad).is_err(), "{bad}");
        }
        assert!(Text::new(&"\u{e9}".repeat(2048)).is_ok());
        assert!(Text::new(&"\u{e9}".repeat(2049)).is_err());
    }
}
