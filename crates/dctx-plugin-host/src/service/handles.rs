//! Host-issued handles, preconditions and private uploads, each bound to its full context.
//! Implements PLUGINS.md §3.3 handle rules and Authority.tla `CheckBinding`: an output handle binds plugin,
//! invocation, policy epoch and generation; uploads bind resource, precondition and invocation. Tokens are
//! opaque host counters, never paths. Depends on domain binding and ports. Used by the broker service.

use std::collections::BTreeMap;

use dctx_plugin_protocol::records::capability::OutputOperation;
use dctx_plugin_protocol::records::scalars::{ProjectPath, Role, Token};

use crate::domain::binding::Binding;
use crate::error::HostRefusal;
use crate::ports::FileIdentity;

/// Most live handles, preconditions or uploads per broker.
pub const MAX_HANDLES: usize = 256;

/// What a handle refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandleKind {
    /// A named project file.
    File {
        /// Project-relative path (never sent to the worker).
        path: ProjectPath,
    },
    /// An engine output snapshot with its source provenance.
    Output {
        /// Output role.
        role: Role,
        /// Producing operation.
        operation: OutputOperation,
        /// Source paths the bytes derive from.
        provenance: Vec<ProjectPath>,
        /// Output bytes.
        bytes: Vec<u8>,
    },
    /// The `git-admin:stage-ignore` administrative resource.
    Repository,
}

/// A handle and the context it is bound to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleRecord {
    /// Bound context.
    pub binding: Binding,
    /// Referent.
    pub kind: HandleKind,
}

/// A precondition token's referent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreconditionRecord {
    /// File identity checked when the token was issued.
    pub identity: FileIdentity,
}

/// A host-private upload under construction or sealed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upload {
    /// File handle it replaces.
    pub resource: Token,
    /// Precondition it was staged against.
    pub precondition: Token,
    /// Invocation that staged it.
    pub invocation: Token,
    /// Staged bytes.
    pub bytes: Vec<u8>,
    /// Whether the final chunk arrived.
    pub sealed: bool,
}

/// Token tables for one broker.
#[derive(Clone, Debug, Default)]
pub struct Handles {
    records: BTreeMap<Token, HandleRecord>,
    preconditions: BTreeMap<Token, (Token, PreconditionRecord)>,
    uploads: BTreeMap<Token, Upload>,
    next: u64,
}

impl Handles {
    /// Issues a fresh opaque token with `prefix`.
    /// # Errors
    /// Refuses if the generated token does not fit the protocol token format.
    pub fn mint(&mut self, prefix: &str) -> Result<Token, HostRefusal> {
        self.next = self.next.saturating_add(1);
        Token::new(&format!("{prefix}:{}", self.next)).map_err(|_| HostRefusal::limit("token-space"))
    }

    /// Registers a handle bound to `binding`.
    /// # Errors
    /// Refuses when the handle table is full or a handle token cannot be minted.
    pub fn register(&mut self, binding: Binding, kind: HandleKind) -> Result<Token, HostRefusal> {
        if self.records.len() >= MAX_HANDLES {
            return Err(HostRefusal::limit("too-many-handles"));
        }
        let token = self.mint("h")?;
        self.records.insert(token.clone(), HandleRecord { binding, kind });
        Ok(token)
    }

    /// Looks a handle up; unknown handles are denied.
    /// # Errors
    /// Refuses an unknown handle.
    pub fn get(&self, token: &Token) -> Result<&HandleRecord, HostRefusal> {
        self.records.get(token).ok_or(HostRefusal::denied("unknown-handle"))
    }

    /// Revokes every handle (shutdown/halt).
    pub fn revoke_all(&mut self) {
        self.records.clear();
        self.preconditions.clear();
        self.uploads.clear();
    }

    /// Issues a precondition token for `handle` at `identity`.
    /// # Errors
    /// Refuses when the precondition table is full or a token cannot be minted.
    pub fn precondition(&mut self, handle: &Token, identity: FileIdentity) -> Result<Token, HostRefusal> {
        if self.preconditions.len() >= MAX_HANDLES {
            return Err(HostRefusal::limit("too-many-preconditions"));
        }
        let token = self.mint("pre")?;
        self.preconditions.insert(token.clone(), (handle.clone(), PreconditionRecord { identity }));
        Ok(token)
    }

    /// The identity a precondition was issued at, if it is bound to `handle`.
    /// # Errors
    /// Refuses an unknown precondition or one bound to another handle.
    pub fn check_precondition(&self, handle: &Token, token: &Token) -> Result<FileIdentity, HostRefusal> {
        match self.preconditions.get(token) {
            Some((bound, record)) if bound == handle => Ok(record.identity),
            Some(_) => Err(HostRefusal::conflict("precondition-bound-elsewhere")),
            None => Err(HostRefusal::conflict("unknown-precondition")),
        }
    }

    /// Drops a consumed precondition.
    pub fn retire_precondition(&mut self, token: &Token) {
        self.preconditions.remove(token);
    }

    /// Starts an upload.
    /// # Errors
    /// Refuses when the upload table is full or a token cannot be minted.
    pub fn start_upload(&mut self, upload: Upload) -> Result<Token, HostRefusal> {
        if self.uploads.len() >= MAX_HANDLES {
            return Err(HostRefusal::limit("too-many-uploads"));
        }
        let token = self.mint("up")?;
        self.uploads.insert(token.clone(), upload);
        Ok(token)
    }

    /// Mutable access to an upload.
    /// # Errors
    /// Refuses an unknown upload token.
    pub fn upload_mut(&mut self, token: &Token) -> Result<&mut Upload, HostRefusal> {
        self.uploads.get_mut(token).ok_or(HostRefusal::conflict("unknown-upload"))
    }

    /// Reclaims an upload.
    pub fn reclaim_upload(&mut self, token: &Token) -> Option<Upload> {
        self.uploads.remove(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::binding::tests::context;

    #[test]
    fn preconditions_bind_their_handle() {
        let mut handles = Handles::default();
        let path = ProjectPath::new(".gitignore").expect("path");
        let a = handles.register(context("op:1"), HandleKind::File { path: path.clone() }).expect("a");
        let b = handles.register(context("op:1"), HandleKind::File { path }).expect("b");
        let identity = FileIdentity { object: 1, version: 1, size: 3 };
        let pre = handles.precondition(&a, identity).expect("pre");
        assert_eq!(handles.check_precondition(&a, &pre), Ok(identity));
        assert!(handles.check_precondition(&b, &pre).is_err());
        handles.revoke_all();
        assert!(handles.get(&a).is_err());
    }
}
