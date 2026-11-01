use crate::domain::{DctxError, SnapshotRequest};
use crate::session::{Session, SnapshotHandle};

use super::REUSE_ATTEMPTS;
use super::keys::{retryable, scoped_source_key};

impl Session {
    /// Takes a snapshot, reusing a persisted generation when every stored content digest still matches.
    ///
    /// The check is `digest_text` equality on the source bytes, the same comparison `read_verified`
    /// uses before it returns `SourceChanged`. Size and modification time only skip that read when
    /// the inventory already differs. An empty `source_key`, or a session with no cache root, walks
    /// the source the same way as [`Self::snapshot`].
    /// # Errors
    /// Propagates request/capability validation, source/store/cache failures, cancellation, deadlines, and snapshot resource-limit errors.
    pub fn snapshot_reusable(
        &mut self,
        request: &SnapshotRequest,
        source_key: &str,
    ) -> Result<SnapshotHandle, DctxError> {
        if self.managed.is_none() || source_key.is_empty() {
            return self.snapshot(request);
        }
        let key = scoped_source_key(source_key, self.rules.as_ref());
        let mut last = DctxError::Busy;
        for _ in 0..REUSE_ATTEMPTS {
            match self.snapshot_reusable_once(request, &key) {
                Err(error) if retryable(&error) => {
                    last = error;
                    std::thread::yield_now();
                }
                other => return other,
            }
        }
        Err(last)
    }

    fn snapshot_reusable_once(
        &mut self,
        request: &SnapshotRequest,
        source_key: &str,
    ) -> Result<SnapshotHandle, DctxError> {
        self.reject_snapshot(request)?;
        self.hooks_before_index()?;
        match self.try_reuse(request, source_key)? {
            Some(handle) => {
                self.hooks_after_index()?;
                Ok(handle)
            }
            None => self.snapshot_inner(request, Some(source_key), false),
        }
    }

    pub(super) fn load_retained(&mut self) -> Result<Option<Vec<u8>>, DctxError> {
        let Some(cache) = self.managed.as_mut() else {
            return Ok(None);
        };
        cache.load_current_segment()
    }

    pub(super) fn pin_retained(
        &mut self,
        now_ms: u64,
    ) -> Result<Option<crate::session::ledger::SnapshotLease>, DctxError> {
        let Some(cache) = self.managed.as_mut() else {
            return Ok(None);
        };
        cache.pin_current(now_ms)
    }
}
