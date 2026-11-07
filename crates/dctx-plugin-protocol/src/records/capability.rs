// SPDX-License-Identifier: Apache-2.0
//! Capability ceilings, grants and liveness policies carried by initialize.
//! Implements capabilities.schema.json, common.schema.json `grant` and dts.schema.json `$defs/liveness`.
//! A capability is a requested ceiling, never consent (S-CAPABILITY). Depends on json, scalars, refusal.
//! Used by `requests::Initialize` and by host grant intersection code.

use crate::json::fields::{
    Fields, Obj, array, boolean, constant, int_in, positive, string, uint, unique_list,
};
use crate::json::value::Json;
use crate::records::scalars::{ProjectPath, Role, Token};
use crate::refusal::Refusal;

wire_enum!(
    /// Engine operation whose outputs may be read.
    OutputOperation {
        /// Pack outputs.
        Pack => "pack",
        /// Index outputs.
        Index => "index",
    }
);

/// One capability ceiling (`capabilities` union).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Capability {
    /// Read one named project file.
    FileRead {
        /// Literal project-relative path.
        path: ProjectPath,
        /// Byte ceiling.
        max_bytes: u64,
    },
    /// Replace one named project file.
    FileWrite {
        /// Literal project-relative path.
        path: ProjectPath,
        /// Separately consented creation (Git v1 fixes false).
        create: bool,
        /// Growth ceiling.
        max_growth_bytes: u64,
        /// Size ceiling.
        max_bytes: u64,
    },
    /// The only v1 program recipe: `git.add-ignore` on `git-admin:stage-ignore`.
    GitAddIgnore {
        /// Output ceiling, 1 through 65536.
        max_output_bytes: u64,
    },
    /// Read committed or planned outputs.
    OutputsRead {
        /// Output roles (1 through 32, unique).
        roles: Vec<Role>,
        /// Operations (1 or 2, unique).
        operations: Vec<OutputOperation>,
        /// Byte ceiling.
        max_bytes: u64,
    },
    /// Plugin-private state.
    State {
        /// Read permission.
        read: bool,
        /// Write permission.
        write: bool,
        /// Reclaim permission.
        reclaim: bool,
        /// Byte ceiling.
        max_bytes: u64,
        /// Whether state may hold source content.
        source_content: bool,
    },
}

impl Capability {
    /// Decodes one closed capability object.
    /// # Errors
    /// Refuses unknown capability kinds, missing/extra fields, or invalid capability values and bounds.
    pub fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let capability = match string(f.req("kind")?)? {
            "file.read" => {
                constant(f.req("scope")?, "project")?;
                Self::FileRead {
                    path: ProjectPath::decode(f.req("path")?)?,
                    max_bytes: positive(f.req("max_bytes")?)?,
                }
            }
            "file.write" => {
                constant(f.req("scope")?, "project")?;
                Self::FileWrite {
                    path: ProjectPath::decode(f.req("path")?)?,
                    create: boolean(f.req("create")?)?,
                    max_growth_bytes: uint(f.req("max_growth_bytes")?)?,
                    max_bytes: positive(f.req("max_bytes")?)?,
                }
            }
            "program.run" => {
                constant(f.req("program")?, "git")?;
                constant(f.req("operation")?, "git.add-ignore")?;
                let resources = f
                    .req("resources")?
                    .as_array()
                    .ok_or(Refusal::schema("expected-array"))?;
                if resources.len() != 1 || resources[0].as_str() != Some("git-admin:stage-ignore") {
                    return Err(Refusal::schema("constant-mismatch"));
                }
                Self::GitAddIgnore {
                    max_output_bytes: int_in(f.req("max_output_bytes")?, 1, 65_536)?,
                }
            }
            "outputs.read" => Self::OutputsRead {
                roles: unique_list(f.req("roles")?, 1, 32, Role::decode)?,
                operations: unique_list(f.req("operations")?, 1, 2, OutputOperation::decode)?,
                max_bytes: positive(f.req("max_bytes")?)?,
            },
            "state" => Self::State {
                read: boolean(f.req("read")?)?,
                write: boolean(f.req("write")?)?,
                reclaim: boolean(f.req("reclaim")?)?,
                max_bytes: positive(f.req("max_bytes")?)?,
                source_content: boolean(f.req("source_content")?)?,
            },
            _ => return Err(Refusal::schema("unknown-capability")),
        };
        f.finish()?;
        Ok(capability)
    }

    /// Encodes the closed capability object.
    pub fn encode(&self) -> Json {
        match self {
            Self::FileRead { path, max_bytes } => Obj::new()
                .put("kind", Json::str("file.read"))
                .put("scope", Json::str("project"))
                .put("path", path.encode())
                .put("max_bytes", Json::uint(*max_bytes))
                .build(),
            Self::FileWrite {
                path,
                create,
                max_growth_bytes,
                max_bytes,
            } => Obj::new()
                .put("kind", Json::str("file.write"))
                .put("scope", Json::str("project"))
                .put("path", path.encode())
                .put("create", Json::Bool(*create))
                .put("max_growth_bytes", Json::uint(*max_growth_bytes))
                .put("max_bytes", Json::uint(*max_bytes))
                .build(),
            Self::GitAddIgnore { max_output_bytes } => Obj::new()
                .put("kind", Json::str("program.run"))
                .put("program", Json::str("git"))
                .put("operation", Json::str("git.add-ignore"))
                .put(
                    "resources",
                    Json::Array(vec![Json::str("git-admin:stage-ignore")]),
                )
                .put("max_output_bytes", Json::uint(*max_output_bytes))
                .build(),
            Self::OutputsRead {
                roles,
                operations,
                max_bytes,
            } => Obj::new()
                .put("kind", Json::str("outputs.read"))
                .put("roles", array(roles, Role::encode))
                .put("operations", array(operations, |op| op.encode()))
                .put("max_bytes", Json::uint(*max_bytes))
                .build(),
            Self::State {
                read,
                write,
                reclaim,
                max_bytes,
                source_content,
            } => Obj::new()
                .put("kind", Json::str("state"))
                .put("read", Json::Bool(*read))
                .put("write", Json::Bool(*write))
                .put("reclaim", Json::Bool(*reclaim))
                .put("max_bytes", Json::uint(*max_bytes))
                .put("source_content", Json::Bool(*source_content))
                .build(),
        }
    }
}

/// An inert grant handle bound to a capability and policy epoch (`grant`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// Opaque grant handle.
    pub handle: Token,
    /// Granted ceiling.
    pub capability: Capability,
    /// Policy epoch the grant belongs to.
    pub policy_epoch: u64,
}

impl Grant {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let grant = Self {
            handle: Token::decode(f.req("handle")?)?,
            capability: Capability::decode(f.req("capability")?)?,
            policy_epoch: uint(f.req("policy_epoch")?)?,
        };
        f.finish()?;
        Ok(grant)
    }

    pub(crate) fn encode(&self) -> Json {
        Obj::new()
            .put("handle", self.handle.encode())
            .put("capability", self.capability.encode())
            .put("policy_epoch", Json::uint(self.policy_epoch))
            .build()
    }
}

/// Liveness policy (`dts#/$defs/liveness`); timings are observation cadence, not runtime kills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    /// Interactive: suspicion offers continue/cancel.
    Interactive {
        /// Sampling cadence.
        sample_ms: u64,
        /// Cancellation grace.
        cancel_grace_ms: u64,
    },
    /// Unattended, failing closed on unresolved suspicion.
    UnattendedFailClosed {
        /// Sampling cadence.
        sample_ms: u64,
        /// Cancellation grace.
        cancel_grace_ms: u64,
    },
    /// Unattended with a bounded last-resort inactivity window.
    UnattendedBoundedObservation {
        /// Sampling cadence.
        sample_ms: u64,
        /// Cancellation grace.
        cancel_grace_ms: u64,
        /// Inactivity observation window.
        inactivity_ms: u64,
    },
}

impl Liveness {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let mode = string(f.req("mode")?)?;
        let sample_ms = positive(f.req("sample_ms")?)?;
        let cancel_grace_ms = positive(f.req("cancel_grace_ms")?)?;
        let liveness = match mode {
            "interactive" => Self::Interactive {
                sample_ms,
                cancel_grace_ms,
            },
            "unattended" => match string(f.req("unresolved")?)? {
                "fail-closed" => Self::UnattendedFailClosed {
                    sample_ms,
                    cancel_grace_ms,
                },
                "bounded-observation" => Self::UnattendedBoundedObservation {
                    sample_ms,
                    cancel_grace_ms,
                    inactivity_ms: positive(f.req("inactivity_ms")?)?,
                },
                _ => return Err(Refusal::schema("unknown-enum-value")),
            },
            _ => return Err(Refusal::schema("unknown-enum-value")),
        };
        f.finish()?;
        Ok(liveness)
    }

    pub(crate) fn encode(&self) -> Json {
        match *self {
            Self::Interactive {
                sample_ms,
                cancel_grace_ms,
            } => Obj::new()
                .put("mode", Json::str("interactive"))
                .put("sample_ms", Json::uint(sample_ms))
                .put("cancel_grace_ms", Json::uint(cancel_grace_ms))
                .build(),
            Self::UnattendedFailClosed {
                sample_ms,
                cancel_grace_ms,
            } => Obj::new()
                .put("mode", Json::str("unattended"))
                .put("sample_ms", Json::uint(sample_ms))
                .put("cancel_grace_ms", Json::uint(cancel_grace_ms))
                .put("unresolved", Json::str("fail-closed"))
                .build(),
            Self::UnattendedBoundedObservation {
                sample_ms,
                cancel_grace_ms,
                inactivity_ms,
            } => Obj::new()
                .put("mode", Json::str("unattended"))
                .put("sample_ms", Json::uint(sample_ms))
                .put("cancel_grace_ms", Json::uint(cancel_grace_ms))
                .put("unresolved", Json::str("bounded-observation"))
                .put("inactivity_ms", Json::uint(inactivity_ms))
                .build(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::decode::{DecodeLimits, decode_object};

    fn json(text: &str) -> Json {
        decode_object(text.as_bytes(), DecodeLimits::WIRE).expect("json")
    }

    #[test]
    fn capabilities_round_trip_and_stay_closed() {
        for text in [
            r#"{"kind":"file.read","scope":"project","path":".gitignore","max_bytes":1048576}"#,
            r#"{"kind":"file.write","scope":"project","path":".gitignore","create":false,"max_growth_bytes":4096,"max_bytes":1052672}"#,
            r#"{"kind":"program.run","program":"git","operation":"git.add-ignore","resources":["git-admin:stage-ignore"],"max_output_bytes":65536}"#,
            r#"{"kind":"outputs.read","roles":["pack-text"],"operations":["pack"],"max_bytes":3}"#,
            r#"{"kind":"state","read":true,"write":true,"reclaim":false,"max_bytes":65536,"source_content":false}"#,
        ] {
            let capability = Capability::decode(&json(text)).expect(text);
            assert_eq!(Capability::decode(&capability.encode()), Ok(capability));
        }
        let refused = [
            r#"{"kind":"network","url":"https://x"}"#,
            r#"{"kind":"file.read","scope":"home","path":".gitignore","max_bytes":1}"#,
            r#"{"kind":"file.read","scope":"project","path":"../x","max_bytes":1}"#,
            r#"{"kind":"program.run","program":"sh","operation":"git.add-ignore","resources":["git-admin:stage-ignore"],"max_output_bytes":1}"#,
            r#"{"kind":"program.run","program":"git","operation":"git.add-ignore","resources":["x"],"max_output_bytes":1}"#,
            r#"{"kind":"outputs.read","roles":["a","a"],"operations":["pack"],"max_bytes":3}"#,
            r#"{"kind":"state","read":true,"write":true,"reclaim":false,"max_bytes":0,"source_content":false}"#,
        ];
        for text in refused {
            assert!(Capability::decode(&json(text)).is_err(), "{text}");
        }
    }

    #[test]
    fn liveness_requires_a_bounded_unattended_policy() {
        for text in [
            r#"{"mode":"interactive","sample_ms":1000,"cancel_grace_ms":5000}"#,
            r#"{"mode":"unattended","sample_ms":1000,"cancel_grace_ms":5000,"unresolved":"fail-closed"}"#,
            r#"{"mode":"unattended","sample_ms":1,"cancel_grace_ms":1,"unresolved":"bounded-observation","inactivity_ms":9}"#,
        ] {
            let liveness = Liveness::decode(&json(text)).expect(text);
            assert_eq!(Liveness::decode(&liveness.encode()), Ok(liveness));
        }
        assert!(
            Liveness::decode(&json(
                r#"{"mode":"unattended","sample_ms":1,"cancel_grace_ms":1}"#
            ))
            .is_err()
        );
        let unbounded = r#"{"mode":"unattended","sample_ms":1,"cancel_grace_ms":1,"unresolved":"bounded-observation"}"#;
        assert!(Liveness::decode(&json(unbounded)).is_err());
        let extra = r#"{"mode":"interactive","sample_ms":1,"cancel_grace_ms":1,"inactivity_ms":9}"#;
        assert!(Liveness::decode(&json(extra)).is_err());
        let grant = Grant::decode(&json(r#"{"handle":"grant:read","capability":{"kind":"file.read","scope":"project","path":"a","max_bytes":1},"policy_epoch":1}"#))
            .expect("grant");
        assert_eq!(Grant::decode(&grant.encode()), Ok(grant));
    }
}
