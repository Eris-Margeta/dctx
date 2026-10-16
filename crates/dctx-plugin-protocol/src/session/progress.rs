//! Progress sequencing for one invocation: payload sequence, per-phase monotone counts and stable units.
//! Implements wire.md §Bootstrap progress rules (W-PROGRESS): seq starts at 1 and strictly increases,
//! completed is monotone and <= total within a phase, unit/total stable per phase, bounded phase table.
//! Depends on records::events and refusal; used by session::Session. Wait references are checked there.

use std::collections::BTreeMap;

use crate::records::events::{Progress, ProgressUnit};
use crate::records::failure::ErrorCode;
use crate::records::scalars::Name;
use crate::refusal::{Predicate, Refusal, Stage};

/// Most distinct phases tracked per invocation.
pub const MAX_PHASES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhaseSample {
    completed: u64,
    total: Option<u64>,
    unit: ProgressUnit,
}

/// Last accepted progress facts for the active invocation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgressTrack {
    last_seq: u64,
    phases: BTreeMap<Name, PhaseSample>,
}

fn refused(reason: &'static str) -> Refusal {
    Refusal::session(Predicate::WProgress, reason)
}

impl ProgressTrack {
    /// Last accepted payload sequence (0 before the first sample).
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    /// Checks `progress` against earlier samples and records it when accepted.
    pub fn accept(&mut self, progress: &Progress) -> Result<(), Refusal> {
        if progress.seq <= self.last_seq {
            return Err(refused("progress-sequence"));
        }
        if progress.total.is_some_and(|total| progress.completed > total) {
            return Err(refused("completed-exceeds-total"));
        }
        let sample = PhaseSample { completed: progress.completed, total: progress.total, unit: progress.unit };
        match self.phases.get(&progress.phase) {
            Some(previous) => {
                if previous.unit != sample.unit || previous.total != sample.total {
                    return Err(refused("unstable-phase-unit"));
                }
                if sample.completed < previous.completed {
                    return Err(refused("progress-regression"));
                }
            }
            None if self.phases.len() >= MAX_PHASES => {
                return Err(Refusal::new(ErrorCode::ResourceLimit, Stage::Session, Predicate::WLimits, "too-many-phases"));
            }
            None => {}
        }
        self.phases.insert(progress.phase.clone(), sample);
        self.last_seq = progress.seq;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::scalars::Token;

    fn sample(seq: u64, phase: &str, completed: u64, total: Option<u64>) -> Progress {
        Progress {
            invocation: Token::new("op:1").expect("token"),
            seq,
            phase: Name::new(phase).expect("name"),
            completed,
            total,
            unit: ProgressUnit::Bytes,
            wait: None,
        }
    }

    #[test]
    fn monotone_within_phase_with_positive_controls() {
        let mut track = ProgressTrack::default();
        track.accept(&sample(1, "read-ignore", 3, Some(3))).expect("first");
        assert_eq!(track.accept(&sample(2, "read-ignore", 1, Some(3))).unwrap_err().reason, "progress-regression");
        track.accept(&sample(2, "read-ignore", 3, Some(3))).expect("equal is not regression");
        assert_eq!(track.accept(&sample(2, "stage", 0, None)).unwrap_err().reason, "progress-sequence");
        track.accept(&sample(5, "stage", 0, None)).expect("gaps allowed, new phase");
        assert_eq!(track.accept(&sample(6, "stage", 1, Some(9))).unwrap_err().reason, "unstable-phase-unit");
        assert_eq!(track.accept(&sample(7, "other", 4, Some(3))).unwrap_err().reason, "completed-exceeds-total");
        assert_eq!(track.last_seq(), 5);
    }

    #[test]
    fn phase_table_is_bounded() {
        let mut track = ProgressTrack::default();
        for index in 0..MAX_PHASES {
            track.accept(&sample(index as u64 + 1, &format!("p{index}"), 0, None)).expect("bounded");
        }
        let refusal = track.accept(&sample(1000, "overflow", 0, None)).unwrap_err();
        assert_eq!(refusal.code, ErrorCode::ResourceLimit);
    }
}
