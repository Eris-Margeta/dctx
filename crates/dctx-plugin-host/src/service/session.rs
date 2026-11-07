// SPDX-License-Identifier: Apache-2.0
//! Host session: one worker's broker, supervisor and bounded inbound request queue.
//! Implements the PLUGINS.md §3.2 backpressure rule (over-limit senders get `ResourceLimit` and are
//! cancelled) and PP8 "no admission after cancel": cancellation halts authority, revokes handles and
//! discards queued work before the process is reaped. Depends on broker, supervisor and budget.
//! Used by the facade (PL-F) once the wire adapter (PL-A) supplies decoded requests.

use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::RequestId;
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::domain::budget::{BoundedQueue, Overflow};
use crate::domain::lifecycle::StopReason;
use crate::domain::liveness::Verdict;
use crate::error::HostRefusal;
use crate::ports::{AuditPort, FilePort, ProcessPort, RecipePort};
use crate::service::broker::{Broker, Reply};
use crate::service::supervisor::Supervisor;

/// One supervised, brokered worker session.
#[derive(Debug)]
pub struct HostSession<A, F, R, P> {
    broker: Broker<A, F, R>,
    supervisor: Supervisor<P>,
    queue: BoundedQueue<(RequestId, Request)>,
}

impl<A: AuditPort, F: FilePort, R: RecipePort, P: ProcessPort> HostSession<A, F, R, P> {
    /// Composes a session; `outstanding` is the negotiated outstanding-request ceiling (≤ 16).
    pub fn new(broker: Broker<A, F, R>, supervisor: Supervisor<P>, outstanding: usize) -> Self {
        Self {
            broker,
            supervisor,
            queue: BoundedQueue::new(outstanding),
        }
    }

    /// Accepts a worker request into the bounded queue; overflow cancels the sender.
    /// # Errors
    /// Refuses closed admission or exhausted queue/tracked-request capacity; resource exhaustion cancels the session.
    pub fn receive(
        &mut self,
        id: RequestId,
        request: Request,
        now: u64,
    ) -> Result<(), HostRefusal> {
        if !self.supervisor.admits() {
            return Err(HostRefusal::new(
                ErrorCode::Cancelled,
                Phase::Broker,
                "admission-closed",
            ));
        }
        if let Err(Overflow::CancelSender(refusal)) = self.queue.push((id.clone(), request)) {
            self.cancel(StopReason::ResourceLimit, now);
            return Err(refusal);
        }
        if let Err(refusal) = self.supervisor.broker_started(id) {
            self.cancel(StopReason::ResourceLimit, now);
            return Err(refusal);
        }
        Ok(())
    }

    /// Serves the oldest queued request, if any.
    pub fn serve_next(&mut self, now: u64) -> Option<(RequestId, Result<Reply, HostRefusal>)> {
        let (id, request) = self.queue.pop()?;
        let result = if self.supervisor.admits() {
            self.broker.handle(&request)
        } else {
            Err(HostRefusal::new(
                ErrorCode::Cancelled,
                Phase::Broker,
                "admission-closed",
            ))
        };
        if result.is_ok() {
            self.supervisor.broker_completed(&id, now);
        }
        Some((id, result))
    }

    /// Samples liveness; a cancel verdict halts the broker immediately.
    pub fn tick(&mut self, now: u64) -> Verdict {
        let verdict = self.supervisor.tick(now);
        if let Verdict::Cancel(_) = verdict {
            self.close();
        }
        verdict
    }

    /// A probe reply arrived.
    pub fn probe_reply(
        &mut self,
        wait: Option<&dctx_plugin_protocol::records::common::Wait>,
        now: u64,
    ) -> Verdict {
        let verdict = self.supervisor.probe_reply(wait, now);
        if let Verdict::Cancel(_) = verdict {
            self.close();
        }
        verdict
    }

    /// User/host cancellation: close admission, revoke handles, drop queued work, start grace.
    pub fn cancel(&mut self, reason: StopReason, now: u64) {
        let _ = self.supervisor.cancel(reason, now);
        self.close();
    }

    /// Queued request count.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// The broker.
    pub fn broker(&self) -> &Broker<A, F, R> {
        &self.broker
    }

    /// The broker, mutably.
    pub fn broker_mut(&mut self) -> &mut Broker<A, F, R> {
        &mut self.broker
    }

    /// The supervisor.
    pub fn supervisor(&self) -> &Supervisor<P> {
        &self.supervisor
    }

    /// The supervisor, mutably.
    pub fn supervisor_mut(&mut self) -> &mut Supervisor<P> {
        &mut self.supervisor
    }

    fn close(&mut self) {
        self.broker.halt();
        self.queue.clear();
    }
}
