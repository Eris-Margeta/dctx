//! Segment store: pins, admission, and the shared coordinator.
//! Implements the short reader handshake in SEGMENTS-v1 and CACHE-v1 admission.
//! Depends on [`crate::ports::StoreFs`] and the pure manifest rules. Publish, recover, and GC share this type.
//! One pin names one generation. Bytes are read from immutable digest names.

use std::collections::{BTreeSet, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::domain::{
    check_root, current_path, decode_manifest, decode_pointer, manifest_digest, manifest_path,
    previous_path, segment_path, DurabilityMode, Manifest, Pin, PinnedSegment, SegmentRef,
    StoreError, StoreOptions, EMERGENCY_METADATA, MAX_ACQUIRE,
};
use crate::ports::StoreFs;
use dctx_map_segment::{decode, segment_object_digest};

pub(crate) struct Lease {
    pub id: u64,
    pub root: String,
    pub generation: u64,
    pub manifest: [u8; 32],
    pub segments: Vec<PinnedSegment>,
}

impl Clone for Lease {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            root: self.root.clone(),
            generation: self.generation,
            manifest: self.manifest,
            segments: self.segments.clone(),
        }
    }
}

pub(crate) struct Coord {
    pub leases: Vec<Lease>,
    pub writing: BTreeSet<[u8; 32]>,
    /// Temp paths a publication is still using. GC must not unlink them.
    pub protecting: HashSet<String>,
    pub next_pin: u64,
    pub pending: HashSet<String>,
    pub cancel_at: Option<crate::domain::Boundary>,
}

impl Default for Coord {
    fn default() -> Self {
        Self {
            leases: Vec::new(),
            writing: BTreeSet::new(),
            protecting: HashSet::new(),
            next_pin: 1,
            pending: HashSet::new(),
            cancel_at: None,
        }
    }
}

pub(crate) struct Closure {
    pub generation: u64,
    pub manifest: [u8; 32],
    pub segments: Vec<SegmentRef>,
}

pub(crate) enum PointerView {
    Absent,
    Broken,
    Closed(Closure),
}

/// Filesystem-backed generations for one namespace.
pub struct SegmentStore<F: StoreFs> {
    pub(crate) fs: F,
    pub(crate) options: StoreOptions,
    pub(crate) coord: Mutex<Coord>,
    pub(crate) cancel: AtomicBool,
    pub(crate) poisoned: AtomicBool,
}

impl<F: StoreFs> SegmentStore<F> {
    /// Opens a store on `fs`. Does not scan or repair pointers; call [`Self::recover`] after a crash.
    pub fn open(fs: F, options: StoreOptions) -> Result<Self, StoreError> {
        options.check()?;
        Ok(Self {
            fs,
            options,
            coord: Mutex::new(Coord::default()),
            cancel: AtomicBool::new(false),
            poisoned: AtomicBool::new(false),
        })
    }

    pub(crate) fn ensure_live(&self) -> Result<(), StoreError> {
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(StoreError::Crashed { boundary: "store" });
        }
        Ok(())
    }

    pub(crate) fn poison(&self) {
        self.poisoned.store(true, Ordering::SeqCst);
    }

    pub(crate) fn coord(&self) -> MutexGuard<'_, Coord> {
        self.coord.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn protect(&self, digest: [u8; 32]) {
        self.coord().writing.insert(digest);
    }

    pub(crate) fn unprotect(&self) {
        self.coord().writing.clear();
    }

    pub(crate) fn protect_path(&self, path: &str) {
        self.coord().protecting.insert(path.to_string());
    }

    pub(crate) fn unprotect_path(&self, path: &str) {
        self.coord().protecting.remove(path);
    }

    /// Asks publish and GC to stop at their next boundary.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Cancels once, when publication reaches `boundary`.
    pub fn cancel_at(&self, boundary: crate::domain::Boundary) {
        self.coord().cancel_at = Some(boundary);
    }

    pub(crate) fn take_cancel(&self, boundary: crate::domain::Boundary) -> bool {
        let armed = self.coord().cancel_at == Some(boundary);
        if armed {
            self.coord().cancel_at = None;
        }
        let flag = self.cancel.swap(false, Ordering::SeqCst);
        armed || flag
    }

    /// True when the current pointer is absent or names a complete closure.
    pub fn current_pointer_closed(&self, root: &str) -> Result<bool, StoreError> {
        self.ensure_live()?;
        check_root(root)?;
        Ok(!matches!(inspect(&self.fs, &current_path(root))?, PointerView::Broken))
    }

    /// Generation inside the current pointer file, even when its closure is missing.
    pub fn raw_current_generation(&self, root: &str) -> Result<Option<u64>, StoreError> {
        self.ensure_live()?;
        check_root(root)?;
        let path = current_path(root);
        if !self.fs.exists(&path)? {
            return Ok(None);
        }
        let bytes = self.fs.read(&path)?;
        Ok(Some(decode_pointer(&bytes)?.generation))
    }

    /// Newest valid committed generation for `root`, ignoring invalid pointer bytes.
    pub fn committed(&self, root: &str) -> Result<Option<u64>, StoreError> {
        self.ensure_live()?;
        Ok(newest(&self.fs, root)?.map(|closure| closure.generation))
    }

    /// Whether an immutable segment object is still stored.
    pub fn contains_segment(&self, digest: &[u8; 32]) -> Result<bool, StoreError> {
        self.ensure_live()?;
        self.fs.exists(&segment_path(digest))
    }

    /// Acquires a short pin on the newest valid generation of `root`.
    pub fn pin(&self, root: &str) -> Result<Pin, StoreError> {
        self.ensure_live()?;
        check_root(root)?;
        for _ in 0..MAX_ACQUIRE {
            let Some(observed) = newest(&self.fs, root)? else {
                return Err(StoreError::InvalidRequest { reason: "empty" });
            };
            let mut coord = self.coord();
            if !still_retained(&self.fs, root, observed.generation, observed.manifest)? {
                continue;
            }
            let id = coord.next_pin;
            coord.next_pin = coord.next_pin.saturating_add(1);
            let segments: Vec<PinnedSegment> = observed
                .segments
                .iter()
                .map(|segment| PinnedSegment::new(segment.digest, segment.role))
                .collect();
            coord.leases.push(Lease {
                id,
                root: root.to_string(),
                generation: observed.generation,
                manifest: observed.manifest,
                segments: segments.clone(),
            });
            return Ok(Pin {
                id,
                root: root.to_string(),
                generation: observed.generation,
                manifest: observed.manifest,
                segments,
            });
        }
        Err(StoreError::Busy)
    }

    /// Drops a pin. A second release is [`StoreError::InvalidRequest`].
    pub fn release(&self, pin: Pin) -> Result<(), StoreError> {
        self.ensure_live()?;
        let mut coord = self.coord();
        let before = coord.leases.len();
        coord.leases.retain(|lease| lease.id != pin.id);
        if coord.leases.len() == before {
            return Err(StoreError::InvalidRequest { reason: "pin" });
        }
        Ok(())
    }

    /// Reads one segment named by `pin`. The bytes are checked against the digest and the codec.
    pub fn read_segment(&self, pin: &Pin, digest: &[u8; 32]) -> Result<Vec<u8>, StoreError> {
        self.ensure_live()?;
        let role = pin.role_of(digest).ok_or(StoreError::InvalidRequest { reason: "pin" })?;
        let bytes = self.fs.read(&segment_path(digest))?;
        let actual = segment_object_digest(&bytes).map_err(|_| StoreError::CorruptIndex { reason: "segment" })?;
        if actual != *digest {
            return Err(StoreError::CorruptIndex { reason: "segment" });
        }
        decode(&bytes, role).map_err(|_| StoreError::CorruptIndex { reason: "segment" })?;
        Ok(bytes)
    }

    pub(crate) fn physical(&self) -> Result<u64, StoreError> {
        let mut total = 0u64;
        for prefix in ["segments/", "manifests/", "tmp/", "roots/", "maintenance/"] {
            for path in self.fs.list_prefix(prefix)? {
                match self.fs.len(&path) {
                    Ok(len) => total = total.saturating_add(len),
                    Err(StoreError::Io { detail }) if detail == "missing" => {}
                    Err(err) => return Err(err),
                }
            }
        }
        Ok(total)
    }

    pub(crate) fn can_admit(&self, extra: u64) -> Result<(), StoreError> {
        let physical = self.physical()?;
        let used = physical.saturating_add(EMERGENCY_METADATA);
        let total = used.saturating_add(extra);
        if total > self.options.cap || total < used {
            return Err(StoreError::CacheLimit {
                requested: extra,
                available: self.options.cap.saturating_sub(used),
            });
        }
        Ok(())
    }

    pub(crate) fn abort_tmp(&self) -> Result<(), StoreError> {
        for path in self.fs.list_prefix("tmp/")? {
            if self.fs.exists(&path)? {
                let _ = self.fs.unlink(&path);
            }
        }
        if self.options.mode == DurabilityMode::Durable && self.fs.capabilities().sync {
            let _ = self.fs.sync_dir();
        }
        Ok(())
    }

    pub(crate) fn sync_file(&self, path: &str) -> Result<(), StoreError> {
        if self.options.mode == DurabilityMode::Durable {
            self.fs.sync_file(path)?;
        }
        Ok(())
    }

    pub(crate) fn sync_dir(&self) -> Result<(), StoreError> {
        if self.options.mode == DurabilityMode::Durable {
            self.fs.sync_dir()?;
        }
        Ok(())
    }

    pub(crate) fn roots(&self) -> Result<Vec<String>, StoreError> {
        let mut roots = BTreeSet::new();
        for path in self.fs.list_prefix("roots/")? {
            if let Some(root) = root_of(&path)
                && check_root(root).is_ok()
            {
                roots.insert(root.to_string());
            }
        }
        Ok(roots.into_iter().collect())
    }
}

fn root_of(path: &str) -> Option<&str> {
    let mut parts = path.split('/');
    if parts.next() != Some("roots") {
        return None;
    }
    parts.next()
}

pub(crate) fn newest<F: StoreFs>(fs: &F, root: &str) -> Result<Option<Closure>, StoreError> {
    check_root(root)?;
    let current = inspect(fs, &current_path(root))?;
    let previous = inspect(fs, &previous_path(root))?;
    match (current, previous) {
        (PointerView::Closed(left), PointerView::Closed(right)) => {
            if left.generation >= right.generation {
                Ok(Some(left))
            } else {
                Ok(Some(right))
            }
        }
        (PointerView::Closed(closure), _) => Ok(Some(closure)),
        (_, PointerView::Closed(closure)) => Ok(Some(closure)),
        (PointerView::Absent, PointerView::Absent) => Ok(None),
        _ => Err(StoreError::CorruptIndex { reason: "pointer" }),
    }
}

fn still_retained<F: StoreFs>(
    fs: &F,
    root: &str,
    generation: u64,
    manifest: [u8; 32],
) -> Result<bool, StoreError> {
    for path in [current_path(root), previous_path(root)] {
        if let PointerView::Closed(closure) = inspect(fs, &path)?
            && closure.generation == generation
            && closure.manifest == manifest
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn inspect<F: StoreFs>(fs: &F, path: &str) -> Result<PointerView, StoreError> {
    if !fs.exists(path)? {
        return Ok(PointerView::Absent);
    }
    let bytes = fs.read(path)?;
    let pointer = match decode_pointer(&bytes) {
        Ok(pointer) => pointer,
        Err(_) => return Ok(PointerView::Broken),
    };
    let man_path = manifest_path(&pointer.digest);
    if !fs.exists(&man_path)? {
        return Ok(PointerView::Broken);
    }
    let man_bytes = fs.read(&man_path)?;
    let man_len = u64::try_from(man_bytes.len()).unwrap_or(u64::MAX);
    if man_len != pointer.length {
        return Ok(PointerView::Broken);
    }
    let digest = match manifest_digest(&man_bytes) {
        Ok(digest) => digest,
        Err(_) => return Ok(PointerView::Broken),
    };
    if digest != pointer.digest {
        return Ok(PointerView::Broken);
    }
    let manifest = match decode_manifest(&man_bytes) {
        Ok(manifest) => manifest,
        Err(_) => return Ok(PointerView::Broken),
    };
    if manifest.generation != pointer.generation {
        return Ok(PointerView::Broken);
    }
    if let Some(root) = root_of(path)
        && manifest.root != root
    {
        return Ok(PointerView::Broken);
    }
    let mut segments = Vec::new();
    for segment in manifest.segments {
        if !segment_ok(fs, &segment)? {
            return Ok(PointerView::Broken);
        }
        segments.push(segment);
    }
    Ok(PointerView::Closed(Closure {
        generation: manifest.generation,
        manifest: pointer.digest,
        segments,
    }))
}

fn segment_ok<F: StoreFs>(fs: &F, segment: &SegmentRef) -> Result<bool, StoreError> {
    let path = segment_path(&segment.digest);
    if !fs.exists(&path)? {
        return Ok(false);
    }
    let bytes = fs.read(&path)?;
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len != segment.length {
        return Ok(false);
    }
    let digest = match segment_object_digest(&bytes) {
        Ok(digest) => digest,
        Err(_) => return Ok(false),
    };
    if digest != segment.digest || decode(&bytes, segment.role).is_err() {
        return Ok(false);
    }
    Ok(true)
}

pub(crate) fn manifest_bytes_ok(bytes: &[u8], expected: &[u8; 32]) -> Result<Manifest, StoreError> {
    let manifest = decode_manifest(bytes)?;
    let digest = manifest_digest(bytes)?;
    if digest != *expected {
        return Err(StoreError::CorruptIndex { reason: "manifest" });
    }
    Ok(manifest)
}
