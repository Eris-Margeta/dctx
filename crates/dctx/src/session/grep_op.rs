//! The grep verb's file access: each indexed file is re-read and re-hashed before it is searched.
//! Implements `dctx map grep` over the pinned generation. Matching and grouping live in the index.
//! A file the policy no longer admits is skipped; a file whose bytes changed refuses the query.

use super::Session;
use crate::domain::{AnswerRecord, DctxError, MapRequest};
use crate::index::{grep_records, key_shaped, rank_key, text_records, Generation};

impl Session {
    /// Hit groups for `request`, one per enclosing symbol, from verified bytes only.
    pub(super) fn grep_hits(&mut self, generation: &Generation, request: &MapRequest) -> Result<Vec<AnswerRecord>, DctxError> {
        grep_records(generation, request, |path, digest| {
            if self.authorize(path).is_err() {
                return Ok(None);
            }
            self.read_verified(path, digest).map(Some)
        })
    }

    /// Literal-hit records for a `find` that matched no symbol, from verified bytes only. Same file gate as [`Self::grep_hits`].
    pub(super) fn text_hits(&mut self, generation: &Generation, request: &MapRequest) -> Result<Vec<AnswerRecord>, DctxError> {
        text_records(generation, request, |path, digest| {
            if self.authorize(path).is_err() {
                return Ok(None);
            }
            self.read_verified(path, digest).map(Some)
        })
    }
}

impl Session {
    /// Find records with literal hits added: all of them when no symbol matched (the `bool` is then true, `text`
    /// coverage), and for a SCREAMING_CASE key the read sites ranked in by [`rank_key`], matched symbols or not.
    pub(super) fn find_with_text(&mut self, generation: &Generation, request: &MapRequest, records: Vec<AnswerRecord>) -> Result<(Vec<AnswerRecord>, bool), DctxError> {
        if let Some(segments) = crate::index::dotted_key::segments(&request.query) {
            return self.find_dotted(generation, request, records, &segments);
        }
        self.find_literal(generation, request, records)
    }

    /// The plain find fallback of [`Self::find_with_text`]: literal hits when no symbol matched, and the ranked read sites of a
    /// SCREAMING_CASE key. Also used by a dotted key that no config symbol defines.
    pub(super) fn find_literal(&mut self, generation: &Generation, request: &MapRequest, records: Vec<AnswerRecord>) -> Result<(Vec<AnswerRecord>, bool), DctxError> {
        let key = key_shaped(&request.query);
        if !(records.is_empty() || key) || !text_fallback_applies(request) {
            return Ok((records, false));
        }
        let texts = self.text_hits(generation, request)?;
        let text_only = records.is_empty() && !texts.is_empty();
        Ok((if key { rank_key(&request.query, records, texts) } else { texts }, text_only))
    }
}

/// True when `find` may fall back to a literal search for `request`: a single token of letters, digits, `_`, `-` or `.`,
/// and no kind filter that excludes `text`. A continuation is fine: it only resumes the same fallback page.
pub(super) fn text_fallback_applies(request: &MapRequest) -> bool {
    let token = !request.query.is_empty() && request.query.chars().all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.'));
    token && (request.kinds.is_empty() || request.kinds.iter().any(|kind| kind == "text"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Verb;

    #[test]
    fn fallback_applies_to_one_token_only() {
        let ask = |query: &str| {
            let mut request = MapRequest::new(Verb::Find);
            request.query = query.to_string();
            request
        };
        assert!(text_fallback_applies(&ask("max_upload_size")));
        assert!(text_fallback_applies(&ask("server.max-upload")));
        assert!(!text_fallback_applies(&ask("two words")));
        assert!(!text_fallback_applies(&ask("")));
        assert!(!text_fallback_applies(&ask("a/b")));
        let mut kinded = ask("key");
        kinded.kinds = vec!["fn".to_string()];
        assert!(!text_fallback_applies(&kinded));
        kinded.kinds.push("text".to_string());
        assert!(text_fallback_applies(&kinded));
    }
}
