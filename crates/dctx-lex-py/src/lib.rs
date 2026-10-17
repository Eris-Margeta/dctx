//! Hand-written, zero-dependency Python structural extractor.
//! Implements decision 077, PARSERS-v1 and SPEC-MAP-MODE 5–6.2 for Python 3.
//! Depends on dctx-map-model facts and dctx-hash; never on tree-sitter.
//! Emits the dctx-map-parse fact shape with a precision label on every fact.
//!
//! Domain, lexer and walker share this file: the packet owns one library path.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod domain {
    use dctx_hash::Sha256;
    use dctx_map_model::{Edge, EdgeKind, Node, StableId};
    use std::fmt;

    /// Source dialect selected by the final path extension.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Dialect {
        /// `.py` and `.pyw`.
        Python,
        /// `.pyi` stub.
        Stub,
    }

    impl Dialect {
        /// Selects a dialect case-insensitively; other extensions are unsupported.
        pub fn for_path(path: &str) -> Option<Self> {
            let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
            let ext = file.rsplit_once('.')?.1.to_ascii_lowercase();
            match ext.as_str() {
                "py" | "pyw" => Some(Self::Python),
                "pyi" => Some(Self::Stub),
                _ => None,
            }
        }
    }

    /// How much syntactic evidence stands behind one fact.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Precision {
        /// A complete dotted name or declaration in balanced, terminated syntax.
        Structural,
        /// A module-level constant recognised by the `ALL_CAPS` convention.
        Convention,
        /// The fact ends after the first damaged span; treat it as a hint.
        Partial,
    }

    /// A fact together with its precision label.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Labeled<T> {
        /// The extracted fact.
        pub fact: T,
        /// Evidence behind the fact.
        pub precision: Precision,
    }

    /// A syntactic reference whose destination has not been resolved.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RawReference {
        /// Source node, including its duplicate ordinal when needed.
        pub src: StableId,
        /// `imports` or `calls`. Never `contains`.
        pub kind: EdgeKind,
        /// Referenced spelling (`pkg.mod`, `pkg::name`, `obj.method`).
        pub name: String,
        /// Lexical scope qualified with `::`; empty means the file root.
        pub scope: String,
        /// `import x as y` / `from m import x as y`.
        pub alias: Option<String>,
        /// One-based inclusive lines of the reference site.
        pub lines: [u32; 2],
        /// File whose replacement invalidates this reference.
        pub owner_path: String,
    }

    /// Why a span could not be extracted with full structure.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DiagnosticReason {
        /// A quote that never closes, including an open f-string field.
        UnterminatedString,
        /// A bracket has no partner, or closes a different bracket.
        UnbalancedBracket,
        /// A dedent lands between indent levels.
        InconsistentIndent,
        /// Tabs and spaces disagree about indent versus dedent.
        MixedTabs,
        /// A statement is not a form this extractor understands.
        UnexpectedToken,
        /// A declaration name cannot form a stable id; the declaration is abstained.
        UnsupportedName,
    }

    /// One bounded diagnostic with its location.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Diagnostic {
        /// One-based line of the problem.
        pub line: u32,
        /// Byte offset of the problem.
        pub byte: u32,
        /// Problem category.
        pub reason: DiagnosticReason,
    }

    /// Completion of an accepted file; refusals are [`LexError`] instead.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Completion {
        /// Every token was understood.
        Complete,
        /// Some spans were damaged; see the diagnostics and fact labels.
        Partial,
    }

    /// Structural facts for one file, in source traversal order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct LexedFile {
        /// Validated, place-relative file path.
        pub path: String,
        /// Dialect used for extraction.
        pub dialect: Dialect,
        /// Whether all input was understood.
        pub completion: Completion,
        /// At most [`LexedFile::MAX_DIAGNOSTICS`] problems, in source order.
        pub diagnostics: Vec<Diagnostic>,
        /// Module, symbols and constants.
        pub nodes: Vec<Labeled<Node>>,
        /// Containment relations owned by this file.
        pub edges: Vec<Labeled<Edge>>,
        /// Unresolved imports and calls; repeated sites remain separate records.
        pub references: Vec<Labeled<RawReference>>,
    }

    impl LexedFile {
        /// Bound on retained diagnostics; later problems still mark the file partial.
        pub const MAX_DIAGNOSTICS: usize = 64;
    }

    /// Bounds on one extraction. Ceilings are in [`Limits::CEILING`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Limits {
        /// Largest accepted file, in bytes.
        pub max_bytes: usize,
        /// Deepest indent, bracket or f-string nesting.
        pub max_depth: u32,
        /// Most nodes plus references emitted for one file.
        pub max_facts: usize,
    }

    impl Limits {
        /// The widest limits this extractor supports.
        pub const CEILING: Self = Self {
            max_bytes: 8 << 20,
            max_depth: 256,
            max_facts: 250_000,
        };

        /// Clamps every limit to its ceiling.
        pub fn narrowed(self) -> Self {
            Self {
                max_bytes: self.max_bytes.min(Self::CEILING.max_bytes),
                max_depth: self.max_depth.min(Self::CEILING.max_depth),
                max_facts: self.max_facts.min(Self::CEILING.max_facts),
            }
        }
    }

    impl Default for Limits {
        fn default() -> Self {
            Self::CEILING
        }
    }

    /// Extraction refusals. No facts are returned with any of these.
    #[derive(Debug)]
    pub enum LexError {
        /// The path is not a Python dialect.
        UnsupportedDialect,
        /// The path or a generated name cannot form a model identity.
        InvalidId(dctx_map_model::InvalidId),
        /// Supplied bytes are not UTF-8.
        InvalidUtf8(std::str::Utf8Error),
        /// The file exceeds the byte limit.
        TooLarge {
            /// Size of the refused input.
            bytes: usize,
            /// Limit in force.
            limit: usize,
        },
        /// Nesting exceeds the depth limit.
        TooDeep {
            /// One-based line where the limit was crossed.
            line: u32,
            /// Limit in force.
            limit: u32,
        },
        /// More facts than the fact limit.
        TooManyFacts {
            /// Limit in force.
            limit: usize,
        },
    }

    impl fmt::Display for LexError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::UnsupportedDialect => f.write_str("path is not a Python file"),
                Self::InvalidId(error) => error.fmt(f),
                Self::InvalidUtf8(error) => error.fmt(f),
                Self::TooLarge { bytes, limit } => {
                    write!(f, "file of {bytes} bytes exceeds the {limit}-byte limit")
                }
                Self::TooDeep { line, limit } => {
                    write!(f, "nesting deeper than {limit} at line {line}")
                }
                Self::TooManyFacts { limit } => write!(f, "file yields more than {limit} facts"),
            }
        }
    }

    impl std::error::Error for LexError {}

    impl From<dctx_map_model::InvalidId> for LexError {
        fn from(error: dctx_map_model::InvalidId) -> Self {
            Self::InvalidId(error)
        }
    }

    impl From<std::str::Utf8Error> for LexError {
        fn from(error: std::str::Utf8Error) -> Self {
            Self::InvalidUtf8(error)
        }
    }

    const HEX: &[u8; 16] = b"0123456789abcdef";

    /// SHA-256 hex of the body with whitespace runs collapsed and edges trimmed.
    pub fn body_fingerprint(body: &str) -> String {
        let normalized = collapse(body);
        let mut hex = String::with_capacity(64);
        if let Ok(digest) = Sha256::digest(normalized.as_bytes()) {
            for byte in digest.as_bytes() {
                hex.push(char::from(HEX[usize::from(byte >> 4)]));
                hex.push(char::from(HEX[usize::from(byte & 15)]));
            }
        }
        hex
    }

    pub(crate) fn collapse(value: &str) -> String {
        value.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Whitespace-normalized text up to and including the first `". "`.
    pub(crate) fn sentence(value: &str) -> String {
        let value = collapse(value);
        match value.find(". ") {
            Some(end) => value[..=end].to_owned(),
            None => value,
        }
    }

    pub(crate) fn comment_summary(source: &str, comments: &[(u32, u32)]) -> String {
        for &(start, end) in comments {
            let raw = source.get(start as usize..end as usize).unwrap_or("");
            let body = raw.trim().trim_start_matches('#').trim();
            if body.is_empty() || body.starts_with('!') {
                continue;
            }
            return sentence(body);
        }
        String::new()
    }

    /// Decodes one docstring token, quotes and recognised escapes removed.
    pub(crate) fn decode_doc(source: &str, start: u32, end: u32) -> Option<String> {
        let raw = source.get(start as usize..end as usize)?;
        let rest = strip_string_prefix(raw);
        let (_, triple) = quote_kind(rest)?;
        let qlen = if triple { 3 } else { 1 };
        if rest.len() < qlen * 2 {
            return None;
        }
        let inner = &rest[qlen..rest.len() - qlen];
        let prefix = &raw[..raw.len() - rest.len()];
        let raw_prefix = prefix.bytes().any(|b| b == b'r' || b == b'R');
        let text = if raw_prefix {
            inner.to_owned()
        } else {
            unescape(inner)
        };
        Some(text)
    }

    fn strip_string_prefix(raw: &str) -> &str {
        let bytes = raw.as_bytes();
        let mut i = 0;
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() && i < 2 {
            i += 1;
        }
        raw.get(i..).unwrap_or(raw)
    }

    fn quote_kind(rest: &str) -> Option<(char, bool)> {
        let mut chars = rest.chars();
        let quote = chars.next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let triple = rest.starts_with(&format!("{quote}{quote}{quote}"));
        Some((quote, triple))
    }

    fn unescape(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        let mut chars = value.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            match chars.next() {
                Some('\n') => {}
                Some('\r') => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some('a') => out.push('\u{7}'),
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                Some('v') => out.push('\u{b}'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        }
        out
    }
}

mod lexer {
    use crate::domain::{Diagnostic, DiagnosticReason, LexError};
    use std::cmp::Ordering;

    /// Token category. Keywords stay `Ident` and are compared by text.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Kind {
        Ident,
        Str,
        Number,
        Punct,
        Newline,
        Indent,
        Dedent,
    }

    /// One token. `doc` is set only for a plain string that can be a docstring.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct Token {
        pub kind: Kind,
        pub start: u32,
        pub end: u32,
        pub doc: bool,
    }

    /// Lexed tokens, physical comment spans and problems.
    pub(crate) struct Lexed {
        pub tokens: Vec<Token>,
        pub comments: Vec<(u32, u32)>,
        pub diagnostics: Vec<Diagnostic>,
        pub line_starts: Vec<u32>,
    }

    impl Lexed {
        /// One-based line of a byte offset. `\n`, `\r\n` and `\r` all break lines.
        pub fn line(&self, byte: u32) -> u32 {
            self.line_starts.partition_point(|&start| start <= byte).max(1) as u32
        }
    }

    #[derive(Clone, Copy)]
    struct Prefix {
        raw: bool,
        formatted: bool,
        doc: bool,
    }

    enum Emitted {
        Open(u8),
        Close(u8),
        Name,
        Other,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Step {
        Pending,
        Cont,
        Done,
    }

    struct Lx<'a> {
        src: &'a str,
        i: usize,
        tokens: Vec<Token>,
        comments: Vec<(u32, u32)>,
        problems: Vec<(u32, DiagnosticReason)>,
        indents: Vec<usize>,
        parens: Vec<(u8, u32)>,
        at_bol: bool,
        max_depth: u32,
        line_starts: Vec<u32>,
        fstrings: u32,
        string_closed: bool,
    }

    pub(crate) fn lex(source: &str, max_depth: u32) -> Result<Lexed, LexError> {
        let line_starts = line_starts(source);
        let mut lx = Lx {
            src: source,
            i: 0,
            tokens: Vec::new(),
            comments: Vec::new(),
            problems: Vec::new(),
            indents: vec![0],
            parens: Vec::new(),
            at_bol: true,
            max_depth,
            line_starts,
            fstrings: 0,
            string_closed: false,
        };
        lx.run()?;
        let diagnostics = lx
            .problems
            .iter()
            .map(|&(byte, reason)| Diagnostic {
                line: line_of(&lx.line_starts, byte),
                byte,
                reason,
            })
            .collect();
        Ok(Lexed {
            tokens: lx.tokens,
            comments: lx.comments,
            diagnostics,
            line_starts: lx.line_starts,
        })
    }

    fn line_starts(source: &str) -> Vec<u32> {
        let bytes = source.as_bytes();
        let mut starts = vec![0];
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                i += 1;
                starts.push(i as u32);
            } else if bytes[i] == b'\r' {
                i += 1;
                if i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                }
                starts.push(i as u32);
            } else {
                i += 1;
            }
        }
        starts
    }

    fn line_of(starts: &[u32], byte: u32) -> u32 {
        starts.partition_point(|&start| start <= byte).max(1) as u32
    }

    impl<'a> Lx<'a> {
        fn run(&mut self) -> Result<(), LexError> {
            if self.src.starts_with('\u{feff}') {
                self.i = '\u{feff}'.len_utf8();
            }
            while self.i < self.src.len() {
                let before = self.i;
                if self.at_bol && self.parens.is_empty() {
                    self.logical_indent()?;
                    if self.i >= self.src.len() {
                        break;
                    }
                    self.at_bol = false;
                }
                self.scan_code()?;
                if self.i == before {
                    self.bump();
                }
            }
            if !self.at_bol {
                let at = self.pos();
                self.emit(Kind::Newline, at, at, false);
            }
            let at = self.pos();
            while self.indents.len() > 1 {
                self.indents.pop();
                self.emit(Kind::Dedent, at, at, false);
            }
            let open = self.parens.clone();
            for (_, byte) in open {
                self.problem(byte, DiagnosticReason::UnbalancedBracket);
            }
            self.parens.clear();
            Ok(())
        }

        fn logical_indent(&mut self) -> Result<(), LexError> {
            loop {
                if self.i >= self.src.len() {
                    return Ok(());
                }
                let at = self.i;
                let mut col = 0usize;
                let mut alt = 0usize;
                let mut saw_tab = false;
                let mut saw_space = false;
                loop {
                    match self.peek() {
                        Some(' ') => {
                            col += 1;
                            alt += 1;
                            saw_space = true;
                            self.bump();
                        }
                        Some('\t') => {
                            saw_tab = true;
                            col = col + 8 - (col % 8);
                            alt += 1;
                            self.bump();
                        }
                        _ => break,
                    }
                }
                match self.peek() {
                    None => return Ok(()),
                    Some('\n' | '\r') => {
                        self.consume_nl();
                        continue;
                    }
                    Some('#') => {
                        self.comment();
                        if self.at_newline() {
                            self.consume_nl();
                        }
                        continue;
                    }
                    _ => {}
                }
                if saw_tab && saw_space {
                    let top = self.indent_top();
                    if relation(col, top) != relation(alt, top) {
                        self.problem(at as u32, DiagnosticReason::MixedTabs);
                    }
                }
                return self.apply_indent(col, at as u32);
            }
        }

        fn apply_indent(&mut self, col: usize, at: u32) -> Result<(), LexError> {
            let top = self.indent_top();
            if col == top {
                return Ok(());
            }
            if col > top {
                if self.indents.len() as u32 > self.max_depth {
                    return Err(LexError::TooDeep {
                        line: self.line_now(),
                        limit: self.max_depth,
                    });
                }
                self.indents.push(col);
                self.emit(Kind::Indent, at, at, false);
                return Ok(());
            }
            while self.indents.len() > 1 && self.indent_top() > col {
                self.indents.pop();
                self.emit(Kind::Dedent, at, at, false);
            }
            if self.indent_top() != col {
                self.problem(at, DiagnosticReason::InconsistentIndent);
            }
            Ok(())
        }

        fn scan_code(&mut self) -> Result<(), LexError> {
            self.skip_spaces();
            if self.i >= self.src.len() {
                return Ok(());
            }
            if self.at_newline() {
                if self.parens.is_empty() {
                    let at = self.pos();
                    self.consume_nl();
                    self.emit(Kind::Newline, at, self.pos(), false);
                    self.at_bol = true;
                } else {
                    self.consume_nl();
                }
                return Ok(());
            }
            if self.at_char('#') {
                self.comment();
                return Ok(());
            }
            if self.continued() {
                self.bump();
                self.consume_nl();
                return Ok(());
            }
            self.scan_one_token().map(|_| ())
        }

        fn scan_one_token(&mut self) -> Result<Emitted, LexError> {
            self.skip_spaces();
            if self.i >= self.src.len() {
                return Ok(Emitted::Other);
            }
            if let Some((len, prefix)) = self.look_prefix() {
                return self.take_string(len, prefix);
            }
            if self.number_start() {
                self.take_number();
                return Ok(Emitted::Other);
            }
            if self.peek().is_some_and(ident_start) {
                return Ok(self.take_ident());
            }
            self.take_op()
        }

        fn take_string(&mut self, prefix_len: usize, prefix: Prefix) -> Result<Emitted, LexError> {
            let start = self.pos();
            self.i += prefix_len;
            let Some((quote, triple)) = self.opening_quote() else {
                return Ok(Emitted::Other);
            };
            self.consume_close(quote, triple);
            if prefix.formatted {
                self.fstring(quote, triple, prefix.raw)?;
                return Ok(Emitted::Other);
            }
            if self.plain_string(quote, triple, prefix.raw) {
                self.emit(Kind::Str, start, self.pos(), prefix.doc);
            } else {
                self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                self.emit(Kind::Str, start, self.pos(), false);
            }
            Ok(Emitted::Other)
        }

        fn plain_string(&mut self, quote: char, triple: bool, raw: bool) -> bool {
            loop {
                if self.i >= self.src.len() {
                    return false;
                }
                if self.at_char('\\') {
                    self.bump();
                    if self.at_newline() {
                        if raw && !triple {
                            return false;
                        }
                        self.consume_nl();
                        continue;
                    }
                    self.bump();
                    continue;
                }
                if !triple && self.at_newline() {
                    return false;
                }
                if self.closes_string(quote, triple) {
                    self.consume_close(quote, triple);
                    return true;
                }
                if self.at_newline() {
                    self.consume_nl();
                } else {
                    self.bump();
                }
            }
        }

        fn fstring(&mut self, quote: char, triple: bool, raw: bool) -> Result<(), LexError> {
            if self.fstrings >= self.max_depth {
                return Err(LexError::TooDeep {
                    line: self.line_now(),
                    limit: self.max_depth,
                });
            }
            self.fstrings += 1;
            let result = self.fstring_body(quote, triple, raw);
            self.fstrings -= 1;
            result
        }

        fn fstring_body(&mut self, quote: char, triple: bool, raw: bool) -> Result<(), LexError> {
            self.string_closed = false;
            loop {
                let before = self.i;
                if self.fstring_finished(quote, triple)? {
                    return Ok(());
                }
                match self.fstring_brace(quote, triple)? {
                    Step::Done => return Ok(()),
                    Step::Cont => continue,
                    Step::Pending => {}
                }
                match self.fstring_escape(raw, triple)? {
                    Step::Done => return Ok(()),
                    Step::Cont => continue,
                    Step::Pending => {}
                }
                if self.at_newline() {
                    self.consume_nl();
                } else {
                    self.bump();
                }
                if self.i == before {
                    return Ok(());
                }
            }
        }

        fn fstring_finished(&mut self, quote: char, triple: bool) -> Result<bool, LexError> {
            if self.string_closed {
                self.string_closed = false;
                if self.closes_string(quote, triple) {
                    self.consume_close(quote, triple);
                }
                return Ok(true);
            }
            if self.i >= self.src.len() {
                self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                return Ok(true);
            }
            if self.closes_string(quote, triple) {
                self.consume_close(quote, triple);
                return Ok(true);
            }
            if !triple && self.at_newline() {
                self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                return Ok(true);
            }
            Ok(false)
        }

        fn fstring_brace(&mut self, quote: char, triple: bool) -> Result<Step, LexError> {
            if self.at_char('{') {
                if self.peek_next() == Some('{') {
                    self.bump();
                    self.bump();
                    return Ok(Step::Cont);
                }
                self.bump();
                self.fexpr(quote, triple)?;
                return Ok(Step::Cont);
            }
            if self.at_char('}') {
                if self.peek_next() == Some('}') {
                    self.bump();
                    self.bump();
                    return Ok(Step::Cont);
                }
                self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                self.bump();
                return Ok(Step::Cont);
            }
            Ok(Step::Pending)
        }

        fn fstring_escape(&mut self, raw: bool, triple: bool) -> Result<Step, LexError> {
            if !self.at_char('\\') {
                return Ok(Step::Pending);
            }
            self.bump();
            if self.at_newline() {
                if raw && !triple {
                    self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                    return Ok(Step::Done);
                }
                self.consume_nl();
            } else {
                self.bump();
            }
            Ok(Step::Cont)
        }

        fn fexpr(&mut self, quote: char, triple: bool) -> Result<(), LexError> {
            let mut paren = 0i32;
            let mut bracket = 0i32;
            let mut brace = 0i32;
            let mut lambdas = Vec::new();
            loop {
                if self.fexpr_closed(quote, triple)? {
                    return Ok(());
                }
                match self.fexpr_delim(quote, triple, paren, bracket, brace, &mut lambdas)? {
                    Step::Done => return Ok(()),
                    Step::Cont => continue,
                    Step::Pending => {}
                }
                self.fexpr_token(&mut paren, &mut bracket, &mut brace, &mut lambdas)?;
            }
        }

        fn fexpr_closed(&mut self, quote: char, triple: bool) -> Result<bool, LexError> {
            if self.string_closed {
                return Ok(true);
            }
            self.skip_f_ws();
            if self.i >= self.src.len() {
                self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                return Ok(true);
            }
            if self.closes_string(quote, triple) {
                self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                self.string_closed = true;
                return Ok(true);
            }
            Ok(false)
        }

        fn fexpr_delim(
            &mut self,
            quote: char,
            triple: bool,
            paren: i32,
            bracket: i32,
            brace: i32,
            lambdas: &mut Vec<(i32, i32, i32)>,
        ) -> Result<Step, LexError> {
            if self.starts_with(":=") {
                self.scan_one_token()?;
                return Ok(Step::Cont);
            }
            if self.at_char(':') && self.lambda_colon(lambdas, paren, bracket, brace) {
                return Ok(Step::Cont);
            }
            if !f_depth_zero(paren, bracket, brace) {
                return Ok(Step::Pending);
            }
            if self.at_char(':') {
                self.fformat(quote, triple)?;
                return Ok(Step::Done);
            }
            if self.at_char('}') {
                self.bump();
                return Ok(Step::Done);
            }
            if self.try_conversion() || self.try_debug_eq() {
                return Ok(Step::Cont);
            }
            Ok(Step::Pending)
        }

        fn fexpr_token(
            &mut self,
            paren: &mut i32,
            bracket: &mut i32,
            brace: &mut i32,
            lambdas: &mut Vec<(i32, i32, i32)>,
        ) -> Result<(), LexError> {
            let before = self.i;
            match self.scan_one_token()? {
                Emitted::Open(b'(') => *paren += 1,
                Emitted::Open(b'[') => *bracket += 1,
                Emitted::Open(b'{') => *brace += 1,
                Emitted::Close(b')') => *paren = paren.saturating_sub(1),
                Emitted::Close(b']') => *bracket = bracket.saturating_sub(1),
                Emitted::Close(b'}') => *brace = brace.saturating_sub(1),
                Emitted::Name if self.last_text_is("lambda") => {
                    lambdas.push((*paren, *bracket, *brace));
                }
                _ => {}
            }
            if self.i == before {
                self.bump();
            }
            Ok(())
        }

        fn lambda_colon(
            &mut self,
            lambdas: &mut Vec<(i32, i32, i32)>,
            paren: i32,
            bracket: i32,
            brace: i32,
        ) -> bool {
            if lambdas.last().copied() != Some((paren, bracket, brace)) {
                return false;
            }
            let at = self.pos();
            self.bump();
            self.emit(Kind::Punct, at, self.pos(), false);
            lambdas.pop();
            true
        }

        fn fformat(&mut self, quote: char, triple: bool) -> Result<(), LexError> {
            self.bump();
            loop {
                if self.fformat_end(quote, triple)? {
                    return Ok(());
                }
                match self.fformat_brace(quote, triple)? {
                    Step::Done => return Ok(()),
                    Step::Cont => continue,
                    Step::Pending => {}
                }
                let before = self.i;
                if self.at_newline() {
                    self.consume_nl();
                } else {
                    self.bump();
                }
                if self.i == before {
                    return Ok(());
                }
            }
        }

        fn fformat_end(&mut self, quote: char, triple: bool) -> Result<bool, LexError> {
            if self.i >= self.src.len() {
                self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                return Ok(true);
            }
            if self.closes_string(quote, triple) {
                self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                self.string_closed = true;
                return Ok(true);
            }
            if !triple && self.at_newline() {
                self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                return Ok(true);
            }
            Ok(false)
        }

        fn fformat_brace(&mut self, quote: char, triple: bool) -> Result<Step, LexError> {
            if self.at_char('{') {
                if self.peek_next() == Some('{') {
                    self.bump();
                    self.bump();
                    return Ok(Step::Cont);
                }
                self.bump();
                self.fexpr(quote, triple)?;
                return Ok(if self.string_closed { Step::Done } else { Step::Cont });
            }
            if self.at_char('}') {
                if self.peek_next() == Some('}') {
                    self.bump();
                    self.bump();
                    return Ok(Step::Cont);
                }
                self.bump();
                return Ok(Step::Done);
            }
            Ok(Step::Pending)
        }

        fn try_conversion(&mut self) -> bool {
            if !self.at_char('!') || self.starts_with("!=") {
                return false;
            }
            let Some(flag) = self.src.as_bytes().get(self.i + 1).copied() else {
                return false;
            };
            if !matches!(flag, b's' | b'r' | b'a') {
                return false;
            }
            let after = self.src.get(self.i + 2..).unwrap_or("");
            let after = after.trim_start_matches([' ', '\t']);
            if after.starts_with([':', '}', '=']) {
                self.i += 2;
                true
            } else {
                false
            }
        }

        fn try_debug_eq(&mut self) -> bool {
            if !self.at_char('=') || self.starts_with("==") {
                return false;
            }
            let after = self.src.get(self.i + 1..).unwrap_or("");
            let after = after.trim_start_matches([' ', '\t']);
            if after.starts_with(['!', ':', '}']) {
                self.bump();
                true
            } else {
                false
            }
        }

        fn skip_f_ws(&mut self) {
            loop {
                let before = self.i;
                self.skip_spaces();
                if self.continued() {
                    self.bump();
                    self.consume_nl();
                    continue;
                }
                if self.at_newline() {
                    self.consume_nl();
                    continue;
                }
                if self.at_char('#') {
                    self.comment();
                    continue;
                }
                if self.i == before {
                    break;
                }
            }
        }

        fn take_ident(&mut self) -> Emitted {
            let start = self.pos();
            self.bump();
            while self.peek().is_some_and(ident_cont) {
                self.bump();
            }
            self.emit(Kind::Ident, start, self.pos(), false);
            Emitted::Name
        }

        fn take_number(&mut self) {
            let start = self.pos();
            if self.at_char('.') {
                self.bump();
                self.consume_digits();
            } else {
                self.consume_digits();
                if self.at_char('x') || self.at_char('X') || self.at_char('o') || self.at_char('O')
                    || self.at_char('b') || self.at_char('B')
                {
                    self.bump();
                    self.consume_digits();
                }
                if self.at_char('.') && self.peek_next() != Some('.') {
                    self.bump();
                    self.consume_digits();
                }
            }
            if self.at_char('e') || self.at_char('E') {
                self.bump();
                if self.at_char('+') || self.at_char('-') {
                    self.bump();
                }
                self.consume_digits();
            }
            if self.at_char('j') || self.at_char('J') {
                self.bump();
            }
            self.emit(Kind::Number, start, self.pos(), false);
        }

        fn consume_digits(&mut self) {
            while self.peek().is_some_and(|ch| ch.is_ascii_hexdigit() || ch == '_') {
                self.bump();
            }
        }

        fn take_op(&mut self) -> Result<Emitted, LexError> {
            let rest = &self.src[self.i..];
            let n = op_len(rest);
            let at = self.pos();
            if n == 1 && self.peek().is_some_and(|ch| !known_punct(ch)) {
                self.problem(at, DiagnosticReason::UnexpectedToken);
            }
            self.i += n.min(rest.len()).max(1).min(self.src.len() - self.i);
            // Multi-byte non-ascii was measured in chars by op_len; ascii ops match bytes.
            if !self.src.is_char_boundary(self.i) {
                while self.i < self.src.len() && !self.src.is_char_boundary(self.i) {
                    self.i += 1;
                }
            }
            self.emit(Kind::Punct, at, self.pos(), false);
            let op = self.src.get(at as usize..self.i).unwrap_or("");
            let emitted = match op {
                "(" | "[" | "{" => Emitted::Open(op.as_bytes()[0]),
                ")" | "]" | "}" => Emitted::Close(op.as_bytes()[0]),
                _ => Emitted::Other,
            };
            match emitted {
                Emitted::Open(kind) => self.push_paren(kind)?,
                Emitted::Close(kind) => self.pop_paren(kind),
                _ => {}
            }
            Ok(emitted)
        }

        fn push_paren(&mut self, kind: u8) -> Result<(), LexError> {
            if self.parens.len() as u32 >= self.max_depth {
                return Err(LexError::TooDeep {
                    line: self.line_now(),
                    limit: self.max_depth,
                });
            }
            self.parens.push((kind, self.pos()));
            Ok(())
        }

        fn pop_paren(&mut self, kind: u8) {
            match self.parens.last().copied() {
                Some((open, _)) if closes(open, kind) => {
                    self.parens.pop();
                }
                _ => self.problem(self.pos(), DiagnosticReason::UnbalancedBracket),
            }
        }

        fn look_prefix(&self) -> Option<(usize, Prefix)> {
            let rest = self.src.get(self.i..)?;
            if rest.starts_with(['\'', '"']) {
                return Some((
                    0,
                    Prefix {
                        raw: false,
                        formatted: false,
                        doc: true,
                    },
                ));
            }
            for len in [2usize, 1] {
                let pre = rest.get(..len)?;
                if !pre.bytes().all(|b| b.is_ascii_alphabetic()) {
                    continue;
                }
                let after = rest.get(len..)?;
                if after.starts_with(['\'', '"']) && valid_prefix(pre) {
                    return Some((len, prefix_flags(pre)));
                }
            }
            None
        }

        fn opening_quote(&self) -> Option<(char, bool)> {
            let rest = self.src.get(self.i..)?;
            let quote = rest.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let triple = {
                let bytes = rest.as_bytes();
                let q = quote as u8;
                bytes.len() >= 3 && bytes[0] == q && bytes[1] == q && bytes[2] == q
            };
            Some((quote, triple))
        }

        fn closes_string(&self, quote: char, triple: bool) -> bool {
            let Some(rest) = self.src.get(self.i..) else {
                return false;
            };
            let bytes = rest.as_bytes();
            let q = quote as u8;
            if triple {
                bytes.len() >= 3 && bytes[0] == q && bytes[1] == q && bytes[2] == q
            } else {
                bytes.first() == Some(&q)
            }
        }

        fn consume_close(&mut self, quote: char, triple: bool) {
            let n = if triple { 3 } else { 1 };
            for _ in 0..n {
                if self.at_char(quote) {
                    self.bump();
                }
            }
        }

        fn number_start(&self) -> bool {
            match self.peek() {
                Some(ch) if ch.is_ascii_digit() => true,
                Some('.') => self.peek_next().is_some_and(|ch| ch.is_ascii_digit()),
                _ => false,
            }
        }

        fn continued(&self) -> bool {
            self.starts_with("\\\n") || self.starts_with("\\\r\n") || self.starts_with("\\\r")
        }

        fn comment(&mut self) {
            let start = self.pos();
            self.bump();
            while let Some(ch) = self.peek() {
                if ch == '\n' || ch == '\r' {
                    break;
                }
                self.bump();
            }
            self.comments.push((start, self.pos()));
        }

        fn skip_spaces(&mut self) {
            while matches!(self.peek(), Some(' ' | '\t' | '\u{c}' | '\u{b}')) {
                self.bump();
            }
        }

        fn consume_nl(&mut self) {
            if self.at_char('\r') {
                self.bump();
            }
            if self.at_char('\n') {
                self.bump();
            }
        }

        fn emit(&mut self, kind: Kind, start: u32, end: u32, doc: bool) {
            self.tokens.push(Token {
                kind,
                start,
                end,
                doc,
            });
        }

        fn problem(&mut self, byte: u32, reason: DiagnosticReason) {
            if self.problems.len() < 128 {
                self.problems.push((byte, reason));
            }
        }

        fn indent_top(&self) -> usize {
            self.indents.last().copied().unwrap_or(0)
        }

        fn line_now(&self) -> u32 {
            line_of(&self.line_starts, self.pos())
        }

        fn pos(&self) -> u32 {
            self.i.min(self.src.len()) as u32
        }

        fn peek(&self) -> Option<char> {
            self.src.get(self.i..)?.chars().next()
        }

        fn peek_next(&self) -> Option<char> {
            self.src.get(self.i..)?.chars().nth(1)
        }

        fn bump(&mut self) -> Option<char> {
            let ch = self.peek()?;
            self.i += ch.len_utf8();
            Some(ch)
        }

        fn at_char(&self, ch: char) -> bool {
            self.peek() == Some(ch)
        }

        fn at_newline(&self) -> bool {
            matches!(self.peek(), Some('\n' | '\r'))
        }

        fn starts_with(&self, text: &str) -> bool {
            self.src.get(self.i..).is_some_and(|rest| rest.starts_with(text))
        }

        fn last_text_is(&self, text: &str) -> bool {
            self.tokens.last().is_some_and(|token| {
                token.kind == Kind::Ident && self.token_text(token) == text
            })
        }

        fn token_text(&self, token: &Token) -> &str {
            self.src
                .get(token.start as usize..token.end as usize)
                .unwrap_or("")
        }
    }

    fn relation(col: usize, top: usize) -> Ordering {
        col.cmp(&top)
    }

    fn f_depth_zero(paren: i32, bracket: i32, brace: i32) -> bool {
        paren == 0 && bracket == 0 && brace == 0
    }

    fn closes(open: u8, close: u8) -> bool {
        matches!((open, close), (b'(', b')') | (b'[', b']') | (b'{', b'}'))
    }

    fn ident_start(ch: char) -> bool {
        ch == '_' || ch.is_alphabetic()
    }

    fn ident_cont(ch: char) -> bool {
        ch == '_' || ch.is_alphanumeric()
    }

    fn known_punct(ch: char) -> bool {
        matches!(
            ch,
            '(' | ')' | '[' | ']' | '{' | '}' | ':' | ';' | ',' | '.' | '=' | '+' | '-' | '*'
                | '/' | '%' | '&' | '|' | '^' | '~' | '<' | '>' | '@' | '\\'
        )
    }

    fn valid_prefix(prefix: &str) -> bool {
        matches!(
            prefix.to_ascii_lowercase().as_str(),
            "r" | "u" | "f" | "b" | "t" | "fr" | "rf" | "br" | "rb" | "tr" | "rt"
        )
    }

    fn prefix_flags(prefix: &str) -> Prefix {
        let lower = prefix.to_ascii_lowercase();
        let formatted = lower.contains('f') || lower.contains('t');
        let bytes = lower.contains('b');
        Prefix {
            raw: lower.contains('r'),
            formatted,
            doc: !formatted && !bytes && matches!(lower.as_str(), "r" | "u"),
        }
    }

    fn op_len(rest: &str) -> usize {
        const THREE: &[&str] = &["**=", "//=", "<<=", ">>=", "..."];
        const TWO: &[&str] = &[
            "->", ":=", "==", "!=", "<=", ">=", "//", "**", "<<", ">>", "+=", "-=", "*=", "/=",
            "%=", "&=", "|=", "^=", "@=",
        ];
        for op in THREE {
            if rest.starts_with(op) {
                return op.len();
            }
        }
        for op in TWO {
            if rest.starts_with(op) {
                return op.len();
            }
        }
        rest.chars().next().map_or(1, char::len_utf8)
    }
}

mod extract {
    use crate::domain::{
        Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Limits,
        Precision, RawReference, body_fingerprint, collapse, comment_summary, decode_doc, sentence,
    };
    use crate::lexer::{Kind, Lexed, Token};
    use dctx_map_model::{
        Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
    };
    use std::cmp::Reverse;
    use std::collections::BTreeMap;

    #[derive(Clone)]
    struct Scope {
        owner: usize,
        name: String,
        class_body: bool,
        module_top: bool,
        in_match: bool,
    }

    impl Scope {
        fn blocked(&self) -> Self {
            Self {
                module_top: false,
                in_match: false,
                ..self.clone()
            }
        }
    }

    struct DraftNode {
        node: Node,
        parent: usize,
        start: u32,
        end: u32,
        precision: Precision,
    }

    struct DraftRef {
        src: usize,
        kind: EdgeKind,
        name: String,
        scope: String,
        alias: Option<String>,
        lines: [u32; 2],
        start: u32,
        end: u32,
        precision: Precision,
    }

    struct Draft {
        nodes: Vec<DraftNode>,
        refs: Vec<DraftRef>,
    }

    struct Walker<'a> {
        src: &'a str,
        toks: &'a [Token],
        lines: &'a [u32],
        i: usize,
        draft: Draft,
        depth: u32,
        max_depth: u32,
        max_facts: usize,
        refused: Option<LexError>,
        problems: Vec<(u32, DiagnosticReason)>,
        path: String,
    }

    pub(crate) fn extract(
        path: &str,
        source: &str,
        dialect: Dialect,
        lexed: &Lexed,
        limits: &Limits,
    ) -> Result<LexedFile, LexError> {
        let _ = StableId::new(NodeKind::Module, path, None, None)?;
        let mut walker = Walker::new(path, source, lexed, limits)?;
        let module_doc = walker.lookahead_module_doc();
        let root = Scope {
            owner: 0,
            name: String::new(),
            class_body: false,
            module_top: true,
            in_match: false,
        };
        while walker.i < walker.toks.len() && walker.refused.is_none() {
            if walker.at(Kind::Newline) {
                walker.bump();
                continue;
            }
            if walker.at(Kind::Dedent) {
                walker.problem_here(DiagnosticReason::InconsistentIndent);
                walker.bump();
                continue;
            }
            if walker.at(Kind::Indent) {
                walker.unexpected_indent(&root);
                continue;
            }
            let mark = walker.i;
            walker.statement(&root);
            if walker.i == mark {
                walker.bump();
            }
        }
        if let Some(text) = module_doc {
            walker.set_module_summary(text);
        }
        walker.finish(dialect, lexed)
    }

    impl<'a> Walker<'a> {
        fn new(
            path: &str,
            source: &'a str,
            lexed: &'a Lexed,
            limits: &Limits,
        ) -> Result<Self, LexError> {
            if limits.max_facts == 0 {
                return Err(LexError::TooManyFacts { limit: 0 });
            }
            let id = StableId::new(NodeKind::Module, path, None, None)?;
            let summary = comment_summary(source, &lexed.comments);
            let end_byte = source.len() as u32;
            let last = if source.is_empty() {
                1
            } else {
                lexed.line(end_byte.saturating_sub(1))
            };
            let module = Node {
                id,
                kind: NodeKind::Module,
                symbol_kind: None,
                path: path.to_owned(),
                lines: [1, last],
                name: path.to_owned(),
                signature: String::new(),
                fingerprint: body_fingerprint(source),
                public: false,
                summary: Summary {
                    text: summary,
                    source: SummarySource::Doc,
                },
            };
            Ok(Self {
                src: source,
                toks: &lexed.tokens,
                lines: &lexed.line_starts,
                i: 0,
                draft: Draft {
                    nodes: vec![DraftNode {
                        node: module,
                        parent: 0,
                        start: 0,
                        end: u32::MAX,
                        precision: Precision::Structural,
                    }],
                    refs: Vec::new(),
                },
                depth: 0,
                max_depth: limits.max_depth,
                max_facts: limits.max_facts,
                refused: None,
                problems: Vec::new(),
                path: path.to_owned(),
            })
        }

        fn unexpected_indent(&mut self, scope: &Scope) {
            self.problem_here(DiagnosticReason::InconsistentIndent);
            self.bump();
            let nested = scope.blocked();
            while self.i < self.toks.len() && self.refused.is_none() && !self.at(Kind::Dedent) {
                if self.at(Kind::Newline) {
                    self.bump();
                    continue;
                }
                let mark = self.i;
                self.statement(&nested);
                if self.i == mark {
                    self.bump();
                }
            }
            if self.at(Kind::Dedent) {
                self.bump();
            }
        }

        fn statement(&mut self, scope: &Scope) {
            if self.refused.is_some() || self.eof() {
                return;
            }
            let start = self.i;
            let decorated = self.leading_decorators(scope);
            if self.definition(start, scope) || self.compound(scope) || self.import_form(scope) {
                return;
            }
            if decorated || self.stray_clause() {
                self.recover_stray(scope);
            } else {
                self.simple(scope);
            }
        }

        fn leading_decorators(&mut self, scope: &Scope) -> bool {
            let mut decorated = false;
            while self.at_op("@") {
                decorated = true;
                self.decorator(scope);
                while self.at(Kind::Newline) {
                    self.bump();
                }
            }
            decorated
        }

        fn definition(&mut self, start: usize, scope: &Scope) -> bool {
            if (self.at_text("async") && self.peek_text("def")) || self.at_text("def") {
                self.function(start, scope);
                return true;
            }
            if self.at_text("class") {
                self.class_stmt(start, scope);
                return true;
            }
            false
        }

        fn compound(&mut self, scope: &Scope) -> bool {
            if self.async_for_or_with(scope)
                || self.word_stmt("if", Self::if_stmt, scope)
                || self.word_stmt("while", Self::while_stmt, scope)
                || self.word_stmt("for", Self::for_stmt, scope)
                || self.word_stmt("try", Self::try_stmt, scope)
                || self.word_stmt("with", Self::with_stmt, scope)
            {
                return true;
            }
            if self.at_text("match") && self.header_follows() {
                self.match_stmt(scope);
                return true;
            }
            if scope.in_match && self.at_text("case") {
                self.case_clause(scope);
                return true;
            }
            false
        }

        fn word_stmt(
            &mut self,
            word: &str,
            run: fn(&mut Self, &Scope),
            scope: &Scope,
        ) -> bool {
            if self.at_text(word) {
                run(self, scope);
                true
            } else {
                false
            }
        }

        fn async_for_or_with(&mut self, scope: &Scope) -> bool {
            if self.at_text("async") && (self.peek_text("for") || self.peek_text("with")) {
                self.bump();
                if self.at_text("for") {
                    self.for_stmt(scope);
                } else {
                    self.with_stmt(scope);
                }
                return true;
            }
            false
        }

        fn import_form(&mut self, scope: &Scope) -> bool {
            if self.at_text("import") {
                self.import_stmt(scope);
                return true;
            }
            if self.at_text("from") {
                self.import_from(scope);
                return true;
            }
            false
        }

        fn stray_clause(&self) -> bool {
            self.at_text("else")
                || self.at_text("elif")
                || self.at_text("except")
                || self.at_text("finally")
        }

        fn recover_stray(&mut self, scope: &Scope) {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.bump();
            self.scan_until_colon();
            if self.at_op(":") {
                self.suite(&scope.blocked());
            } else {
                self.sync_line();
            }
        }

        fn function(&mut self, sig_at: usize, scope: &Scope) {
            if self.at_text("async") {
                self.bump();
            }
            if !self.eat_text("def") {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            }
            let Some(local) = self.take_name() else {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            };
            let header = self.i;
            let (_, hi) = self.scan_until_colon();
            self.calls(header, hi, scope);
            if !self.at_op(":") {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            }
            let colon = self.i;
            let doc = self.lookahead_suite_doc();
            let symbol = if scope.class_body {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            let qualified = qualify(scope, &local);
            let Some(owner) = self.emit_symbol(scope, &qualified, &local, symbol, sig_at, colon, doc)
            else {
                self.suite(&scope.blocked());
                return;
            };
            let inner = Scope {
                owner,
                name: qualified,
                class_body: false,
                module_top: false,
                in_match: false,
            };
            self.suite(&inner);
            self.finish_body(owner, colon);
        }

        fn class_stmt(&mut self, sig_at: usize, scope: &Scope) {
            self.bump();
            let Some(local) = self.take_name() else {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            };
            let header = self.i;
            let (_, hi) = self.scan_until_colon();
            self.calls(header, hi, scope);
            if !self.at_op(":") {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            }
            let colon = self.i;
            let doc = self.lookahead_suite_doc();
            let qualified = qualify(scope, &local);
            let Some(owner) =
                self.emit_symbol(scope, &qualified, &local, SymbolKind::Class, sig_at, colon, doc)
            else {
                self.suite(&scope.blocked());
                return;
            };
            let inner = Scope {
                owner,
                name: qualified,
                class_body: true,
                module_top: false,
                in_match: false,
            };
            self.suite(&inner);
            self.finish_body(owner, colon);
        }

        fn suite(&mut self, scope: &Scope) {
            if self.refused.is_some() {
                return;
            }
            if !self.at_op(":") {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            }
            if self.depth >= self.max_depth {
                self.refuse_depth();
                return;
            }
            self.depth += 1;
            self.bump();
            if self.at(Kind::Newline) {
                self.bump();
                if self.at(Kind::Indent) {
                    self.bump();
                    self.block(scope);
                    if self.at(Kind::Dedent) {
                        self.bump();
                    }
                }
                self.depth -= 1;
                return;
            }
            while self.refused.is_none() && !self.eof() && !self.at(Kind::Newline) && !self.at(Kind::Dedent)
            {
                let mark = self.i;
                self.statement(scope);
                if self.i == mark {
                    self.bump();
                }
                if !self.eat_op(";") {
                    break;
                }
            }
            if self.at(Kind::Newline) {
                self.bump();
            }
            self.depth -= 1;
        }

        fn block(&mut self, scope: &Scope) {
            while self.refused.is_none() && !self.eof() && !self.at(Kind::Dedent) {
                if self.at(Kind::Newline) {
                    self.bump();
                    continue;
                }
                let mark = self.i;
                self.statement(scope);
                if self.i == mark {
                    self.bump();
                }
            }
        }

        fn if_stmt(&mut self, scope: &Scope) {
            let nested = scope.blocked();
            loop {
                self.bump();
                let (lo, hi) = self.scan_until_colon();
                self.calls(lo, hi, scope);
                if !self.at_op(":") {
                    self.problem_here(DiagnosticReason::UnexpectedToken);
                    self.sync_line();
                    return;
                }
                self.suite(&nested);
                if self.at_text("elif") {
                    continue;
                }
                break;
            }
            self.optional_else(&nested);
        }

        fn while_stmt(&mut self, scope: &Scope) {
            self.header_suite(scope, true);
        }

        fn with_stmt(&mut self, scope: &Scope) {
            self.header_suite(scope, false);
        }

        fn header_suite(&mut self, scope: &Scope, else_clause: bool) {
            self.bump();
            let nested = scope.blocked();
            let (lo, hi) = self.scan_until_colon();
            self.calls(lo, hi, scope);
            if !self.at_op(":") {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            }
            self.suite(&nested);
            if else_clause {
                self.optional_else(&nested);
            }
        }

        fn for_stmt(&mut self, scope: &Scope) {
            self.bump();
            let nested = scope.blocked();
            let mut depth = 0i32;
            let mut expr = None;
            while !self.eof() {
                if depth == 0 && (self.at_op(":") || self.at(Kind::Newline)) {
                    break;
                }
                if depth == 0 && self.at_text("in") && expr.is_none() {
                    expr = Some(self.i + 1);
                }
                depth += self.delta(self.i);
                if !self.bump() {
                    break;
                }
            }
            if let Some(lo) = expr {
                self.calls(lo, self.i, scope);
            }
            if !self.at_op(":") {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            }
            self.suite(&nested);
            self.optional_else(&nested);
        }

        fn try_stmt(&mut self, scope: &Scope) {
            self.bump();
            let nested = scope.blocked();
            if !self.at_op(":") {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            }
            self.suite(&nested);
            while self.at_text("except") {
                self.bump();
                let (lo, hi) = self.scan_until_colon();
                self.calls(lo, hi, scope);
                if !self.at_op(":") {
                    break;
                }
                self.suite(&nested);
            }
            self.optional_else(&nested);
            if self.at_text("finally") {
                self.bump();
                if self.at_op(":") {
                    self.suite(&nested);
                }
            }
        }

        fn match_stmt(&mut self, scope: &Scope) {
            self.bump();
            let (lo, hi) = self.scan_until_colon();
            self.calls(lo, hi, scope);
            let mut body = scope.blocked();
            body.in_match = true;
            if self.at_op(":") {
                self.suite(&body);
            } else {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
            }
        }

        fn case_clause(&mut self, scope: &Scope) {
            self.bump();
            let mut depth = 0i32;
            let mut guard = None;
            while !self.eof() {
                if depth == 0 && self.at_op(":") {
                    break;
                }
                if depth == 0 && self.at_text("if") && guard.is_none() {
                    guard = Some(self.i);
                }
                depth += self.delta(self.i);
                if !self.bump() {
                    break;
                }
            }
            if let Some(lo) = guard {
                self.calls(lo, self.i, scope);
            }
            let nested = scope.blocked();
            if self.at_op(":") {
                self.suite(&nested);
            } else {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
            }
        }

        fn optional_else(&mut self, scope: &Scope) {
            if self.at_text("else") {
                self.bump();
                if self.at_op(":") {
                    self.suite(scope);
                }
            }
        }

        fn import_stmt(&mut self, scope: &Scope) {
            let lo = self.i;
            self.bump();
            loop {
                let module = self.read_dotted();
                if module.is_empty() {
                    self.problem_here(DiagnosticReason::UnexpectedToken);
                    break;
                }
                let alias = self.read_alias();
                let end = self.i.saturating_sub(1).max(lo);
                self.emit_ref(scope, EdgeKind::Imports, module, alias, lo, end, Precision::Structural);
                if !self.eat_op(",") {
                    break;
                }
            }
        }

        fn import_from(&mut self, scope: &Scope) {
            let lo = self.i;
            self.bump();
            let module = self.read_relative();
            if module.is_empty() || !self.eat_text("import") {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            }
            if self.eat_op("*") {
                let end = self.i.saturating_sub(1).max(lo);
                self.emit_ref(
                    scope,
                    EdgeKind::Imports,
                    format!("{module}::*"),
                    None,
                    lo,
                    end,
                    Precision::Structural,
                );
                return;
            }
            let paren = self.eat_op("(");
            loop {
                if paren && self.eat_op(")") {
                    break;
                }
                if self.at(Kind::Newline) || self.at(Kind::Dedent) || self.eof() {
                    break;
                }
                let Some(name) = self.take_name() else {
                    self.problem_here(DiagnosticReason::UnexpectedToken);
                    break;
                };
                let alias = self.read_alias();
                let end = self.i.saturating_sub(1).max(lo);
                self.emit_ref(
                    scope,
                    EdgeKind::Imports,
                    format!("{module}::{name}"),
                    alias,
                    lo,
                    end,
                    Precision::Structural,
                );
                if !self.eat_op(",") {
                    if paren {
                        let _ = self.eat_op(")");
                    }
                    break;
                }
            }
        }

        fn simple(&mut self, scope: &Scope) {
            let lo = self.i;
            if lo >= self.toks.len() {
                return;
            }
            let mut depth = 0i32;
            while self.i < self.toks.len() {
                if depth == 0
                    && (self.at(Kind::Newline)
                        || self.at(Kind::Dedent)
                        || self.at(Kind::Indent)
                        || self.at_op(";"))
                {
                    break;
                }
                depth += self.delta(self.i);
                if !self.bump() {
                    break;
                }
            }
            let hi = self.i;
            if scope.module_top {
                self.constants(lo, hi, scope);
            }
            self.calls(lo, hi, scope);
        }

        fn decorator(&mut self, scope: &Scope) {
            self.bump();
            let lo = self.i;
            let mut depth = 0i32;
            while self.i < self.toks.len() {
                if depth == 0 && (self.at(Kind::Newline) || self.at(Kind::Dedent)) {
                    break;
                }
                depth += self.delta(self.i);
                if !self.bump() {
                    break;
                }
            }
            self.calls(lo, self.i, scope);
        }

        fn constants(&mut self, lo: usize, hi: usize, scope: &Scope) {
            for (start, end) in assignment_targets(self, lo, hi) {
                let Some(names) = const_pattern(self, start, end) else {
                    continue;
                };
                for name in names {
                    self.emit_const(scope, &name, lo, hi);
                }
            }
        }

        fn calls(&mut self, lo: usize, hi: usize, scope: &Scope) {
            let mut j = lo;
            while j < hi {
                if let Some(paren) = chain_paren(self, j, lo, hi) {
                    let name = dotted(self, j, paren);
                    let end = group_end(self, paren, hi);
                    if !name.is_empty() {
                        self.emit_ref(scope, EdgeKind::Calls, name, None, j, end, Precision::Structural);
                    }
                    j = paren;
                    continue;
                }
                j += 1;
            }
        }

        fn scan_until_colon(&mut self) -> (usize, usize) {
            let lo = self.i;
            let mut depth = 0i32;
            while self.i < self.toks.len() {
                if depth <= 0
                    && (self.at_op(":")
                        || self.at(Kind::Newline)
                        || self.at(Kind::Dedent)
                        || self.at(Kind::Indent))
                {
                    break;
                }
                depth += self.delta(self.i);
                if !self.bump() {
                    break;
                }
            }
            (lo, self.i)
        }

        fn header_follows(&self) -> bool {
            let mut j = self.i;
            let mut depth = 0i32;
            while j < self.toks.len() {
                if depth <= 0 && self.op_at(j) == Some(":") {
                    return true;
                }
                if depth <= 0
                    && matches!(
                        self.kind_at(j),
                        Some(Kind::Newline | Kind::Dedent | Kind::Indent)
                    )
                {
                    return false;
                }
                depth += self.delta(j);
                j += 1;
            }
            false
        }

        #[allow(clippy::too_many_arguments)]
        fn emit_symbol(
            &mut self,
            scope: &Scope,
            qualified: &str,
            local: &str,
            symbol: SymbolKind,
            sig_at: usize,
            colon: usize,
            doc: Option<String>,
        ) -> Option<usize> {
            if !name_ok(qualified) {
                self.problem_here(DiagnosticReason::UnsupportedName);
                return None;
            }
            let start = self.tok_start(sig_at);
            let end = self.tok_end(colon);
            let signature = collapse(self.slice(start, end));
            let summary = match doc {
                Some(text) => Summary {
                    text,
                    source: SummarySource::Doc,
                },
                None => Summary {
                    text: signature.clone(),
                    source: SummarySource::Signature,
                },
            };
            let id = StableId::new(NodeKind::Symbol, &self.path, Some(qualified), None).ok()?;
            let node = Node {
                id,
                kind: NodeKind::Symbol,
                symbol_kind: Some(symbol),
                path: self.path.clone(),
                lines: [self.line_of(start), self.line_of(end.saturating_sub(1).max(start))],
                name: qualified.to_owned(),
                signature,
                fingerprint: body_fingerprint(""),
                public: is_public(local),
                summary,
            };
            self.push_node(node, scope.owner, start, end, Precision::Structural)
        }

        fn finish_body(&mut self, owner: usize, colon: usize) {
            let Some(node) = self.draft.nodes.get(owner) else {
                return;
            };
            let colon_end = node.end;
            let started = node.start;
            let (body, end_byte) = match content_span(self.toks, colon.saturating_add(1), self.i) {
                Some((start, end)) => (self.slice(start, end).to_owned(), end),
                None => (String::new(), colon_end),
            };
            let line = self.line_of(end_byte.saturating_sub(1).max(started));
            let Some(slot) = self.draft.nodes.get_mut(owner) else {
                return;
            };
            slot.node.fingerprint = body_fingerprint(&body);
            slot.node.lines[1] = line;
            slot.end = end_byte.max(slot.end);
        }

        fn emit_const(&mut self, scope: &Scope, local: &str, lo: usize, hi: usize) {
            if !name_ok(local) {
                self.problem_here(DiagnosticReason::UnsupportedName);
                return;
            }
            let (start, end) = self.span_bytes(lo, hi);
            let signature = collapse(self.slice(start, end));
            let qualified = qualify(scope, local);
            let Ok(id) = StableId::new(NodeKind::Symbol, &self.path, Some(&qualified), None) else {
                self.problem_here(DiagnosticReason::UnsupportedName);
                return;
            };
            let node = Node {
                id,
                kind: NodeKind::Symbol,
                symbol_kind: Some(SymbolKind::Const),
                path: self.path.clone(),
                lines: [self.line_of(start), self.line_of(end.saturating_sub(1).max(start))],
                name: qualified,
                signature: signature.clone(),
                fingerprint: body_fingerprint(self.slice(start, end)),
                public: is_public(local),
                summary: Summary {
                    text: signature,
                    source: SummarySource::Signature,
                },
            };
            let _ = self.push_node(node, scope.owner, start, end, Precision::Convention);
        }

        #[allow(clippy::too_many_arguments)]
        fn emit_ref(
            &mut self,
            scope: &Scope,
            kind: EdgeKind,
            name: String,
            alias: Option<String>,
            lo: usize,
            hi: usize,
            precision: Precision,
        ) {
            if name.is_empty() || self.refused.is_some() {
                return;
            }
            if self.draft.nodes.len() + self.draft.refs.len() >= self.max_facts {
                self.refused = Some(LexError::TooManyFacts {
                    limit: self.max_facts,
                });
                self.i = self.toks.len();
                return;
            }
            let start = self.tok_start(lo);
            let end = self.tok_end(hi).max(start);
            self.draft.refs.push(DraftRef {
                src: scope.owner,
                kind,
                name,
                scope: scope.name.clone(),
                alias,
                lines: [self.line_of(start), self.line_of(end.saturating_sub(1).max(start))],
                start,
                end,
                precision,
            });
        }

        fn push_node(
            &mut self,
            node: Node,
            parent: usize,
            start: u32,
            end: u32,
            precision: Precision,
        ) -> Option<usize> {
            if self.draft.nodes.len() + self.draft.refs.len() >= self.max_facts {
                self.refused = Some(LexError::TooManyFacts {
                    limit: self.max_facts,
                });
                self.i = self.toks.len();
                return None;
            }
            self.draft.nodes.push(DraftNode {
                node,
                parent,
                start,
                end,
                precision,
            });
            Some(self.draft.nodes.len() - 1)
        }

        fn set_module_summary(&mut self, text: String) {
            if let Some(module) = self.draft.nodes.first_mut() {
                module.node.summary = Summary {
                    text,
                    source: SummarySource::Doc,
                };
            }
        }

        fn lookahead_module_doc(&self) -> Option<String> {
            let mut j = 0;
            while self.kind_at(j) == Some(Kind::Newline) {
                j += 1;
            }
            let hi = statement_end(self, j);
            self.doc_statement(j, hi)
        }

        fn lookahead_suite_doc(&self) -> Option<String> {
            let mut j = self.i;
            if self.op_at(j) != Some(":") {
                return None;
            }
            j += 1;
            if self.kind_at(j) == Some(Kind::Newline) {
                j += 1;
            }
            if self.kind_at(j) == Some(Kind::Indent) {
                j += 1;
            }
            while self.kind_at(j) == Some(Kind::Newline) {
                j += 1;
            }
            let hi = statement_end(self, j);
            self.doc_statement(j, hi)
        }

        fn doc_statement(&self, mut lo: usize, mut hi: usize) -> Option<String> {
            if lo >= hi {
                return None;
            }
            if self.op_at(lo) == Some("(") && hi > lo && self.op_at(hi - 1) == Some(")") {
                lo += 1;
                hi -= 1;
            }
            if lo >= hi {
                return None;
            }
            let mut text = String::new();
            for token in self.toks.get(lo..hi)? {
                if token.kind != Kind::Str || !token.doc {
                    return None;
                }
                text.push_str(&decode_doc(self.src, token.start, token.end)?);
            }
            let text = sentence(&text);
            if text.is_empty() { None } else { Some(text) }
        }

        fn finish(mut self, dialect: Dialect, lexed: &Lexed) -> Result<LexedFile, LexError> {
            if let Some(error) = self.refused.take() {
                return Err(error);
            }
            let mut order: Vec<usize> = (1..self.draft.nodes.len()).collect();
            order.sort_by_key(|&index| {
                (
                    self.draft.nodes[index].start,
                    Reverse(self.draft.nodes[index].end),
                )
            });
            let mut remap = vec![0; self.draft.nodes.len()];
            for (position, &old) in order.iter().enumerate() {
                remap[old] = position + 1;
            }
            let mut slots: Vec<Option<DraftNode>> = self.draft.nodes.drain(..).map(Some).collect();
            let mut nodes = Vec::with_capacity(slots.len());
            if let Some(module) = slots.first_mut() {
                nodes.extend(module.take());
            }
            for &old in &order {
                if let Some(slot) = slots.get_mut(old) {
                    nodes.extend(slot.take());
                }
            }
            for node in nodes.iter_mut().skip(1) {
                node.parent = remap.get(node.parent).copied().unwrap_or(0);
            }
            assign_ordinals(&mut nodes, &self.path)?;
            self.draft.refs.sort_by_key(|reference| (reference.start, Reverse(reference.end)));
            let mut diagnostics = lexed.diagnostics.clone();
            diagnostics.extend(self.problems.iter().map(|&(byte, reason)| Diagnostic {
                line: lexed.line(byte),
                byte,
                reason,
            }));
            diagnostics.sort_by_key(|diagnostic| diagnostic.byte);
            let first = diagnostics.first().map_or(u32::MAX, |diagnostic| diagnostic.byte);
            let path = self.path.clone();
            let edges = nodes
                .iter()
                .skip(1)
                .map(|child| Labeled {
                    fact: Edge {
                        src: nodes
                            .get(child.parent)
                            .map(|parent| parent.node.id.clone())
                            .unwrap_or_else(|| child.node.id.clone()),
                        dst: child.node.id.clone(),
                        kind: EdgeKind::Contains,
                        conf: Confidence::Exact,
                        owner_path: path.clone(),
                    },
                    precision: label(child.end, child.precision, first),
                })
                .collect();
            let references = self
                .draft
                .refs
                .into_iter()
                .filter_map(|reference| {
                    let src = nodes.get(*remap.get(reference.src)?)?.node.id.clone();
                    Some(Labeled {
                        precision: label(reference.end, reference.precision, first),
                        fact: RawReference {
                            src,
                            kind: reference.kind,
                            name: reference.name,
                            scope: reference.scope,
                            alias: reference.alias,
                            lines: reference.lines,
                            owner_path: path.clone(),
                        },
                    })
                })
                .collect();
            let completion = if diagnostics.is_empty() {
                Completion::Complete
            } else {
                Completion::Partial
            };
            diagnostics.truncate(LexedFile::MAX_DIAGNOSTICS);
            let nodes = nodes
                .into_iter()
                .map(|node| Labeled {
                    precision: if node.end == u32::MAX {
                        node.precision
                    } else {
                        label(node.end, node.precision, first)
                    },
                    fact: node.node,
                })
                .collect();
            Ok(LexedFile {
                path,
                dialect,
                completion,
                diagnostics,
                nodes,
                edges,
                references,
            })
        }

        fn read_dotted(&mut self) -> String {
            let mut module = String::new();
            let Some(first) = self.take_name() else {
                return module;
            };
            module.push_str(&first);
            while self.at_op(".") && self.peek_name() {
                self.bump();
                if let Some(part) = self.take_name() {
                    module.push('.');
                    module.push_str(&part);
                }
            }
            module
        }

        fn read_relative(&mut self) -> String {
            let mut module = String::new();
            loop {
                if self.eat_op("...") {
                    module.push_str("...");
                    continue;
                }
                if self.eat_op(".") {
                    module.push('.');
                    continue;
                }
                break;
            }
            if let Some(first) = self.take_name() {
                module.push_str(&first);
                while self.at_op(".") && self.peek_name() {
                    self.bump();
                    if let Some(part) = self.take_name() {
                        module.push('.');
                        module.push_str(&part);
                    }
                }
            }
            module
        }

        fn read_alias(&mut self) -> Option<String> {
            if self.at_text("as") && self.peek_name() {
                self.bump();
                self.take_name()
            } else {
                None
            }
        }

        fn take_name(&mut self) -> Option<String> {
            if !self.at_name() {
                return None;
            }
            let name = self.text().to_owned();
            self.bump();
            Some(name)
        }

        fn sync_line(&mut self) {
            let mut depth = 0i32;
            while self.i < self.toks.len() {
                if depth <= 0 && (self.at(Kind::Newline) || self.at(Kind::Dedent)) {
                    if self.at(Kind::Newline) {
                        self.bump();
                    }
                    return;
                }
                depth += self.delta(self.i);
                if !self.bump() {
                    return;
                }
            }
        }

        fn refuse_depth(&mut self) {
            let line = self.line_of(self.tok_start(self.i));
            self.refused = Some(LexError::TooDeep {
                line,
                limit: self.max_depth,
            });
            self.i = self.toks.len();
        }

        fn problem_here(&mut self, reason: DiagnosticReason) {
            if self.problems.len() >= 128 {
                return;
            }
            let at = if self.toks.is_empty() {
                0
            } else {
                self.tok_start(self.i.min(self.toks.len() - 1))
            };
            self.problems.push((at, reason));
        }

        fn span_bytes(&self, lo: usize, hi: usize) -> (u32, u32) {
            if lo >= hi || lo >= self.toks.len() {
                let at = self.tok_start(lo.min(self.toks.len().saturating_sub(1)));
                return (at, at);
            }
            let end_i = (hi - 1).min(self.toks.len() - 1);
            (self.tok_start(lo), self.tok_end(end_i))
        }

        fn delta(&self, index: usize) -> i32 {
            match self.op_at(index) {
                Some("(" | "[" | "{") => 1,
                Some(")" | "]" | "}") => -1,
                _ => 0,
            }
        }

        fn line_of(&self, byte: u32) -> u32 {
            self.lines.partition_point(|&start| start <= byte).max(1) as u32
        }

        fn slice(&self, start: u32, end: u32) -> &str {
            let (start, end) = (start as usize, end as usize);
            if start <= end {
                self.src.get(start..end).unwrap_or("")
            } else {
                ""
            }
        }

        fn text(&self) -> &str {
            self.toks
                .get(self.i)
                .map(|token| self.token_text(token))
                .unwrap_or("")
        }

        fn token_text(&self, token: &Token) -> &str {
            self.src
                .get(token.start as usize..token.end as usize)
                .unwrap_or("")
        }

        fn text_at(&self, index: usize) -> &str {
            self.toks
                .get(index)
                .map(|token| self.token_text(token))
                .unwrap_or("")
        }

        fn kind_at(&self, index: usize) -> Option<Kind> {
            self.toks.get(index).map(|token| token.kind)
        }

        fn op_at(&self, index: usize) -> Option<&str> {
            let token = self.toks.get(index)?;
            if token.kind == Kind::Punct {
                Some(self.token_text(token))
            } else {
                None
            }
        }

        fn at(&self, kind: Kind) -> bool {
            self.kind_at(self.i) == Some(kind)
        }

        fn at_text(&self, text: &str) -> bool {
            self.at(Kind::Ident) && self.text() == text
        }

        fn at_name(&self) -> bool {
            self.at(Kind::Ident) && !self.text().is_empty() && !is_keyword(self.text())
        }

        fn at_op(&self, op: &str) -> bool {
            self.op_at(self.i) == Some(op)
        }

        fn peek_text(&self, text: &str) -> bool {
            self.kind_at(self.i + 1) == Some(Kind::Ident) && self.text_at(self.i + 1) == text
        }

        fn peek_name(&self) -> bool {
            self.kind_at(self.i + 1) == Some(Kind::Ident)
                && !is_keyword(self.text_at(self.i + 1))
                && !self.text_at(self.i + 1).is_empty()
        }

        fn eat_text(&mut self, text: &str) -> bool {
            if self.at_text(text) {
                self.bump();
                true
            } else {
                false
            }
        }

        fn eat_op(&mut self, op: &str) -> bool {
            if self.at_op(op) {
                self.bump();
                true
            } else {
                false
            }
        }

        fn bump(&mut self) -> bool {
            if self.i < self.toks.len() {
                self.i += 1;
                true
            } else {
                false
            }
        }

        fn eof(&self) -> bool {
            self.i >= self.toks.len()
        }

        fn tok_start(&self, index: usize) -> u32 {
            self.toks
                .get(index)
                .map_or(self.src.len() as u32, |token| token.start)
        }

        fn tok_end(&self, index: usize) -> u32 {
            self.toks
                .get(index)
                .map_or(self.src.len() as u32, |token| token.end)
        }
    }

    fn statement_end(walker: &Walker<'_>, lo: usize) -> usize {
        let mut j = lo;
        let mut depth = 0i32;
        while j < walker.toks.len() {
            if depth <= 0
                && (walker.op_at(j) == Some(";")
                    || matches!(
                        walker.kind_at(j),
                        Some(Kind::Newline | Kind::Dedent | Kind::Indent)
                    ))
            {
                break;
            }
            depth += walker.delta(j);
            j += 1;
        }
        j
    }

    fn assignment_targets(walker: &Walker<'_>, lo: usize, hi: usize) -> Vec<(usize, usize)> {
        let mut depth = 0i32;
        let mut colon = None;
        let mut eqs = Vec::new();
        let mut index = lo;
        while index < hi {
            if depth <= 0 && walker.op_at(index) == Some(":") && colon.is_none() && eqs.is_empty()
            {
                colon = Some(index);
            }
            if depth <= 0 && walker.op_at(index) == Some("=") {
                eqs.push(index);
            }
            depth += walker.delta(index);
            index += 1;
        }
        if let Some(colon_at) = colon
            && eqs.first().is_none_or(|eq| colon_at < *eq)
        {
            return vec![(lo, colon_at)];
        }
        let mut targets = Vec::new();
        let mut from = lo;
        for eq in eqs {
            targets.push((from, eq));
            from = eq + 1;
        }
        targets
    }

    fn const_pattern(walker: &Walker<'_>, lo: usize, hi: usize) -> Option<Vec<String>> {
        if lo >= hi {
            return None;
        }
        let mut names = Vec::new();
        for index in lo..hi {
            if walker.kind_at(index) == Some(Kind::Ident) {
                let text = walker.text_at(index);
                if is_keyword(text) || !is_constant(text) {
                    return None;
                }
                names.push(text.to_owned());
            } else if !matches!(walker.op_at(index), Some("(" | ")" | "[" | "]" | "," | "*")) {
                return None;
            }
        }
        if names.is_empty() { None } else { Some(names) }
    }

    fn chain_paren(walker: &Walker<'_>, start: usize, lo: usize, hi: usize) -> Option<usize> {
        if start > lo && walker.op_at(start - 1) == Some(".") {
            return None;
        }
        if !is_call_name(walker, start) {
            return None;
        }
        let mut index = start + 1;
        while index + 1 < hi && walker.op_at(index) == Some(".") && is_call_name(walker, index + 1)
        {
            index += 2;
        }
        if index < hi && walker.op_at(index) == Some("(") {
            Some(index)
        } else {
            None
        }
    }

    fn is_call_name(walker: &Walker<'_>, index: usize) -> bool {
        walker.kind_at(index) == Some(Kind::Ident)
            && !is_keyword(walker.text_at(index))
            && !walker.text_at(index).is_empty()
    }

    fn dotted(walker: &Walker<'_>, start: usize, paren: usize) -> String {
        let mut name = String::new();
        for index in start..paren {
            if walker.kind_at(index) == Some(Kind::Ident) {
                if !name.is_empty() {
                    name.push('.');
                }
                name.push_str(walker.text_at(index));
            }
        }
        name
    }

    fn group_end(walker: &Walker<'_>, open: usize, hi: usize) -> usize {
        let mut depth = 0i32;
        let mut index = open;
        while index < hi {
            depth += walker.delta(index);
            if depth == 0 {
                return index;
            }
            index += 1;
        }
        hi.saturating_sub(1).max(open)
    }

    fn content_span(tokens: &[Token], lo: usize, hi: usize) -> Option<(u32, u32)> {
        let hi = hi.min(tokens.len());
        let lo = lo.min(hi);
        let mut start = None;
        let mut end = 0u32;
        for token in &tokens[lo..hi] {
            if matches!(token.kind, Kind::Newline | Kind::Indent | Kind::Dedent) {
                continue;
            }
            if start.is_none() {
                start = Some(token.start);
            }
            end = token.end;
        }
        start.map(|start| (start, end.max(start)))
    }

    fn label(end: u32, base: Precision, first: u32) -> Precision {
        if end > first {
            Precision::Partial
        } else {
            base
        }
    }

    fn qualify(scope: &Scope, local: &str) -> String {
        if scope.name.is_empty() {
            local.to_owned()
        } else {
            format!("{}::{local}", scope.name)
        }
    }

    fn is_public(name: &str) -> bool {
        if name.starts_with("__") && name.ends_with("__") && name.len() > 4 {
            return true;
        }
        !name.starts_with('_')
    }

    fn is_constant(name: &str) -> bool {
        let mut chars = name.chars();
        if !chars.next().is_some_and(|ch| ch.is_ascii_uppercase()) {
            return false;
        }
        name.chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    }

    fn name_ok(name: &str) -> bool {
        !name.is_empty()
            && !name.contains(['~', '#'])
            && !name.chars().any(char::is_control)
    }

    fn is_keyword(text: &str) -> bool {
        matches!(
            text,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
        )
    }

    fn assign_ordinals(nodes: &mut [DraftNode], path: &str) -> Result<(), LexError> {
        let mut counts = BTreeMap::new();
        for draft in nodes.iter().skip(1) {
            *counts
                .entry((draft.node.kind, draft.node.name.clone()))
                .or_insert(0u32) += 1;
        }
        let mut ordinals = BTreeMap::new();
        for draft in nodes.iter_mut().skip(1) {
            let node = &mut draft.node;
            let key = (node.kind, node.name.clone());
            let next = ordinals.entry(key.clone()).or_insert(0u32);
            *next += 1;
            let ordinal = (counts.get(&key).copied().unwrap_or(0) > 1).then_some(*next);
            node.id = StableId::new(node.kind, path, Some(&node.name), ordinal)?;
        }
        Ok(())
    }
}

pub use domain::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Limits,
    Precision, RawReference, body_fingerprint,
};

/// Parser identity recorded with every fact key (PARSERS-v1 provenance).
pub const PARSER_ID: &str = "dctx-lex-py";
/// Bumped whenever extraction output can change for the same bytes.
pub const PARSER_REVISION: u32 = 1;

/// Bounded structural extractor; stateless apart from its limits.
#[derive(Debug, Clone, Copy, Default)]
pub struct PyLexer {
    limits: Limits,
}

impl PyLexer {
    /// Builds an extractor; limits above the ceilings are narrowed to them.
    pub fn new(limits: Limits) -> Self {
        Self {
            limits: limits.narrowed(),
        }
    }

    /// Extracts facts from one place-relative path and its authorized bytes.
    pub fn extract(&self, path: &str, bytes: &[u8]) -> Result<LexedFile, LexError> {
        let dialect = Dialect::for_path(path).ok_or(LexError::UnsupportedDialect)?;
        if bytes.len() > self.limits.max_bytes {
            return Err(LexError::TooLarge {
                bytes: bytes.len(),
                limit: self.limits.max_bytes,
            });
        }
        let source = std::str::from_utf8(bytes)?;
        let lexed = lexer::lex(source, self.limits.max_depth)?;
        extract::extract(path, source, dialect, &lexed, &self.limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_the_known_sha256_of_collapsed_text() {
        assert_eq!(
            body_fingerprint("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(body_fingerprint(" a\n\t b "), body_fingerprint("a b"));
    }

    #[test]
    fn refusals_are_typed_and_have_positive_controls() {
        let lexer = PyLexer::default();
        assert!(matches!(
            lexer.extract("x.js", b""),
            Err(LexError::UnsupportedDialect)
        ));
        assert!(matches!(
            lexer.extract("x.py", &[0xff]),
            Err(LexError::InvalidUtf8(_))
        ));
        assert!(matches!(
            lexer.extract("../x.py", b""),
            Err(LexError::InvalidId(_))
        ));
        let small = PyLexer::new(Limits {
            max_bytes: 4,
            ..Limits::default()
        });
        assert!(matches!(
            small.extract("x.py", b"hello"),
            Err(LexError::TooLarge { bytes: 5, limit: 4 })
        ));
        assert!(small.extract("x.py", b"a=1").is_ok());
        assert!(lexer.extract("x.py", b"").is_ok());
        assert_eq!(
            Dialect::for_path("pkg/app.PYW"),
            Some(Dialect::Python)
        );
        assert_eq!(Dialect::for_path("types.pyi"), Some(Dialect::Stub));
        assert_eq!(Dialect::for_path("notes.md"), None);
    }

    #[test]
    fn limits_never_widen_past_the_ceiling() {
        let wide = PyLexer::new(Limits {
            max_bytes: usize::MAX,
            max_depth: u32::MAX,
            max_facts: usize::MAX,
        });
        assert_eq!(wide.limits, Limits::CEILING);
        let deep = "(".repeat(300);
        assert!(matches!(
            PyLexer::default().extract("d.py", deep.as_bytes()),
            Err(LexError::TooDeep { .. })
        ));
        let shallow = "(".repeat(8) + &")".repeat(8);
        assert!(PyLexer::default().extract("d.py", shallow.as_bytes()).is_ok());
        let narrow = PyLexer::new(Limits {
            max_depth: 4,
            ..Limits::default()
        });
        assert!(narrow.extract("d.py", b"f(g(h(i())))").is_ok());
        assert!(matches!(
            narrow.extract("d.py", "(((((x)))))".as_bytes()),
            Err(LexError::TooDeep { limit: 4, .. })
        ));
        let few = PyLexer::new(Limits {
            max_facts: 1,
            ..Limits::default()
        });
        assert!(matches!(
            few.extract("d.py", b"def f():\n    pass\n"),
            Err(LexError::TooManyFacts { limit: 1 })
        ));
        assert!(few.extract("d.py", b"x = 1\n").is_ok());
        let room = PyLexer::new(Limits {
            max_facts: 2,
            ..Limits::default()
        });
        assert!(room.extract("d.py", b"def f():\n    pass\n").is_ok());
        assert!(
            LexError::TooDeep { line: 4, limit: 8 }
                .to_string()
                .contains("line 4")
        );
    }

    #[test]
    fn strings_and_comments_do_not_invent_declarations() {
        let source = "# def fake():\ns = \"def fake():\"\ns2 = b\"def fake():\"\ndef real():\n    return 1\n";
        let facts = PyLexer::default().extract("m.py", source.as_bytes()).unwrap();
        let names: Vec<_> = facts.nodes.iter().skip(1).map(|node| node.fact.name.as_str()).collect();
        assert_eq!(names, ["real"]);
        assert_eq!(facts.completion, Completion::Complete);
        assert!(facts.references.is_empty());
    }
}
