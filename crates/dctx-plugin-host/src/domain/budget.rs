//! Quotas, wire-size arithmetic and bounded request queues (backpressure).
//! Implements Resources.tla `Receive`/`Drain` (PP7): the ceiling is the minimum of every layer, every
//! charge is checked with non-wrapping arithmetic before allocation, and chunks must fit the frame after
//! base64/JSON overhead. Over-limit senders get `ResourceLimit` and are cancelled (PLUGINS.md §3.2).
//! Depends on trace. Used by the broker and the host session pump.

use std::collections::VecDeque;

use crate::domain::trace::{ModelAction, Trace};
use crate::error::HostRefusal;

/// One accounted resource dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dimension {
    /// Cumulative delivered content bytes.
    Content,
    /// Admitted broker actions.
    Actions,
    /// Live private upload bytes.
    Upload,
    /// Retained diagnostics bytes.
    Diagnostics,
    /// Queued inbound requests.
    Queue,
}

/// A quota whose limit is the minimum of the supplied ceilings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quota {
    limit: u64,
    used: u64,
}

impl Quota {
    /// The minimum of `ceilings`; an empty list grants nothing.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(ceilings: &[u64]) -> Self {
        Self { limit: ceilings.iter().copied().min().unwrap_or(0), used: 0 }
    }

    /// Checks `used + n <= limit` without wrapping, before anything is allocated.
    /// # Errors
    /// Refuses quota overflow or a total above the configured limit.
    pub fn check(&self, n: u64) -> Result<u64, HostRefusal> {
        match self.used.checked_add(n) {
            Some(total) if total <= self.limit => Ok(total),
            _ => Err(HostRefusal::limit("quota-exceeded")),
        }
    }

    /// Charges `n` after a successful check.
    /// # Errors
    /// Refuses if any charged quota would overflow or exceed its limit; charges are validated before mutation.
    pub fn charge(&mut self, n: u64) -> Result<(), HostRefusal> {
        self.used = self.check(n)?;
        Ok(())
    }

    /// Releases `n` (never below zero).
    pub fn release(&mut self, n: u64) {
        self.used = self.used.saturating_sub(n);
    }

    /// Effective limit.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Used amount.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn used(&self) -> u64 {
        self.used
    }
}

/// The per-invocation budget, one quota per dimension.
#[derive(Clone, Debug)]
pub struct Budget {
    quotas: [(Dimension, Quota); 5],
    trace: Trace,
}

impl Budget {
    /// Builds a budget from per-dimension ceilings (each the list of layer ceilings).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(content: &[u64], actions: &[u64], upload: &[u64], diagnostics: &[u64], queue: &[u64]) -> Self {
        Self {
            quotas: [
                (Dimension::Content, Quota::new(content)),
                (Dimension::Actions, Quota::new(actions)),
                (Dimension::Upload, Quota::new(upload)),
                (Dimension::Diagnostics, Quota::new(diagnostics)),
                (Dimension::Queue, Quota::new(queue)),
            ],
            trace: Trace::default(),
        }
    }

    /// `Resources.Receive`: check every listed dimension first, then charge all or none.
    /// # Errors
    /// Refuses if any charged quota would overflow or exceed its limit; charges are validated before mutation.
    pub fn charge(&mut self, charges: &[(Dimension, u64)]) -> Result<(), HostRefusal> {
        self.trace.push(ModelAction::ResourcesReceive);
        for (dimension, n) in charges {
            self.quota(*dimension).check(*n)?;
        }
        for (dimension, n) in charges {
            self.quota_mut(*dimension).charge(*n)?;
        }
        Ok(())
    }

    /// `Resources.Drain`: release a charge.
    pub fn release(&mut self, dimension: Dimension, n: u64) {
        self.quota_mut(dimension).release(n);
        self.trace.push(ModelAction::ResourcesDrain);
    }

    /// Quota of `dimension`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn quota(&self, dimension: Dimension) -> Quota {
        self.quotas.iter().find(|(d, _)| *d == dimension).map_or(Quota::new(&[]), |(_, q)| *q)
    }

    /// Model actions taken.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn trace(&self) -> &Trace {
        &self.trace
    }

    fn quota_mut(&mut self, dimension: Dimension) -> &mut Quota {
        let index = self.quotas.iter().position(|(d, _)| *d == dimension).unwrap_or(0);
        &mut self.quotas[index].1
    }
}

/// Encoded size of `raw` bytes in canonical padded base64, without wrapping.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn base64_len(raw: u64) -> Option<u64> {
    raw.checked_add(2).map(|n| n / 3).and_then(|groups| groups.checked_mul(4))
}

/// Whether a `raw`-byte chunk fits `frame_cap` after base64 and `envelope` bytes of JSON overhead.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn chunk_fits_frame(raw: u64, envelope: u64, frame_cap: u64) -> bool {
    base64_len(raw).and_then(|n| n.checked_add(envelope)).is_some_and(|total| total <= frame_cap)
}

/// What the host does with an over-limit sender.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overflow {
    /// Refuse with `ResourceLimit` and cancel the invocation that sent it.
    CancelSender(HostRefusal),
}

/// A bounded FIFO queue of inbound requests: never grows past its capacity.
#[derive(Clone, Debug)]
pub struct BoundedQueue<T> {
    items: VecDeque<T>,
    capacity: usize,
}

impl<T> BoundedQueue<T> {
    /// An empty queue holding at most `capacity` items.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(capacity: usize) -> Self {
        Self { items: VecDeque::new(), capacity }
    }

    /// Enqueues `item`, or reports the overflow without storing it.
    /// # Errors
    /// Returns `Overflow::CancelSender` when the outstanding-request capacity is reached.
    pub fn push(&mut self, item: T) -> Result<(), Overflow> {
        if self.items.len() >= self.capacity {
            return Err(Overflow::CancelSender(HostRefusal::limit("outstanding-requests")));
        }
        self.items.push_back(item);
        Ok(())
    }

    /// Dequeues the oldest item.
    pub fn pop(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    /// Queued count.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether nothing is queued.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Drops every queued item (cancellation discards unserved work).
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_is_the_minimum_and_exact_limit_succeeds() {
        let mut quota = Quota::new(&[3, 2, 3, 3]);
        assert_eq!(quota.limit(), 2);
        quota.charge(2).expect("exact limit");
        assert!(quota.charge(1).is_err());
        assert_eq!(quota.used(), 2);
        quota.release(2);
        assert!(quota.charge(1).is_ok());
    }

    #[test]
    fn checked_sum_never_wraps() {
        let mut quota = Quota::new(&[u64::MAX]);
        quota.charge(u64::MAX - 1).expect("near max");
        assert!(quota.charge(2).is_err());
        assert_eq!(quota.used(), u64::MAX - 1);
    }

    #[test]
    fn aggregate_charge_is_all_or_nothing() {
        let mut budget = Budget::new(&[10], &[1], &[10], &[10], &[10]);
        budget.charge(&[(Dimension::Content, 5), (Dimension::Actions, 1)]).expect("first");
        assert!(budget.charge(&[(Dimension::Content, 5), (Dimension::Actions, 1)]).is_err());
        assert_eq!(budget.quota(Dimension::Content).used(), 5);
        budget.charge(&[(Dimension::Content, 5)]).expect("content alone fits");
    }

    #[test]
    fn wire_overhead_counts() {
        assert_eq!(base64_len(65_536), Some(87_384));
        assert!(!chunk_fits_frame(65_536, 200, 65_536));
        assert!(chunk_fits_frame(49_000, 200, 65_536));
        assert_eq!(base64_len(u64::MAX), None);
    }

    #[test]
    fn queue_refuses_past_capacity_without_storing() {
        let mut queue = BoundedQueue::new(2);
        queue.push(1).expect("one");
        queue.push(2).expect("two");
        assert!(matches!(queue.push(3), Err(Overflow::CancelSender(_))));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop(), Some(1));
        queue.push(3).expect("room again");
    }
}
