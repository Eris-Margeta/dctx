//! Bounded append-only effect evidence for PLUGINS.md §4.4 and PP3.
//! The host orders intent/effect/completion; this adapter acknowledges only synced frames.
//! Recovery retains unresolved receipt links and refuses torn/corrupt logs without truncation.
//! Storage is injected; the Unix filesystem implementation is gated by the facade's `fs` feature.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use dctx_hash::Sha256;
use dctx_plugin_host::ports::{AuditKind, AuditPort, AuditRecord, AuditUnavailable, MAX_RECOVERY_RECORDS, RecoveryRecord, RecoveryState};
use dctx_plugin_host::{ActionKey, ArgDigest};
use dctx_plugin_protocol::records::scalars::{Digest, PluginId, Token};

const MAX_CAPACITY: usize = 64 * 1024 * 1024;
const MAX_FRAME: usize = 2048;

/// Exclusive storage: bounded snapshot and append acknowledged only after durable synchronization.
pub(crate) trait AuditStorage {
    fn read(&mut self, capacity: usize) -> Result<Vec<u8>, AuditUnavailable>;
    fn append_sync(&mut self, frame: &[u8]) -> Result<(), AuditUnavailable>;
}

/// Plugin, invocation and receipt together identify an intent in this log.
type Link = (String, String, String);

#[derive(Default)]
struct Evidence {
    sequence: u64,
    // Completed links remain reserved, so receipt reuse cannot hide an older intent.
    links: BTreeMap<Link, bool>,
    recovery: Vec<RecoveryRecord>,
    legacy_effects: bool,
}

struct Entry {
    kind: AuditKind,
    link: Option<Link>,
    uncertain: bool,
    recovery: Option<RecoveryRecord>,
}

impl Evidence {
    fn parse(&self, payload: &str) -> Result<Entry, AuditUnavailable> {
        let f: Vec<_> = payload.split('\t').collect();
        if !matches!((f.first().copied(), f.len()), (Some("1"), 12) | (Some("2"), 16))
            || f[1].parse::<u64>().ok() != self.sequence.checked_add(1)
            || PluginId::new(f[3]).is_err()
            || Token::new(f[4]).is_err()
            || f[5].parse::<u64>().is_err()
            || !safe_name(f[6])
            || !safe_name(f[11])
            || (f[7] != "-" && Token::new(f[7]).is_err())
            || (f[8] != "-" && (f[8].len() != 64 || !f[8].bytes().all(|b| b.is_ascii_hexdigit())))
            || f[9].parse::<u64>().is_err()
            || (f[10] != "-" && Token::new(f[10]).is_err())
        {
            return Err(AuditUnavailable);
        }
        let kind = match f[2] {
            "intent" => AuditKind::Intent,
            "completion" => AuditKind::Completion,
            "delivery" => AuditKind::Delivery,
            "refusal" => AuditKind::Refusal,
            "lifecycle" => AuditKind::Lifecycle,
            _ => return Err(AuditUnavailable),
        };
        let link = (f[10] != "-").then(|| (f[3].to_owned(), f[4].to_owned(), f[10].to_owned()));
        if matches!(kind, AuditKind::Intent | AuditKind::Completion) {
            let key = link.as_ref().ok_or(AuditUnavailable)?;
            if (kind == AuditKind::Intent && self.links.contains_key(key))
                || (kind == AuditKind::Completion && self.links.get(key) != Some(&true))
            {
                return Err(AuditUnavailable);
            }
        }
        let recovery = if f[0] == "2" {
            let state = RecoveryState::parse(f[15])?;
            let expected = match state {
                RecoveryState::Pending => AuditKind::Intent,
                RecoveryState::Archived => AuditKind::Lifecycle,
                _ => AuditKind::Completion,
            };
            if kind != expected || self.recovery.len() >= MAX_RECOVERY_RECORDS {
                return Err(AuditUnavailable);
            }
            Some(RecoveryRecord {
                key: ActionKey {
                    artifact: Digest::new(f[12]).map_err(|_| AuditUnavailable)?,
                    root: Token::new(f[13]).map_err(|_| AuditUnavailable)?,
                    invocation: Token::new(f[4]).map_err(|_| AuditUnavailable)?,
                    action_id: Token::new(f[14]).map_err(|_| AuditUnavailable)?,
                },
                args: ArgDigest::from_hex(f[8]).map_err(|_| AuditUnavailable)?,
                receipt: Token::new(f[10]).map_err(|_| AuditUnavailable)?,
                state,
            })
        } else {
            None
        };
        let uncertain =
            recovery.as_ref().map_or(f[11] == "unknown", |r| matches!(r.state, RecoveryState::Unknown | RecoveryState::Partial));
        Ok(Entry { kind, link, uncertain, recovery })
    }

    fn accept(&mut self, entry: Entry) {
        self.sequence += 1;
        if let Some(record) = entry.recovery {
            self.recovery.push(record);
        } else if matches!(entry.kind, AuditKind::Intent | AuditKind::Completion) {
            self.legacy_effects = true;
        }
        if let Some(link) = entry.link.filter(|_| matches!(entry.kind, AuditKind::Intent | AuditKind::Completion)) {
            self.links.insert(link, entry.kind == AuditKind::Intent || entry.uncertain);
        }
    }
}

/// A failed write/sync poisons this writer; reopening revalidates the durable prefix.
pub(crate) struct DurableAudit<S> {
    storage: S,
    capacity: usize,
    used: usize,
    damaged: bool,
    evidence: Evidence,
}

impl<S: AuditStorage> DurableAudit<S> {
    pub(crate) fn new(mut storage: S, capacity: usize) -> Result<Self, AuditUnavailable> {
        if capacity > MAX_CAPACITY {
            return Err(AuditUnavailable);
        }
        let bytes = storage.read(capacity)?;
        if bytes.len() > capacity {
            return Err(AuditUnavailable);
        }
        let mut audit = Self { storage, capacity, used: bytes.len(), damaged: false, evidence: Evidence::default() };
        for frame in bytes.split_inclusive(|b| *b == b'\n') {
            let entry = (|| {
                if frame.len() > MAX_FRAME {
                    return Err(AuditUnavailable);
                }
                let line = std::str::from_utf8(frame.strip_suffix(b"\n").ok_or(AuditUnavailable)?).map_err(|_| AuditUnavailable)?;
                let (digest, payload) = line.split_once('\t').ok_or(AuditUnavailable)?;
                if digest != checksum(payload.as_bytes())? {
                    return Err(AuditUnavailable);
                }
                audit.evidence.parse(payload)
            })();
            match entry {
                Ok(entry) => audit.evidence.accept(entry),
                Err(_) => {
                    audit.damaged = true;
                    break;
                }
            }
        }
        Ok(audit)
    }

    #[cfg(test)]
    pub(crate) fn uncertain(&self) -> Vec<Link> {
        self.evidence.links.iter().filter(|(_, pending)| **pending).map(|(link, _)| link.clone()).collect()
    }
}

impl<S: AuditStorage> DurableAudit<S> {
    fn write(&mut self, r: &AuditRecord, recovery: Option<&RecoveryRecord>) -> Result<(), AuditUnavailable> {
        if self.damaged || !safe_name(r.method) || !safe_name(r.reason) {
            return Err(AuditUnavailable);
        }
        let kind = match r.kind {
            AuditKind::Intent => "intent",
            AuditKind::Completion => "completion",
            AuditKind::Delivery => "delivery",
            AuditKind::Refusal => "refusal",
            AuditKind::Lifecycle => "lifecycle",
        };
        let sequence = self.evidence.sequence.checked_add(1).ok_or(AuditUnavailable)?;
        let args = r.args.map_or_else(|| "-".to_owned(), |d| hex(d.as_bytes()));
        let version = if recovery.is_some() { 2 } else { 1 };
        let mut payload = format!(
            "{version}\t{sequence}\t{kind}\t{}\t{}\t{}\t{}\t{}\t{args}\t{}\t{}\t{}",
            r.plugin.as_str(),
            r.invocation.as_str(),
            r.epoch,
            r.method,
            r.handle.as_ref().map_or("-", Token::as_str),
            r.bytes,
            r.receipt.as_ref().map_or("-", Token::as_str),
            r.reason
        );
        if let Some(bound) = recovery {
            if r.invocation != bound.key.invocation || r.args != Some(bound.args) || r.receipt.as_ref() != Some(&bound.receipt) {
                return Err(AuditUnavailable);
            }
            payload.push_str(&format!(
                "\t{}\t{}\t{}\t{}",
                bound.key.artifact.as_str(),
                bound.key.root.as_str(),
                bound.key.action_id.as_str(),
                bound.state.as_str()
            ));
        }
        let entry = self.evidence.parse(&payload)?;
        let frame = format!("{}\t{payload}\n", checksum(payload.as_bytes())?);
        if frame.len() > MAX_FRAME || frame.len() > self.capacity.saturating_sub(self.used) {
            return Err(AuditUnavailable);
        }
        if self.storage.append_sync(frame.as_bytes()).is_err() {
            self.damaged = true;
            return Err(AuditUnavailable);
        }
        self.used += frame.len();
        self.evidence.accept(entry);
        Ok(())
    }
}

impl<S: AuditStorage> AuditPort for DurableAudit<S> {
    fn append(&mut self, record: &AuditRecord) -> Result<(), AuditUnavailable> {
        self.write(record, None)
    }

    fn append_effect(&mut self, record: &AuditRecord, recovery: &RecoveryRecord) -> Result<(), AuditUnavailable> {
        self.write(record, Some(recovery))
    }

    fn recover(&mut self, limit: usize) -> Result<Vec<RecoveryRecord>, AuditUnavailable> {
        if self.damaged || self.evidence.legacy_effects || self.evidence.recovery.len() > limit.min(MAX_RECOVERY_RECORDS) {
            return Err(AuditUnavailable);
        }
        Ok(self.evidence.recovery.clone())
    }
}

fn safe_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    bytes.iter().flat_map(|b| [char::from(DIGITS[usize::from(b >> 4)]), char::from(DIGITS[usize::from(b & 15)])]).collect()
}

fn checksum(bytes: &[u8]) -> Result<String, AuditUnavailable> {
    Sha256::digest(bytes).map(|digest| hex(digest.as_bytes())).map_err(|_| AuditUnavailable)
}

#[cfg(feature = "fs")]
pub(crate) fn open(directory: &std::path::Path, capacity: usize) -> Result<Box<dyn AuditPort + Send>, AuditUnavailable> {
    if capacity > MAX_CAPACITY {
        return Err(AuditUnavailable);
    }
    #[cfg(unix)]
    {
        Ok(Box::new(DurableAudit::new(disk::Storage::open(directory)?, capacity)?))
    }
    #[cfg(not(unix))]
    {
        let _ = (directory, capacity);
        Err(AuditUnavailable)
    }
}

#[cfg(all(feature = "fs", unix))]
pub(crate) mod disk {
    use super::{AuditStorage, AuditUnavailable};
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;

    pub(crate) struct Storage {
        file: File,
        directory: File,
    }

    impl Storage {
        pub(crate) fn open(directory: &Path) -> Result<Self, AuditUnavailable> {
            Self::open_io(directory).map_err(|_| AuditUnavailable)
        }

        fn open_io(directory: &Path) -> std::io::Result<Self> {
            let denied = || std::io::Error::from(std::io::ErrorKind::PermissionDenied);
            let meta = fs::symlink_metadata(directory)?;
            if !meta.is_dir() || meta.mode() & 0o777 != 0o700 {
                return Err(denied());
            }
            let dir = File::open(directory)?;
            let held = dir.metadata()?;
            if (held.dev(), held.ino()) != (meta.dev(), meta.ino()) {
                return Err(denied());
            }
            let path = directory.join("effects.audit");
            let file = match OpenOptions::new().read(true).append(true).create_new(true).mode(0o600).open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let m = fs::symlink_metadata(&path)?;
                    if !m.is_file() || m.nlink() != 1 || m.uid() != meta.uid() || m.mode() & 0o777 != 0o600 {
                        return Err(denied());
                    }
                    OpenOptions::new().read(true).append(true).open(&path)?
                }
                Err(error) => return Err(error),
            };
            file.try_lock().map_err(|_| denied())?;
            let m = file.metadata()?;
            let named = fs::symlink_metadata(&path)?;
            if !m.is_file()
                || m.nlink() != 1
                || m.uid() != meta.uid()
                || m.mode() & 0o777 != 0o600
                || (m.dev(), m.ino()) != (named.dev(), named.ino())
                || named.file_type().is_symlink()
            {
                return Err(denied());
            }
            file.sync_all()?;
            dir.sync_all()?;
            Ok(Self { file, directory: dir })
        }
    }

    impl AuditStorage for Storage {
        fn read(&mut self, capacity: usize) -> Result<Vec<u8>, AuditUnavailable> {
            let mut bytes = Vec::new();
            Read::by_ref(&mut self.file).take(capacity as u64 + 1).read_to_end(&mut bytes).map_err(|_| AuditUnavailable)?;
            if bytes.len() > capacity {
                return Err(AuditUnavailable);
            }
            Ok(bytes)
        }

        fn append_sync(&mut self, frame: &[u8]) -> Result<(), AuditUnavailable> {
            self.file
                .write_all(frame)
                .and_then(|()| self.file.sync_all())
                .and_then(|()| self.directory.sync_all())
                .map_err(|_| AuditUnavailable)
        }
    }
}
