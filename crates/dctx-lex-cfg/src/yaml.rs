//! YAML block mappings, sequences, flow collections, anchors and multi-document files.
//! Implements the configuration extractor for `.yaml` and `.yml` (PARSERS-v1).
//! Anchors and aliases are recorded and never expanded. The draft in `path` owns facts.
use crate::domain::{Dialect, LexError, LexedFile, Limits};
use crate::path::{Cur, Draft};

mod block;
mod flow;
mod text;

pub(crate) fn extract(path: &str, source: &str, limits: &Limits) -> Result<LexedFile, LexError> {
    let mut parser = Parser {
        cur: Cur::new(source),
        draft: Draft::open(path, source, limits.max_facts, limits.max_depth)?,
        max_depth: limits.max_depth,
        broke: false,
    };
    parser.walk()?;
    parser.draft.finish(Dialect::Yaml)
}

struct Parser<'a> {
    cur: Cur<'a>,
    draft: Draft<'a>,
    max_depth: u32,
    broke: bool,
}

#[cfg(test)]
mod tests;
