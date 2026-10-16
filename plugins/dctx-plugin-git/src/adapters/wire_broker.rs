//! The broker port over the framed session: send a `p:` request, then drain host traffic until its answer.
//! Implements wire.md "A worker may request broker actions while host awaits invoke; both sides drain
//! concurrently" for a single-threaded worker: probes are answered (waiting on the request), a cancel is
//! accepted and ends the wait (the request is abandoned). Depends on adapters::link and ports.
//! Used by adapters::worker during one invoke.

use std::io::{Read, Write};

use dctx_plugin_protocol::Accepted;
use dctx_plugin_protocol::records::broker::{ChunkRead, FileReplace, FileStage, GitAddIgnore};
use dctx_plugin_protocol::records::common::Wait;
use dctx_plugin_protocol::records::envelope::SessionBody;
use dctx_plugin_protocol::records::events::ProgressUnit;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::results::{ChunkResult, ProbeState, ResultBody};
use dctx_plugin_protocol::records::scalars::RequestId;

use crate::adapters::WorkerError;
use crate::adapters::link::Link;
use crate::ports::{Broker, BrokerFailure, ProgramRan, Replaced, Uploaded};

/// A broker bound to the live link for one invoke. A fatal link error is kept for the worker.
pub struct WireBroker<'a, R, W> {
    link: &'a mut Link<R, W>,
    /// The first fatal error seen; the worker ends the session with it.
    pub fatal: Option<WorkerError>,
}

impl<'a, R: Read, W: Write> WireBroker<'a, R, W> {
    /// Borrows the link for one invoke.
    pub fn new(link: &'a mut Link<R, W>) -> Self {
        Self { link, fatal: None }
    }

    fn fail(&mut self, error: WorkerError) -> BrokerFailure {
        let reason = error.reason();
        self.fatal.get_or_insert(error);
        BrokerFailure::Protocol(reason)
    }

    fn call(&mut self, request: Request) -> Result<ResultBody, BrokerFailure> {
        if self.link.cancelled {
            return Err(BrokerFailure::Cancelled);
        }
        if self.fatal.is_some() {
            return Err(BrokerFailure::Protocol("link-failed"));
        }
        let id = self.link.next_request_id().map_err(|error| self.fail(error))?;
        self.link.send(SessionBody::Request { id: id.clone(), request }).map_err(|error| self.fail(error))?;
        self.wait(&id)
    }

    fn wait(&mut self, id: &RequestId) -> Result<ResultBody, BrokerFailure> {
        loop {
            let message = match self.link.recv() {
                Ok(Some(message)) => message,
                Ok(None) => return Err(self.fail(WorkerError::Closed)),
                Err(error) => return Err(self.fail(error)),
            };
            let accepted = self.link.observe_host(&message).map_err(|error| self.fail(error))?;
            match (&message.body, accepted) {
                (SessionBody::Response { id: answered, .. }, Accepted::Result { body, .. }) if answered == id => {
                    return Ok(body);
                }
                (SessionBody::Response { id: answered, .. }, Accepted::Error { error, .. }) if answered == id => {
                    return Err(BrokerFailure::Refused(error));
                }
                (SessionBody::Request { id: host, request: Request::Probe(probe) }, _) => {
                    let body = ResultBody::Probe {
                        invocation: probe.invocation.clone(),
                        challenge: probe.challenge.clone(),
                        last_progress_seq: self.link.progress_seq(),
                        state: ProbeState::Waiting,
                        wait: Some(Wait::BrokerRequest(id.clone())),
                    };
                    self.link.respond(host.clone(), &body).map_err(|error| self.fail(error))?;
                }
                (SessionBody::Request { id: host, request: Request::Cancel { .. } }, _) => {
                    self.link.cancelled = true;
                    self.link.abandon(id.clone());
                    self.link.respond(host.clone(), &ResultBody::CancelAccepted).map_err(|error| self.fail(error))?;
                    return Err(BrokerFailure::Cancelled);
                }
                _ => return Err(self.fail(WorkerError::Protocol(crate::adapters::violation("unexpected-host-message")))),
            }
        }
    }
}

fn unexpected() -> BrokerFailure {
    BrokerFailure::Protocol("wrong-result-type")
}

impl<R: Read, W: Write> Broker for WireBroker<'_, R, W> {
    fn file_read(&mut self, read: &ChunkRead) -> Result<ChunkResult, BrokerFailure> {
        match self.call(Request::FileRead(read.clone()))? {
            ResultBody::Chunk(chunk) => Ok(chunk),
            _ => Err(unexpected()),
        }
    }

    fn file_stage(&mut self, stage: &FileStage) -> Result<Uploaded, BrokerFailure> {
        match self.call(Request::FileStage(stage.clone()))? {
            ResultBody::FileStage { upload, next_offset, complete, .. } => Ok(Uploaded { upload, next_offset, complete }),
            _ => Err(unexpected()),
        }
    }

    fn file_replace(&mut self, replace: &FileReplace) -> Result<Replaced, BrokerFailure> {
        match self.call(Request::FileReplace(replace.clone()))? {
            ResultBody::Replaced { precondition, receipt } => Ok(Replaced { precondition, receipt }),
            _ => Err(unexpected()),
        }
    }

    fn program_run(&mut self, run: &GitAddIgnore) -> Result<ProgramRan, BrokerFailure> {
        match self.call(Request::ProgramRun(run.clone()))? {
            ResultBody::ProgramRun { status, exit_code, receipt, .. } => Ok(ProgramRan { status, exit_code, receipt }),
            _ => Err(unexpected()),
        }
    }

    fn progress(&mut self, phase: &'static str, completed: u64, total: Option<u64>, unit: ProgressUnit) {
        if self.fatal.is_none() {
            self.link.progress(phase, completed, total, unit);
        }
    }
}
