// SPDX-License-Identifier: Apache-2.0
//! Temp write, validate, sync, immutable rename, then the generation pointer.
//! Implements SEGMENTS-v1 publication order and the P2 crash cuts in [`crate::domain::Boundary`].
//! Depends on [`super::store::SegmentStore`] and `dctx-map-segment` validation. Recovery reads the result.
//! The pointer directory sync completes durable publication on Unix; on Windows the
//! write-through pointer rename already does so. Recovery validates either committed closure.

use crate::SegmentRole;
use crate::domain::{
    Boundary, Manifest, POINTER_LEN, PublishRequest, Published, SegmentBlob, SegmentRef,
    StoreError, StoreOptions, check_root, current_path, encode_manifest, encode_pointer,
    manifest_digest, manifest_path, previous_path, segment_path, tmp_path,
};
use crate::ports::StoreFs;
use dctx_map_segment::{decode, segment_object_digest};

use super::store::{SegmentStore, inspect, manifest_bytes_ok, newest};

mod prepare;
mod stage;

use self::prepare::{build_manifest, prepare};

struct Prepared {
    digest: [u8; 32],
    role: SegmentRole,
    bytes: Vec<u8>,
}

struct Attempt<'a, F: StoreFs> {
    store: &'a SegmentStore<F>,
    root: String,
    locked: bool,
    crashed: bool,
    discard_tmp: bool,
}

impl<'a, F: StoreFs> Attempt<'a, F> {
    fn lock(store: &'a SegmentStore<F>, root: &str) -> Result<Self, StoreError> {
        let mut attempt = Self {
            store,
            root: root.to_string(),
            locked: false,
            crashed: false,
            discard_tmp: true,
        };
        store.fs.try_lock(root)?;
        attempt.locked = true;
        if let Err(err) = attempt.cut(Boundary::AfterLock) {
            if !attempt.crashed {
                let _ = store.fs.unlock(root);
                attempt.locked = false;
            }
            return Err(err);
        }
        Ok(attempt)
    }

    fn cut(&mut self, boundary: Boundary) -> Result<(), StoreError> {
        if let Err(err) = self.store.fs.checkpoint(boundary) {
            if matches!(err, StoreError::Crashed { .. }) {
                self.crashed = true;
                self.store.poison();
            }
            return Err(err);
        }
        if self.store.take_cancel(boundary) {
            return Err(StoreError::Cancelled);
        }
        Ok(())
    }
}

impl<F: StoreFs> Drop for Attempt<'_, F> {
    fn drop(&mut self) {
        if self.crashed {
            return;
        }
        self.store.unprotect();
        if self.discard_tmp {
            let _ = self.store.abort_tmp();
        }
        if self.locked {
            let _ = self.store.fs.unlock(&self.root);
        }
    }
}

struct KeepPath<'a, F: StoreFs> {
    store: &'a SegmentStore<F>,
    path: String,
}

impl<'a, F: StoreFs> KeepPath<'a, F> {
    fn new(store: &'a SegmentStore<F>, path: &str) -> Self {
        store.protect_path(path);
        Self {
            store,
            path: path.to_string(),
        }
    }
}

impl<F: StoreFs> Drop for KeepPath<'_, F> {
    fn drop(&mut self) {
        self.store.unprotect_path(&self.path);
    }
}

struct Place {
    replace: bool,
    protect: Option<[u8; 32]>,
}

impl<F: StoreFs> SegmentStore<F> {
    /// Publishes one generation. Crash cuts leave the previous valid generation or this one.
    ///
    /// # Errors
    /// Refuses invalid requests, crashed or cancelled stores, unsupported durability, busy writers, conflicts and capacity limits; propagates codec, IO and injected-crash failures.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "Keep the public publication entry point consuming its request; borrowing would change its ownership contract."
    )]
    pub fn publish(&self, request: PublishRequest) -> Result<Published, StoreError> {
        self.ensure_live()?;
        self.require_durability()?;
        check_root(&request.root)?;
        let mut attempt = Attempt::lock(self, &request.root)?;
        let result = self.publish_locked(&request, &mut attempt);
        if result.is_ok() {
            attempt.discard_tmp = false;
        }
        if matches!(result, Err(StoreError::Crashed { .. })) {
            attempt.crashed = true;
        }
        result
    }

    /// Installs a pointer before its manifest exists. This is the P2 falsifying mutation.
    ///
    /// # Errors
    /// Refuses invalid roots, crashed stores or unsupported durability; propagates locking, IO, codec and injected-crash failures.
    pub fn publish_unvalidated_pointer(
        &self,
        root: &str,
        generation: u64,
    ) -> Result<(), StoreError> {
        self.ensure_live()?;
        self.require_durability()?;
        check_root(root)?;
        let attempt = Attempt::lock(self, root)?;
        if self.fs.exists(&current_path(root))? {
            let bytes = self.fs.read(&current_path(root))?;
            self.stage(
                &attempt,
                "previous.partial",
                &previous_path(root),
                &bytes,
                &Place {
                    replace: true,
                    protect: None,
                },
                |_| Ok(()),
            )?;
        }
        let pointer = encode_pointer(&crate::domain::Pointer {
            generation,
            digest: [0xff; 32],
            length: 1,
        })?;
        self.stage(
            &attempt,
            "pointer.partial",
            &current_path(root),
            &pointer,
            &Place {
                replace: true,
                protect: None,
            },
            |_| Ok(()),
        )?;
        drop(attempt);
        Ok(())
    }

    fn require_durability(&self) -> Result<(), StoreError> {
        if self.options.mode != crate::domain::DurabilityMode::Durable {
            return Ok(());
        }
        let caps = self.fs.capabilities();
        if !caps.sync {
            return Err(StoreError::UnsupportedDurability { capability: "sync" });
        }
        if !caps.atomic_replace {
            return Err(StoreError::UnsupportedDurability {
                capability: "atomic-replace",
            });
        }
        if !caps.os_lock {
            return Err(StoreError::UnsupportedDurability { capability: "lock" });
        }
        Ok(())
    }

    fn publish_locked(
        &self,
        request: &PublishRequest,
        attempt: &mut Attempt<'_, F>,
    ) -> Result<Published, StoreError> {
        if !self.current_pointer_closed(&request.root)? {
            return Err(StoreError::CorruptIndex { reason: "pointer" });
        }
        let current = newest(&self.fs, &request.root)?;
        match &current {
            Some(closure) if request.generation != closure.generation.saturating_add(1) => {
                return Err(StoreError::InvalidRequest {
                    reason: "generation",
                });
            }
            None if request.generation != 1 => {
                return Err(StoreError::InvalidRequest {
                    reason: "generation",
                });
            }
            _ => {}
        }
        let prepared = prepare(&request.segments)?;
        let manifest = build_manifest(
            &self.options,
            request,
            &prepared,
            current.as_ref().map(|c| c.manifest),
        );
        let man_bytes = encode_manifest(&manifest)?;
        let man_digest = manifest_digest(&man_bytes)?;
        let extra = self.extra_bytes(&prepared, man_bytes.len(), &request.root)?;
        self.can_admit(extra)?;
        attempt.cut(Boundary::AfterReserve)?;

        let mut digests = Vec::new();
        for (index, segment) in prepared.iter().enumerate() {
            let dest = segment_path(&segment.digest);
            if self.fs.exists(&dest)? {
                let have = self.fs.read(&dest)?;
                if have != segment.bytes {
                    return Err(StoreError::CorruptIndex { reason: "segment" });
                }
            } else {
                let tmp = format!("segment-{index}.partial");
                let bytes = segment.bytes.clone();
                let digest = segment.digest;
                let role = segment.role;
                self.stage(
                    attempt,
                    &tmp,
                    &dest,
                    &bytes,
                    &Place {
                        replace: false,
                        protect: Some(digest),
                    },
                    |read| {
                        decode(read, role)
                            .map_err(|_| StoreError::CorruptIndex { reason: "segment" })?;
                        let actual = segment_object_digest(read)
                            .map_err(|_| StoreError::CorruptIndex { reason: "segment" })?;
                        if actual != digest {
                            return Err(StoreError::CorruptIndex { reason: "segment" });
                        }
                        Ok(())
                    },
                )?;
            }
            digests.push(segment.digest);
        }

        let manifest_dest = manifest_path(&man_digest);
        if self.fs.exists(&manifest_dest)? {
            let have = self.fs.read(&manifest_dest)?;
            if have != man_bytes {
                return Err(StoreError::CorruptIndex { reason: "manifest" });
            }
        } else {
            self.stage(
                attempt,
                "manifest.partial",
                &manifest_dest,
                &man_bytes,
                &Place {
                    replace: false,
                    protect: Some(man_digest),
                },
                |read| manifest_bytes_ok(read, &man_digest).map(|_| ()),
            )?;
        }

        if self.fs.exists(&current_path(&request.root))? {
            let bytes = self.fs.read(&current_path(&request.root))?;
            self.stage(
                attempt,
                "previous.partial",
                &previous_path(&request.root),
                &bytes,
                &Place {
                    replace: true,
                    protect: None,
                },
                |read| {
                    if decode_pointer_ok(read).is_err() {
                        return Err(StoreError::CorruptIndex { reason: "pointer" });
                    }
                    Ok(())
                },
            )?;
        }
        attempt.cut(Boundary::AfterPreviousRetain)?;

        let pointer = encode_pointer(&crate::domain::Pointer {
            generation: request.generation,
            digest: man_digest,
            length: u64::try_from(man_bytes.len())
                .map_err(|_| StoreError::InvalidRequest { reason: "size" })?,
        })?;
        self.stage_pointer(attempt, &request.root, &pointer)?;
        attempt.cut(Boundary::BeforeUnlock)?;
        Ok(Published {
            root: request.root.clone(),
            generation: request.generation,
            manifest: man_digest,
            segments: digests,
        })
    }

    fn extra_bytes(
        &self,
        prepared: &[Prepared],
        manifest_len: usize,
        root: &str,
    ) -> Result<u64, StoreError> {
        let mut extra = u64::try_from(manifest_len)
            .map_err(|_| StoreError::InvalidRequest { reason: "size" })?;
        for segment in prepared {
            let path = segment_path(&segment.digest);
            if self.fs.exists(&path)? {
                let have = self.fs.read(&path)?;
                if have != segment.bytes {
                    return Err(StoreError::CorruptIndex { reason: "segment" });
                }
            } else {
                extra =
                    extra.saturating_add(u64::try_from(segment.bytes.len()).unwrap_or(u64::MAX));
            }
        }
        if !self.fs.exists(&current_path(root))? {
            extra = extra.saturating_add(POINTER_LEN);
        }
        if !self.fs.exists(&previous_path(root))? {
            extra = extra.saturating_add(POINTER_LEN);
        }
        Ok(extra)
    }

    pub(super) fn write_durable(
        &self,
        tmp_name: &str,
        dest: &str,
        bytes: &[u8],
        replace: bool,
        validate: impl FnOnce(&[u8]) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let tmp = tmp_path(tmp_name);
        let _keep = KeepPath::new(self, &tmp);
        if self.fs.exists(&tmp)? {
            let _ = self.fs.unlink(&tmp);
        }
        self.fs.create(&tmp)?;
        self.fs.write(&tmp, bytes)?;
        self.fs.flush(&tmp)?;
        self.sync_file(&tmp)?;
        let read_back = self.fs.read(&tmp)?;
        validate(&read_back)?;
        if replace || self.fs.exists(dest)? {
            self.fs.rename_replace(&tmp, dest)?;
        } else {
            self.fs.rename_new(&tmp, dest)?;
        }
        self.sync_dir()?;
        Ok(())
    }
}

fn decode_pointer_ok(bytes: &[u8]) -> Result<(), StoreError> {
    crate::domain::decode_pointer(bytes).map(|_| ())
}
