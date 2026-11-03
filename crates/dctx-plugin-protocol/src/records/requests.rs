// SPDX-License-Identifier: Apache-2.0
//! Session request params: the host control requests and the dispatch over every method.
//! Implements requests.schema.json (`initialize`, `probe`, `cancel`, `shutdown`, plus `broker::`* forms) and the
//! S-REPLAY action input: canonical `{method, params}` without `params.action_id` (wire.md §Bytes).
//! Depends on json, common, capability, invocation, broker, method; used by envelope and session.

use crate::json::encode::to_canonical;
use crate::json::fields::{Fields, Obj, array, constant_true, list, uint, unique_list};
use crate::json::value::Json;
use crate::records::broker::{ChunkRead, FileReplace, FileStage, GitAddIgnore, OutputList, StateReclaim, StateWrite};
use crate::records::capability::{Grant, Liveness};
use crate::records::common::{Config, HookName, Identity, Limits, Resource, Wait, decode_config, encode_config};
use crate::records::common::{decode_hook_decl, encode_hook_decl};
use crate::records::invocation::Invocation;
use crate::records::method::Method;
use crate::records::scalars::{Digest, Token};
use crate::refusal::Refusal;

/// `initialize`: the first session binding with inert grants and resources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Initialize {
    /// Verified identity (must match the handshake).
    pub identity: Identity,
    /// Validated config.
    pub config: Config,
    /// Effective hooks (at most 5, unique).
    pub hooks: Vec<HookName>,
    /// Inert grants (at most 64).
    pub grants: Vec<Grant>,
    /// Resource handles (at most 128).
    pub resources: Vec<Resource>,
    /// Policy epoch.
    pub policy_epoch: u64,
    /// Immutable plugin-plan digest.
    pub plan_digest: Digest,
    /// Effective limits.
    pub limits: Limits,
    /// Liveness policy.
    pub liveness: Liveness,
}

wire_enum!(
    /// Why the host cancels an invocation.
    CancelReason {
        /// User request.
        User => "user",
        /// Policy revocation.
        Revoked => "revoked",
        /// Budget exhausted.
        Budget => "budget",
        /// Liveness stall.
        Stalled => "stalled",
        /// Session shutdown.
        Shutdown => "shutdown",
        /// Peer protocol error.
        ProtocolError => "protocol-error",
    }
);

wire_enum!(
    /// Why the host ends a session.
    ShutdownReason {
        /// Normal end.
        SessionEnded => "session-ended",
        /// Plugin disabled.
        Disabled => "disabled",
        /// Policy revocation.
        Revoked => "revoked",
        /// Peer protocol error.
        ProtocolError => "protocol-error",
    }
);

/// `probe`: a responsiveness challenge for the active invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    /// Active invocation.
    pub invocation: Token,
    /// Challenge the worker must echo exactly.
    pub challenge: Token,
    /// Last progress sequence the host saw.
    pub last_progress_seq: u64,
    /// Dependency the host believes is pending.
    pub wait: Option<Wait>,
}

/// A typed session request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// `initialize`.
    Initialize(Box<Initialize>),
    /// `invoke`.
    Invoke(Box<Invocation>),
    /// `probe`.
    Probe(Probe),
    /// `cancel`.
    Cancel {
        /// Active invocation.
        invocation: Token,
        /// Reason.
        reason: CancelReason,
    },
    /// `shutdown`.
    Shutdown {
        /// Reason.
        reason: ShutdownReason,
    },
    /// `output.read`.
    OutputRead(ChunkRead),
    /// `output.list`.
    OutputList(OutputList),
    /// `file.read`.
    FileRead(ChunkRead),
    /// `file.stage`.
    FileStage(FileStage),
    /// `file.replace`.
    FileReplace(FileReplace),
    /// `program.run` (`git.add-ignore`).
    ProgramRun(GitAddIgnore),
    /// `state.read`.
    StateRead(ChunkRead),
    /// `state.write`.
    StateWrite(StateWrite),
    /// `state.reclaim`.
    StateReclaim(StateReclaim),
}

impl Request {
    /// The request's method.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn method(&self) -> Method {
        match self {
            Self::Initialize(_) => Method::Initialize,
            Self::Invoke(_) => Method::Invoke,
            Self::Probe(_) => Method::Probe,
            Self::Cancel { .. } => Method::Cancel,
            Self::Shutdown { .. } => Method::Shutdown,
            Self::OutputRead(_) => Method::OutputRead,
            Self::OutputList(_) => Method::OutputList,
            Self::FileRead(_) => Method::FileRead,
            Self::FileStage(_) => Method::FileStage,
            Self::FileReplace(_) => Method::FileReplace,
            Self::ProgramRun(_) => Method::ProgramRun,
            Self::StateRead(_) => Method::StateRead,
            Self::StateWrite(_) => Method::StateWrite,
            Self::StateReclaim(_) => Method::StateReclaim,
        }
    }

    /// Decodes `params` for `method`.
    /// # Errors
    /// Refuses parameters that violate the selected method's closed schema, bounds, or semantic rules.
    pub fn decode(method: Method, params: &Json) -> Result<Self, Refusal> {
        Ok(match method {
            Method::Initialize => Self::Initialize(Box::new(decode_initialize(params)?)),
            Method::Invoke => Self::Invoke(Box::new(Invocation::decode(params)?)),
            Method::Probe => {
                let mut f = Fields::of(params)?;
                let probe = Probe {
                    invocation: Token::decode(f.req("invocation")?)?,
                    challenge: Token::decode(f.req("challenge")?)?,
                    last_progress_seq: uint(f.req("last_progress_seq")?)?,
                    wait: f.opt("wait").map(Wait::decode).transpose()?,
                };
                f.finish()?;
                Self::Probe(probe)
            }
            Method::Cancel => {
                let mut f = Fields::of(params)?;
                let cancel = Self::Cancel {
                    invocation: Token::decode(f.req("invocation")?)?,
                    reason: CancelReason::decode(f.req("reason")?)?,
                };
                f.finish()?;
                cancel
            }
            Method::Shutdown => {
                let mut f = Fields::of(params)?;
                let shutdown = Self::Shutdown { reason: ShutdownReason::decode(f.req("reason")?)? };
                f.finish()?;
                shutdown
            }
            Method::OutputRead => Self::OutputRead(ChunkRead::decode(params, "handle", false)?),
            Method::OutputList => Self::OutputList(OutputList::decode(params)?),
            Method::FileRead => Self::FileRead(ChunkRead::decode(params, "resource", true)?),
            Method::FileStage => Self::FileStage(FileStage::decode(params)?),
            Method::FileReplace => Self::FileReplace(FileReplace::decode(params)?),
            Method::ProgramRun => Self::ProgramRun(GitAddIgnore::decode(params)?),
            Method::StateRead => Self::StateRead(ChunkRead::decode(params, "handle", true)?),
            Method::StateWrite => Self::StateWrite(StateWrite::decode(params)?),
            Method::StateReclaim => Self::StateReclaim(StateReclaim::decode(params)?),
        })
    }

    /// Encodes the closed `params` object.
    pub fn encode_params(&self) -> Json {
        let with_action = |obj: Obj, action: &Token| obj.put("action_id", action.encode()).build();
        match self {
            Self::Initialize(init) => encode_initialize(init),
            Self::Invoke(invocation) => invocation.encode(),
            Self::Probe(probe) => Obj::new()
                .put("invocation", probe.invocation.encode())
                .put("challenge", probe.challenge.encode())
                .put("last_progress_seq", Json::uint(probe.last_progress_seq))
                .maybe("wait", probe.wait.as_ref().map(Wait::encode))
                .build(),
            Self::Cancel { invocation, reason } => {
                Obj::new().put("invocation", invocation.encode()).put("reason", reason.encode()).build()
            }
            Self::Shutdown { reason } => Obj::new().put("reason", reason.encode()).build(),
            Self::OutputRead(read) | Self::StateRead(read) => read.encode("handle"),
            Self::OutputList(list) => list.encode(),
            Self::FileRead(read) => read.encode("resource"),
            Self::FileStage(stage) => stage.encode(),
            Self::FileReplace(replace) => with_action(replace.encode_without_action(), &replace.action_id),
            Self::ProgramRun(run) => with_action(run.encode_without_action(), &run.action_id),
            Self::StateWrite(write) => with_action(write.encode_without_action(), &write.action_id),
            Self::StateReclaim(reclaim) => with_action(reclaim.encode_without_action(), &reclaim.action_id),
        }
    }

    /// Replay key and canonical argument bytes `{method, params}` without `action_id` (S-REPLAY).
    /// `None` for requests that carry no action ID.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn action_input(&self) -> Option<(&Token, Vec<u8>)> {
        let (action, params) = match self {
            Self::FileReplace(replace) => (&replace.action_id, replace.encode_without_action()),
            Self::ProgramRun(run) => (&run.action_id, run.encode_without_action()),
            Self::StateWrite(write) => (&write.action_id, write.encode_without_action()),
            Self::StateReclaim(reclaim) => (&reclaim.action_id, reclaim.encode_without_action()),
            _ => return None,
        };
        let input = Obj::new().put("method", self.method().encode()).put("params", params.build()).build();
        Some((action, to_canonical(&input)))
    }
}

fn decode_initialize(params: &Json) -> Result<Initialize, Refusal> {
    let mut f = Fields::of(params)?;
    let init = Initialize {
        identity: Identity::decode(f.req("identity")?)?,
        config: decode_config(f.req("config")?)?,
        hooks: unique_list(f.req("hooks")?, 0, 5, decode_hook_decl)?,
        grants: list(f.req("grants")?, 0, 64, Grant::decode)?,
        resources: list(f.req("resources")?, 0, 128, Resource::decode)?,
        policy_epoch: uint(f.req("policy_epoch")?)?,
        plan_digest: Digest::decode(f.req("plan_digest")?)?,
        limits: Limits::decode(f.req("limits")?)?,
        liveness: Liveness::decode(f.req("liveness")?)?,
    };
    constant_true(f.req("cancellation")?)?;
    constant_true(f.req("progress")?)?;
    f.finish()?;
    Ok(init)
}

fn encode_initialize(init: &Initialize) -> Json {
    Obj::new()
        .put("identity", init.identity.encode())
        .put("config", encode_config(&init.config))
        .put("hooks", array(&init.hooks, encode_hook_decl))
        .put("grants", array(&init.grants, Grant::encode))
        .put("resources", array(&init.resources, Resource::encode))
        .put("policy_epoch", Json::uint(init.policy_epoch))
        .put("plan_digest", init.plan_digest.encode())
        .put("limits", init.limits.encode())
        .put("cancellation", Json::Bool(true))
        .put("progress", Json::Bool(true))
        .put("liveness", init.liveness.encode())
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::decode::{DecodeLimits, decode_object};

    fn params(text: &str) -> Json {
        decode_object(text.as_bytes(), DecodeLimits::WIRE).expect("json")
    }

    #[test]
    fn broker_requests_round_trip_and_refuse_extras() {
        let cases = [
            (Method::FileRead, r#"{"resource":"file:ignore","offset":1,"length":2,"precondition":"pre:1"}"#),
            (Method::OutputRead, r#"{"handle":"out:1","offset":0,"length":3}"#),
            (Method::FileStage, r#"{"resource":"file:ignore","precondition":"pre:1","offset":0,"bytes":"","final":true}"#),
            (Method::StateWrite, r#"{"handle":"state:1","precondition":"pre:s","bytes":"YWJj","provenance":["a"],"action_id":"a:1"}"#),
            (Method::StateReclaim, r#"{"handle":"state:1","precondition":"pre:s","action_id":"a:1"}"#),
        ];
        for (method, text) in cases {
            let request = Request::decode(method, &params(text)).expect(text);
            assert_eq!(request.method(), method);
            assert_eq!(Request::decode(method, &request.encode_params()), Ok(request));
        }
        let output_precondition = r#"{"handle":"out:1","offset":0,"length":3,"precondition":"pre:1"}"#;
        assert!(Request::decode(Method::OutputRead, &params(output_precondition)).is_err());
        let zero = r#"{"resource":"file:ignore","offset":0,"length":0}"#;
        assert!(Request::decode(Method::FileRead, &params(zero)).is_err());
        let argv = r#"{"operation":"git.add-ignore","args":["add","."],"action_id":"action:stage"}"#;
        assert!(Request::decode(Method::ProgramRun, &params(argv)).is_err());
    }

    #[test]
    fn action_input_excludes_action_id() {
        let text = r#"{"resource":"file:ignore","precondition":"pre:1","upload":"upload:1","action_id":"action:replace"}"#;
        let request = Request::decode(Method::FileReplace, &params(text)).expect("valid");
        let (action, input) = request.action_input().expect("has action");
        assert_eq!(action.as_str(), "action:replace");
        let expected = r#"{"method":"file.replace","params":{"precondition":"pre:1","resource":"file:ignore","upload":"upload:1"}}"#;
        assert_eq!(String::from_utf8(input).expect("utf8"), expected);
        let read = Request::decode(Method::StateRead, &params(r#"{"handle":"s","offset":0,"length":1}"#)).expect("ok");
        assert!(read.action_input().is_none());
    }
}
