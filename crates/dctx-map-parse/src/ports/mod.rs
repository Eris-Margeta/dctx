//! Syntax parser boundary for authorized file contents.
//! Implements SPEC-MAP-MODE section 6.2 without filesystem access.
//! Uses domain facts; adapters implement this port for indexers.
use crate::{ParseError, ParsedFile};

/// Stateless syntax extraction; the caller handles scan permission and IO.
pub trait Parser {
    /// Parses one place-relative path and its already-authorized UTF-8 bytes.
    fn parse(&self, path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError>;
}
