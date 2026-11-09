// SPDX-License-Identifier: Apache-2.0
//! Shared operation outcomes and response encoding for PLUGINS.md §5.1.
//! Depends on distribution errors, facade capabilities and render primitives.
//! Used by the dispatcher and each plugin operation module.

use dctx::Capabilities;
use dctx_plugin_distribution::{Distribution, DistributionError};

use super::{render, Env};

/// Rendered command result. `code` is the process status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Zero is success. One is a typed refusal. Two is usage.
    pub code: u8,
    /// Text or one JSON object.
    pub body: String,
}

pub(super) fn dist<'a>(env: &'a Env<'_>) -> Distribution<'a> {
    Distribution::new(env.network, env.store, env.clock, env.trust, env.policy)
}

/// Success. JSON keeps the facade counters; text is `text()` only, without diagnostics.
pub(super) fn done(
    command: &str,
    json: bool,
    env: &Env<'_>,
    fill: impl FnOnce(String) -> String,
    text: impl FnOnce() -> String,
) -> Outcome {
    if json {
        let fields = format!(
            "\"ok\":true,\"command\":{},\"facade_git\":{},\"facade_spawns\":{},\"facade_git_children\":{}",
            render::json_string(command),
            if Capabilities::current().git { "true" } else { "false" },
            env.host.spawns(),
            env.host.git_children()
        );
        Outcome {
            code: 0,
            body: format!("{{{}}}", fill(fields)),
        }
    } else {
        Outcome {
            code: 0,
            body: text(),
        }
    }
}

pub(super) fn ok(command: &str, text: &str, json: bool, env: &Env<'_>) -> Outcome {
    done(
        command,
        json,
        env,
        |fields| format!("{fields},\"text\":{}", render::json_string(text)),
        || text.to_owned(),
    )
}

/// Two-line text failure, as in docs/CLI-EXIT-CODES.md. Printed on stderr.
pub(super) fn failure_text(command: &str, message: &str, hint: &str) -> String {
    format!("❌ Error: {command}: {message}\nhint: {hint}")
}

/// Refusal without a known plugin id.
pub(super) fn fail(command: &str, error: &DistributionError, json: bool) -> Outcome {
    fail_for(command, None, error, json)
}

/// Refusal. `id` names the plugin in the text message and hint when known.
pub(super) fn fail_for(
    command: &str,
    id: Option<&str>,
    error: &DistributionError,
    json: bool,
) -> Outcome {
    let name = code_name(error);
    if json {
        Outcome {
            code: 1,
            body: format!(
                "{{\"ok\":false,\"command\":{},\"error\":{},\"detail\":{}}}",
                render::json_string(command),
                render::json_string(name),
                render::json_string(&error.to_string())
            ),
        }
    } else {
        Outcome {
            code: 1,
            body: failure_text(
                command,
                &format!("{} ({name})", message(id, error)),
                &hint(command, id, error),
            ),
        }
    }
}

pub(super) fn usage(command: &str, reason: &str, json: bool) -> Outcome {
    if json {
        Outcome {
            code: 2,
            body: format!(
                "{{\"ok\":false,\"command\":{},\"error\":\"Usage\",\"detail\":{}}}",
                render::json_string(command),
                render::json_string(reason)
            ),
        }
    } else {
        let hint = if command == "git" {
            "dctx git takes one verb: status or protect"
        } else {
            "run `dctx plugin help` for the command shapes"
        };
        Outcome {
            code: 2,
            body: failure_text(command, reason, hint),
        }
    }
}

/// Human text for a refusal. Variants whose `Display` is only the code name get words.
fn message(id: Option<&str>, error: &DistributionError) -> String {
    let subject = id.map_or_else(|| "the plugin".to_owned(), render::safe_summary);
    match error {
        DistributionError::ConsentRequired => {
            format!("{subject} needs your consent for its capabilities before it can run")
        }
        DistributionError::Disabled => format!("{subject} is disabled"),
        DistributionError::NotInstalled => format!("{subject} is not installed"),
        DistributionError::NotActive => format!("{subject} has no active version"),
        DistributionError::Quarantined => {
            format!("{subject} failed reverification and is quarantined")
        }
        DistributionError::ConfinementRequired => format!(
            "{subject} is a third-party plugin, and enabling one needs OS confinement this host does not provide"
        ),
        DistributionError::Leased => format!("a running worker still uses {subject}"),
        DistributionError::Busy => "another plugin operation holds the registry lock".to_owned(),
        DistributionError::Interrupted => {
            "the plugin store failed or was interrupted mid-operation".to_owned()
        }
        DistributionError::NotFound => "no matching plugin, release or file was found".to_owned(),
        DistributionError::UnknownKey => "no trusted signing key is configured".to_owned(),
        DistributionError::FingerprintMismatch => {
            "the key fingerprint does not match the key".to_owned()
        }
        DistributionError::KeyRevoked => "the signing key has been revoked".to_owned(),
        DistributionError::UnknownPublisher => {
            format!("no publisher is pinned for {subject}")
        }
        DistributionError::SignerChanged => {
            format!("{subject} is signed by a different key than its pinned publisher")
        }
        DistributionError::IdentityMismatch => {
            "the manifest names a different plugin id".to_owned()
        }
        DistributionError::ArtifactRevoked => "this release artifact has been revoked".to_owned(),
        DistributionError::UnsupportedTarget => {
            "the release has no artifact for this platform".to_owned()
        }
        DistributionError::TargetMismatch => {
            "the payload does not match the selected platform".to_owned()
        }
        DistributionError::IntegrityFailure => {
            "the payload does not match its signed SHA-256".to_owned()
        }
        DistributionError::SumsMismatch => {
            "SHA256SUMS disagrees with the signed manifest".to_owned()
        }
        DistributionError::ManifestDigestMismatch => {
            "the manifest differs from the digest named by the index".to_owned()
        }
        other => other.to_string(),
    }
}

/// Next step for a refusal.
fn hint(command: &str, id: Option<&str>, error: &DistributionError) -> String {
    let id = id.map_or_else(|| "<id>".to_owned(), render::safe_summary);
    match error {
        DistributionError::ConsentRequired | DistributionError::NotActive => format!(
            "review its capabilities with `dctx plugin info {id}`, then approve them with `dctx plugin enable {id} --approve`"
        ),
        DistributionError::Disabled => {
            format!("turn it back on with `dctx plugin enable {id} --approve`")
        }
        DistributionError::NotInstalled => {
            "`dctx plugin list` shows the installed and bundled plugins".to_owned()
        }
        DistributionError::ConfinementRequired => {
            "only the bundled dctx/git plugin can be enabled on this host".to_owned()
        }
        DistributionError::Busy | DistributionError::Leased => {
            "retry when the other plugin command or worker has finished".to_owned()
        }
        DistributionError::UnknownKey | DistributionError::Network(_)
            if matches!(command, "search" | "install" | "update") =>
        {
            "this build has no online plugin discovery; install an offline bundle with `dctx plugin install --file <bundle> --id <publisher/name>`".to_owned()
        }
        DistributionError::StaleRevocation { .. } => {
            "the revocation state must be refreshed before this check can pass".to_owned()
        }
        DistributionError::Quarantined
        | DistributionError::IntegrityFailure
        | DistributionError::SizeMismatch { .. } => {
            format!("remove it with `dctx plugin remove {id}` and install a fresh copy")
        }
        _ => "run `dctx plugin help` for the command shapes".to_owned(),
    }
}

pub(super) fn code_name(error: &DistributionError) -> &'static str {
    match error {
        DistributionError::Malformed { .. } => "Malformed",
        DistributionError::Signature(_) => "Signature",
        DistributionError::UnknownKey => "UnknownKey",
        DistributionError::FingerprintMismatch => "FingerprintMismatch",
        DistributionError::KeyRevoked => "KeyRevoked",
        DistributionError::UnknownPublisher => "UnknownPublisher",
        DistributionError::SignerChanged => "SignerChanged",
        DistributionError::IdentityMismatch => "IdentityMismatch",
        DistributionError::ArtifactRevoked => "ArtifactRevoked",
        DistributionError::Downgrade { .. } => "Downgrade",
        DistributionError::Replay { .. } => "Replay",
        DistributionError::UnsupportedTarget => "UnsupportedTarget",
        DistributionError::TargetMismatch => "TargetMismatch",
        DistributionError::SizeMismatch { .. } => "SizeMismatch",
        DistributionError::IntegrityFailure => "IntegrityFailure",
        DistributionError::SumsMismatch => "SumsMismatch",
        DistributionError::ManifestDigestMismatch => "ManifestDigestMismatch",
        DistributionError::LimitExceeded { .. } => "LimitExceeded",
        DistributionError::Origin(_) => "Origin",
        DistributionError::Network(_) => "Network",
        DistributionError::Busy => "Busy",
        DistributionError::Interrupted => "Interrupted",
        DistributionError::NotInstalled => "NotInstalled",
        DistributionError::NotActive => "NotActive",
        DistributionError::Quarantined => "Quarantined",
        DistributionError::Disabled => "Disabled",
        DistributionError::ConsentRequired => "ConsentRequired",
        DistributionError::ConfinementRequired => "ConfinementRequired",
        DistributionError::StaleRevocation { .. } => "StaleRevocation",
        DistributionError::Leased => "Leased",
        DistributionError::NotFound => "NotFound",
    }
}
