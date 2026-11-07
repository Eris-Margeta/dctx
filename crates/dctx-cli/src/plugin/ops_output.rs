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

pub(super) fn facade_note(env: &Env<'_>) -> String {
    let caps = Capabilities::current();
    format!(
        "facade_git={} spawns={} git_children={}",
        caps.git,
        env.host.spawns(),
        env.host.git_children()
    )
}

pub(super) fn done(
    command: &str,
    json: bool,
    env: &Env<'_>,
    fill: impl FnOnce(String) -> String,
) -> Outcome {
    let note = facade_note(env);
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
        let extra = fill(String::new());
        Outcome {
            code: 0,
            body: format!("{command} {note} {extra}"),
        }
    }
}

pub(super) fn ok(command: &str, text: &str, json: bool, env: &Env<'_>) -> Outcome {
    if json {
        done(command, true, env, |fields| {
            format!("{fields},\"text\":{}", render::json_string(text))
        })
    } else {
        let _ = facade_note(env);
        Outcome {
            code: 0,
            body: text.to_owned(),
        }
    }
}

pub(super) fn fail(command: &str, error: &DistributionError, json: bool) -> Outcome {
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
            body: format!("{command}: {error}"),
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
        Outcome {
            code: 2,
            body: format!("{command}: {reason}"),
        }
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
