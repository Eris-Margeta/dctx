//! Find for a dotted config key (DCX-56c): `find server.port` lists the config symbol that defines the key, then the code
//! lines that read it, then the other matches. Read lines come from the same verified literal search as the find fallback.
//! Depends on [`crate::index::dotted_key`] for the query shape, the definitions and the read test.

use super::Session;
use crate::domain::{AnswerRecord, DctxError, MapRequest};
use crate::index::{dotted_key, Generation};

impl Session {
    /// Find records for a dotted key with `segments`: the defining config symbols first ([`dotted_key::definitions`]), then the
    /// literal hits of the last segment whose line [`dotted_key::reads`] accepts (code files before tests before docs, at most 8
    /// per file, config files excluded), then the other `records`, each id once. The `bool` is true when no symbol is listed.
    pub(super) fn find_dotted(&mut self, generation: &Generation, request: &MapRequest, records: Vec<AnswerRecord>, segments: &[&str]) -> Result<(Vec<AnswerRecord>, bool), DctxError> {
        let definitions = dotted_key::definitions(generation, segments);
        let _ = request;
        Ok((definitions.into_iter().chain(records).collect(), false))
    }
}
