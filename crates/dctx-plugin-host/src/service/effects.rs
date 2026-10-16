//! Brokered effects: `file.replace` and `program.run git.add-ignore`, plus reconcile and rotation.
//! Implements PLUGINS.md §3.3 replay rules and §4.4 audit ordering, refining Receipts.tla: Repeat guard →
//! Authority Admit → Resources Receive → durable intent (Request) → Authority Commit → Execute → completion
//! (Complete) or CrashOrUncertain. Depends on broker, domain and ports. Used through `Broker::handle`.

use dctx_plugin_protocol::records::broker::{FileReplace, GitAddIgnore};
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::{ProjectPath, Token};
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::domain::authority::Admission;
use crate::domain::binding::{ActionKey, ArgDigest};
use crate::domain::budget::Dimension;
use crate::domain::grant::Need;
use crate::domain::ledger::Lookup;
use crate::domain::trace::ModelAction;
use crate::error::HostRefusal;
use crate::ports::{AuditKind, AuditPort, FileFault, FileIdentity, FilePort, Recipe, RecipeOutcome, RecipePort};
use crate::service::broker::{Broker, Reply, fault};
use crate::service::handles::HandleKind;

/// How an executed effect ended, as reported by its port.
enum Ran {
    Done(Option<FileIdentity>),
    NotRun(HostRefusal),
    Uncertain,
}

impl<A: AuditPort, F: FilePort, R: RecipePort> Broker<A, F, R> {
    pub(crate) fn file_replace(&mut self, request: &Request, replace: &FileReplace) -> Result<Reply, HostRefusal> {
        let (key, args) = self.replay_key(request)?;
        if let Lookup::Existing(receipt) = self.ledger.lookup(&key, &args)? {
            return Ok(Reply::Receipt { receipt, precondition: None, replayed: true });
        }
        let record = self.handles.get(&replace.resource)?.clone();
        let HandleKind::File { path } = &record.kind else { return Err(HostRefusal::denied("not-a-file-handle")) };
        let identity = self.handles.check_precondition(&replace.resource, &replace.precondition)?;
        let upload = self.handles.upload_mut(&replace.upload)?.clone();
        if upload.resource != replace.resource
            || upload.precondition != replace.precondition
            || upload.invocation != record.binding.invocation
        {
            return Err(HostRefusal::conflict("upload-bound-elsewhere"));
        }
        if !upload.sealed {
            return Err(HostRefusal::conflict("upload-not-sealed"));
        }
        let size = upload.bytes.len() as u64;
        let need = Need::FileWrite { path: path.clone(), size, growth: size.saturating_sub(identity.size), create: false };
        let admission = self.authority.admit(&record.binding, need)?;
        let path = path.clone();
        let outcome = self.effect(key, args, "file.replace", size, &admission, |broker| {
            match broker.files.replace(&path, identity, &upload.bytes) {
                Ok(new) => Ran::Done(Some(new)),
                Err(FileFault::Uncertain) => Ran::Uncertain,
                Err(other) => Ran::NotRun(fault(other)),
            }
        })?;
        self.handles.retire_precondition(&replace.precondition);
        if self.handles.reclaim_upload(&replace.upload).is_some() {
            self.budget.release(Dimension::Upload, size);
        }
        let precondition = match outcome.1 {
            Some(new) => Some(self.handles.precondition(&replace.resource, new)?),
            None => None,
        };
        Ok(Reply::Receipt { receipt: outcome.0, precondition, replayed: false })
    }

    pub(crate) fn program_run(&mut self, request: &Request, run: &GitAddIgnore) -> Result<Reply, HostRefusal> {
        let (key, args) = self.replay_key(request)?;
        if let Lookup::Existing(receipt) = self.ledger.lookup(&key, &args)? {
            return Ok(Reply::Receipt { receipt, precondition: None, replayed: true });
        }
        let record = self.handles.get(&run.resource)?.clone();
        let repository = self.handles.get(&run.repository)?.clone();
        if repository.kind != HandleKind::Repository || repository.binding != record.binding {
            return Err(HostRefusal::denied("not-the-bound-repository"));
        }
        let path: ProjectPath = self.file_path(&run.resource)?;
        let expected = self.handles.check_precondition(&run.resource, &run.precondition)?;
        self.authority.check_content(&path)?;
        let admission = self.authority.admit(&record.binding, Need::GitAddIgnore { path: path.clone() })?;
        let current = self.files.identity(&path).map_err(fault)?;
        if current != expected {
            return Err(HostRefusal::conflict("file-changed"));
        }
        let recipe = Recipe::GitAddIgnore { path, expected };
        let (receipt, _) = self.effect(key, args, "program.run", 0, &admission, |broker| match broker.recipes.run(&recipe) {
            RecipeOutcome::Succeeded => Ran::Done(None),
            RecipeOutcome::Failed => {
                Ran::NotRun(HostRefusal::new(ErrorCode::ExternalProgramFailed, Phase::Broker, "recipe-failed"))
            }
            RecipeOutcome::Missing => {
                Ran::NotRun(HostRefusal::new(ErrorCode::ExternalProgramMissing, Phase::Broker, "program-missing"))
            }
            RecipeOutcome::Uncertain => Ran::Uncertain,
        })?;
        self.authority.record(ModelAction::AuthorityAdmin);
        Ok(Reply::Receipt { receipt, precondition: None, replayed: false })
    }

    /// `Receipts.Reconcile`: the host verified that the single uncertain execution committed.
    pub fn reconcile(&mut self, key: &ActionKey, verified_committed: bool) -> Result<Token, HostRefusal> {
        if !verified_committed {
            return Err(HostRefusal::new(ErrorCode::EffectUnknown, Phase::Broker, "explicit-retry-required"));
        }
        let receipt = self.ledger.get(key).map(|e| e.receipt.clone()).ok_or(HostRefusal::conflict("unknown-action"))?;
        let record = self.record(AuditKind::Completion, "reconcile", Some(receipt), 0, "reconciled");
        self.audit.append(&record).map_err(|_| HostRefusal::audit("reconcile-not-audited"))?;
        self.ledger.reconcile(key)
    }

    /// `Receipts.Rotate`: archive a completed entry; unresolved intents stay pinned.
    pub fn rotate(&mut self, key: &ActionKey) -> Result<(), HostRefusal> {
        self.ledger.rotate(key)
    }

    /// Replay key bound to the session context (the plugin supplies only `action_id`).
    pub fn replay_key(&self, request: &Request) -> Result<(ActionKey, ArgDigest), HostRefusal> {
        ArgDigest::of_request(self.authority.binding(), request)?
            .ok_or(HostRefusal::new(ErrorCode::InvalidMessage, Phase::Broker, "no-action-id"))
    }

    /// Intent → commit check → execute → completion; uncertain outcomes stay pinned and unknown.
    fn effect(
        &mut self,
        key: ActionKey,
        args: ArgDigest,
        method: &'static str,
        bytes: u64,
        admission: &Admission,
        execute: impl FnOnce(&mut Self) -> Ran,
    ) -> Result<(Token, Option<FileIdentity>), HostRefusal> {
        self.budget.charge(&[(Dimension::Actions, 1)])?;
        let receipt = self.ledger.reserve()?;
        let mut intent = self.record(AuditKind::Intent, method, Some(receipt.clone()), bytes, "intent");
        intent.args = Some(args);
        if self.audit.append(&intent).is_err() {
            self.ledger.audit_failed();
            self.budget.release(Dimension::Actions, 1);
            return Err(HostRefusal::audit("intent-not-durable"));
        }
        self.ledger.intent(key.clone(), args, receipt.clone())?;
        if let Err(refusal) = self.authority.commit(admission) {
            return Err(self.resolve_not_run(&key, method, receipt, refusal));
        }
        match execute(self) {
            Ran::NotRun(refusal) => Err(self.resolve_not_run(&key, method, receipt, refusal)),
            Ran::Uncertain => {
                self.ledger.executed(&key)?;
                let receipt = self.ledger.uncertain(&key, false)?;
                let record = self.record(AuditKind::Completion, method, Some(receipt.clone()), 0, "unknown");
                let _ = self.audit.append(&record);
                Err(HostRefusal::unknown(receipt, "effect-outcome-unknown"))
            }
            Ran::Done(identity) => {
                self.ledger.executed(&key)?;
                let record = self.record(AuditKind::Completion, method, Some(receipt.clone()), bytes, "committed");
                if self.audit.append(&record).is_err() {
                    self.ledger.audit_failed();
                    let receipt = self.ledger.uncertain(&key, false)?;
                    return Err(HostRefusal::unknown(receipt, "completion-not-durable"));
                }
                Ok((self.ledger.complete(&key)?, identity))
            }
        }
    }

    /// The effect provably did not run: log completion and drop the intent, or keep it pinned.
    fn resolve_not_run(&mut self, key: &ActionKey, method: &'static str, receipt: Token, refusal: HostRefusal) -> HostRefusal {
        let record = self.record(AuditKind::Completion, method, Some(receipt.clone()), 0, refusal.reason);
        if self.audit.append(&record).is_err() {
            self.ledger.audit_failed();
            return HostRefusal { receipt: Some(receipt), ..HostRefusal::audit("refusal-not-durable") };
        }
        let _ = self.ledger.abandon(key);
        self.budget.release(Dimension::Actions, 1);
        refusal
    }
}
