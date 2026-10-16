//! Confined source session: rules, inventory, authorized read, and validation.
//! Implements ARCHITECTURE-PROPOSAL §7 DX-B3 and decision 078.
//! Depends on [`super::rules`], [`super::walk`], and `dctx-hash`. Engines use [`FsSource`].

use super::error::SourceError;
use super::locate::{self, Located};
use super::read::{ReadAttempt, file_stamp, read_authorized};
use super::rules::load_policy;
use super::std_fs::StdFs;
use super::walk;
use crate::domain::{
    AuthorizedFile, FileHandlingDecision, FreshnessMode, RecordedIdentity, SelectionPolicy,
    SourceFile, StaleReason, Validation,
};
use crate::ports::{FsKind, SourceFs};
use std::{fs, io, path::PathBuf};

/// Options for one source root. Global rules are opt-in; nothing is read from the home directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOptions {
    /// Directory that holds plain `ignore` and `omitcontent` files, when the caller wants them.
    pub global_dir: Option<PathBuf>,
    /// Ignored paths stay closed and may be listed as metadata only.
    pub debug_ignored: bool,
    /// `Strict` hashes included files. `Metadata` stops at the pre-filter.
    pub freshness: FreshnessMode,
}

impl Default for SourceOptions {
    fn default() -> Self {
        Self {
            global_dir: None,
            debug_ignored: false,
            freshness: FreshnessMode::Strict,
        }
    }
}

/// A filesystem source rooted at one absolute directory.
pub struct FsSource<F> {
    root: PathBuf,
    policy: SelectionPolicy,
    fs: F,
    debug: bool,
    mode: FreshnessMode,
}

impl FsSource<StdFs> {
    /// Canonicalizes `root` and loads rules from the process filesystem.
    pub fn open(root: &std::path::Path, options: SourceOptions) -> Result<Self, SourceError> {
        let root = fs::canonicalize(root).map_err(|source| SourceError::io(root, source))?;
        Self::load(root, options, StdFs)
    }
}

impl<F: SourceFs> FsSource<F> {
    /// Loads rules through `fs`. `root` must already be absolute.
    pub fn load(root: PathBuf, options: SourceOptions, fs: F) -> Result<Self, SourceError> {
        if !root.is_absolute() {
            return Err(SourceError::InvalidPath { path: root });
        }
        let meta = fs
            .metadata(&root)
            .map_err(|source| SourceError::io(&root, source))?;
        if meta.kind != FsKind::Directory {
            return Err(SourceError::InvalidPath { path: root });
        }
        let policy = load_policy(&root, options.global_dir.as_deref(), &fs)?;
        Ok(Self {
            root,
            policy,
            fs,
            debug: options.debug_ignored,
            mode: options.freshness,
        })
    }

    /// Filesystem used for this session, including a spy wrapper.
    pub fn filesystem(&self) -> &F {
        &self.fs
    }

    /// Lists visible files. Excluded paths are not opened. Escapes fail the inventory.
    pub fn inventory(&self) -> Result<Vec<SourceFile>, SourceError> {
        walk::inventory(&self.root, &self.policy, &self.fs, self.debug, self.mode)
    }

    /// Reads one included file. A race returns [`SourceError::Stale`] and no bytes.
    pub fn read(&self, relative: &str) -> Result<AuthorizedFile, SourceError> {
        let resolved = self.included(relative)?;
        match read_authorized(&self.fs, &resolved.disk)? {
            ReadAttempt::Stale(reason) => Err(SourceError::Stale {
                path: resolved.disk,
                reason,
            }),
            ReadAttempt::Stable(stable) => Ok(AuthorizedFile {
                relative: resolved.relative,
                stamp: stable.stamp,
                digest: stable.digest,
                bytes: stable.bytes,
            }),
        }
    }

    /// Checks a recorded identity. Metadata mode does not open the file.
    /// Strict mode hashes with `dctx-hash` and reports a same-stamp byte change as a mismatch.
    pub fn validate(
        &self,
        relative: &str,
        previous: &RecordedIdentity,
        mode: FreshnessMode,
    ) -> Result<Validation, SourceError> {
        let resolved = match self.locate(relative)? {
            Located::Missing => {
                return Ok(Validation::Stale {
                    reason: StaleReason::Missing,
                });
            }
            Located::Denied { relative } => return Err(SourceError::Denied { relative }),
            Located::File(resolved) if resolved.decision == FileHandlingDecision::Include => {
                resolved
            }
            Located::File(resolved) => {
                return Err(SourceError::Denied {
                    relative: resolved.relative,
                });
            }
        };
        match mode {
            FreshnessMode::Metadata => self.validate_metadata(&resolved.disk, previous),
            FreshnessMode::Strict => self.validate_strict(&resolved.disk, previous),
        }
    }

    fn included(&self, relative: &str) -> Result<locate::Resolved, SourceError> {
        match self.locate(relative)? {
            Located::Missing => Err(SourceError::io(
                self.root.join(relative),
                io::Error::new(io::ErrorKind::NotFound, "missing source file"),
            )),
            Located::Denied { relative } => Err(SourceError::Denied { relative }),
            Located::File(resolved) if resolved.decision.should_read_content() => Ok(resolved),
            Located::File(resolved) => Err(SourceError::Denied {
                relative: resolved.relative,
            }),
        }
    }

    fn locate(&self, relative: &str) -> Result<Located, SourceError> {
        locate::locate(&self.root, &self.policy, &self.fs, relative, self.debug)
    }

    fn validate_metadata(
        &self,
        path: &std::path::Path,
        previous: &RecordedIdentity,
    ) -> Result<Validation, SourceError> {
        let stamp = match file_stamp(&self.fs, path) {
            Ok(stamp) => stamp,
            Err(SourceError::Io { ref source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(Validation::Stale {
                    reason: StaleReason::Missing,
                });
            }
            Err(error) => return Err(error),
        };
        if stamp.metadata_matches(&previous.stamp) {
            Ok(Validation::MetadataMatch { stamp })
        } else {
            Ok(Validation::MetadataChanged { stamp })
        }
    }

    fn validate_strict(
        &self,
        path: &std::path::Path,
        previous: &RecordedIdentity,
    ) -> Result<Validation, SourceError> {
        match read_authorized(&self.fs, path)? {
            ReadAttempt::Stale(reason) => Ok(Validation::Stale { reason }),
            ReadAttempt::Stable(stable) if stable.digest == previous.digest => {
                Ok(Validation::DigestMatch {
                    stamp: stable.stamp,
                    digest: stable.digest,
                })
            }
            ReadAttempt::Stable(stable) => Ok(Validation::DigestMismatch {
                stamp: stable.stamp,
                digest: stable.digest,
            }),
        }
    }
}
