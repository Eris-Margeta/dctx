//! Fixed-point lexical score for one document. Ranking revision 1.
//! Implements SEGMENTS-v1 deterministic scoring. Arithmetic stays in `u128` and then saturates to `i64`.
//! Depends on [`super::kinds::Field`] weights. Query code supplies the hits; this module does not read VCS state.
//! There is no recency term. Equal scores break toward the smaller public id.

use std::cmp::Ordering;

use super::error::SegmentError;
use super::kinds::Field;

/// Revision frozen with these constants.
pub const RANKING_REVISION: u16 = 1;
/// Scale applied inside the lexical quotient and the idf term.
pub const SCORE_SCALE: u64 = 1_000_000;

/// One field contribution to a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldHit {
    /// Identifier, path, or doc.
    pub field: Field,
    /// Occurrences of the term in the document. Zero is refused.
    pub term_frequency: u32,
    /// Documents in the corpus that contain the term.
    pub document_frequency: u32,
}

/// Inputs for [`rank_score`]. Token counts are the postings lengths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "Exact-name, public, tier-zero and focus are independent ranking boosts; all combinations are meaningful in the existing public input.")]
pub struct RankInput<'a> {
    /// Documents in the corpus. Zero is refused.
    pub document_count: u32,
    /// Floor of the mean token count. Zero is treated as one.
    pub average_tokens: u32,
    /// Token count of the document being scored.
    pub document_tokens: u32,
    /// Field hits for the query terms.
    pub hits: &'a [FieldHit],
    /// Exact local-name boost.
    pub exact_name: bool,
    /// Public-item boost.
    pub public: bool,
    /// Tier-0 boost.
    pub tier0: bool,
    /// Out-degree used for the centrality boost.
    pub degree: u32,
    /// Focus-path boost.
    pub focus: bool,
}

/// Floor of the mean token count. An empty slice scores as zero.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn average_document_tokens(lengths: &[u32]) -> u32 {
    if lengths.is_empty() {
        return 0;
    }
    let sum = lengths.iter().fold(0u128, |acc, length| acc.saturating_add(u128::from(*length)));
    let len = u128::try_from(lengths.len()).unwrap_or(u128::MAX);
    u32::try_from(sum / len).unwrap_or(u32::MAX)
}

/// Lexical score plus boosts. Higher is better.
///
/// # Errors
/// Refuses an empty corpus, zero term frequency, document frequency above the corpus size or overflowing intermediate arithmetic.
pub fn rank_score(input: &RankInput<'_>) -> Result<i64, SegmentError> {
    if input.document_count == 0 {
        return Err(SegmentError::Ranking("empty corpus"));
    }
    let mut total = 0u128;
    for hit in input.hits {
        total = total.saturating_add(lexical_part(input, hit)?);
    }
    total = total.saturating_add(boosts(input));
    Ok(i64::try_from(total).unwrap_or(i64::MAX))
}

/// Orders a higher score first, then the public id ascending.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn cmp_ranked(a_score: i64, a_id: &str, b_score: i64, b_id: &str) -> Ordering {
    b_score.cmp(&a_score).then_with(|| a_id.cmp(b_id))
}

fn lexical_part(input: &RankInput<'_>, hit: &FieldHit) -> Result<u128, SegmentError> {
    if hit.term_frequency == 0 {
        return Err(SegmentError::Ranking("zero term frequency"));
    }
    let n = u128::from(input.document_count);
    let df = u128::from(hit.document_frequency);
    if df > n {
        return Err(SegmentError::Ranking("document frequency"));
    }
    let tf = u128::from(hit.term_frequency);
    let avg = u128::from(input.average_tokens.max(1));
    let dl = u128::from(input.document_tokens);
    let weight = u128::from(hit.field.weight());
    let scale = u128::from(SCORE_SCALE);
    // Revision 1: BM25 saturation with k1 = 1.2 and b = 0.75, then field weight and (n - df) / n.
    let den = add(mul(20, mul(tf, avg)?)?, mul(6, add(avg, mul(3, dl)?)?)?)?;
    if den == 0 {
        return Err(SegmentError::Arithmetic);
    }
    let quot = mul(mul(44, tf)?, mul(scale, avg)?)? / den;
    let weighted = mul(quot, weight)?;
    let idf = mul(n - df, scale)? / n;
    Ok(mul(weighted, idf)? / scale)
}

fn boosts(input: &RankInput<'_>) -> u128 {
    let mut boost = 0u128;
    if input.exact_name {
        boost += 2_000_000;
    }
    if input.public {
        boost += 200_000;
    }
    if input.tier0 {
        boost += 30_000;
    }
    let places = u128::from(input.degree.saturating_add(1).ilog2());
    boost += 15_000 * places;
    if input.focus {
        boost += 100_000;
    }
    boost
}

fn mul(left: u128, right: u128) -> Result<u128, SegmentError> {
    left.checked_mul(right).ok_or(SegmentError::Arithmetic)
}

fn add(left: u128, right: u128) -> Result<u128, SegmentError> {
    left.checked_add(right).ok_or(SegmentError::Arithmetic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_score_is_6345000_and_ties_break_by_id() {
        let hits = [FieldHit {
            field: Field::Identifier,
            term_frequency: 1,
            document_frequency: 1,
        }];
        let score = rank_score(&RankInput {
            document_count: 2,
            average_tokens: 4,
            document_tokens: 4,
            hits: &hits,
            exact_name: true,
            public: true,
            tier0: true,
            degree: 1,
            focus: true,
        })
        .unwrap();
        assert_eq!(score, 6_345_000);
        let flat = [FieldHit {
            field: Field::Identifier,
            term_frequency: 1,
            document_frequency: 2,
        }];
        assert_eq!(
            rank_score(&RankInput {
                document_count: 2,
                average_tokens: 4,
                document_tokens: 4,
                hits: &flat,
                exact_name: false,
                public: false,
                tier0: false,
                degree: 0,
                focus: false,
            })
            .unwrap(),
            0
        );
        assert_eq!(cmp_ranked(1, "a", 1, "b"), Ordering::Less);
        assert_eq!(cmp_ranked(5, "b", 1, "a"), Ordering::Less);
        assert_eq!(
            rank_score(&RankInput {
                document_count: 0,
                average_tokens: 1,
                document_tokens: 1,
                hits: &[],
                exact_name: false,
                public: false,
                tier0: false,
                degree: 0,
                focus: false,
            })
            .unwrap_err(),
            SegmentError::Ranking("empty corpus")
        );
        assert_eq!(average_document_tokens(&[]), 0);
        assert_eq!(average_document_tokens(&[1, 2, 3]), 2);
    }
}
