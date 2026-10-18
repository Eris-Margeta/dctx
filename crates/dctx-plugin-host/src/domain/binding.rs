//! Opaque binding context, replay keys and argument digests.
//! Implements Authority.tla `Context`/`CheckBinding` (plugin, artifact, root, invocation, session,
//! generation, namespace) and properties.md `K` and `D(args)` (S-REPLAY). Depends on dctx-hash and
//! protocol scalars. Used by authority (handle checks), ledger (keys) and the broker service.

use dctx_hash::Sha256;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::{Digest, PluginId, Token};

use crate::error::HostRefusal;

/// Every field a host handle is bound to; a handle is usable only under an equal context.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Binding {
    /// Plugin ID.
    pub plugin: PluginId,
    /// Verified artifact digest.
    pub artifact: Digest,
    /// Host-opened project root identity.
    pub root: Token,
    /// Active invocation.
    pub invocation: Token,
    /// Session.
    pub session: Token,
    /// Output/index generation.
    pub generation: u64,
    /// Plugin state namespace.
    pub namespace: Token,
}

/// The host-bound replay key `K = (artifact, project, invocation, action_id)`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionKey {
    /// Artifact digest.
    pub artifact: Digest,
    /// Project identity.
    pub root: Token,
    /// Invocation.
    pub invocation: Token,
    /// Plugin-chosen action ID.
    pub action_id: Token,
}

impl ActionKey {
    /// Binds `action_id` to the host context; the plugin chooses only the last field.
    pub fn bind(context: &Binding, action_id: &Token) -> Self {
        Self {
            artifact: context.artifact.clone(),
            root: context.root.clone(),
            invocation: context.invocation.clone(),
            action_id: action_id.clone(),
        }
    }
}

/// `D(args)`: SHA-256 of canonical `{method, params-without-action_id}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArgDigest([u8; 32]);

impl ArgDigest {
    /// Reconstructs a validated hexadecimal SHA-256 digest without rehashing its text.
    pub fn from_hex(value: &str) -> Result<Self, HostRefusal> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(HostRefusal::audit("invalid-argument-digest"));
        }
        let mut bytes = [0; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| HostRefusal::audit("invalid-argument-digest"))?;
        }
        Ok(Self(bytes))
    }

    /// Digests canonical argument bytes.
    pub fn of(bytes: &[u8]) -> Result<Self, HostRefusal> {
        Sha256::digest(bytes).map(|digest| Self(*digest.as_bytes())).map_err(|_| HostRefusal::limit("argument-digest-too-long"))
    }

    /// Replay key and argument digest of an effect request; `None` for requests without `action_id`.
    pub fn of_request(context: &Binding, request: &Request) -> Result<Option<(ActionKey, Self)>, HostRefusal> {
        match request.action_input() {
            Some((action_id, bytes)) => Ok(Some((ActionKey::bind(context, action_id), Self::of(&bytes)?))),
            None => Ok(None),
        }
    }

    /// Raw digest bytes (private local metadata; never source content).
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn context(invocation: &str) -> Binding {
        Binding {
            plugin: PluginId::new("dctx/git").expect("id"),
            artifact: Digest::new(&format!("sha256:{}", "a".repeat(64))).expect("digest"),
            root: Token::new("project:1").expect("root"),
            invocation: Token::new(invocation).expect("invocation"),
            session: Token::new("s:1").expect("session"),
            generation: 1,
            namespace: Token::new("ns:git").expect("namespace"),
        }
    }

    #[test]
    fn equal_local_action_ids_differ_across_invocations() {
        let action = Token::new("a:1").expect("action");
        let first = ActionKey::bind(&context("op:1"), &action);
        let second = ActionKey::bind(&context("op:2"), &action);
        assert_ne!(first, second);
        assert_eq!(first, ActionKey::bind(&context("op:1"), &action));
    }

    #[test]
    fn digest_distinguishes_arguments() {
        assert_eq!(ArgDigest::of(b"a").expect("a"), ArgDigest::of(b"a").expect("a"));
        assert_ne!(ArgDigest::of(b"a").expect("a"), ArgDigest::of(b"b").expect("b"));
    }
}
