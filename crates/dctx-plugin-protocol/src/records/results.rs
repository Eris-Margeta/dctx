//! Successful result records selected by the pending request's method (never by the union alone).
//! Implements results.schema.json and wire.md §Bootstrap "the pending request's method selects the exact
//! result schema" (W-RESULT). Depends on json, common, invocation, bytes, method; used by envelope/session.

use crate::json::fields::{Fields, Obj, array, boolean, constant, constant_true, int_in, list, uint, unique_list};
use crate::json::value::Json;
use crate::records::bytes::Chunk;
use crate::records::common::{CommandRecord, Diagnostic, Wait};
use crate::records::invocation::{CommittedOutput, PlannedOutput};
use crate::records::method::Method;
use crate::records::scalars::{Name, Token};
use crate::refusal::Refusal;

wire_enum!(
    /// Terminal status of a successful invoke.
    InvokeStatus {
        /// Completed.
        Ok => "ok",
        /// Skipped with a declared reason.
        Skipped => "skipped",
        /// Completed with a warning.
        Warning => "warning",
    }
);

wire_enum!(
    /// Worker state reported by a probe answer.
    ProbeState {
        /// Running.
        Running => "running",
        /// Waiting on a dependency.
        Waiting => "waiting",
        /// Cancelling.
        Cancelling => "cancelling",
    }
);

wire_enum!(
    /// Outcome of the brokered program.
    ProgramStatus {
        /// Exit success.
        Succeeded => "succeeded",
        /// Exit failure.
        Failed => "failed",
    }
);

/// A delivered byte chunk (`output.read`, `file.read`, `state.read` results).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkResult {
    /// Delivered bytes.
    pub bytes: Chunk,
    /// Offset of the first byte.
    pub offset: u64,
    /// Total resource size.
    pub size: u64,
    /// Whether `offset + len == size`.
    pub eof: bool,
    /// Precondition token (absent for `output.read`).
    pub precondition: Option<Token>,
    /// Host receipt.
    pub receipt: Token,
}

/// An output listed by `output.list`: planned or committed, per the active hook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListedOutput {
    /// Planned output metadata.
    Planned(PlannedOutput),
    /// Committed output metadata.
    Committed(CommittedOutput),
}

/// A typed successful result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultBody {
    /// `initialize` → `ready`.
    Ready,
    /// `invoke`.
    Invoke {
        /// Terminal status.
        status: InvokeStatus,
        /// Declared reason code.
        reason: Name,
        /// Receipt references (checked for ownership, never proof).
        receipts: Vec<Token>,
        /// Diagnostics (at most 64).
        diagnostics: Vec<Diagnostic>,
        /// Command output; empty for ordinary hooks.
        command_output: Vec<CommandRecord>,
    },
    /// `probe`.
    Probe {
        /// Echoed invocation.
        invocation: Token,
        /// Echoed challenge.
        challenge: Token,
        /// Last progress sequence.
        last_progress_seq: u64,
        /// Worker state.
        state: ProbeState,
        /// Pending dependency.
        wait: Option<Wait>,
    },
    /// `cancel` → accepted.
    CancelAccepted,
    /// `shutdown` → closing.
    Closing,
    /// `output.read`, `file.read` or `state.read`.
    Chunk(ChunkResult),
    /// `output.list`.
    OutputList {
        /// Page entries (at most 64).
        outputs: Vec<ListedOutput>,
        /// Next host cursor; absent at the end.
        next: Option<Token>,
    },
    /// `file.stage`.
    FileStage {
        /// Upload ID.
        upload: Token,
        /// Next expected offset.
        next_offset: u64,
        /// Whether the upload is sealed.
        complete: bool,
        /// Host receipt.
        receipt: Token,
    },
    /// `file.replace` or `state.write`: new precondition and receipt.
    Replaced {
        /// New precondition token.
        precondition: Token,
        /// Host receipt.
        receipt: Token,
    },
    /// `program.run`.
    ProgramRun {
        /// Program status.
        status: ProgramStatus,
        /// Exit code.
        exit_code: u32,
        /// Diagnostics (at most 64).
        diagnostics: Vec<Diagnostic>,
        /// Host receipt.
        receipt: Token,
    },
    /// `state.reclaim`.
    StateReclaim {
        /// Reclaimed bytes.
        reclaimed_bytes: u64,
        /// Host receipt.
        receipt: Token,
    },
}

impl ResultBody {
    /// Decodes `value` with the exact result schema of `method`.
    /// # Errors
    /// Refuses a value that violates the selected method's result schema, bounds, or semantic rules.
    pub fn decode(method: Method, value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let body = match method {
            Method::Initialize => {
                constant(f.req("status")?, "ready")?;
                Self::Ready
            }
            Method::Invoke => Self::Invoke {
                status: InvokeStatus::decode(f.req("status")?)?,
                reason: Name::decode(f.req("reason")?)?,
                receipts: unique_list(f.req("receipts")?, 0, 64, Token::decode)?,
                diagnostics: list(f.req("diagnostics")?, 0, 64, Diagnostic::decode)?,
                command_output: list(f.req("command_output")?, 0, 64, CommandRecord::decode)?,
            },
            Method::Probe => Self::Probe {
                invocation: Token::decode(f.req("invocation")?)?,
                challenge: Token::decode(f.req("challenge")?)?,
                last_progress_seq: uint(f.req("last_progress_seq")?)?,
                state: ProbeState::decode(f.req("state")?)?,
                wait: f.opt("wait").map(Wait::decode).transpose()?,
            },
            Method::Cancel => {
                constant_true(f.req("accepted")?)?;
                Self::CancelAccepted
            }
            Method::Shutdown => {
                constant(f.req("status")?, "closing")?;
                Self::Closing
            }
            Method::OutputRead | Method::FileRead | Method::StateRead => Self::Chunk(ChunkResult {
                bytes: Chunk::decode(f.req("bytes")?)?,
                offset: uint(f.req("offset")?)?,
                size: uint(f.req("size")?)?,
                eof: boolean(f.req("eof")?)?,
                precondition: if method == Method::OutputRead {
                    None
                } else {
                    Some(Token::decode(f.req("precondition")?)?)
                },
                receipt: Token::decode(f.req("receipt")?)?,
            }),
            Method::OutputList => Self::OutputList {
                outputs: list(f.req("outputs")?, 0, 64, |item| {
                    if Fields::of(item)?.has("location") {
                        PlannedOutput::decode(item).map(ListedOutput::Planned)
                    } else {
                        CommittedOutput::decode(item).map(ListedOutput::Committed)
                    }
                })?,
                next: f.opt("next").map(Token::decode).transpose()?,
            },
            Method::FileStage => Self::FileStage {
                upload: Token::decode(f.req("upload")?)?,
                next_offset: uint(f.req("next_offset")?)?,
                complete: boolean(f.req("complete")?)?,
                receipt: Token::decode(f.req("receipt")?)?,
            },
            Method::FileReplace | Method::StateWrite => Self::Replaced {
                precondition: Token::decode(f.req("precondition")?)?,
                receipt: Token::decode(f.req("receipt")?)?,
            },
            Method::ProgramRun => Self::ProgramRun {
                status: ProgramStatus::decode(f.req("status")?)?,
                exit_code: int_in(f.req("exit_code")?, 0, u64::from(u32::MAX))
                    .map(|code| u32::try_from(code).unwrap_or(u32::MAX))?,
                diagnostics: list(f.req("diagnostics")?, 0, 64, Diagnostic::decode)?,
                receipt: Token::decode(f.req("receipt")?)?,
            },
            Method::StateReclaim => Self::StateReclaim {
                reclaimed_bytes: uint(f.req("reclaimed_bytes")?)?,
                receipt: Token::decode(f.req("receipt")?)?,
            },
        };
        f.finish()?;
        Ok(body)
    }

    /// Whether `value` matches at least one result shape (the generic `anyOf` union).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn matches_any(value: &Json) -> bool {
        Method::ALL.iter().any(|method| Self::decode(*method, value).is_ok())
    }

    /// Encodes the closed result object.
    pub fn encode(&self) -> Json {
        match self {
            Self::Ready => Obj::new().put("status", Json::str("ready")).build(),
            Self::Invoke { status, reason, receipts, diagnostics, command_output } => Obj::new()
                .put("status", status.encode())
                .put("reason", reason.encode())
                .put("receipts", array(receipts, Token::encode))
                .put("diagnostics", array(diagnostics, Diagnostic::encode))
                .put("command_output", array(command_output, CommandRecord::encode))
                .build(),
            Self::Probe { invocation, challenge, last_progress_seq, state, wait } => Obj::new()
                .put("invocation", invocation.encode())
                .put("challenge", challenge.encode())
                .put("last_progress_seq", Json::uint(*last_progress_seq))
                .put("state", state.encode())
                .maybe("wait", wait.as_ref().map(Wait::encode))
                .build(),
            Self::CancelAccepted => Obj::new().put("accepted", Json::Bool(true)).build(),
            Self::Closing => Obj::new().put("status", Json::str("closing")).build(),
            Self::Chunk(chunk) => Obj::new()
                .put("bytes", chunk.bytes.encode())
                .put("offset", Json::uint(chunk.offset))
                .put("size", Json::uint(chunk.size))
                .put("eof", Json::Bool(chunk.eof))
                .maybe("precondition", chunk.precondition.as_ref().map(Token::encode))
                .put("receipt", chunk.receipt.encode())
                .build(),
            Self::OutputList { outputs, next } => Obj::new()
                .put(
                    "outputs",
                    array(outputs, |output| match output {
                        ListedOutput::Planned(planned) => planned.encode(),
                        ListedOutput::Committed(committed) => committed.encode(),
                    }),
                )
                .maybe("next", next.as_ref().map(Token::encode))
                .build(),
            Self::FileStage { upload, next_offset, complete, receipt } => Obj::new()
                .put("upload", upload.encode())
                .put("next_offset", Json::uint(*next_offset))
                .put("complete", Json::Bool(*complete))
                .put("receipt", receipt.encode())
                .build(),
            Self::Replaced { precondition, receipt } => {
                Obj::new().put("precondition", precondition.encode()).put("receipt", receipt.encode()).build()
            }
            Self::ProgramRun { status, exit_code, diagnostics, receipt } => Obj::new()
                .put("status", status.encode())
                .put("exit_code", Json::uint(u64::from(*exit_code)))
                .put("diagnostics", array(diagnostics, Diagnostic::encode))
                .put("receipt", receipt.encode())
                .build(),
            Self::StateReclaim { reclaimed_bytes, receipt } => Obj::new()
                .put("reclaimed_bytes", Json::uint(*reclaimed_bytes))
                .put("receipt", receipt.encode())
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
    fn pending_method_selects_the_exact_shape() {
        let ready = json(r#"{"status":"ready"}"#);
        assert_eq!(ResultBody::decode(Method::Initialize, &ready), Ok(ResultBody::Ready));
        assert!(ResultBody::decode(Method::FileRead, &ready).is_err());
        assert!(ResultBody::decode(Method::Shutdown, &ready).is_err());
        assert!(ResultBody::matches_any(&ready));
        assert!(!ResultBody::matches_any(&json(r#"{"status":"done"}"#)));
        let replaced = json(r#"{"precondition":"pre:2","receipt":"r"}"#);
        assert!(ResultBody::decode(Method::FileReplace, &replaced).is_ok());
        assert!(ResultBody::decode(Method::StateWrite, &replaced).is_ok());
        let read = json(r#"{"bytes":"YWJj","offset":0,"size":3,"eof":true,"receipt":"r"}"#);
        assert!(ResultBody::decode(Method::OutputRead, &read).is_ok());
        assert!(ResultBody::decode(Method::FileRead, &read).is_err());
    }

    #[test]
    fn listed_outputs_are_planned_xor_committed() {
        let page = json(r#"{"outputs":[{"handle":"a","role":"pack-text","bytes":3},{"handle":"b","role":"x","location":"stdout"}],"next":"c:2"}"#);
        let body = ResultBody::decode(Method::OutputList, &page).expect("valid");
        assert_eq!(ResultBody::decode(Method::OutputList, &body.encode()), Ok(body));
        let neither = json(r#"{"outputs":[{"handle":"a","role":"x"}]}"#);
        assert!(ResultBody::decode(Method::OutputList, &neither).is_err());
        let program = json(r#"{"status":"failed","exit_code":4294967295,"diagnostics":[],"receipt":"r"}"#);
        let body = ResultBody::decode(Method::ProgramRun, &program).expect("valid");
        assert_eq!(ResultBody::decode(Method::ProgramRun, &body.encode()), Ok(body));
    }
}
