use super::*;

struct Cuts {
    create: Boundary,
    write: Boundary,
    flush: Boundary,
    sync: Boundary,
    validate: Boundary,
    rename: Boundary,
    dir_sync: Boundary,
}

impl<F: StoreFs> SegmentStore<F> {
    pub(super) fn stage(
        &self,
        attempt: &Attempt<'_, F>,
        tmp_name: &str,
        dest: &str,
        bytes: &[u8],
        place: &Place,
        validate: impl FnOnce(&[u8]) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        if !tmp_name.starts_with("segment") && !tmp_name.starts_with("manifest") {
            return self.write_durable(tmp_name, dest, bytes, place.replace, validate);
        }
        self.stage_cut(attempt, tmp_name, dest, bytes, place, validate)
    }

    fn stage_cut(
        &self,
        attempt: &Attempt<'_, F>,
        tmp_name: &str,
        dest: &str,
        bytes: &[u8],
        place: &Place,
        validate: impl FnOnce(&[u8]) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let cuts = if tmp_name.starts_with("segment") {
            Cuts {
                create: Boundary::AfterSegmentCreate,
                write: Boundary::AfterSegmentWrite,
                flush: Boundary::AfterSegmentFlush,
                sync: Boundary::AfterSegmentSync,
                validate: Boundary::AfterSegmentValidate,
                rename: Boundary::AfterSegmentRename,
                dir_sync: Boundary::AfterSegmentDirSync,
            }
        } else {
            Cuts {
                create: Boundary::AfterManifestCreate,
                write: Boundary::AfterManifestWrite,
                flush: Boundary::AfterManifestFlush,
                sync: Boundary::AfterManifestSync,
                validate: Boundary::AfterManifestValidate,
                rename: Boundary::AfterManifestRename,
                dir_sync: Boundary::AfterManifestDirSync,
            }
        };
        let tmp = tmp_path(tmp_name);
        let _keep = KeepPath::new(self, &tmp);
        if self.fs.exists(&tmp)? {
            let _ = self.fs.unlink(&tmp);
            let _ = self.sync_dir();
        }
        if let Some(digest) = place.protect {
            self.protect(digest);
        }
        self.fs.create(&tmp)?;
        attempt_cut(attempt, cuts.create)?;
        self.fs.write(&tmp, bytes)?;
        attempt_cut(attempt, cuts.write)?;
        self.fs.flush(&tmp)?;
        attempt_cut(attempt, cuts.flush)?;
        self.sync_file(&tmp)?;
        attempt_cut(attempt, cuts.sync)?;
        let read_back = self.fs.read(&tmp)?;
        if read_back != bytes {
            return Err(StoreError::CorruptIndex { reason: "segment" });
        }
        validate(&read_back)?;
        attempt_cut(attempt, cuts.validate)?;
        if place.replace {
            self.fs.rename_replace(&tmp, dest)?;
        } else {
            self.fs.rename_new(&tmp, dest)?;
        }
        attempt_cut(attempt, cuts.rename)?;
        self.sync_dir()?;
        attempt_cut(attempt, cuts.dir_sync)?;
        Ok(())
    }

    pub(super) fn stage_pointer(&self, attempt: &mut Attempt<'_, F>, root: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let tmp = tmp_path("pointer.partial");
        let _keep = KeepPath::new(self, &tmp);
        if self.fs.exists(&tmp)? {
            let _ = self.fs.unlink(&tmp);
        }
        self.fs.create(&tmp)?;
        attempt.cut(Boundary::AfterPointerCreate)?;
        self.fs.write(&tmp, bytes)?;
        attempt.cut(Boundary::AfterPointerWrite)?;
        self.fs.flush(&tmp)?;
        attempt.cut(Boundary::AfterPointerFlush)?;
        self.sync_file(&tmp)?;
        attempt.cut(Boundary::AfterPointerSync)?;
        let read_back = self.fs.read(&tmp)?;
        decode_pointer_ok(&read_back)?;
        let _ = inspect(&self.fs, &manifest_path_of_pointer(&read_back)?);
        self.fs.rename_replace(&tmp, &current_path(root))?;
        attempt.cut(Boundary::AfterPointerRename)?;
        self.sync_dir()?;
        attempt.cut(Boundary::AfterPointerDirSync)?;
        Ok(())
    }
}

fn attempt_cut<F: StoreFs>(attempt: &Attempt<'_, F>, boundary: Boundary) -> Result<(), StoreError> {
    // `cut` needs `&mut Attempt`. Stage holds a shared attempt during segment cuts.
    if let Err(err) = attempt.store.fs.checkpoint(boundary) {
        if matches!(err, StoreError::Crashed { .. }) {
            attempt.store.poison();
        }
        return Err(err);
    }
    if attempt.store.take_cancel(boundary) {
        return Err(StoreError::Cancelled);
    }
    Ok(())
}

fn manifest_path_of_pointer(bytes: &[u8]) -> Result<String, StoreError> {
    let pointer = crate::domain::decode_pointer(bytes)?;
    Ok(manifest_path(&pointer.digest))
}
