// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
//! Mmap token dictionary and delta-varint file/token postings.
use crate::Entry;
use crate::format::{invalid, number, put, token_byte};
use std::io;

const HEADER: usize = 24;

#[derive(Clone, Copy)]
struct Occurrence {
    offset: u32,
    len: u32,
    file: u32,
}

fn word(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}
fn push_word(out: &mut Vec<u8>, value: usize) -> io::Result<()> {
    out.extend_from_slice(
        &u32::try_from(value)
            .map_err(|_| invalid("name index exceeds 4 GiB"))?
            .to_le_bytes(),
    );
    Ok(())
}
fn gram(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) << 16 | u32::from(bytes[1]) << 8 | u32::from(bytes[2])
}

fn short_key(bytes: &[u8]) -> u32 {
    match bytes {
        [a] => 0x0100_0000 | u32::from(*a),
        [a, b] => 0x0200_0000 | (u32::from(*a) << 8) | u32::from(*b),
        _ => unreachable!(),
    }
}
fn contains_key(token: &[u8], value: usize) -> bool {
    if value <= 0xff_ffff {
        token.windows(3).any(|w| gram(w) as usize == value)
    } else {
        token.len() < 3 && short_key(token) as usize == value
    }
}
fn key_contains(value: usize, query: &[u8]) -> bool {
    let bytes = u32::try_from(value).expect("u32 gram key").to_be_bytes();
    let data = match bytes[0] {
        0 => &bytes[1..],
        1 => &bytes[3..],
        2 => &bytes[2..],
        _ => return false,
    };
    data.windows(query.len()).any(|w| w == query)
}

/// Thread-local tokens gathered alongside file metadata, without opening source contents.
#[derive(Default)]
pub(crate) struct Prepared {
    pool: Vec<u8>,
    occurrences: Vec<Occurrence>,
}
impl Prepared {
    pub(crate) fn add(&mut self, name: &[u8], file: usize) -> io::Result<()> {
        for token in name.split(|&b| !token_byte(b)).filter(|s| !s.is_empty()) {
            let offset =
                u32::try_from(self.pool.len()).map_err(|_| invalid("token pool overflow"))?;
            let len = u32::try_from(token.len()).map_err(|_| invalid("token length overflow"))?;
            offset
                .checked_add(len)
                .ok_or_else(|| invalid("token pool overflow"))?;
            self.occurrences.push(Occurrence {
                offset,
                len,
                file: u32::try_from(file).map_err(|_| invalid("too many files"))?,
            });
            self.pool.extend(token.iter().map(u8::to_ascii_lowercase));
        }
        Ok(())
    }
    pub(crate) fn reserve(&mut self, locals: impl Iterator<Item = (usize, usize)>) {
        let (mut bytes, mut records) = (0, 0);
        for (b, r) in locals {
            bytes += b;
            records += r;
        }
        self.pool.reserve(bytes);
        self.occurrences.reserve(records);
    }
    pub(crate) fn lengths(&self) -> (usize, usize) {
        (self.pool.len(), self.occurrences.len())
    }
    pub(crate) fn append(&mut self, mut local: Self, file_base: usize) -> io::Result<()> {
        self.pool
            .len()
            .checked_add(local.pool.len())
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| invalid("token pool overflow"))?;
        let base = u32::try_from(self.pool.len()).map_err(|_| invalid("token pool overflow"))?;
        let file_base = u32::try_from(file_base).map_err(|_| invalid("too many files"))?;
        for item in &mut local.occurrences {
            item.offset = item
                .offset
                .checked_add(base)
                .ok_or_else(|| invalid("token pool overflow"))?;
            item.file = item
                .file
                .checked_add(file_base)
                .ok_or_else(|| invalid("too many files"))?;
        }
        self.pool.append(&mut local.pool);
        self.occurrences.append(&mut local.occurrences);
        Ok(())
    }
}

pub(crate) fn prepare(entries: &[Entry]) -> io::Result<Prepared> {
    let threads = std::thread::available_parallelism()?.get().min(16);
    let locals = std::thread::scope(|scope| {
        let chunk_size = entries.len().div_ceil(threads).max(1);
        let handles: Vec<_> = entries
            .chunks(chunk_size)
            .enumerate()
            .map(|(chunk, slice)| {
                scope.spawn(move || -> io::Result<_> {
                    let mut local = Prepared::default();
                    for (id, entry) in slice.iter().enumerate() {
                        local.add(
                            entry.path.rsplit(|&b| b == b'/').next().unwrap(),
                            chunk * chunk_size + id,
                        )?;
                    }
                    Ok(local)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("token worker"))
            .collect::<io::Result<Vec<_>>>()
    })?;
    let mut prepared = Prepared::default();
    prepared.reserve(locals.iter().map(Prepared::lengths));
    for local in locals {
        prepared.append(local, 0)?;
    }
    Ok(prepared)
}

#[allow(
    clippy::too_many_lines,
    reason = "keep index section construction together"
)]
pub(crate) fn build(prepared: Prepared) -> io::Result<Vec<u8>> {
    let Prepared {
        pool,
        mut occurrences,
    } = prepared;
    let key = |o: Occurrence| &pool[o.offset as usize..(o.offset + o.len) as usize];
    occurrences.sort_unstable_by(|a, b| key(*a).cmp(key(*b)));
    let mut dictionary = Vec::new();
    let mut offsets = Vec::new();
    let mut at = 0;
    while at < occurrences.len() {
        let token = key(occurrences[at]);
        let mut end = at + 1;
        while end < occurrences.len() && key(occurrences[end]) == token {
            end += 1;
        }
        occurrences[at..end].sort_unstable_by_key(|o| o.file);
        offsets.push(dictionary.len());
        put(&mut dictionary, token.len() as u64)?;
        dictionary.extend_from_slice(token);
        let count = occurrences[at..end]
            .windows(2)
            .filter(|w| w[0].file != w[1].file)
            .count()
            + 1;
        put(&mut dictionary, count as u64)?;
        let mut previous = 0;
        let mut last = None;
        for o in &occurrences[at..end] {
            if last != Some(o.file) {
                put(&mut dictionary, u64::from(o.file - previous))?;
                previous = o.file;
                last = Some(o.file);
            }
        }
        at = end;
    }
    offsets.push(dictionary.len());
    drop(occurrences);
    drop(pool);
    let tokens = offsets.len() - 1;
    let mut grams = Vec::new();
    for id in 0..tokens {
        let record = &dictionary[offsets[id]..offsets[id + 1]];
        let mut cursor = 0;
        let len = number(record, &mut cursor)?;
        if len < 3 {
            grams.push((u64::from(short_key(&record[cursor..cursor + len])) << 32) | id as u64);
        }
        for window in record[cursor..cursor + len].windows(3) {
            grams.push((u64::from(gram(window)) << 32) | id as u64);
        }
    }
    grams.sort_unstable();
    grams.dedup();
    let mut gram_directory = Vec::new();
    let mut gram_data = Vec::new();
    let mut at = 0;
    while at < grams.len() {
        let value = grams[at] >> 32;
        push_word(&mut gram_directory, value as usize)?;
        push_word(&mut gram_directory, gram_data.len())?;
        let mut previous = 0;
        while at < grams.len() && grams[at] >> 32 == value {
            let id = grams[at] & 0xffff_ffff;
            put(&mut gram_data, id - previous)?;
            previous = id;
            at += 1;
        }
        push_word(&mut gram_directory, gram_data.len())?;
    }
    let dict_start = HEADER + offsets.len() * 4;
    let gram_dir_start = dict_start + dictionary.len();
    let gram_data_start = gram_dir_start + gram_directory.len();
    let total = gram_data_start + gram_data.len();
    let mut out = Vec::with_capacity(total);
    for field in [
        tokens,
        gram_directory.len() / 12,
        dict_start,
        gram_dir_start,
        gram_data_start,
        total,
    ] {
        push_word(&mut out, field)?;
    }
    for offset in offsets {
        push_word(&mut out, offset)?;
    }
    out.extend(dictionary);
    out.extend(gram_directory);
    out.extend(gram_data);
    Ok(out)
}

pub(crate) struct Index<'a> {
    bytes: &'a [u8],
}
impl<'a> Index<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
    fn tokens(&self) -> usize {
        word(self.bytes, 0)
    }
    fn record(&self, id: usize) -> &'a [u8] {
        let base = word(self.bytes, 8);
        &self.bytes[base + word(self.bytes, HEADER + id * 4)
            ..base + word(self.bytes, HEADER + (id + 1) * 4)]
    }
    fn token(&self, id: usize) -> &'a [u8] {
        let record = self.record(id);
        let mut cursor = 0;
        let len = number(record, &mut cursor).expect("validated token");
        &record[cursor..cursor + len]
    }
    fn postings(&self, id: usize) -> &'a [u8] {
        let record = self.record(id);
        let mut cursor = 0;
        let len = number(record, &mut cursor).expect("validated token");
        cursor += len;
        number(record, &mut cursor).expect("validated count");
        &record[cursor..]
    }
    #[allow(
        clippy::too_many_lines,
        reason = "validate all index boundaries before queries"
    )]
    pub(crate) fn validate(&self, files: usize) -> io::Result<()> {
        let b = self.bytes;
        if b.len() < HEADER {
            return Err(invalid("short name index"));
        }
        let tokens = self.tokens();
        let grams = word(b, 4);
        let dict = word(b, 8);
        let gd = word(b, 12);
        let data = word(b, 16);
        if tokens
            .checked_add(1)
            .and_then(|n| n.checked_mul(4))
            .and_then(|n| n.checked_add(HEADER))
            != Some(dict)
            || dict > gd
            || gd > data
            || data > b.len()
            || word(b, 20) != b.len()
            || grams.checked_mul(12).and_then(|n| n.checked_add(gd)) != Some(data)
        {
            return Err(invalid("name index boundaries"));
        }
        if word(b, HEADER) != 0 || word(b, HEADER + tokens * 4) != gd - dict {
            return Err(invalid("dictionary endpoints"));
        }
        let mut previous: &[u8] = &[];
        for id in 0..tokens {
            let start = word(b, HEADER + id * 4);
            let end = word(b, HEADER + (id + 1) * 4);
            let r = b[dict..gd]
                .get(start..end)
                .ok_or_else(|| invalid("token offsets"))?;
            let mut c = 0;
            let len = number(r, &mut c)?;
            let token = r
                .get(c..c.checked_add(len).ok_or_else(|| invalid("token length"))?)
                .ok_or_else(|| invalid("short token"))?;
            if token.is_empty()
                || token <= previous
                || !token
                    .iter()
                    .all(|&v| token_byte(v) && !v.is_ascii_uppercase())
            {
                return Err(invalid("token order/bytes"));
            }
            previous = token;
            c += len;
            let count = number(r, &mut c)?;
            if count == 0 || count > files {
                return Err(invalid("posting count"));
            }
            let mut file = 0usize;
            for n in 0..count {
                let delta = number(r, &mut c)?;
                file = file
                    .checked_add(delta)
                    .ok_or_else(|| invalid("posting overflow"))?;
                if (n > 0 && delta == 0) || file >= files {
                    return Err(invalid("posting order/bounds"));
                }
            }
            if c != r.len() {
                return Err(invalid("trailing posting bytes"));
            }
        }
        let mut last_end = 0;
        let mut last_gram = None;
        for id in 0..grams {
            let at = gd + id * 12;
            let value = word(b, at);
            let start = word(b, at + 4);
            let end = word(b, at + 8);
            if value > 0x0200_ffff
                || last_gram.is_some_and(|v| v >= value)
                || start != last_end
                || start >= end
            {
                return Err(invalid("gram directory"));
            }
            let r = b[data..]
                .get(start..end)
                .ok_or_else(|| invalid("gram offsets"))?;
            let mut c = 0;
            let mut token = 0usize;
            while c < r.len() {
                let first = c == 0;
                let delta = number(r, &mut c)?;
                token = token
                    .checked_add(delta)
                    .ok_or_else(|| invalid("gram posting overflow"))?;
                if (!first && delta == 0)
                    || token >= tokens
                    || !contains_key(self.token(token), value)
                {
                    return Err(invalid("gram posting"));
                }
            }
            last_gram = Some(value);
            last_end = end;
        }
        if last_end != b.len() - data {
            return Err(invalid("trailing gram bytes"));
        }
        Ok(())
    }
    fn lower_bound(&self, token: &[u8]) -> usize {
        let (mut lo, mut hi) = (0, self.tokens());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.token(mid) < token {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
    #[allow(
        clippy::too_many_lines,
        reason = "keep query selection and lazy posting consumption together"
    )]
    pub(crate) fn find(&self, query: &[u8], mode: super::QueryMode, limit: usize) -> Vec<usize> {
        use super::QueryMode;
        if query.is_empty() || !query.iter().copied().all(token_byte) || limit == 0 {
            return Vec::new();
        }
        let query = query.to_ascii_lowercase();
        let mut found = std::collections::HashSet::new();
        let mut result = Vec::new();
        let mut emit = |id| {
            let bytes = self.postings(id);
            let (mut c, mut file) = (0, 0);
            while c < bytes.len() {
                file += number(bytes, &mut c).expect("validated posting");
                if matches!(mode, QueryMode::Exact) || found.insert(file) {
                    result.push(file);
                    if result.len() == limit {
                        return true;
                    }
                }
            }
            false
        };
        match mode {
            QueryMode::Exact | QueryMode::Prefix => {
                let mut id = self.lower_bound(&query);
                while id < self.tokens()
                    && match mode {
                        QueryMode::Exact => self.token(id) == query,
                        _ => self.token(id).starts_with(&query),
                    }
                {
                    if emit(id) {
                        break;
                    }
                    id += 1;
                }
            }
            QueryMode::Substring => {
                let mut best: Option<&[u8]> = None;
                for window in query.windows(3) {
                    let value = gram(window) as usize;
                    let (mut lo, mut hi) = (0, word(self.bytes, 4));
                    let base = word(self.bytes, 12);
                    while lo < hi {
                        let mid = lo + (hi - lo) / 2;
                        if word(self.bytes, base + mid * 12) < value {
                            lo = mid + 1;
                        } else {
                            hi = mid;
                        }
                    }
                    if lo == word(self.bytes, 4) || word(self.bytes, base + lo * 12) != value {
                        return Vec::new();
                    }
                    let at = base + lo * 12;
                    let data = word(self.bytes, 16);
                    let postings = &self.bytes
                        [data + word(self.bytes, at + 4)..data + word(self.bytes, at + 8)];
                    if best.is_none_or(|b| postings.len() < b.len()) {
                        best = Some(postings);
                    }
                }
                if let Some(bytes) = best {
                    let (mut c, mut id) = (0, 0);
                    while c < bytes.len() {
                        id += number(bytes, &mut c).expect("validated gram");
                        if self.token(id).windows(query.len()).any(|w| w == query) && emit(id) {
                            break;
                        }
                    }
                } else {
                    // Merge matching trigram lists, including singleton keys for
                    // tokens shorter than three bytes. No full dictionary scan.
                    use std::cmp::Reverse;
                    use std::collections::BinaryHeap;
                    let mut heap = BinaryHeap::new();
                    let base = word(self.bytes, 12);
                    let data = word(self.bytes, 16);
                    for i in 0..word(self.bytes, 4) {
                        let at = base + i * 12;
                        if key_contains(word(self.bytes, at), &query) {
                            let bytes = &self.bytes
                                [data + word(self.bytes, at + 4)..data + word(self.bytes, at + 8)];
                            let mut c = 0;
                            let id = number(bytes, &mut c).expect("validated gram");
                            heap.push(Reverse((id, c, i)));
                        }
                    }
                    let mut previous = None;
                    while let Some(Reverse((id, mut c, i))) = heap.pop() {
                        if previous != Some(id) && emit(id) {
                            break;
                        }
                        previous = Some(id);
                        let at = base + i * 12;
                        let bytes = &self.bytes
                            [data + word(self.bytes, at + 4)..data + word(self.bytes, at + 8)];
                        if c < bytes.len() {
                            let next = id + number(bytes, &mut c).expect("validated gram");
                            heap.push(Reverse((next, c, i)));
                        }
                    }
                }
            }
        }
        result
    }
}
