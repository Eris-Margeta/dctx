// SPDX-License-Identifier: Apache-2.0
//! Worker supervision: pre-exec admission, spawn, liveness sampling, cancellation, force and reap.
//! Implements Lifecycle.tla over the process port (PP4, PP8): admission and spawn follow the decision 080
//! disposition; liveness verdicts come from the progress feed; cancellation grace is the only timed
//! escalation and starts after cancel; reap refusal stays visible. Depends on domain and ports.
//! Used by `HostSession`.

use dctx_plugin_protocol::records::capability::Liveness;
use dctx_plugin_protocol::records::common::Wait;
use dctx_plugin_protocol::records::events::Progress;
use dctx_plugin_protocol::records::scalars::{RequestId, Token};
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::domain::lifecycle::{LaunchFacts, StopReason, Worker, WorkerPhase};
use crate::domain::liveness::{LivenessFeed, Verdict};
use crate::error::HostRefusal;
use crate::ports::{ProcessId, ProcessPort, SpawnFault};

/// Supervises one worker over the process port `P`.
#[derive(Debug)]
pub struct Supervisor<P> {
    worker: Worker,
    feed: Option<LivenessFeed>,
    policy: Liveness,
    process: P,
    pid: Option<ProcessId>,
    cancel_at: Option<u64>,
}

impl<P: ProcessPort> Supervisor<P> {
    /// A supervisor for an installed worker with host-established `facts`.
    pub fn new(facts: LaunchFacts, policy: Liveness, process: P) -> Self {
        Self {
            worker: Worker::new(facts),
            feed: None,
            policy,
            process,
            pid: None,
            cancel_at: None,
        }
    }

    /// `EnableOrRefuse` then `Start`: no exec unless verified, compatible and admitted by disposition.
    /// # Errors
    /// Refuses inadmissible launch facts, unavailable confinement, spawn failure, or an invalid lifecycle transition.
    pub fn launch(&mut self) -> Result<(), HostRefusal> {
        self.worker.enable()?;
        match self.process.spawn() {
            Ok(pid) => {
                self.pid = Some(pid);
                self.worker.started()
            }
            Err(SpawnFault::UnsupportedPlatform) => Err(HostRefusal::new(
                ErrorCode::UnsupportedPlatform,
                Phase::Confine,
                "no-confinement-provider",
            )),
            Err(SpawnFault::Failed) => Err(HostRefusal::new(
                ErrorCode::PluginExited,
                Phase::Confine,
                "spawn-failed",
            )),
        }
    }

    /// `Negotiate`.
    /// # Errors
    /// Refuses unless the worker has spawned.
    pub fn negotiated(&mut self) -> Result<(), HostRefusal> {
        self.worker.negotiated()
    }

    /// `Initialize`.
    /// # Errors
    /// Refuses unless protocol negotiation has completed.
    pub fn initialized(&mut self) -> Result<(), HostRefusal> {
        self.worker.initialized()
    }

    /// `Invoke` at monotonic `now`; starts a fresh liveness feed.
    /// # Errors
    /// Refuses an active invocation or any phase other than ready.
    pub fn invoke(&mut self, now: u64) -> Result<(), HostRefusal> {
        self.worker.invoke()?;
        self.feed = Some(LivenessFeed::new(self.policy, now));
        Ok(())
    }

    /// `Finish`: terminal response received.
    /// # Errors
    /// Refuses unless an invocation is running.
    pub fn finish(&mut self) -> Result<(), HostRefusal> {
        self.worker.finish()?;
        self.feed = None;
        Ok(())
    }

    /// A progress event for the active invocation.
    /// # Errors
    /// Refuses a missing active liveness feed or invalid progress/wait information.
    pub fn progress(&mut self, sample: &Progress, now: u64) -> Result<bool, HostRefusal> {
        self.feed_mut()?.progress(sample, now)
    }

    /// A broker request became outstanding.
    /// # Errors
    /// Refuses a missing active liveness feed or exhausted tracked-request capacity.
    pub fn broker_started(&mut self, id: RequestId) -> Result<(), HostRefusal> {
        self.feed_mut()?.broker_started(id)
    }

    /// A broker request completed with an effect or delivery.
    pub fn broker_completed(&mut self, id: &RequestId, now: u64) {
        if let Some(feed) = self.feed.as_mut() {
            feed.broker_completed(id, now);
        }
    }

    /// Registers a host-known dependency.
    /// # Errors
    /// Refuses a missing active liveness feed or exhausted tracked-dependency capacity.
    pub fn register_dependency(&mut self, dependency: Token) -> Result<(), HostRefusal> {
        self.feed_mut()?.register_dependency(dependency)
    }

    /// Settles a dependency as resolved or dead.
    pub fn settle_dependency(&mut self, dependency: &Token, alive: bool, now: u64) {
        if let Some(feed) = self.feed.as_mut() {
            feed.settle_dependency(dependency, alive, now);
        }
    }

    /// A probe reply arrived.
    pub fn probe_reply(&mut self, wait: Option<&Wait>, now: u64) -> Verdict {
        let verdict = self
            .feed
            .as_mut()
            .map_or(Verdict::Continue, |feed| feed.probe_reply(wait, now));
        self.apply(verdict, now)
    }

    /// Samples liveness while running, escalates after cancellation grace and reaps exited trees.
    pub fn tick(&mut self, now: u64) -> Verdict {
        match self.worker.phase() {
            WorkerPhase::Running => {
                let evidence = match self.pid {
                    Some(pid) => self.process.evidence(pid),
                    None => crate::domain::liveness::ProcessEvidence::Unknown,
                };
                let verdict = self
                    .feed
                    .as_mut()
                    .map_or(Verdict::Continue, |feed| feed.sample(now, evidence));
                self.apply(verdict, now)
            }
            WorkerPhase::Cancelling(_) => {
                let since = self.cancel_at.map_or(0, |at| now.saturating_sub(at));
                if since >= self.cancel_grace() {
                    if let Some(pid) = self.pid {
                        self.process.force(pid);
                    }
                    let _ = self.worker.exited();
                    self.reap();
                }
                Verdict::Continue
            }
            WorkerPhase::Exited(_) => {
                self.reap();
                Verdict::Continue
            }
            _ => Verdict::Continue,
        }
    }

    /// `Halt`/`CancelStalled`: close admission now and start the cancellation grace.
    /// # Errors
    /// Refuses lifecycle phases in which cancellation is not admitted.
    pub fn cancel(&mut self, reason: StopReason, now: u64) -> Result<(), HostRefusal> {
        self.worker.cancel(reason)?;
        self.cancel_at.get_or_insert(now);
        Ok(())
    }

    /// The worker acknowledged cancellation (`cancelled` event) and exited.
    /// # Errors
    /// Refuses an inadmissible worker-exit transition.
    pub fn acknowledged(&mut self) -> Result<(), HostRefusal> {
        self.worker.exited()?;
        self.reap();
        Ok(())
    }

    /// Whether broker admission is open.
    pub fn admits(&self) -> bool {
        self.worker.admits()
    }

    /// The lifecycle.
    pub fn worker(&self) -> &Worker {
        &self.worker
    }

    /// The liveness feed of the active invocation.
    pub fn feed(&self) -> Option<&LivenessFeed> {
        self.feed.as_ref()
    }

    /// Process port.
    pub fn process(&self) -> &P {
        &self.process
    }

    /// Process port, mutably (fault injection).
    pub fn process_mut(&mut self) -> &mut P {
        &mut self.process
    }

    fn cancel_grace(&self) -> u64 {
        match self.policy {
            Liveness::Interactive {
                cancel_grace_ms, ..
            }
            | Liveness::UnattendedFailClosed {
                cancel_grace_ms, ..
            }
            | Liveness::UnattendedBoundedObservation {
                cancel_grace_ms, ..
            } => cancel_grace_ms,
        }
    }

    fn apply(&mut self, verdict: Verdict, now: u64) -> Verdict {
        if let Verdict::Cancel(reason) = verdict {
            let _ = self.cancel(reason, now);
        }
        verdict
    }

    fn reap(&mut self) {
        let reaped = self.pid.is_some_and(|pid| self.process.reap(pid));
        let _ = self.worker.reap(reaped);
    }

    fn feed_mut(&mut self) -> Result<&mut LivenessFeed, HostRefusal> {
        self.feed.as_mut().ok_or(HostRefusal::new(
            ErrorCode::InvalidMessage,
            Phase::Invoke,
            "no-active-invocation",
        ))
    }
}
