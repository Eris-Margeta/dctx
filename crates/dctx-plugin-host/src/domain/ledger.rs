//! Receipt ledger: durable intent, execution, completion, uncertainty, replay and retention.
//! Implements Receipts.tla `Request`, `Execute`, `Complete`, `CrashOrUncertain`, `Repeat`, `Reconcile`
//! and `Rotate` (PP3, PP5). Pure state; the broker appends audit records before calling each step.
//! Depends on binding and trace. Used by the broker service and hook reports (PP10 truth).

use std::collections::BTreeMap;

use dctx_plugin_protocol::records::scalars::Token;

use crate::domain::binding::{ActionKey, ArgDigest};
use crate::domain::trace::{ModelAction, Trace};
use crate::error::HostRefusal;

/// Status of one ledger entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryStatus {
    /// Intent is durable; the effect has not run.
    Pending,
    /// The effect ran; completion is not yet durable.
    Executed,
    /// Completion is durable.
    Committed,
    /// The outcome is unknown (crash or completion-log failure).
    Unknown,
    /// Part of the effect happened.
    Partial,
}

/// One retained ledger entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Argument digest bound at intent.
    pub args: ArgDigest,
    /// Current status.
    pub status: EntryStatus,
    /// Host receipt ID.
    pub receipt: Token,
    /// Number of external executions (never above one).
    pub executions: u32,
    /// Whether the completed evidence was rotated into an archive segment.
    pub archived: bool,
}

/// Result of looking a key up before admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// No entry: a new intent may be allocated.
    Absent,
    /// Same key and arguments already committed: return this receipt, run nothing.
    Existing(Token),
}

/// Bounded ledger of host-bound action keys.
#[derive(Clone, Debug)]
pub struct Ledger {
    entries: BTreeMap<ActionKey, Entry>,
    capacity: usize,
    next: u64,
    trace: Trace,
}

impl Ledger {
    /// An empty ledger retaining at most `capacity` keys.
    pub fn new(capacity: usize) -> Self {
        Self { entries: BTreeMap::new(), capacity, next: 1, trace: Trace::default() }
    }

    /// `Receipts.Repeat` guard: existing receipt, conflict, or uncertain refusal; never a re-execution.
    pub fn lookup(&mut self, key: &ActionKey, args: &ArgDigest) -> Result<Lookup, HostRefusal> {
        let Some(entry) = self.entries.get(key) else { return Ok(Lookup::Absent) };
        self.trace.push(ModelAction::ReceiptsRepeat);
        if entry.args != *args {
            return Err(HostRefusal::conflict("action-id-reused-with-different-arguments"));
        }
        match entry.status {
            EntryStatus::Committed => Ok(Lookup::Existing(entry.receipt.clone())),
            EntryStatus::Pending | EntryStatus::Executed => Err(HostRefusal::conflict("action-in-flight")),
            EntryStatus::Unknown | EntryStatus::Partial => {
                Err(HostRefusal::unknown(entry.receipt.clone(), "uncertain-effect-needs-reconcile"))
            }
        }
    }

    /// Allocates the next receipt ID for an intent the caller is about to log durably.
    pub fn reserve(&mut self) -> Result<Token, HostRefusal> {
        if self.entries.len() >= self.capacity {
            return Err(HostRefusal::limit("ledger-full"));
        }
        let receipt = Token::new(&format!("receipt:{}", self.next)).map_err(|_| HostRefusal::limit("receipt-id"))?;
        self.next = self.next.saturating_add(1);
        Ok(receipt)
    }

    /// `Receipts.Request`: record the durable intent under `receipt`.
    pub fn intent(&mut self, key: ActionKey, args: ArgDigest, receipt: Token) -> Result<(), HostRefusal> {
        if self.entries.contains_key(&key) {
            return Err(HostRefusal::conflict("intent-exists"));
        }
        self.entries.insert(key, Entry { args, status: EntryStatus::Pending, receipt, executions: 0, archived: false });
        self.trace.push(ModelAction::ReceiptsRequest);
        Ok(())
    }

    /// `Receipts.Execute`: the external effect ran exactly once.
    pub fn executed(&mut self, key: &ActionKey) -> Result<(), HostRefusal> {
        let entry = self.entry(key, EntryStatus::Pending)?;
        entry.status = EntryStatus::Executed;
        entry.executions = entry.executions.saturating_add(1);
        self.trace.push(ModelAction::ReceiptsExecute);
        Ok(())
    }

    /// `Receipts.Complete`: completion is durable.
    pub fn complete(&mut self, key: &ActionKey) -> Result<Token, HostRefusal> {
        let entry = self.entry(key, EntryStatus::Executed)?;
        entry.status = EntryStatus::Committed;
        let receipt = entry.receipt.clone();
        self.trace.push(ModelAction::ReceiptsComplete);
        Ok(receipt)
    }

    /// Resolves a pending intent whose effect provably never ran (precondition refusal logged).
    pub fn abandon(&mut self, key: &ActionKey) -> Result<(), HostRefusal> {
        self.entry(key, EntryStatus::Pending)?;
        self.entries.remove(key);
        Ok(())
    }

    /// `Receipts.CrashOrUncertain`: the outcome is unknown or partial; the intent stays pinned.
    pub fn uncertain(&mut self, key: &ActionKey, partial: bool) -> Result<Token, HostRefusal> {
        let entry = self.entries.get_mut(key).ok_or(HostRefusal::conflict("unknown-action"))?;
        if !matches!(entry.status, EntryStatus::Pending | EntryStatus::Executed) {
            return Err(HostRefusal::conflict("not-in-flight"));
        }
        entry.status = if partial { EntryStatus::Partial } else { EntryStatus::Unknown };
        let receipt = entry.receipt.clone();
        self.trace.push(ModelAction::ReceiptsCrashOrUncertain);
        Ok(receipt)
    }

    /// `Receipts.Reconcile`: trusted evidence shows the single execution committed.
    pub fn reconcile(&mut self, key: &ActionKey) -> Result<Token, HostRefusal> {
        let entry = self.entries.get_mut(key).ok_or(HostRefusal::conflict("unknown-action"))?;
        if !matches!(entry.status, EntryStatus::Unknown | EntryStatus::Partial) || entry.executions != 1 {
            return Err(HostRefusal::conflict("nothing-to-reconcile"));
        }
        entry.status = EntryStatus::Committed;
        let receipt = entry.receipt.clone();
        self.trace.push(ModelAction::ReceiptsReconcile);
        Ok(receipt)
    }

    /// `Receipts.Rotate`: archive committed evidence; unresolved intents are pinned and refuse.
    pub fn rotate(&mut self, key: &ActionKey) -> Result<(), HostRefusal> {
        let entry = self.entries.get_mut(key).ok_or(HostRefusal::conflict("unknown-action"))?;
        if entry.status != EntryStatus::Committed {
            return Err(HostRefusal::conflict("unresolved-intent-pinned"));
        }
        entry.archived = true;
        self.trace.push(ModelAction::ReceiptsRotate);
        Ok(())
    }

    /// Records `Receipts.AuditFailure` observed by the broker.
    pub fn audit_failed(&mut self) {
        self.trace.push(ModelAction::ReceiptsAuditFailure);
    }

    /// Entry for `key`, if retained.
    pub fn get(&self, key: &ActionKey) -> Option<&Entry> {
        self.entries.get(key)
    }

    /// Committed receipts for `invocation` (the source of truth for reports).
    pub fn committed(&self, invocation: &Token) -> Vec<Token> {
        self.entries
            .iter()
            .filter(|(key, entry)| key.invocation == *invocation && entry.status == EntryStatus::Committed)
            .map(|(_, entry)| entry.receipt.clone())
            .collect()
    }

    /// Model actions taken.
    pub fn trace(&self) -> &Trace {
        &self.trace
    }

    fn entry(&mut self, key: &ActionKey, expected: EntryStatus) -> Result<&mut Entry, HostRefusal> {
        match self.entries.get_mut(key) {
            Some(entry) if entry.status == expected => Ok(entry),
            Some(_) => Err(HostRefusal::conflict("wrong-ledger-state")),
            None => Err(HostRefusal::conflict("unknown-action")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::binding::tests::context;
    use dctx_plugin_protocol::EffectState;

    fn key(invocation: &str) -> ActionKey {
        ActionKey::bind(&context(invocation), &Token::new("a:1").expect("action"))
    }

    fn commit(ledger: &mut Ledger, key: &ActionKey, args: ArgDigest) -> Token {
        let receipt = ledger.reserve().expect("receipt");
        ledger.intent(key.clone(), args, receipt).expect("intent");
        ledger.executed(key).expect("executed");
        ledger.complete(key).expect("complete")
    }

    #[test]
    fn identical_replay_returns_existing_receipt_and_different_args_conflict() {
        let mut ledger = Ledger::new(4);
        let (a, b) = (ArgDigest::of(b"a").expect("a"), ArgDigest::of(b"b").expect("b"));
        assert_eq!(ledger.lookup(&key("op:1"), &a), Ok(Lookup::Absent));
        let receipt = commit(&mut ledger, &key("op:1"), a);
        assert_eq!(ledger.lookup(&key("op:1"), &a), Ok(Lookup::Existing(receipt)));
        assert_eq!(ledger.lookup(&key("op:1"), &b).map_err(|e| e.code), Err(dctx_plugin_protocol::ErrorCode::Conflict));
        assert_eq!(ledger.get(&key("op:1")).map(|e| e.executions), Some(1));
        assert_eq!(ledger.lookup(&key("op:2"), &a), Ok(Lookup::Absent));
    }

    #[test]
    fn uncertain_entries_refuse_replay_until_reconciled() {
        let mut ledger = Ledger::new(4);
        let a = ArgDigest::of(b"a").expect("a");
        let receipt = ledger.reserve().expect("receipt");
        ledger.intent(key("op:1"), a, receipt).expect("intent");
        ledger.executed(&key("op:1")).expect("executed");
        ledger.uncertain(&key("op:1"), false).expect("unknown");
        let refused = ledger.lookup(&key("op:1"), &a).expect_err("uncertain");
        assert_eq!(refused.effect_state, EffectState::Unknown);
        assert!(!refused.retryable());
        assert!(ledger.rotate(&key("op:1")).is_err());
        ledger.reconcile(&key("op:1")).expect("reconciled");
        assert!(matches!(ledger.lookup(&key("op:1"), &a), Ok(Lookup::Existing(_))));
        ledger.rotate(&key("op:1")).expect("rotated");
        assert!(matches!(ledger.lookup(&key("op:1"), &a), Ok(Lookup::Existing(_))));
    }

    #[test]
    fn capacity_is_bounded() {
        let mut ledger = Ledger::new(1);
        commit(&mut ledger, &key("op:1"), ArgDigest::of(b"a").expect("a"));
        assert_eq!(ledger.reserve().map_err(|e| e.reason), Err("ledger-full"));
    }
}
