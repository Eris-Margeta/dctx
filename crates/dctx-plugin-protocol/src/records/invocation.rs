// SPDX-License-Identifier: Apache-2.0
//! Invoke payloads: the six closed hook/command invocation forms.
//! Implements invocation.schema.json (before/after pack, before/after index, cache-gc, command) and
//! wire.md §Invocation boundaries: projects and resources are opaque handles, never OS paths.
//! Depends on json, scalars, common and refusal; used by `requests::Request::Invoke` and session hook checks.

use crate::json::fields::{
    Fields, Obj, array, constant_int, constant_true, list, string, uint, unique_list,
};
use crate::json::value::Json;
use crate::records::common::{Config, HookName, Limits, decode_config, encode_config};
use crate::records::scalars::{Alias, Digest, Name, Role, SemVer, Text, Token};
use crate::refusal::Refusal;

wire_enum!(
    /// Where a planned output will be written.
    OutputLocation {
        /// Inside the project.
        Project => "project",
        /// Outside the project (no path disclosed).
        OutsideProject => "outside-project",
        /// Standard output.
        Stdout => "stdout",
        /// Clipboard.
        Clipboard => "clipboard",
    }
);

wire_enum!(
    /// Why the index runs.
    IndexIntent {
        /// Full build.
        Build => "build",
        /// Refresh.
        Refresh => "refresh",
        /// Lazy indexing.
        Lazy => "lazy",
    }
);

wire_enum!(
    /// Freshness of a published index.
    Freshness {
        /// Fresh.
        Fresh => "fresh",
        /// Stale.
        Stale => "stale",
    }
);

/// Planned output metadata, before creation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedOutput {
    /// Opaque handle.
    pub handle: Token,
    /// Output role.
    pub role: Role,
    /// Destination class.
    pub location: OutputLocation,
    /// Planned size, if known.
    pub bytes: Option<u64>,
}

/// Committed output metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedOutput {
    /// Opaque handle.
    pub handle: Token,
    /// Output role.
    pub role: Role,
    /// Committed size.
    pub bytes: u64,
}

impl PlannedOutput {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let output = Self {
            handle: Token::decode(f.req("handle")?)?,
            role: Role::decode(f.req("role")?)?,
            location: OutputLocation::decode(f.req("location")?)?,
            bytes: f.opt("bytes").map(uint).transpose()?,
        };
        f.finish()?;
        Ok(output)
    }

    pub(crate) fn encode(&self) -> Json {
        Obj::new()
            .put("handle", self.handle.encode())
            .put("role", self.role.encode())
            .put("location", self.location.encode())
            .maybe("bytes", self.bytes.map(Json::uint))
            .build()
    }
}

impl CommittedOutput {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let output = Self {
            handle: Token::decode(f.req("handle")?)?,
            role: Role::decode(f.req("role")?)?,
            bytes: uint(f.req("bytes")?)?,
        };
        f.finish()?;
        Ok(output)
    }

    pub(crate) fn encode(&self) -> Json {
        Obj::new()
            .put("handle", self.handle.encode())
            .put("role", self.role.encode())
            .put("bytes", Json::uint(self.bytes))
            .build()
    }
}

/// Hook-specific invocation payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookPayload {
    /// `before-pack-output-written`: planned outputs after authorization.
    BeforePackOutputWritten {
        /// First page of planned outputs (at most 64).
        outputs: Vec<PlannedOutput>,
        /// Host-bound continuation cursor.
        next: Option<Token>,
        /// Repository handle, when granted.
        repository: Option<Token>,
    },
    /// `after-pack-output-written`: committed outputs and generation.
    AfterPackOutputWritten {
        /// First page of committed outputs (at most 64).
        outputs: Vec<CommittedOutput>,
        /// Host-bound continuation cursor.
        next: Option<Token>,
        /// Committed generation.
        generation: Token,
    },
    /// `before-index`: after frozen selection, before content reads.
    BeforeIndex {
        /// Index intent.
        intent: IndexIntent,
        /// Selected file count.
        files: u64,
        /// Selected byte count.
        bytes: u64,
    },
    /// `after-index`: after complete publication.
    AfterIndex {
        /// Published generation.
        generation: Token,
        /// File count.
        files: u64,
        /// Symbol count.
        symbols: u64,
        /// Edge count.
        edges: u64,
        /// Freshness.
        freshness: Freshness,
    },
    /// `cache-gc`: aggregate counts and the plugin's own state handles.
    CacheGc {
        /// Reclaimed bytes.
        reclaimed_bytes: u64,
        /// Reclaimed objects.
        reclaimed_objects: u64,
        /// Own state handles (at most 64, unique).
        state_handles: Vec<Token>,
    },
    /// `command`: a declared alias/verb with bounded argv; never a shell.
    Command {
        /// Declared alias.
        alias: Alias,
        /// Declared verb.
        verb: Name,
        /// Arguments (at most 64).
        args: Vec<Text>,
    },
}

/// One invoke request's params.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    /// Invocation token.
    pub invocation: Token,
    /// Project handle (optional only for cache-gc and command).
    pub project: Option<Token>,
    /// Policy epoch.
    pub policy_epoch: u64,
    /// Selection digest (optional only for command).
    pub selection_digest: Option<Digest>,
    /// Host version.
    pub host_version: SemVer,
    /// Grant handles in force (at most 64, unique).
    pub grants: Vec<Token>,
    /// Effective limits.
    pub limits: Limits,
    /// Effective config.
    pub config: Config,
    /// Hook payload.
    pub payload: HookPayload,
}

impl Invocation {
    /// The declared hook this invocation runs, or `None` for a command.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn hook(&self) -> Option<HookName> {
        match self.payload {
            HookPayload::BeforePackOutputWritten { .. } => Some(HookName::BeforePackOutputWritten),
            HookPayload::AfterPackOutputWritten { .. } => Some(HookName::AfterPackOutputWritten),
            HookPayload::BeforeIndex { .. } => Some(HookName::BeforeIndex),
            HookPayload::AfterIndex { .. } => Some(HookName::AfterIndex),
            HookPayload::CacheGc { .. } => Some(HookName::CacheGc),
            HookPayload::Command { .. } => None,
        }
    }

    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let hook = string(f.req("hook")?)?;
        constant_int(f.req("hook_version")?, 1)?;
        let optional_project = matches!(hook, "cache-gc" | "command");
        let project = match f.opt("project") {
            Some(value) => Some(Token::decode(value)?),
            None if optional_project => None,
            None => return Err(Refusal::schema("missing-field")),
        };
        let selection_digest = match f.opt("selection_digest") {
            Some(value) => Some(Digest::decode(value)?),
            None if hook == "command" => None,
            None => return Err(Refusal::schema("missing-field")),
        };
        let payload = match hook {
            "before-pack-output-written" => HookPayload::BeforePackOutputWritten {
                outputs: list(f.req("outputs")?, 0, 64, PlannedOutput::decode)?,
                next: f.opt("next").map(Token::decode).transpose()?,
                repository: f.opt("repository").map(Token::decode).transpose()?,
            },
            "after-pack-output-written" => {
                constant_true(f.req("output_committed")?)?;
                HookPayload::AfterPackOutputWritten {
                    outputs: list(f.req("outputs")?, 0, 64, CommittedOutput::decode)?,
                    next: f.opt("next").map(Token::decode).transpose()?,
                    generation: Token::decode(f.req("generation")?)?,
                }
            }
            "before-index" => {
                let mut counts = Fields::of(f.req("counts")?)?;
                let payload = HookPayload::BeforeIndex {
                    intent: IndexIntent::decode(f.req("intent")?)?,
                    files: uint(counts.req("files")?)?,
                    bytes: uint(counts.req("bytes")?)?,
                };
                counts.finish()?;
                payload
            }
            "after-index" => {
                constant_true(f.req("output_committed")?)?;
                let mut counts = Fields::of(f.req("counts")?)?;
                let payload = HookPayload::AfterIndex {
                    generation: Token::decode(f.req("generation")?)?,
                    files: uint(counts.req("files")?)?,
                    symbols: uint(counts.req("symbols")?)?,
                    edges: uint(counts.req("edges")?)?,
                    freshness: Freshness::decode(f.req("freshness")?)?,
                };
                counts.finish()?;
                payload
            }
            "cache-gc" => {
                constant_true(f.req("output_committed")?)?;
                let mut counts = Fields::of(f.req("counts")?)?;
                let payload = HookPayload::CacheGc {
                    reclaimed_bytes: uint(counts.req("reclaimed_bytes")?)?,
                    reclaimed_objects: uint(counts.req("reclaimed_objects")?)?,
                    state_handles: unique_list(f.req("state_handles")?, 0, 64, Token::decode)?,
                };
                counts.finish()?;
                payload
            }
            "command" => HookPayload::Command {
                alias: Alias::decode(f.req("alias")?)?,
                verb: Name::decode(f.req("verb")?)?,
                args: list(f.req("args")?, 0, 64, Text::decode)?,
            },
            _ => return Err(Refusal::schema("unknown-hook")),
        };
        let invocation = Self {
            invocation: Token::decode(f.req("invocation")?)?,
            project,
            policy_epoch: uint(f.req("policy_epoch")?)?,
            selection_digest,
            host_version: SemVer::decode(f.req("host_version")?)?,
            grants: unique_list(f.req("grants")?, 0, 64, Token::decode)?,
            limits: Limits::decode(f.req("limits")?)?,
            config: decode_config(f.req("config")?)?,
            payload,
        };
        f.finish()?;
        Ok(invocation)
    }

    pub(crate) fn encode(&self) -> Json {
        let hook = self.hook().map_or("command", HookName::as_str);
        let base = Obj::new()
            .put("invocation", self.invocation.encode())
            .put("hook_version", Json::Int(1))
            .maybe("project", self.project.as_ref().map(Token::encode))
            .put("policy_epoch", Json::uint(self.policy_epoch))
            .maybe(
                "selection_digest",
                self.selection_digest.as_ref().map(Digest::encode),
            )
            .put("host_version", self.host_version.encode())
            .put("grants", array(&self.grants, Token::encode))
            .put("limits", self.limits.encode())
            .put("config", encode_config(&self.config))
            .put("hook", Json::str(hook));
        let counts = |pairs: &[(&str, u64)]| {
            pairs
                .iter()
                .fold(Obj::new(), |obj, (key, value)| {
                    obj.put(key, Json::uint(*value))
                })
                .build()
        };
        let object = match &self.payload {
            HookPayload::BeforePackOutputWritten {
                outputs,
                next,
                repository,
            } => base
                .put("outputs", array(outputs, PlannedOutput::encode))
                .maybe("next", next.as_ref().map(Token::encode))
                .maybe("repository", repository.as_ref().map(Token::encode)),
            HookPayload::AfterPackOutputWritten {
                outputs,
                next,
                generation,
            } => base
                .put("outputs", array(outputs, CommittedOutput::encode))
                .maybe("next", next.as_ref().map(Token::encode))
                .put("generation", generation.encode())
                .put("output_committed", Json::Bool(true)),
            HookPayload::BeforeIndex {
                intent,
                files,
                bytes,
            } => base
                .put("intent", intent.encode())
                .put("counts", counts(&[("files", *files), ("bytes", *bytes)])),
            HookPayload::AfterIndex {
                generation,
                files,
                symbols,
                edges,
                freshness,
            } => base
                .put("generation", generation.encode())
                .put(
                    "counts",
                    counts(&[("files", *files), ("symbols", *symbols), ("edges", *edges)]),
                )
                .put("freshness", freshness.encode())
                .put("output_committed", Json::Bool(true)),
            HookPayload::CacheGc {
                reclaimed_bytes,
                reclaimed_objects,
                state_handles,
            } => base
                .put(
                    "counts",
                    counts(&[
                        ("reclaimed_bytes", *reclaimed_bytes),
                        ("reclaimed_objects", *reclaimed_objects),
                    ]),
                )
                .put("state_handles", array(state_handles, Token::encode))
                .put("output_committed", Json::Bool(true)),
            HookPayload::Command { alias, verb, args } => base
                .put("alias", alias.encode())
                .put("verb", verb.encode())
                .put("args", array(args, Text::encode)),
        };
        object.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::decode::{DecodeLimits, decode_object};

    const BASE: &str = r#""invocation":"op:1","hook_version":1,"policy_epoch":1,"host_version":"0.26.0","grants":[],"limits":{"frame_bytes":1048576,"read_bytes":1,"actions":1,"outstanding_requests":16,"diagnostics_bytes":1,"chunk_bytes":1,"state_bytes":1,"upload_bytes":1,"queue_bytes":1,"memory_bytes":1,"work_units":1},"config":{}"#;

    fn invocation(rest: &str) -> Result<Invocation, Refusal> {
        let text = format!("{{{BASE},{rest}}}");
        Invocation::decode(&decode_object(text.as_bytes(), DecodeLimits::WIRE).expect("json"))
    }

    #[test]
    fn command_may_omit_project_and_selection_but_hooks_may_not() {
        let command =
            invocation(r#""hook":"command","alias":"git","verb":"status","args":["--x"]"#)
                .expect("command");
        assert_eq!(command.hook(), None);
        assert_eq!(Invocation::decode(&command.encode()), Ok(command));
        let digest = r#""selection_digest":"sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855""#;
        let gc = invocation(&format!(r#"{digest},"hook":"cache-gc","counts":{{"reclaimed_bytes":3,"reclaimed_objects":1}},"state_handles":["state:1"],"output_committed":true"#))
            .expect("gc");
        assert_eq!(gc.hook(), Some(HookName::CacheGc));
        assert_eq!(Invocation::decode(&gc.encode()), Ok(gc));
        let index = format!(
            r#"{digest},"hook":"before-index","intent":"build","counts":{{"files":1,"bytes":3}}"#
        );
        assert_eq!(invocation(&index).unwrap_err().reason, "missing-field");
        assert!(invocation(&format!(r#""project":"project:1",{index}"#)).is_ok());
    }

    #[test]
    fn committed_hooks_require_output_committed_true() {
        let digest = r#""project":"p:1","selection_digest":"sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855""#;
        let after = format!(
            r#"{digest},"hook":"after-index","generation":"generation:1","counts":{{"files":1,"symbols":0,"edges":0}},"freshness":"fresh","output_committed":COMMITTED"#
        );
        assert!(invocation(&after.replace("COMMITTED", "true")).is_ok());
        assert!(invocation(&after.replace("COMMITTED", "false")).is_err());
        assert!(
            invocation(
                &after
                    .replace("COMMITTED", "true")
                    .replace("\"edges\":0", "\"edges\":0,\"x\":1")
            )
            .is_err()
        );
        assert_eq!(
            invocation(&format!(r#"{digest},"hook":"rm","args":[]"#))
                .unwrap_err()
                .reason,
            "unknown-hook"
        );
    }
}
