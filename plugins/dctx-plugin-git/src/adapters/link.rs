// SPDX-License-Identifier: Apache-2.0
//! The worker's side of one framed session: strict decode, session tracking of both directions, sending.
//! Implements wire.md §Bytes and §Bootstrap and lifecycle via dctx-plugin-protocol's `FrameReader`,
//! `write_frame` and `Session` (W-STATE, W-BINDING, W-PROGRESS). Only inherited stdio streams are used.
//! Used by `adapters::wire_broker` (broker calls) and `adapters::worker` (lifecycle loop).

use std::collections::BTreeSet;
use std::io::{Read, Write};

use dctx_plugin_protocol::records::envelope::{Outcome, PendingResult, SessionBody, SessionMessage};
use dctx_plugin_protocol::records::events::{Event, Progress, ProgressUnit};
use dctx_plugin_protocol::records::results::ResultBody;
use dctx_plugin_protocol::records::scalars::{Name, RequestId, Sender, Token};
use dctx_plugin_protocol::{
    Accepted, DecodeLimits, FrameReader, Session, StreamError, WireError, WireMessage, write_frame,
};

use crate::adapters::{WorkerError, violation};

/// One negotiated session over a reader/writer pair.
pub struct Link<R, W> {
    reader: FrameReader<R>,
    writer: W,
    session: Option<Session>,
    session_id: Option<Token>,
    next_request: u64,
    event_seq: u64,
    progress_seq: u64,
    /// The active invocation token, while an invoke runs.
    pub invocation: Option<Token>,
    /// Set when the host cancelled the active invocation.
    pub cancelled: bool,
    abandoned: BTreeSet<RequestId>,
}

impl<R: Read, W: Write> Link<R, W> {
    /// Wraps the negotiated streams; the session starts with the first session message.
    pub fn new(reader: FrameReader<R>, writer: W) -> Self {
        Self {
            reader,
            writer,
            session: None,
            session_id: None,
            next_request: 0,
            event_seq: 0,
            progress_seq: 0,
            invocation: None,
            cancelled: false,
            abandoned: BTreeSet::new(),
        }
    }

    /// Starts tracking with the host-chosen session token.
    pub fn start(&mut self, id: Token, session: Session) {
        self.session_id = Some(id);
        self.session = Some(session);
    }

    /// Whether the session has started.
    pub fn started(&self) -> bool {
        self.session.is_some()
    }

    /// Next host message, `None` at a clean end of stream. Late answers to abandoned requests are dropped.
    /// # Errors
    /// Returns stream/decode failures or a protocol refusal for bootstrap traffic after negotiation.
    pub fn recv(&mut self) -> Result<Option<SessionMessage>, WorkerError> {
        loop {
            let Some(frame) = self.reader.next_frame().map_err(stream)? else { return Ok(None) };
            match WireMessage::decode(&frame, DecodeLimits::WIRE).map_err(WorkerError::Protocol)? {
                WireMessage::Bootstrap(_) => return Err(WorkerError::Protocol(violation("bootstrap-after-hello"))),
                WireMessage::Session(message) => {
                    let abandoned = match &message.body {
                        SessionBody::Response { id, .. } => self.abandoned.remove(id),
                        _ => false,
                    };
                    if !abandoned {
                        return Ok(Some(message));
                    }
                }
            }
        }
    }

    /// Validates a host message against the session; a refusal ends the session.
    /// # Errors
    /// Refuses an unstarted session or a host message that violates session state/bindings.
    pub fn observe_host(&mut self, message: &SessionMessage) -> Result<Accepted, WorkerError> {
        let session = self.session.as_mut().ok_or(WorkerError::Protocol(violation("session-not-started")))?;
        session.observe(Sender::Host, message).map_err(WorkerError::Protocol)
    }

    /// Sends a body after the tracker accepts it as a plugin message.
    /// # Errors
    /// Returns an unstarted-session refusal, an invalid local transition, or a frame/write failure.
    pub fn send(&mut self, body: SessionBody) -> Result<(), WorkerError> {
        let (Some(session), Some(id)) = (self.session.as_mut(), self.session_id.clone()) else {
            return Err(WorkerError::Protocol(violation("session-not-started")));
        };
        let message = SessionMessage { session: id, extensions: None, body };
        session.observe(Sender::Plugin, &message).map_err(WorkerError::Internal)?;
        let cap = session.frame_cap();
        write_frame(&mut self.writer, &WireMessage::Session(message).encode(), cap).map_err(stream)
    }

    /// Sends a successful response.
    /// # Errors
    /// Propagates session-transition, framing, or write failures from sending the response.
    pub fn respond(&mut self, id: RequestId, body: &ResultBody) -> Result<(), WorkerError> {
        self.send(SessionBody::Response { id, outcome: Outcome::Result(PendingResult::from_body(body)) })
    }

    /// Sends an error response.
    /// # Errors
    /// Propagates session-transition, framing, or write failures from sending the error.
    pub fn respond_error(&mut self, id: RequestId, error: WireError) -> Result<(), WorkerError> {
        self.send(SessionBody::Response { id, outcome: Outcome::Error(Box::new(error)) })
    }

    /// A fresh plugin request ID.
    /// # Errors
    /// Returns an internal refusal if the generated request ID is invalid.
    pub fn next_request_id(&mut self) -> Result<RequestId, WorkerError> {
        self.next_request += 1;
        RequestId::new(&format!("p:{}", self.next_request)).map_err(WorkerError::Internal)
    }

    /// Marks a request whose answer will be ignored (after cancellation).
    pub fn abandon(&mut self, id: RequestId) {
        self.abandoned.insert(id);
    }

    /// Sends an event with the next envelope sequence.
    /// # Errors
    /// Propagates session-transition, framing, or write failures from sending the event.
    pub fn event(&mut self, event: Event) -> Result<(), WorkerError> {
        self.event_seq += 1;
        let seq = self.event_seq;
        self.send(SessionBody::Event { seq, event })
    }

    /// Resets per-invocation state for a new invoke.
    pub fn begin(&mut self, invocation: Token) {
        self.invocation = Some(invocation);
        self.cancelled = false;
        self.progress_seq = 0;
    }

    /// Last progress payload sequence sent for this invocation.
    pub fn progress_seq(&self) -> u64 {
        self.progress_seq
    }

    /// Best-effort progress sample; dropped after cancellation or if the tracker would refuse it.
    pub fn progress(&mut self, phase: &'static str, completed: u64, total: Option<u64>, unit: ProgressUnit) {
        let (Some(invocation), false, Ok(phase)) = (self.invocation.clone(), self.cancelled, Name::new(phase)) else {
            return;
        };
        let seq = self.progress_seq + 1;
        let progress = Progress { invocation, seq, phase, completed, total, unit, wait: None };
        if self.event(Event::Progress(progress)).is_ok() {
            self.progress_seq = seq;
        }
    }
}

/// Maps a stream failure to a worker error.
pub(crate) fn stream(error: StreamError) -> WorkerError {
    match error {
        StreamError::Io(kind) => WorkerError::Io(kind),
        StreamError::Protocol(refusal) => WorkerError::Protocol(refusal),
    }
}
