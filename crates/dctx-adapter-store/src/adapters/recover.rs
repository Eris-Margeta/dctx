//! Recovery to the newest valid pointer closure.
//! Implements SEGMENTS-v1: a crash leaves the old or new committed generation, never a staged object.
//! Depends on [`super::store`] inspection. Publication installs the bytes this module trusts.
//! A newer manifest without a valid pointer is not a generation. Stuck locks are not cleared.

use crate::domain::{current_path, previous_path, Recovery, StoreError};
use crate::ports::StoreFs;

use super::store::{inspect, PointerView, SegmentStore};

enum Class {
    Keep,
    Repair,
    Empty,
    Corrupt,
}

impl<F: StoreFs> SegmentStore<F> {
    /// Selects the newest valid closure and discards temp names.
    /// A broken current pointer is copied back from the previous closure when that closure validates.
    ///
    /// # Errors
    /// Refuses a crashed store, busy writer or unrecoverable pointer closure; propagates inspection, repair, cleanup and durability failures.
    pub fn recover(&self) -> Result<Recovery, StoreError> {
        self.ensure_live()?;
        if !self.fs.held_roots().is_empty() {
            return Err(StoreError::Busy);
        }
        let roots = self.roots()?;
        let mut held: Vec<String> = Vec::new();
        for root in &roots {
            if let Err(err) = self.fs.try_lock(root) {
                for locked in &held {
                    let _ = self.fs.unlock(locked);
                }
                return Err(err);
            }
            held.push(root.clone());
        }
        let result = self.repair_all(&roots);
        for root in &held {
            let _ = self.fs.unlock(root);
        }
        result
    }

    fn repair_all(&self, roots: &[String]) -> Result<Recovery, StoreError> {
        let mut repairs = Vec::new();
        for root in roots {
            match classify(&self.fs, root)? {
                Class::Keep | Class::Empty => {}
                Class::Repair => repairs.push(root.clone()),
                Class::Corrupt => return Err(StoreError::CorruptIndex { reason: "pointer" }),
            }
        }
        for root in &repairs {
            let bytes = self.fs.read(&previous_path(root))?;
            self.write_durable("recover-pointer.partial", &current_path(root), &bytes, true, |_| Ok(()))?;
        }
        let discarded_tmp = self.discard_tmp()?;
        let mut best: Option<(u64, String)> = None;
        for root in roots {
            if let Some(generation) = self.committed(root)? {
                let replace = best.as_ref().is_none_or(|(have, _)| generation > *have);
                if replace {
                    best = Some((generation, root.clone()));
                }
            }
        }
        let (generation, root) = match best {
            Some((generation, root)) => (Some(generation), Some(root)),
            None => (None, None),
        };
        Ok(Recovery {
            generation,
            root,
            discarded_tmp,
            repaired_pointer: !repairs.is_empty(),
        })
    }

    fn discard_tmp(&self) -> Result<u64, StoreError> {
        let mut total = 0u64;
        for path in self.fs.list_prefix("tmp/")? {
            if !self.fs.exists(&path)? {
                continue;
            }
            total = total.saturating_add(self.fs.len(&path)?);
            self.fs.unlink(&path)?;
        }
        self.sync_dir()?;
        Ok(total)
    }
}

fn classify<F: StoreFs>(fs: &F, root: &str) -> Result<Class, StoreError> {
    let current = inspect(fs, &current_path(root))?;
    let previous = inspect(fs, &previous_path(root))?;
    Ok(match (current, previous) {
        (PointerView::Closed(current), PointerView::Closed(previous)) if previous.generation > current.generation => {
            Class::Repair
        }
        (PointerView::Closed(_), _) => Class::Keep,
        (PointerView::Absent | PointerView::Broken, PointerView::Closed(_)) => Class::Repair,
        (PointerView::Absent, PointerView::Absent) => Class::Empty,
        _ => Class::Corrupt,
    })
}
