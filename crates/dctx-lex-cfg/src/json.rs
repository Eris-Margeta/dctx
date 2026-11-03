// SPDX-License-Identifier: Apache-2.0
//! JSON and JSONC structural walk.
//! Implements the configuration extractor for `.json` and `.jsonc` (PARSERS-v1).
//! Comments are summaries in JSONC and diagnostics in JSON. Object trailing
//! commas are accepted; array trailing commas are JSONC-only. `path` owns facts.
//! Unescaped strings are borrowed from the source; escapes decode in one pass.
#[cfg(test)]
use std::fmt::Write as _;
use crate::domain::{DiagnosticReason, Dialect, LexError, LexedFile, Limits};
use crate::path::{Cur, Draft, Kind};
use std::borrow::Cow;

mod grammar;
mod scan;
mod strings;

pub(crate) fn extract(
    path: &str,
    source: &str,
    jsonc: bool,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    walk(path, source, jsonc, limits, false)
}

#[cfg(test)]
pub(crate) fn extract_oracle(
    path: &str,
    source: &str,
    jsonc: bool,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    walk(path, source, jsonc, limits, true)
}

fn walk(
    path: &str,
    source: &str,
    jsonc: bool,
    limits: &Limits,
    legacy: bool,
) -> Result<LexedFile, LexError> {
    let _ = legacy; // Only test builds enable the legacy string-decoder oracle.
    let mut parser = Parser {
        cur: Cur::new(source),
        draft: Draft::open(path, source, limits.max_facts, limits.max_depth)?,
        jsonc,
        max_depth: limits.max_depth,
        #[cfg(test)]
        legacy_strings: legacy,
    };
    parser.skip();
    if !parser.cur.eof() {
        match parser.cur.byte() {
            Some(b'{') => parser.object(Some(0), 1)?,
            Some(b'[') => parser.array(Some(0), 1)?,
            _ => parser.scalar(None, 1)?,
        }
        let _ = parser.skip();
        if !parser.cur.eof() {
            parser.diagnose(parser.cur.i, DiagnosticReason::InvalidSyntax);
        }
    }
    parser.draft.finish(if jsonc { Dialect::Jsonc } else { Dialect::Json })
}

struct Parser<'a> {
    cur: Cur<'a>,
    draft: Draft<'a>,
    jsonc: bool,
    max_depth: u32,
    /// Test builds keep the pre-change per-character string decoder as an oracle.
    #[cfg(test)]
    legacy_strings: bool,
}

#[cfg(test)]
mod tests;
