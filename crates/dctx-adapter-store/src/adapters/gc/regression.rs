// SPDX-License-Identifier: Apache-2.0
//! Deterministic stale-mark schedule, using both filesystem adapters.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, TryLockError};
use std::time::Duration;

use super::*;
use crate::domain::{
    Boundary, PublishRequest, Revisions, StoreOptions, decode_manifest, encode_manifest,
    manifest_digest, manifest_path,
};
use crate::ports::FsCapabilities;
use crate::{MemoryFs, StdFs};

const WAIT: Duration = Duration::from_secs(10);

struct Pause {
    skip: usize,
    captured: Sender<()>,
    resume: Receiver<()>,
}

struct PausedFs<F> {
    inner: F,
    pause: Mutex<Option<Pause>>,
}

impl<F: StoreFs> StoreFs for PausedFs<F> {
    fn capabilities(&self) -> FsCapabilities {
        self.inner.capabilities()
    }
    fn checkpoint(&self, boundary: Boundary) -> Result<(), StoreError> {
        self.inner.checkpoint(boundary)
    }
    fn try_lock(&self, root: &str) -> Result<(), StoreError> {
        self.inner.try_lock(root)
    }
    fn unlock(&self, root: &str) -> Result<(), StoreError> {
        self.inner.unlock(root)
    }
    fn lock_held(&self, root: &str) -> bool {
        self.inner.lock_held(root)
    }
    fn held_roots(&self) -> Vec<String> {
        self.inner.held_roots()
    }
    fn create(&self, path: &str) -> Result<(), StoreError> {
        self.inner.create(path)
    }
    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        self.inner.write(path, bytes)
    }
    fn flush(&self, path: &str) -> Result<(), StoreError> {
        self.inner.flush(path)
    }
    fn sync_file(&self, path: &str) -> Result<(), StoreError> {
        self.inner.sync_file(path)
    }
    fn sync_dir(&self) -> Result<(), StoreError> {
        self.inner.sync_dir()
    }
    fn rename_new(&self, from: &str, to: &str) -> Result<(), StoreError> {
        self.inner.rename_new(from, to)
    }
    fn rename_replace(&self, from: &str, to: &str) -> Result<(), StoreError> {
        self.inner.rename_replace(from, to)
    }
    fn read(&self, path: &str) -> Result<Vec<u8>, StoreError> {
        let bytes = self.inner.read(path)?;
        let pause = {
            let mut pause = self.pause.lock().unwrap();
            if path != current_path("main") {
                None
            } else if let Some(pending) = pause.as_mut()
                && pending.skip > 0
            {
                pending.skip -= 1;
                None
            } else {
                pause.take()
            }
        };
        if let Some(pause) = pause {
            pause.captured.send(()).unwrap();
            pause.resume.recv_timeout(WAIT).unwrap();
        }
        Ok(bytes)
    }
    fn exists(&self, path: &str) -> Result<bool, StoreError> {
        self.inner.exists(path)
    }
    fn len(&self, path: &str) -> Result<u64, StoreError> {
        self.inner.len(path)
    }
    fn unlink(&self, path: &str) -> Result<(), StoreError> {
        self.inner.unlink(path)
    }
    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        self.inner.list_prefix(prefix)
    }
}

fn request(generation: u64) -> PublishRequest {
    PublishRequest {
        root: "main".into(),
        generation,
        snapshot: [4; 32],
        view: [5; 32],
        policy_epoch: 1,
        segments: Vec::new(),
        revisions: Revisions::structural(),
    }
}

#[test]
fn gc_cannot_unlink_a_manifest_reused_by_the_next_publication() {
    stale_mark_schedule(MemoryFs::posix());
    #[cfg(any(unix, windows))]
    {
        let dir = tempfile::tempdir().unwrap();
        stale_mark_schedule(StdFs::open(dir.path()).unwrap());
    }
}

fn stale_mark_schedule<F: StoreFs>(fs: F) {
    let store = SegmentStore::open(
        PausedFs {
            inner: fs,
            pause: Mutex::new(None),
        },
        StoreOptions::capped(8 * 1024 * 1024),
    )
    .unwrap();
    let first = store.publish(request(1)).unwrap();

    // A cancelled publication can leave its immutable manifest behind. A retry
    // reuses exactly these bytes, so it must be safe from a pending GC unlink.
    let bytes = store.fs.read(&manifest_path(&first.manifest)).unwrap();
    let mut orphan = decode_manifest(&bytes).unwrap();
    orphan.generation = 2;
    orphan.previous = Some(first.manifest);
    let bytes = encode_manifest(&orphan).unwrap();
    let path = manifest_path(&manifest_digest(&bytes).unwrap());
    store.fs.create(&path).unwrap();
    store.fs.write(&path, &bytes).unwrap();
    store.fs.flush(&path).unwrap();
    store.fs.sync_file(&path).unwrap();
    store.fs.sync_dir().unwrap();

    let (captured_tx, captured_rx) = channel();
    let (resume_tx, resume_rx) = channel();
    *store.fs.pause.lock().unwrap() = Some(Pause {
        skip: 1, // Skip the initial mark; stop in the fresh mark before unlink.
        captured: captured_tx,
        resume: resume_rx,
    });
    std::thread::scope(|scope| {
        let gc = scope.spawn(|| store.gc(GcBudget { objects: 1 }));
        captured_rx.recv_timeout(WAIT).unwrap();

        // Probe without waiting or sleeps. If GC has no visibility transaction,
        // force publication to finish while its captured pointer is still old.
        // Otherwise, finish GC first, as the visibility lock requires.
        let gc_excludes_publish = match store.visibility.try_write() {
            Err(TryLockError::WouldBlock) => true,
            Ok(guard) => {
                drop(guard);
                false
            }
            Err(err) => panic!("{err}"),
        };
        if !gc_excludes_publish {
            store.publish(request(2)).unwrap();
        }
        resume_tx.send(()).unwrap();
        assert_eq!(gc.join().unwrap().unwrap().unlinked, 1);
        if gc_excludes_publish {
            store.publish(request(2)).unwrap();
        }
    });

    // Without serialization, GC unlinks generation 2's now-current manifest
    // from its stale mark and this publication fails with a broken closure.
    store.publish(request(3)).unwrap();
    assert_eq!(store.committed("main").unwrap(), Some(3));
    assert!(store.current_pointer_closed("main").unwrap());
    let pin = store.pin("main").unwrap();
    assert_eq!(pin.generation(), 3);
    store.release(pin).unwrap();
}
