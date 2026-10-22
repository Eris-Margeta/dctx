//! `Cancellation` port: cooperative cancellation checked at work boundaries.
//! Implements contracts/API-PORTS-v1.md `Cancellation.checkpoint`: returns `Cancelled` and never
//! kills a thread; cancellation before a commit point leaves the committed state unchanged.
//! Depends on dctx-core `DctxError`. Used by dctx-pack at every directory, file and publish.

use dctx_core::error::DctxError;

/// A cancellation signal the caller may raise from another thread.
pub trait Cancellation {
    /// True once cancellation was requested.
    fn is_cancelled(&self) -> bool;

    /// `Err(Cancelled)` once cancellation was requested, otherwise `Ok`.
    ///
    /// # Errors
    /// Returns `Cancelled` once cancellation has been requested.
    fn checkpoint(&self) -> Result<(), DctxError> {
        if self.is_cancelled() {
            Err(DctxError::Cancelled)
        } else {
            Ok(())
        }
    }
}
