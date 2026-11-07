// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
//! Checked mmap reader and streaming writer; no source file content is stored.

use memmap2::Mmap;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::Path;

const HEADER: usize = 64;
const MAGIC: &[u8; 8] = b"DCTXCAT1";

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Extension-based routing hint, deliberately obtained without reading content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Tier {
    /// Recognized programming-language extension.
    Code = 0,
    /// Markdown extension.
    Markdown = 1,
    /// Unrecognized extension, including extensionless files.
    Other = 2,
    /// Recognized binary/media/archive extension; not content sniffing.
    Binary = 3,
}

impl Tier {
    pub(crate) fn from_name(name: &[u8]) -> Self {
        let extension = name.rsplit(|&b| b == b'.').next().unwrap_or_default();
        let extension = extension.to_ascii_lowercase();
        match extension.as_slice() {
            b"md" | b"markdown" | b"mdx" => Self::Markdown,
            b"rs" | b"c" | b"h" | b"cpp" | b"hpp" | b"cc" | b"py" | b"js" | b"jsx" | b"ts"
            | b"tsx" | b"go" | b"java" | b"kt" | b"swift" | b"rb" | b"sh" | b"cs" | b"lua"
            | b"php" | b"scala" | b"zig" => Self::Code,
            b"png" | b"jpg" | b"jpeg" | b"gif" | b"webp" | b"ico" | b"pdf" | b"zip" | b"gz"
            | b"xz" | b"zst" | b"tar" | b"7z" | b"exe" | b"dll" | b"so" | b"a" | b"o" | b"wasm"
            | b"mp4" | b"mp3" | b"woff" | b"woff2" | b"ttf" | b"sqlite" | b"db" | b"bin" => {
                Self::Binary
            }
            _ => Self::Other,
        }
    }
}

/// Unix stat fields retained without loss, including device as part of file identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    pub size: u64,
    pub mtime_seconds: i64,
    pub mtime_nanos: u32,
    pub inode: u64,
    pub device: u64,
    pub tier: Tier,
}

/// Root-relative raw path and its metadata. Symlinks are excluded by the walker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub path: Vec<u8>,
    pub metadata: Metadata,
}

fn put(out: &mut impl Write, mut value: u64) -> io::Result<()> {
    let mut encoded = [0; 10];
    let mut len = 0;
    loop {
        encoded[len] = u8::try_from(value & 0x7f).expect("seven bits");
        value >>= 7;
        len += 1;
        if value == 0 {
            break;
        }
        encoded[len - 1] |= 0x80;
    }
    out.write_all(&encoded[..len])
}

fn get(bytes: &[u8], cursor: &mut usize) -> io::Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| invalid("truncated varint"))?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(invalid("varint overflow"));
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            if shift > 0 && byte == 0 {
                return Err(invalid("noncanonical varint"));
            }
            return Ok(value);
        }
    }
    Err(invalid("varint overflow"))
}

fn number(bytes: &[u8], cursor: &mut usize) -> io::Result<usize> {
    usize::try_from(get(bytes, cursor)?).map_err(|_| invalid("integer too large"))
}

fn name(bytes: &[u8], cursor: &mut usize) -> io::Result<Range<usize>> {
    let len = number(bytes, cursor)?;
    let end = cursor
        .checked_add(len)
        .ok_or_else(|| invalid("name overflow"))?;
    let range = *cursor..end;
    let value = bytes
        .get(range.clone())
        .ok_or_else(|| invalid("truncated name"))?;
    if value.contains(&0) || value.contains(&b'/') || value == b"." || value == b".." {
        return Err(invalid("invalid path component"));
    }
    *cursor = end;
    Ok(range)
}

fn parent(path: &[u8]) -> (&[u8], &[u8]) {
    match path.iter().rposition(|&b| b == b'/') {
        Some(at) => (&path[..at], &path[at + 1..]),
        None => (&[], path),
    }
}

fn zigzag(value: i64) -> u64 {
    u64::from_ne_bytes(((value << 1) ^ (value >> 63)).to_ne_bytes())
}

fn unzigzag(value: u64) -> i64 {
    i64::from_ne_bytes((value >> 1).to_ne_bytes()) ^ -i64::try_from(value & 1).unwrap()
}

fn valid_component(value: &[u8]) -> bool {
    !value.is_empty() && value != b"." && value != b".." && !value.contains(&0)
}

/// Write a new catalog, refusing to overwrite an existing file.
///
/// `directories` contains root-relative directory bytes and git-marker flags,
/// including the empty root. Files are sorted in place. No file content is read.
/// The caller must remove an incomplete output if an I/O error occurs.
///
/// # Errors
/// Returns I/O errors, malformed input errors, or an error above format limits.
#[allow(
    clippy::too_many_lines,
    reason = "keep section layout and streaming publication together"
)]
#[allow(
    clippy::missing_panics_doc,
    reason = "usize fits u64 on supported Unix targets; conversions are invariant checks"
)]
pub fn write_catalog(
    output: &Path,
    entries: &mut [Entry],
    directories: &mut [(Vec<u8>, bool)],
) -> io::Result<()> {
    directories.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    if directories.first().map(|d| d.0.as_slice()) != Some(b"") {
        return Err(invalid("missing root directory"));
    }
    let mut ids = BTreeMap::<&[u8], usize>::new();
    let mut directory_bytes = Vec::new();
    let mut git_roots = 0_u64;
    for (id, (path, git)) in directories.iter().enumerate() {
        let (prefix, suffix) = parent(path);
        if id != 0 && (!valid_component(suffix) || path.starts_with(b"/")) {
            return Err(invalid("invalid directory"));
        }
        let parent_id = if id == 0 {
            0
        } else {
            *ids.get(prefix)
                .ok_or_else(|| invalid("missing directory parent"))?
        };
        if ids.insert(path, id).is_some() {
            return Err(invalid("duplicate directory"));
        }
        put(&mut directory_bytes, u64::try_from(parent_id).unwrap())?;
        directory_bytes.push(u8::from(*git));
        put(&mut directory_bytes, u64::try_from(suffix.len()).unwrap())?;
        directory_bytes.extend_from_slice(suffix);
        git_roots += u64::from(*git);
    }
    let index_start = HEADER + directory_bytes.len();
    let records_start = index_start
        .checked_add(
            entries
                .len()
                .checked_add(1)
                .and_then(|n| n.checked_mul(4))
                .ok_or_else(|| invalid("too many records"))?,
        )
        .ok_or_else(|| invalid("catalog overflow"))?;
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)?;
    let mut out = BufWriter::new(file);
    out.write_all(&[0; HEADER])?;
    out.write_all(&directory_bytes)?;
    out.seek(SeekFrom::Start(u64::try_from(records_start).unwrap()))?;
    let mut offsets = Vec::with_capacity(entries.len() + 1);
    let mut encoded = Vec::new();
    let mut offset = 0_u32;
    let mut previous: Option<&[u8]> = None;
    for entry in entries.iter() {
        if previous == Some(entry.path.as_slice()) {
            return Err(invalid("duplicate file"));
        }
        previous = Some(&entry.path);
        let (prefix, suffix) = parent(&entry.path);
        if !valid_component(suffix) || entry.path.starts_with(b"/") {
            return Err(invalid("invalid file path"));
        }
        let id = *ids
            .get(prefix)
            .ok_or_else(|| invalid("missing file parent"))?;
        let metadata = entry.metadata;
        if metadata.mtime_nanos >= 1_000_000_000 {
            return Err(invalid("invalid mtime nanoseconds"));
        }
        offsets.push(offset);
        encoded.clear();
        put(&mut encoded, u64::try_from(id).unwrap())?;
        put(&mut encoded, u64::try_from(suffix.len()).unwrap())?;
        encoded.extend_from_slice(suffix);
        put(&mut encoded, metadata.size)?;
        put(&mut encoded, zigzag(metadata.mtime_seconds))?;
        put(&mut encoded, u64::from(metadata.mtime_nanos))?;
        put(&mut encoded, metadata.inode)?;
        put(&mut encoded, metadata.device)?;
        encoded.push(metadata.tier as u8);
        offset = offset
            .checked_add(u32::try_from(encoded.len()).map_err(|_| invalid("record too large"))?)
            .ok_or_else(|| invalid("record section exceeds 4 GiB"))?;
        out.write_all(&encoded)?;
    }
    offsets.push(offset);
    let end = out.stream_position()?;
    out.seek(SeekFrom::Start(u64::try_from(index_start).unwrap()))?;
    for offset in offsets {
        out.write_all(&offset.to_le_bytes())?;
    }
    out.seek(SeekFrom::Start(0))?;
    out.write_all(MAGIC)?;
    for value in [
        u64::try_from(entries.len()).unwrap(),
        u64::try_from(directories.len()).unwrap(),
        git_roots,
        u64::try_from(index_start).unwrap(),
        u64::try_from(records_start).unwrap(),
        end,
        0,
    ] {
        out.write_all(&value.to_le_bytes())?;
    }
    out.flush()?;
    out.get_ref().sync_all()
}

struct Directory {
    parent: usize,
    name: Range<usize>,
    shard: usize,
    in_git: bool,
}

/// Validated mmap and a small directory table. Does not load the full file list.
pub struct Catalog {
    map: Mmap,
    directories: Vec<Directory>,
    files: usize,
    index_start: usize,
    records_start: usize,
    git_roots: usize,
    shards: usize,
}

impl Catalog {
    /// Open and validate every record before accepting queries.
    ///
    /// The backing file must not be modified/truncated while mapped. The benchmark
    /// owns its outputs and never mutates them after publication.
    ///
    /// # Errors
    /// Returns I/O errors or rejects malformed, truncated, or unsupported catalogs.
    #[allow(
        clippy::missing_panics_doc,
        reason = "fixed header slices are bounded by the preceding header-length check"
    )]
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        // SAFETY: read-only mapping; callers must keep the file immutable as documented.
        let map = unsafe { Mmap::map(&file)? };
        if map.get(..8) != Some(MAGIC) || map.len() < HEADER {
            return Err(invalid("bad catalog magic/header"));
        }
        let field = |at| -> io::Result<usize> {
            usize::try_from(u64::from_le_bytes(map[at..at + 8].try_into().unwrap()))
                .map_err(|_| invalid("header overflow"))
        };
        let files = field(8)?;
        let dirs = field(16)?;
        let git_roots = field(24)?;
        let index_start = field(32)?;
        let records_start = field(40)?;
        if field(48)? != map.len()
            || field(56)? != 0
            || index_start < HEADER
            || records_start > map.len()
            || dirs == 0
            || dirs > index_start.saturating_sub(HEADER) / 3
            || index_start.checked_add(
                files
                    .checked_add(1)
                    .and_then(|n| n.checked_mul(4))
                    .ok_or_else(|| invalid("index overflow"))?,
            ) != Some(records_start)
        {
            return Err(invalid("invalid section boundaries/counts"));
        }
        let mut catalog = Self {
            map,
            directories: Vec::new(),
            files,
            index_start,
            records_start,
            git_roots,
            shards: 0,
        };
        let mut cursor = HEADER;
        let mut found_git = 0;
        let mut shard_keys = BTreeSet::new();
        for id in 0..dirs {
            let bytes = &catalog.map[..index_start];
            let parent = number(bytes, &mut cursor)?;
            let flag = *bytes
                .get(cursor)
                .ok_or_else(|| invalid("missing directory flags"))?;
            cursor += 1;
            let name = name(bytes, &mut cursor)?;
            if flag > 1
                || (id == 0 && (parent != 0 || !name.is_empty()))
                || (id > 0 && (parent >= id || name.is_empty()))
            {
                return Err(invalid("invalid directory parent/name/flags"));
            }
            let git = flag == 1;
            found_git += usize::from(git);
            let in_git = git || (id > 0 && catalog.directories[parent].in_git);
            let shard = if git || id == 0 || (parent == 0 && !in_git) {
                id
            } else {
                catalog.directories[parent].shard
            };
            // Count git roots and top-level fallback shards, even if empty.
            if git || (id > 0 && parent == 0 && !in_git) {
                shard_keys.insert(shard);
            }
            catalog.directories.push(Directory {
                parent,
                name,
                shard,
                in_git,
            });
        }
        if cursor != index_start || found_git != git_roots || catalog.offset(0) != 0 {
            return Err(invalid("invalid directory table or first offset"));
        }
        for id in 0..files {
            let (parent, _, metadata) = catalog.record(id)?;
            if metadata.mtime_nanos >= 1_000_000_000 {
                return Err(invalid("mtime nanoseconds out of range"));
            }
            shard_keys.insert(catalog.directories[parent].shard);
        }
        if catalog.offset(files) != catalog.map.len() - records_start {
            return Err(invalid("trailing record bytes"));
        }
        catalog.shards = shard_keys.len();
        Ok(catalog)
    }

    fn offset(&self, id: usize) -> usize {
        let at = self.index_start + id * 4;
        u32::from_le_bytes(self.map[at..at + 4].try_into().unwrap()) as usize
    }

    fn bytes(&self, id: usize) -> io::Result<&[u8]> {
        if id >= self.files {
            return Err(invalid("file id out of range"));
        }
        let start = self.offset(id);
        let end = self.offset(id + 1);
        self.map[self.records_start..]
            .get(start..end)
            .ok_or_else(|| invalid("invalid record offsets"))
    }

    fn record(&self, id: usize) -> io::Result<(usize, &[u8], Metadata)> {
        let bytes = self.bytes(id)?;
        let mut cursor = 0;
        let parent = number(bytes, &mut cursor)?;
        if parent >= self.directories.len() {
            return Err(invalid("file parent out of range"));
        }
        let range = name(bytes, &mut cursor)?;
        if range.is_empty() {
            return Err(invalid("empty filename"));
        }
        let size = get(bytes, &mut cursor)?;
        let mtime_seconds = unzigzag(get(bytes, &mut cursor)?);
        let mtime_nanos =
            u32::try_from(get(bytes, &mut cursor)?).map_err(|_| invalid("mtime overflow"))?;
        let inode = get(bytes, &mut cursor)?;
        let device = get(bytes, &mut cursor)?;
        let tier = match bytes.get(cursor) {
            Some(0) => Tier::Code,
            Some(1) => Tier::Markdown,
            Some(2) => Tier::Other,
            Some(3) => Tier::Binary,
            _ => return Err(invalid("invalid tier")),
        };
        if cursor + 1 != bytes.len() {
            return Err(invalid("trailing record data"));
        }
        Ok((
            parent,
            &bytes[range],
            Metadata {
                size,
                mtime_seconds,
                mtime_nanos,
                inode,
                device,
                tier,
            },
        ))
    }

    /// Number of regular files in this snapshot.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files
    }
    /// Whether this snapshot has no files.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files == 0
    }
    /// Complete catalog size, including directories and the record-offset table.
    #[must_use]
    pub fn size(&self) -> usize {
        self.map.len()
    }
    /// Git directory/file markers found, without entering or parsing them.
    #[must_use]
    pub fn git_roots(&self) -> usize {
        self.git_roots
    }
    /// Git roots plus top-level/root fallback shards represented by the catalog.
    #[must_use]
    pub fn shards(&self) -> usize {
        self.shards
    }
    /// Get basename bytes by record number.
    /// # Errors
    /// Returns an error if `id` is out of range.
    pub fn basename(&self, id: usize) -> io::Result<&[u8]> {
        let bytes = self.bytes(id)?;
        let mut cursor = 0;
        number(bytes, &mut cursor)?;
        let range = name(bytes, &mut cursor)?;
        Ok(&bytes[range])
    }
    /// Reconstruct one full relative path and its stat fields.
    /// # Errors
    /// Returns an error if `id` is out of range.
    pub fn entry(&self, id: usize) -> io::Result<Entry> {
        let (mut directory, name, metadata) = self.record(id)?;
        let mut components = vec![name];
        while directory != 0 {
            let dir = &self.directories[directory];
            components.push(&self.map[dir.name.clone()]);
            directory = dir.parent;
        }
        components.reverse();
        Ok(Entry {
            path: components.join(&b'/'),
            metadata,
        })
    }
    /// Find all paths with an exact ASCII-case-insensitive basename token.
    /// Tokens are maximal runs of ASCII alphanumerics or non-ASCII bytes; all
    /// punctuation (including `_`, `-`, `.`) separates tokens. No symbol index.
    /// Empty/multiple-token queries return no results. Results are not capped.
    #[must_use]
    #[allow(
        clippy::missing_panics_doc,
        reason = "open validates every immutable mapped record before accepting queries"
    )]
    pub fn find(&self, token: &[u8]) -> Vec<usize> {
        if token.is_empty() || !token.iter().copied().all(token_byte) {
            return Vec::new();
        }
        (0..self.files)
            .filter(|&id| {
                // Entire map was validated once in open(), and remains immutable.
                let bytes = self.basename(id).expect("validated catalog");
                bytes
                    .split(|&b| !token_byte(b))
                    .any(|part| part.eq_ignore_ascii_case(token))
            })
            .collect()
    }
}

fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte >= 0x80
}
