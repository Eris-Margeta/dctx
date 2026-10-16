//! The decision 077 liveness feed: progress evidence, not wall-clock deadlines.
//! Implements PLUGINS.md §3.4 and Lifecycle.tla `Advance`, `ResolveDependency`, `Observe`, `Probe`,
//! `Classify`, `ObserveWait` and `ElapsedObservation` (PP8). Useful progress is an advancing counter, a new
//! phase, a completed broker action or a resolved dependency; heartbeats and repeats are not. Time only
//! sets the sampling cadence and the caller's optional inactivity window. Depends on protocol records.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use dctx_plugin_protocol::records::capability::Liveness;
use dctx_plugin_protocol::records::common::Wait;
use dctx_plugin_protocol::records::events::{Progress, ProgressUnit};
use dctx_plugin_protocol::records::scalars::{Name, RequestId, Token};
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::domain::lifecycle::StopReason;
use crate::domain::trace::{ModelAction, Trace};
use crate::error::HostRefusal;

/// Most pending broker requests, dependencies or distinct phases tracked.
pub const MAX_TRACKED: usize = 64;

/// OS-level process evidence from the process port; unknown is never treated as zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessEvidence {
    /// Alive; CPU/IO delta when the OS reports it.
    Alive {
        /// CPU/IO activity delta, if known.
        activity: Option<u64>,
    },
    /// Exited or pipes broke.
    Exited,
    /// The OS cannot tell.
    Unknown,
}

/// Observation state (`running -> suspect -> probe -> progressing|waiting|stalled`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Observation {
    /// Useful progress observed recently.
    Running,
    /// No useful progress since the last sample.
    Suspect,
    /// A probe is outstanding.
    Probing,
    /// Responsive and waiting on a live dependency or broker request.
    Waiting(Wait),
    /// Evidence shows no progress.
    Stalled,
}

/// What the supervisor should do after an observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Keep going.
    Continue,
    /// Send a `probe`.
    SendProbe,
    /// Interactive: offer continue/cancel to the user with the last known activity.
    AskUser,
    /// Cancel, then reap.
    Cancel(StopReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dependency {
    Pending,
    Resolved,
    Dead,
}

/// Per-invocation liveness feed.
#[derive(Clone, Debug)]
pub struct LivenessFeed {
    policy: Liveness,
    last_seq: u64,
    phases: BTreeMap<Name, (u64, Option<u64>, ProgressUnit)>,
    recent: VecDeque<Progress>,
    dependencies: BTreeMap<Token, Dependency>,
    broker: BTreeSet<RequestId>,
    state: Observation,
    useful_since_sample: bool,
    last_useful_at: u64,
    last_sample_at: u64,
    evidence: ProcessEvidence,
    trace: Trace,
}

impl LivenessFeed {
    /// A feed for an invocation admitted at `now` under the caller's `policy`.
    pub fn new(policy: Liveness, now: u64) -> Self {
        Self {
            policy,
            last_seq: 0,
            phases: BTreeMap::new(),
            recent: VecDeque::with_capacity(2),
            dependencies: BTreeMap::new(),
            broker: BTreeSet::new(),
            state: Observation::Running,
            useful_since_sample: false,
            last_useful_at: now,
            last_sample_at: now,
            evidence: ProcessEvidence::Unknown,
            trace: Trace::default(),
        }
    }

    /// Accepts a validated progress sample; returns whether it was useful progress.
    pub fn progress(&mut self, sample: &Progress, now: u64) -> Result<bool, HostRefusal> {
        if let Some(wait) = &sample.wait
            && !self.wait_known(wait)
        {
            return Err(HostRefusal::new(ErrorCode::InvalidMessage, Phase::Invoke, "unknown-wait"));
        }
        let useful = self.accept(sample)?;
        if self.recent.len() == 2 {
            self.recent.pop_front();
        }
        self.recent.push_back(sample.clone());
        if useful {
            self.useful(now);
        }
        Ok(useful)
    }

    /// A plugin broker request is outstanding.
    pub fn broker_started(&mut self, id: RequestId) -> Result<(), HostRefusal> {
        if self.broker.len() >= MAX_TRACKED {
            return Err(HostRefusal::limit("too-many-tracked-requests"));
        }
        self.broker.insert(id);
        Ok(())
    }

    /// A broker action completed: useful progress.
    pub fn broker_completed(&mut self, id: &RequestId, now: u64) {
        if self.broker.remove(id) {
            self.useful(now);
        }
    }

    /// Registers a host-known live dependency.
    pub fn register_dependency(&mut self, dependency: Token) -> Result<(), HostRefusal> {
        if self.dependencies.len() >= MAX_TRACKED {
            return Err(HostRefusal::limit("too-many-dependencies"));
        }
        self.dependencies.insert(dependency, Dependency::Pending);
        Ok(())
    }

    /// `ResolveDependency`: resolved is useful progress; dead is stall evidence.
    pub fn settle_dependency(&mut self, dependency: &Token, alive: bool, now: u64) {
        if let Some(state) = self.dependencies.get_mut(dependency) {
            *state = if alive { Dependency::Resolved } else { Dependency::Dead };
            self.trace.push(ModelAction::LifecycleResolveDependency);
            if alive {
                self.useful(now);
            }
        }
    }

    /// Periodic sample at the configured cadence; `now` is from the injected monotonic clock.
    pub fn sample(&mut self, now: u64, evidence: ProcessEvidence) -> Verdict {
        if now.saturating_sub(self.last_sample_at) < self.cadence() {
            return Verdict::Continue;
        }
        self.last_sample_at = now;
        self.evidence = evidence;
        self.trace.push(ModelAction::LifecycleElapsedObservation);
        if evidence == ProcessEvidence::Exited {
            self.state = Observation::Stalled;
            return self.decide(now);
        }
        if std::mem::take(&mut self.useful_since_sample) {
            self.state = Observation::Running;
            return Verdict::Continue;
        }
        match self.state.clone() {
            Observation::Running | Observation::Suspect => {
                self.trace.push(ModelAction::LifecycleObserve);
                self.trace.push(ModelAction::LifecycleProbe);
                self.state = Observation::Probing;
                Verdict::SendProbe
            }
            Observation::Waiting(wait) if self.wait_live(&wait) => Verdict::Continue,
            Observation::Waiting(_) => {
                self.trace.push(ModelAction::LifecycleObserveWait);
                self.trace.push(ModelAction::LifecycleProbe);
                self.state = Observation::Probing;
                Verdict::SendProbe
            }
            Observation::Probing => {
                self.trace.push(ModelAction::LifecycleClassify);
                self.state = Observation::Stalled;
                self.decide(now)
            }
            Observation::Stalled => self.decide(now),
        }
    }

    /// `Classify` on a probe reply: responsiveness alone never counts as progress.
    pub fn probe_reply(&mut self, wait: Option<&Wait>, now: u64) -> Verdict {
        self.trace.push(ModelAction::LifecycleClassify);
        if std::mem::take(&mut self.useful_since_sample) {
            self.state = Observation::Running;
            return Verdict::Continue;
        }
        match wait {
            Some(wait) if self.wait_live(wait) => {
                self.state = Observation::Waiting(wait.clone());
                Verdict::Continue
            }
            _ => {
                self.state = Observation::Stalled;
                self.decide(now)
            }
        }
    }

    /// Current observation.
    pub fn observation(&self) -> &Observation {
        &self.state
    }

    /// The last two sanitized progress samples.
    pub fn recent(&self) -> impl Iterator<Item = &Progress> {
        self.recent.iter()
    }

    /// Model actions taken.
    pub fn trace(&self) -> &Trace {
        &self.trace
    }

    /// W-PROGRESS sequencing; useful when the phase is new or its completed count advanced.
    fn accept(&mut self, sample: &Progress) -> Result<bool, HostRefusal> {
        let refuse = |reason| Err(HostRefusal::new(ErrorCode::InvalidMessage, Phase::Invoke, reason));
        if sample.seq <= self.last_seq {
            return refuse("progress-sequence");
        }
        if sample.total.is_some_and(|total| sample.completed > total) {
            return refuse("completed-exceeds-total");
        }
        let useful = match self.phases.get(&sample.phase) {
            Some((completed, total, unit)) => {
                if *total != sample.total || *unit != sample.unit {
                    return refuse("unstable-phase-unit");
                }
                if sample.completed < *completed {
                    return refuse("progress-regression");
                }
                sample.completed > *completed
            }
            None if self.phases.len() >= MAX_TRACKED => {
                return Err(HostRefusal::new(ErrorCode::ResourceLimit, Phase::Invoke, "too-many-phases"));
            }
            None => true,
        };
        self.last_seq = sample.seq;
        self.phases.insert(sample.phase.clone(), (sample.completed, sample.total, sample.unit));
        Ok(useful)
    }

    fn useful(&mut self, now: u64) {
        self.useful_since_sample = true;
        self.last_useful_at = now;
        self.state = Observation::Running;
        self.trace.push(ModelAction::LifecycleAdvance);
    }

    fn decide(&self, now: u64) -> Verdict {
        let reason = if self.evidence == ProcessEvidence::Unknown {
            StopReason::UnresolvedSuspicion
        } else {
            StopReason::Stalled
        };
        let dead = self.evidence == ProcessEvidence::Exited;
        match self.policy {
            Liveness::Interactive { .. } if !dead => Verdict::AskUser,
            Liveness::UnattendedBoundedObservation { inactivity_ms, .. }
                if !dead && now.saturating_sub(self.last_useful_at) < inactivity_ms =>
            {
                Verdict::Continue
            }
            _ => Verdict::Cancel(if dead { StopReason::Stalled } else { reason }),
        }
    }

    fn cadence(&self) -> u64 {
        match self.policy {
            Liveness::Interactive { sample_ms, .. }
            | Liveness::UnattendedFailClosed { sample_ms, .. }
            | Liveness::UnattendedBoundedObservation { sample_ms, .. } => sample_ms,
        }
    }

    fn wait_known(&self, wait: &Wait) -> bool {
        match wait {
            Wait::BrokerRequest(id) => self.broker.contains(id),
            Wait::Dependency(token) => self.dependencies.contains_key(token),
        }
    }

    fn wait_live(&self, wait: &Wait) -> bool {
        match wait {
            Wait::BrokerRequest(id) => self.broker.contains(id),
            Wait::Dependency(token) => self.dependencies.get(token) == Some(&Dependency::Pending),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(seq: u64, phase: &str, completed: u64) -> Progress {
        Progress {
            invocation: Token::new("op:1").expect("op"),
            seq,
            phase: Name::new(phase).expect("phase"),
            completed,
            total: None,
            unit: ProgressUnit::Items,
            wait: None,
        }
    }

    const FAIL_CLOSED: Liveness = Liveness::UnattendedFailClosed { sample_ms: 10, cancel_grace_ms: 10 };

    #[test]
    fn advancing_counter_is_useful_repeat_is_not() {
        let mut feed = LivenessFeed::new(FAIL_CLOSED, 0);
        assert_eq!(feed.progress(&sample(1, "scan", 1), 0), Ok(true));
        assert_eq!(feed.progress(&sample(2, "scan", 1), 1), Ok(false));
        assert_eq!(feed.progress(&sample(3, "scan", 2), 2), Ok(true));
        assert_eq!(feed.progress(&sample(4, "write", 0), 3), Ok(true));
        assert_eq!(feed.progress(&sample(5, "scan", 2), 4), Ok(false));
        assert!(feed.progress(&sample(5, "scan", 3), 5).is_err());
        assert_eq!(feed.recent().count(), 2);
    }

    #[test]
    fn cadence_is_not_a_deadline() {
        let mut feed = LivenessFeed::new(FAIL_CLOSED, 0);
        assert_eq!(feed.sample(5, ProcessEvidence::Alive { activity: None }), Verdict::Continue);
        for step in 1..100u64 {
            feed.progress(&sample(step, "scan", step), step * 1_000_000).expect("progress");
            let verdict = feed.sample(step * 1_000_000 + 1, ProcessEvidence::Alive { activity: Some(1) });
            assert_eq!(verdict, Verdict::Continue);
        }
    }

    #[test]
    fn unknown_wait_reference_refuses() {
        let mut feed = LivenessFeed::new(FAIL_CLOSED, 0);
        let mut waiting = sample(1, "scan", 0);
        waiting.wait = Some(Wait::Dependency(Token::new("dep:1").expect("dep")));
        assert!(feed.progress(&waiting, 0).is_err());
        feed.register_dependency(Token::new("dep:1").expect("dep")).expect("registered");
        assert_eq!(feed.progress(&waiting, 0), Ok(true));
    }
}
