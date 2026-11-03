// SPDX-License-Identifier: Apache-2.0
//! Segment domain: envelope, dictionary, postings, adjacency and ranking.
//! Implements SEGMENTS-v1 and ARCHITECTURE-PROPOSAL §3. No IO and no git.
//! Depends on `dctx-hash` through the checksum module. The crate root re-exports this API.
//! Decoded strings borrow the caller's bytes. Unknown versions and bad checksums are errors.

mod adjacency;
mod builder;
mod checksum;
mod compile;
#[cfg(test)]
mod corrupt;
mod cursor;
mod decode;
mod dictionary;
mod directory;
mod draft;
mod error;
mod fact_record;
mod facts;
mod header;
mod kinds;
mod limits;
mod nodes;
mod postings;
mod prefix;
mod ranking;
#[cfg(test)]
mod roundtrip;
mod validated;
mod varint;

pub use adjacency::{ForwardEdge, ResolvedReverse, Target, UnresolvedReverse};
pub use builder::SegmentBuilder;
pub use checksum::{envelope_checksum, segment_object_digest};
pub use decode::{decode, decode_with_limits};
pub use prefix::PrefixEntry;
pub use draft::{
    DeclarationDraft, DocId, EdgeDraft, EdgeEnd, FactDraft, NodeDraft, PostingDraft, RawRefDraft,
    TermDraft, ViewKeyDraft,
};
pub use error::SegmentError;
pub use facts::{Declaration, Fact, FactKey, Provenance, RawRef};
pub use kinds::{
    Completion, Confidence, Coverage, DeclarationKind, EdgeKind, Field, Language, SectionKind,
    SegmentRole, SummarySource,
};
pub use limits::DecoderLimits;
pub use nodes::{ViewKey, ViewNode};
pub use postings::{Postings, TermHit, TermRow};
pub use ranking::{
    average_document_tokens, cmp_ranked, rank_score, FieldHit, RankInput, RANKING_REVISION,
    SCORE_SCALE,
};
pub use validated::ValidatedSegment;
