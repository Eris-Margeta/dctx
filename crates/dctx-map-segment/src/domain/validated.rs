//! A segment whose lengths, checksums, and section bodies have been checked.
//! Implements SEGMENTS-v1 `ValidatedSegment`. Borrowed strings point into the caller's bytes.
//! Depends on the section types. [`super::decode`] is the only constructor.
//! Query code reads dictionary ranges, postings, adjacency, and degree from here.

use super::adjacency::{ForwardEdge, ResolvedReverse, UnresolvedReverse};
use super::directory::{self, Entry};
use super::error::SegmentError;
use super::facts::Fact;
use super::kinds::{Field, SectionKind, SegmentRole};
use super::nodes::{ViewKey, ViewNode};
use super::postings::{Postings, TermRow};
use super::prefix::PrefixEntry;

/// One decoded segment. The slices live as long as `bytes`.
#[derive(Debug)]
pub struct ValidatedSegment<'a> {
    pub(crate) role: SegmentRole,
    pub(crate) minor: u16,
    pub(crate) bytes: &'a [u8],
    pub(crate) entries: Vec<Entry>,
    pub(crate) dictionary: Vec<&'a str>,
    pub(crate) prefixes: Vec<PrefixEntry<'a>>,
    pub(crate) facts: Vec<Fact>,
    pub(crate) postings: Postings,
    pub(crate) view_key: Option<ViewKey>,
    pub(crate) nodes: Vec<ViewNode>,
    pub(crate) forward: Vec<ForwardEdge>,
    pub(crate) reverse_resolved: Vec<ResolvedReverse>,
    pub(crate) reverse_unresolved: Vec<UnresolvedReverse>,
}

impl<'a> ValidatedSegment<'a> {
    /// Fact or view role supplied by the caller.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn role(&self) -> SegmentRole {
        self.role
    }

    /// Minor version. Major 1 accepts every minor.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn minor(&self) -> u16 {
        self.minor
    }

    /// The bytes that were decoded.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Sorted unique strings.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn dictionary(&self) -> &[&'a str] {
        &self.dictionary
    }

    /// Sparse prefixes stored in the segment.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn prefixes(&self) -> &[PrefixEntry<'a>] {
        &self.prefixes
    }

    /// File-local facts. Empty for a view.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }

    /// Document lengths and term rows. Empty when the section was omitted.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn postings(&self) -> &Postings {
        &self.postings
    }

    /// View nodes in dense-id order.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn nodes(&self) -> &[ViewNode] {
        &self.nodes
    }

    /// View key. `None` for a fact segment.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn view_key(&self) -> Option<&ViewKey> {
        self.view_key.as_ref()
    }

    /// Forward edges sorted by source, kind, confidence, and target.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn forward(&self) -> &[ForwardEdge] {
        &self.forward
    }

    /// Resolved edges sorted by target.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn reverse_resolved(&self) -> &[ResolvedReverse] {
        &self.reverse_resolved
    }

    /// Unresolved spellings sorted by name.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn reverse_unresolved(&self) -> &[UnresolvedReverse] {
        &self.reverse_unresolved
    }

    /// Dictionary index of `value`, when it is present.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn find_string(&self, value: &str) -> Option<u32> {
        let index = self.dictionary.binary_search_by(|item| (*item).cmp(value)).ok()?;
        u32::try_from(index).ok()
    }

    /// Borrows the string at `id`.
    ///
    /// # Errors
    /// Refuses an unrepresentable or absent dictionary index.
    pub fn string(&self, id: u32) -> Result<&'a str, SegmentError> {
        let index = usize::try_from(id).map_err(|_| SegmentError::Arithmetic)?;
        self.dictionary.get(index).copied().ok_or(SegmentError::Dictionary("id"))
    }

    /// Half-open dictionary slice of strings that start with `prefix`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn string_range(&self, prefix: &str) -> &[&'a str] {
        let start = self
            .dictionary
            .partition_point(|text| !text.starts_with(prefix) && *text < prefix);
        let end = self
            .dictionary
            .partition_point(|text| text.starts_with(prefix) || *text < prefix);
        self.dictionary.get(start..end).unwrap_or(&[])
    }

    /// Payload of a known section, after validation.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn section_payload(&self, kind: SectionKind) -> Option<&'a [u8]> {
        let entry = self.entries.iter().find(|entry| entry.kind == kind.code())?;
        directory::payload(self.bytes, entry).ok()
    }

    /// Out-degree of a view node. `None` when `source` is not a dense id.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn degree(&self, source: u32) -> Option<u32> {
        let node_count = u32::try_from(self.nodes.len()).ok()?;
        if source >= node_count {
            return None;
        }
        u32::try_from(self.forward_from(source).len()).ok()
    }

    /// Forward edges whose source is `source`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn forward_from(&self, source: u32) -> &[ForwardEdge] {
        let start = self.forward.partition_point(|edge| edge.source < source);
        let end = self.forward.partition_point(|edge| edge.source <= source);
        self.forward.get(start..end).unwrap_or(&[])
    }

    /// The row for `field` and the dictionary string `term`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn term(&self, field: Field, term: &str) -> Option<&TermRow> {
        let id = self.find_string(term)?;
        self.postings.row(field, id)
    }

    /// Rows for one field, in term-id order.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn rows_for(&self, field: Field) -> &[TermRow] {
        let code = field.code();
        let start = self.postings.rows.partition_point(|row| row.field.code() < code);
        let end = self.postings.rows.partition_point(|row| row.field.code() <= code);
        self.postings.rows.get(start..end).unwrap_or(&[])
    }
}
