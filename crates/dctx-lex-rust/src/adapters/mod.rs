//! Extraction pipeline: bound, decode, lex, check nesting, parse with syn, else line scan.
//! Implements PARSERS-v1 "Refuse oversized input before allocating an AST" and C05.
//! Uses proc-macro2/syn on a dedicated bounded-stack worker thread per file, so the
//! per-thread span map is released and deep input cannot exhaust the caller's stack.
mod nesting;
mod spans;
mod syntax;
mod syntax_parts;
mod syntax_refs;

use crate::domain::line_scan;
use crate::{Extraction, Limits, Refusal};
use std::borrow::Cow;

/// Stack reserved for the parse worker; with `Limits::CEILING` nesting this leaves
/// a wide margin for syn's recursive descent, visitor and drop in debug builds.
const WORKER_STACK_BYTES: usize = 128 << 20;

/// Extracts file-local facts from one authorized Rust file.
///
/// Oversized and non-UTF-8 input is refused with no facts. Lexing, nesting and
/// syntax refusals return `Completion::Partial` with line-scan facts labelled
/// `Precision::LineScan`. Never panics on malformed input.
///
/// # Errors
/// Returns [`Refusal::TooLarge`] for input beyond the byte limit or
/// [`Refusal::InvalidUtf8`] for invalid UTF-8. Other refusals accompany partial facts.
pub fn extract(bytes: &[u8], limits: &Limits) -> Result<Extraction, Refusal> {
    if bytes.len() > limits.max_bytes() {
        return Err(Refusal::TooLarge {
            bytes: bytes.len(),
            limit: limits.max_bytes(),
        });
    }
    let source = std::str::from_utf8(bytes).map_err(|e| Refusal::InvalidUtf8 {
        valid_up_to: e.valid_up_to(),
    })?;
    let source = blank_preamble(source);
    match on_worker(&source, limits) {
        Ok(facts) => Ok(facts),
        Err(refusal) => Ok(line_scan::extract(&source, limits, refusal)),
    }
}

fn on_worker(source: &str, limits: &Limits) -> Result<Extraction, Refusal> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("dctx-lex-rust".into())
            .stack_size(WORKER_STACK_BYTES)
            .spawn_scoped(scope, || syntax::extract(source, limits))
            .map_err(|_| Refusal::Worker)?
            .join()
            .map_err(|_| Refusal::Worker)?
    })
}

/// Replaces a byte-order mark and a shebang line with spaces of equal byte length,
/// so spans stay offsets into the caller's bytes.
fn blank_preamble(source: &str) -> Cow<'_, str> {
    let bom = source.starts_with('\u{feff}').then_some('\u{feff}'.len_utf8());
    let body = &source[bom.unwrap_or(0)..];
    let shebang = body.starts_with("#!") && !body[2..].trim_start().starts_with('[');
    if bom.is_none() && !shebang {
        return Cow::Borrowed(source);
    }
    let blank_to = bom.unwrap_or(0) + if shebang { body.find('\n').unwrap_or(body.len()) } else { 0 };
    let mut owned = " ".repeat(blank_to);
    owned.push_str(&source[blank_to..]);
    Cow::Owned(owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Completion, Coverage};

    #[test]
    fn preamble_is_blanked_without_moving_offsets() {
        let source = "\u{feff}#!/usr/bin/env run\nfn main() {}";
        let blanked = blank_preamble(source);
        assert_eq!(blanked.len(), source.len());
        assert_eq!(blanked.find("fn main"), source.find("fn main"));
        assert!(matches!(blank_preamble("#![forbid(unsafe_code)]"), Cow::Borrowed(_)));
    }

    #[test]
    fn refusals_without_facts_have_positive_controls() {
        let tight = Limits::narrowed(4, 64, 100);
        assert_eq!(
            extract(b"fn a() {}", &tight),
            Err(Refusal::TooLarge { bytes: 9, limit: 4 })
        );
        assert!(extract(b"fn a", &tight).is_ok());
        assert_eq!(
            extract(b"fn \xff", &Limits::default()),
            Err(Refusal::InvalidUtf8 { valid_up_to: 3 })
        );
        let facts = extract(b"fn a() {}", &Limits::default());
        assert_eq!(facts.map(|f| (f.completion, f.coverage)), Ok((Completion::Complete, Coverage::Syntax)));
    }

    #[test]
    fn shebang_files_keep_byte_spans() {
        let source = "#!/usr/bin/env run\nfn main() {}\n";
        let facts = extract(source.as_bytes(), &Limits::default()).unwrap_or_else(|e| panic!("{e}"));
        let main = &facts.declarations[0];
        assert_eq!(&source[main.span.start..main.span.end], "fn main() {}");
        assert_eq!(main.span.lines, [2, 2]);
    }
}
