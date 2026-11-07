// SPDX-License-Identifier: Apache-2.0
//! The broker port: every effect the plugin needs, as host requests (never direct filesystem or process).
//! Implements PLUGINS.md §3.3 for the four git-profile methods (`file.read`, `file.stage`, `file.replace`,
//! `program.run` git.add-ignore) plus progress events. Depends on dctx-plugin-protocol records only.
//! Implemented by `adapters::wire_broker` (framed stdio) and by the tests' fake broker.

use dctx_plugin_protocol::WireError;
use dctx_plugin_protocol::records::broker::{ChunkRead, FileReplace, FileStage, GitAddIgnore};
use dctx_plugin_protocol::records::events::ProgressUnit;
use dctx_plugin_protocol::records::results::{ChunkResult, ProgramStatus};
use dctx_plugin_protocol::records::scalars::Token;

/// Why a broker request did not produce a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrokerFailure {
    /// The host answered with a typed error (its effect state is in the error).
    Refused(Box<WireError>),
    /// The host cancelled the invocation while this request was outstanding.
    Cancelled,
    /// The link broke or the answer violated the protocol; a pending effect's outcome is unknown.
    Protocol(&'static str),
}

/// A `file.stage` result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uploaded {
    /// Host upload ID.
    pub upload: Token,
    /// Next expected offset.
    pub next_offset: u64,
    /// Whether the upload is sealed.
    pub complete: bool,
}

/// A `file.replace` result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replaced {
    /// New resource precondition.
    pub precondition: Token,
    /// Host receipt.
    pub receipt: Token,
}

/// A `program.run` result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramRan {
    /// Exit classification.
    pub status: ProgramStatus,
    /// Exit code.
    pub exit_code: u32,
    /// Host receipt.
    pub receipt: Token,
}

/// Host-brokered effects available to the git plugin.
pub trait Broker {
    /// `file.read`: one bounded chunk of a granted file.
    /// # Errors
    /// Returns a host refusal, cancellation, or link/protocol failure while reading the granted chunk.
    fn file_read(&mut self, read: &ChunkRead) -> Result<ChunkResult, BrokerFailure>;
    /// `file.stage`: one upload chunk in host-private storage.
    /// # Errors
    /// Returns a host refusal, cancellation, or link/protocol failure while staging the upload.
    fn file_stage(&mut self, stage: &FileStage) -> Result<Uploaded, BrokerFailure>;
    /// `file.replace`: atomic compare-and-replace from a sealed upload.
    /// # Errors
    /// Returns a host refusal, cancellation, or link/protocol failure; uncertain effects must not be retried automatically.
    fn file_replace(&mut self, replace: &FileReplace) -> Result<Replaced, BrokerFailure>;
    /// `program.run` with the fixed `git.add-ignore` recipe.
    /// # Errors
    /// Returns a host refusal, cancellation, or link/protocol failure; uncertain effects must not be retried automatically.
    fn program_run(&mut self, run: &GitAddIgnore) -> Result<ProgramRan, BrokerFailure>;
    /// A progress sample for the active invocation (best effort; a broken link fails the next request).
    fn progress(
        &mut self,
        phase: &'static str,
        completed: u64,
        total: Option<u64>,
        unit: ProgressUnit,
    );
}
