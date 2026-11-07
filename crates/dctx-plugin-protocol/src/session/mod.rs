// SPDX-License-Identifier: Apache-2.0
//! Pure session tracker: validates every post-hello message against lifecycle state and pending requests.
//! Implements wire.md §Bootstrap and lifecycle (W-STATE, W-BINDING, W-RESULT, W-PROGRESS, W-LIMITS):
//! sender-owned unique IDs, one active invoke, exactly-once out-of-order responses, method-selected results,
//! strictly increasing event sequences and negotiated extensions. A refusal leaves the state unchanged.
//! Depends on records, negotiate and refusal; used by host and worker adapters (which own the IO).

mod checks;
mod progress;

use std::collections::{BTreeMap, BTreeSet};

pub use checks::{check_chunk, check_result};
pub use progress::{MAX_PHASES, ProgressTrack};

use crate::negotiate::Negotiated;
use crate::records::common::{HookName, Limits, Wait};
use crate::records::envelope::{Outcome, SessionBody, SessionMessage};
use crate::records::events::Event;
use crate::records::failure::{ErrorCode, WireError};
use crate::records::method::Method;
use crate::records::requests::Request;
use crate::records::results::ResultBody;
use crate::records::scalars::{RequestId, Sender, Token};
use crate::refusal::{Predicate, Refusal, Stage};

/// Lifecycle state after negotiation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// Hello accepted; waiting for initialize.
    Negotiated,
    /// Initialize sent; waiting for ready.
    Initializing,
    /// Ready for one invoke.
    Ready,
    /// One invocation active; broker requests admitted.
    Active,
    /// Cancel sent; broker requests refuse.
    Cancelling,
    /// Shutdown sent.
    Closing,
    /// Shutdown answered.
    Closed,
    /// Initialization failed.
    Failed,
}

/// What an accepted message means for the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Accepted {
    /// A request was admitted to the pending table.
    Request(Method),
    /// A pending request completed successfully.
    Result {
        /// Method of the completed request.
        method: Method,
        /// Typed result.
        body: ResultBody,
    },
    /// A pending request completed with an error.
    Error {
        /// Method of the completed request.
        method: Method,
        /// Typed error.
        error: Box<WireError>,
    },
    /// An event was accepted.
    Event,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveInvocation {
    invocation: Token,
    cancelled_event: bool,
    progress: ProgressTrack,
}

/// Session tracker for one host/worker session.
#[derive(Clone, Debug)]
pub struct Session {
    id: Token,
    negotiated: Negotiated,
    state: SessionState,
    limits: Limits,
    hooks: Vec<HookName>,
    seen: BTreeSet<RequestId>,
    pending: BTreeMap<RequestId, Request>,
    active: Option<ActiveInvocation>,
    last_event_seq: u64,
    dependencies: BTreeSet<Token>,
}

fn binding(reason: &'static str) -> Refusal {
    Refusal::session(Predicate::WBinding, reason)
}

fn state_refusal(code: ErrorCode, reason: &'static str) -> Refusal {
    Refusal::new(code, Stage::Session, Predicate::WState, reason)
}

impl Session {
    /// Starts a session with host-created `id` after a successful negotiation.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new(id: Token, negotiated: Negotiated) -> Self {
        Self {
            id,
            limits: negotiated.limits,
            hooks: negotiated.hooks.clone(),
            negotiated,
            state: SessionState::Negotiated,
            seen: BTreeSet::new(),
            pending: BTreeMap::new(),
            active: None,
            last_event_seq: 0,
            dependencies: BTreeSet::new(),
        }
    }

    /// Current state.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Frame cap now in force (lowered by initialize, never raised).
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn frame_cap(&self) -> u32 {
        u32::try_from(self.limits.frame_bytes).unwrap_or(crate::frame::MAX_FRAME_CAP)
    }

    /// The active invocation token, if any.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn active_invocation(&self) -> Option<&Token> {
        self.active.as_ref().map(|active| &active.invocation)
    }

    /// Number of outstanding requests sent by `sender`.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn outstanding(&self, sender: Sender) -> usize {
        self.pending
            .keys()
            .filter(|id| id.sender() == sender)
            .count()
    }

    /// Registers a live host dependency a progress wait may name.
    pub fn register_dependency(&mut self, dependency: Token) {
        self.dependencies.insert(dependency);
    }

    /// Validates one message sent by `sender` and applies its transition.
    /// # Errors
    /// Refuses ended sessions, wrong bindings/directions, unnegotiated extensions, or invalid request/response transitions.
    pub fn observe(
        &mut self,
        sender: Sender,
        message: &SessionMessage,
    ) -> Result<Accepted, Refusal> {
        if matches!(self.state, SessionState::Closed | SessionState::Failed) {
            return Err(state_refusal(ErrorCode::InvalidMessage, "session-ended"));
        }
        if message.session != self.id {
            return Err(binding("wrong-session"));
        }
        let negotiated = &self.negotiated.extensions;
        if message
            .extensions
            .as_ref()
            .is_some_and(|ext| ext.keys().any(|name| !negotiated.contains(name)))
        {
            return Err(Refusal::session(
                Predicate::WNegotiate,
                "unnegotiated-extension",
            ));
        }
        match &message.body {
            SessionBody::Request { id, request } => self.request(sender, id, request),
            SessionBody::Response { id, outcome } => self.response(sender, id, outcome),
            SessionBody::Event { seq, event } => self.event(sender, *seq, event),
        }
    }

    fn request(
        &mut self,
        sender: Sender,
        id: &RequestId,
        request: &Request,
    ) -> Result<Accepted, Refusal> {
        let method = request.method();
        if id.sender() != sender {
            return Err(binding("wrong-id-direction"));
        }
        if method.sender() != sender {
            return Err(binding("wrong-direction"));
        }
        if self.seen.contains(id) {
            return Err(binding("duplicate-request-id"));
        }
        let next = self.admit(request)?;
        if let Request::Initialize(init) = request {
            self.limits = init.limits;
            self.hooks.clone_from(&init.hooks);
        }
        if let Request::Invoke(invocation) = request {
            let invocation = invocation.invocation.clone();
            self.active = Some(ActiveInvocation {
                invocation,
                cancelled_event: false,
                progress: ProgressTrack::default(),
            });
        }
        self.state = next;
        self.seen.insert(id.clone());
        self.pending.insert(id.clone(), request.clone());
        Ok(Accepted::Request(method))
    }

    fn matches_active(&self, invocation: &Token) -> Result<(), Refusal> {
        if self.active_invocation() == Some(invocation) {
            Ok(())
        } else {
            Err(binding("unknown-invocation"))
        }
    }

    /// Checks state permission for `request` and returns the state after admission.
    fn admit(&self, request: &Request) -> Result<SessionState, Refusal> {
        use SessionState as S;
        let wrong = || state_refusal(ErrorCode::InvalidMessage, "wrong-state");
        match (request, self.state) {
            (Request::Initialize(init), S::Negotiated) => {
                if init.identity != self.negotiated.identity {
                    return Err(Refusal::new(
                        ErrorCode::IntegrityFailure,
                        Stage::Session,
                        Predicate::WNegotiate,
                        "identity-mismatch",
                    ));
                }
                if init.limits.frame_bytes > u64::from(self.negotiated.frame_cap) {
                    return Err(Refusal::session(Predicate::WNegotiate, "frame-cap-raised"));
                }
                if init
                    .hooks
                    .iter()
                    .any(|hook| !self.negotiated.hooks.contains(hook))
                {
                    return Err(Refusal::session(Predicate::WHooks, "undeclared-hook"));
                }
                Ok(S::Initializing)
            }
            (Request::Invoke(invocation), S::Ready) => match invocation.hook() {
                Some(hook) if !self.hooks.contains(&hook) => {
                    Err(Refusal::session(Predicate::WHooks, "undeclared-hook"))
                }
                _ => Ok(S::Active),
            },
            (Request::Invoke(_), S::Active | S::Cancelling) => {
                Err(state_refusal(ErrorCode::Busy, "invocation-active"))
            }
            (Request::Probe(probe), S::Active | S::Cancelling) => {
                self.matches_active(&probe.invocation).map(|()| self.state)
            }
            (Request::Cancel { invocation, .. }, S::Active) => {
                self.matches_active(invocation).map(|()| S::Cancelling)
            }
            (Request::Shutdown { .. }, S::Ready | S::Cancelling) => Ok(S::Closing),
            (broker, S::Active) if broker.method().is_broker() => {
                if !self
                    .negotiated
                    .apis
                    .iter()
                    .any(|api| api.as_str() == broker.method().as_str())
                {
                    return Err(Refusal::new(
                        ErrorCode::UnsupportedCapability,
                        Stage::Session,
                        Predicate::WNegotiate,
                        "api-not-negotiated",
                    ));
                }
                if let Request::OutputList(list) = broker {
                    self.matches_active(&list.invocation)?;
                }
                let limit = usize::try_from(self.limits.outstanding_requests).unwrap_or(usize::MAX);
                if self.outstanding(Sender::Plugin) >= limit {
                    return Err(Refusal::new(
                        ErrorCode::ResourceLimit,
                        Stage::Session,
                        Predicate::WLimits,
                        "outstanding-requests",
                    ));
                }
                Ok(S::Active)
            }
            (broker, S::Cancelling) if broker.method().is_broker() => {
                Err(state_refusal(ErrorCode::Cancelled, "request-after-cancel"))
            }
            _ => Err(wrong()),
        }
    }

    fn response(
        &mut self,
        sender: Sender,
        id: &RequestId,
        outcome: &Outcome,
    ) -> Result<Accepted, Refusal> {
        if id.sender() == sender {
            return Err(binding("wrong-id-direction"));
        }
        let Some(request) = self.pending.get(id) else {
            return Err(binding(if self.seen.contains(id) {
                "duplicate-response"
            } else {
                "unknown-response"
            }));
        };
        let method = request.method();
        let accepted = match outcome {
            Outcome::Result(result) => {
                let body = result
                    .resolve(method)
                    .map_err(|_| Refusal::session(Predicate::WResult, "wrong-result-type"))?;
                check_result(request, &body)?;
                Accepted::Result { method, body }
            }
            Outcome::Error(error) => Accepted::Error {
                method,
                error: error.clone(),
            },
        };
        self.pending.remove(id);
        let failed = matches!(accepted, Accepted::Error { .. });
        match method {
            Method::Initialize => {
                self.state = if failed {
                    SessionState::Failed
                } else {
                    SessionState::Ready
                }
            }
            Method::Invoke => {
                self.state = SessionState::Ready;
                self.active = None;
                self.pending
                    .retain(|pending, _| pending.sender() == Sender::Host);
            }
            Method::Shutdown => self.state = SessionState::Closed,
            _ => {}
        }
        Ok(accepted)
    }

    fn event(&mut self, sender: Sender, seq: u64, event: &Event) -> Result<Accepted, Refusal> {
        if sender != Sender::Plugin {
            return Err(binding("host-event"));
        }
        if seq <= self.last_event_seq {
            return Err(Refusal::session(Predicate::WProgress, "event-sequence"));
        }
        let expected = match event {
            Event::Progress(_) => SessionState::Active,
            Event::Cancelled { .. } => SessionState::Cancelling,
        };
        if self.state != expected {
            return Err(state_refusal(
                ErrorCode::InvalidMessage,
                "unsolicited-event",
            ));
        }
        self.matches_active(event.invocation())?;
        let known_wait = |wait: &Wait| match wait {
            Wait::BrokerRequest(id) => {
                id.sender() == Sender::Plugin && self.pending.contains_key(id)
            }
            Wait::Dependency(token) => self.dependencies.contains(token),
        };
        let mut active = self.active.clone().ok_or(binding("unknown-invocation"))?;
        match event {
            Event::Progress(progress) => {
                if progress.wait.as_ref().is_some_and(|wait| !known_wait(wait)) {
                    return Err(Refusal::session(Predicate::WProgress, "unknown-wait"));
                }
                active.progress.accept(progress)?;
            }
            Event::Cancelled { .. } => {
                if active.cancelled_event {
                    return Err(state_refusal(
                        ErrorCode::InvalidMessage,
                        "duplicate-cancelled",
                    ));
                }
                active.cancelled_event = true;
            }
        }
        self.active = Some(active);
        self.last_event_seq = seq;
        Ok(Accepted::Event)
    }
}
