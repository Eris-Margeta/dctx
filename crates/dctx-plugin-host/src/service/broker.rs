// SPDX-License-Identifier: Apache-2.0
//! The broker: admits, audits and serves worker requests through the file, recipe and audit ports.
//! Implements PLUGINS.md §3.3/§4.3/§4.4 for `file.read`, `output.read` and `file.stage` here, and the
//! effect methods in `effects`. Every serve passes Authority `Admit` → Resources `Receive` → Authority
//! `Commit` → audit → `Read`. Depends on domain, ports and handles. Used by `HostSession` and PL-F.

use std::collections::BTreeMap;

use dctx_plugin_protocol::records::broker::{ChunkRead, FileStage};
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::Token;
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::domain::authority::Authority;
use crate::domain::budget::{Budget, Dimension, chunk_fits_frame};
use crate::domain::grant::Need;
use crate::domain::ledger::Ledger;
use crate::domain::trace::ModelAction;
use crate::error::HostRefusal;
use crate::ports::{AuditKind, AuditPort, AuditRecord, FileFault, FilePort, RecipePort};
use crate::service::handles::{HandleKind, Handles, Upload};

/// JSON envelope bytes reserved around a base64 chunk when checking the negotiated frame.
pub const ENVELOPE_OVERHEAD: u64 = 1024;

/// A successful broker reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// A bounded content chunk.
    Chunk {
        /// Delivered bytes.
        bytes: Vec<u8>,
        /// Total resource size.
        size: u64,
        /// Precondition token for later chunks and writes.
        precondition: Option<Token>,
    },
    /// An upload chunk was staged.
    Staged {
        /// Upload token.
        upload: Token,
        /// Whether the upload is sealed.
        sealed: bool,
    },
    /// An effect receipt.
    Receipt {
        /// Host receipt ID.
        receipt: Token,
        /// New precondition after a replacement.
        precondition: Option<Token>,
        /// Whether this is an existing receipt returned without a new effect.
        replayed: bool,
    },
}

/// Broker over the audit `A`, file `F` and recipe `R` ports.
#[derive(Debug)]
pub struct Broker<A, F, R> {
    pub(crate) authority: Authority,
    pub(crate) ledger: Ledger,
    pub(crate) budget: Budget,
    pub(crate) handles: Handles,
    pub(crate) delivered: BTreeMap<Token, u64>,
    pub(crate) frame_bytes: u64,
    pub(crate) audit: A,
    pub(crate) files: F,
    pub(crate) recipes: R,
}

impl<A: AuditPort, F: FilePort, R: RecipePort> Broker<A, F, R> {
    /// A broker for one session; `frame_bytes` is the negotiated operation frame ceiling.
    /// Requests must already have passed protocol decoding and session validation. Ports are
    /// trusted adapters: they must honor their identity, byte-bound and durable-audit contracts.
    pub fn new(
        authority: Authority,
        ledger: Ledger,
        budget: Budget,
        frame_bytes: u64,
        ports: (A, F, R),
    ) -> Self {
        let (audit, files, recipes) = ports;
        Self {
            authority,
            ledger,
            budget,
            handles: Handles::default(),
            delivered: BTreeMap::new(),
            frame_bytes,
            audit,
            files,
            recipes,
        }
    }

    /// Registers a handle bound to the session's current context.
    /// # Errors
    /// Refuses an exhausted handle table or an invalid generated handle token.
    pub fn register(&mut self, kind: HandleKind) -> Result<Token, HostRefusal> {
        let binding = self.authority.binding().clone();
        self.handles.register(binding, kind)
    }

    /// Registers a handle bound to another context (a stale or foreign handle in tests and recovery).
    /// # Errors
    /// Refuses an exhausted handle table or an invalid generated handle token.
    pub fn register_bound(
        &mut self,
        binding: crate::domain::binding::Binding,
        kind: HandleKind,
    ) -> Result<Token, HostRefusal> {
        self.handles.register(binding, kind)
    }

    /// Host-side precondition for a selected file handle (supplied with initialize resources).
    /// # Errors
    /// Refuses an unknown/non-file handle, excluded source content, a file identity fault, or exhausted precondition storage.
    pub fn issue_precondition(&mut self, handle: &Token) -> Result<Token, HostRefusal> {
        let path = self.file_path(handle)?;
        self.authority.check_content(&path)?;
        let identity = self.files.identity(&path).map_err(fault)?;
        self.handles.precondition(handle, identity)
    }

    /// Serves one worker request; refusals are audited best-effort and returned.
    /// # Errors
    /// Refuses unsupported methods, invalid bindings/grants, quota violations, file races, unavailable audit, or uncertain broker effects.
    pub fn handle(&mut self, request: &Request) -> Result<Reply, HostRefusal> {
        let result = match request {
            Request::FileRead(read) => self.file_read(read),
            Request::OutputRead(read) => self.output_read(read),
            Request::FileStage(stage) => self.file_stage(stage),
            Request::FileReplace(replace) => self.file_replace(request, replace),
            Request::ProgramRun(run) => self.program_run(request, run),
            _ => Err(HostRefusal::new(
                ErrorCode::UnsupportedCapability,
                Phase::Broker,
                "method-not-brokered",
            )),
        };
        if let Err(refusal) = &result {
            let record = self.record(
                AuditKind::Refusal,
                request.method().as_str(),
                None,
                0,
                refusal.reason,
            );
            let _ = self.audit.append(&record);
        }
        result
    }

    /// Disable/cancel: close admission and revoke every ephemeral handle and upload.
    /// Releases all live upload-byte charges. Repeating the call keeps admission closed.
    pub fn halt(&mut self) {
        self.authority.halt();
        self.handles.revoke_all();
        let retained = self.budget.quota(Dimension::Upload).used();
        self.budget.release(Dimension::Upload, retained);
    }

    /// Session authority (policy changes go through it).
    pub fn authority_mut(&mut self) -> &mut Authority {
        &mut self.authority
    }

    /// Session authority.
    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    /// Receipt ledger.
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// Budget.
    pub fn budget(&self) -> &Budget {
        &self.budget
    }

    /// Audit port.
    pub fn audit(&self) -> &A {
        &self.audit
    }

    /// Audit port, mutably (fault injection).
    pub fn audit_mut(&mut self) -> &mut A {
        &mut self.audit
    }

    /// File port.
    pub fn files(&self) -> &F {
        &self.files
    }

    /// File port, mutably (outside writers in tests).
    pub fn files_mut(&mut self) -> &mut F {
        &mut self.files
    }

    /// Recipe port.
    pub fn recipes(&self) -> &R {
        &self.recipes
    }

    /// Recipe port, mutably.
    pub fn recipes_mut(&mut self) -> &mut R {
        &mut self.recipes
    }

    fn file_read(&mut self, read: &ChunkRead) -> Result<Reply, HostRefusal> {
        let record = self.handles.get(&read.handle)?.clone();
        let HandleKind::File { path } = &record.kind else {
            return Err(HostRefusal::denied("not-a-file-handle"));
        };
        let total = self.delivered_after(&read.handle, read.length)?;
        self.authority.check_content(path)?;
        let admission = self.authority.admit(
            &record.binding,
            Need::FileRead {
                path: path.clone(),
                bytes: total,
            },
        )?;
        self.check_frame(read.length)?;
        let (identity, precondition) = if let Some(token) = &read.precondition {
            (self.handles.check_precondition(&read.handle, token)?, None)
        } else {
            let identity = self.files.identity(path).map_err(fault)?;
            (
                identity,
                Some(self.handles.precondition(&read.handle, identity)?),
            )
        };
        self.budget.charge(&[(Dimension::Content, read.length)])?;
        if let Err(refusal) = self.authority.commit(&admission) {
            self.budget.release(Dimension::Content, read.length);
            return Err(refusal);
        }
        let bytes = match self.files.read(path, identity, read.offset, read.length) {
            Ok(bytes) => bytes,
            Err(f) => {
                self.budget.release(Dimension::Content, read.length);
                return Err(fault(f));
            }
        };
        self.deliver(&read.handle, "file.read", read.length, bytes.len() as u64)?;
        Ok(Reply::Chunk {
            bytes,
            size: identity.size,
            precondition,
        })
    }

    fn output_read(&mut self, read: &ChunkRead) -> Result<Reply, HostRefusal> {
        let record = self.handles.get(&read.handle)?.clone();
        let HandleKind::Output {
            role,
            operation,
            provenance,
            bytes,
        } = &record.kind
        else {
            return Err(HostRefusal::denied("not-an-output-handle"));
        };
        let total = self.delivered_after(&read.handle, read.length)?;
        let need = Need::OutputRead {
            role: role.clone(),
            operation: *operation,
            bytes: total,
        };
        self.authority.check_provenance(provenance)?;
        let admission = self.authority.admit(&record.binding, need)?;
        self.check_frame(read.length)?;
        self.budget.charge(&[(Dimension::Content, read.length)])?;
        if let Err(refusal) = self.authority.commit(&admission) {
            self.budget.release(Dimension::Content, read.length);
            return Err(refusal);
        }
        let start = usize::try_from(read.offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let end = usize::try_from(read.offset.saturating_add(read.length))
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let chunk = bytes[start..end].to_vec();
        self.deliver(&read.handle, "output.read", read.length, chunk.len() as u64)?;
        Ok(Reply::Chunk {
            bytes: chunk,
            size: bytes.len() as u64,
            precondition: None,
        })
    }

    fn file_stage(&mut self, stage: &FileStage) -> Result<Reply, HostRefusal> {
        let record = self.handles.get(&stage.resource)?.clone();
        let HandleKind::File { path } = &record.kind else {
            return Err(HostRefusal::denied("not-a-file-handle"));
        };
        let identity = self
            .handles
            .check_precondition(&stage.resource, &stage.precondition)?;
        let len = stage.bytes.len() as u64;
        let size = stage
            .offset
            .checked_add(len)
            .ok_or(HostRefusal::limit("upload-size"))?;
        let growth = size.saturating_sub(identity.size);
        let need = Need::FileWrite {
            path: path.clone(),
            size,
            growth,
            create: false,
        };
        let admission = self.authority.admit(&record.binding, need)?;
        let invocation = record.binding.invocation.clone();
        let upload = match &stage.upload {
            None if stage.offset == 0 => None,
            None => return Err(HostRefusal::conflict("upload-must-start-at-zero")),
            Some(token) => {
                let upload = self.handles.upload_mut(token)?;
                if upload.resource != stage.resource
                    || upload.precondition != stage.precondition
                    || upload.invocation != invocation
                {
                    return Err(HostRefusal::conflict("upload-bound-elsewhere"));
                }
                if upload.sealed || upload.bytes.len() as u64 != stage.offset {
                    return Err(HostRefusal::conflict("upload-not-contiguous"));
                }
                Some(token.clone())
            }
        };
        self.budget.charge(&[(Dimension::Upload, len)])?;
        if let Err(refusal) = self.authority.commit(&admission) {
            self.budget.release(Dimension::Upload, len);
            return Err(refusal);
        }
        let token = match upload {
            Some(token) => token,
            None => match self.handles.start_upload(Upload {
                resource: stage.resource.clone(),
                precondition: stage.precondition.clone(),
                invocation,
                bytes: Vec::new(),
                sealed: false,
            }) {
                Ok(token) => token,
                Err(refusal) => {
                    self.budget.release(Dimension::Upload, len);
                    return Err(refusal);
                }
            },
        };
        let upload = self.handles.upload_mut(&token)?;
        upload.bytes.extend_from_slice(stage.bytes.as_bytes());
        upload.sealed = stage.final_chunk;
        Ok(Reply::Staged {
            upload: token,
            sealed: stage.final_chunk,
        })
    }

    pub(crate) fn record(
        &self,
        kind: AuditKind,
        method: &'static str,
        receipt: Option<Token>,
        bytes: u64,
        reason: &'static str,
    ) -> AuditRecord {
        let binding = self.authority.binding();
        AuditRecord {
            kind,
            plugin: binding.plugin.clone(),
            invocation: binding.invocation.clone(),
            epoch: self.authority.epoch(),
            method,
            handle: None,
            args: None,
            bytes,
            receipt,
            reason,
        }
    }

    pub(crate) fn file_path(
        &self,
        handle: &Token,
    ) -> Result<dctx_plugin_protocol::records::scalars::ProjectPath, HostRefusal> {
        match &self.handles.get(handle)?.kind {
            HandleKind::File { path } => Ok(path.clone()),
            _ => Err(HostRefusal::denied("not-a-file-handle")),
        }
    }

    fn delivered_after(&self, handle: &Token, length: u64) -> Result<u64, HostRefusal> {
        self.delivered
            .get(handle)
            .copied()
            .unwrap_or(0)
            .checked_add(length)
            .ok_or(HostRefusal::limit("read-total"))
    }

    fn check_frame(&self, length: u64) -> Result<(), HostRefusal> {
        if chunk_fits_frame(length, ENVELOPE_OVERHEAD, self.frame_bytes) {
            Ok(())
        } else {
            Err(HostRefusal::limit("chunk-exceeds-frame"))
        }
    }

    /// Audits the delivery before the bytes leave the host, then records `Authority.Read`.
    fn deliver(
        &mut self,
        handle: &Token,
        method: &'static str,
        charged: u64,
        actual: u64,
    ) -> Result<(), HostRefusal> {
        self.budget
            .release(Dimension::Content, charged.saturating_sub(actual));
        let mut record = self.record(AuditKind::Delivery, method, None, actual, "delivered");
        record.handle = Some(handle.clone());
        if self.audit.append(&record).is_err() {
            self.budget.release(Dimension::Content, actual);
            self.ledger.audit_failed();
            return Err(HostRefusal::audit("delivery-not-audited"));
        }
        let total = self.delivered.entry(handle.clone()).or_insert(0);
        *total = total.saturating_add(actual);
        self.authority.record(ModelAction::AuthorityRead);
        Ok(())
    }
}

/// Maps a file port fault to a refusal with no effect.
pub(crate) fn fault(fault: FileFault) -> HostRefusal {
    match fault {
        FileFault::Missing => HostRefusal::denied("file-missing"),
        FileFault::Changed => HostRefusal::conflict("file-changed"),
        FileFault::Uncertain => HostRefusal::conflict("file-state-uncertain"),
    }
}
