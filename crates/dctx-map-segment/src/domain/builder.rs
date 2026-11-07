// SPDX-License-Identifier: Apache-2.0
//! Builds one fact or view segment from owned drafts.
//! Implements SEGMENTS-v1 packing: required sections, optional prefix and postings, no gaps.
//! Depends on [`super::compile`] and [`super::directory`]. Callers pass the bytes to [`super::decode`].
//! `build` does not decode. Two builds of the same drafts return equal bytes.

use super::compile::{self, Input};
use super::directory;
use super::draft::{self, EdgeDraft, FactDraft, NodeDraft, TermDraft, ViewKeyDraft};
use super::error::SegmentError;
use super::kinds::SegmentRole;
use super::limits::DecoderLimits;

/// Accumulates drafts for one role and writes a segment.
#[derive(Debug)]
pub struct SegmentBuilder {
    role: SegmentRole,
    limits: DecoderLimits,
    facts: Vec<FactDraft>,
    terms: Vec<TermDraft>,
    lengths: Option<Vec<u32>>,
    view_key: Option<ViewKeyDraft>,
    nodes: Vec<NodeDraft>,
    edges: Vec<EdgeDraft>,
}

impl SegmentBuilder {
    /// An empty fact segment. Dictionary and facts are required and may be empty.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn fact() -> Self {
        Self::new(SegmentRole::Fact)
    }

    /// An empty view segment. [`Self::set_view_key`] is required before [`Self::build`].
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn view() -> Self {
        Self::new(SegmentRole::View)
    }

    /// Replaces the ceilings. A value above the standard cap is clamped when building.
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn with_limits(mut self, limits: DecoderLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Adds a fact. Refused on a view builder.
    ///
    /// # Errors
    /// Refuses view builders and invalid fact drafts.
    pub fn add_fact(&mut self, draft: FactDraft) -> Result<(), SegmentError> {
        if self.role != SegmentRole::Fact {
            return Err(SegmentError::Role("fact"));
        }
        draft::check_fact(&draft)?;
        self.facts.push(draft);
        Ok(())
    }

    /// Adds a term row. A path field is refused on a fact builder.
    ///
    /// # Errors
    /// Refuses empty terms, invalid postings or a field not permitted for the builder role.
    pub fn add_term(&mut self, draft: TermDraft) -> Result<(), SegmentError> {
        draft::check_term(&draft, self.role)?;
        self.terms.push(draft);
        Ok(())
    }

    /// Sets per-document token lengths. An empty list omits that input.
    ///
    /// # Errors
    /// Refuses a list longer than the effective record ceiling.
    pub fn set_lengths(&mut self, lengths: Vec<u32>) -> Result<(), SegmentError> {
        if u64::try_from(lengths.len()).unwrap_or(u64::MAX) > self.limits.effective().max_records {
            return Err(SegmentError::Limit("records"));
        }
        self.lengths = if lengths.is_empty() {
            None
        } else {
            Some(lengths)
        };
        Ok(())
    }

    /// Sets the view key. Refused on a fact builder.
    ///
    /// # Errors
    /// Refuses fact builders and invalid view-key fields.
    pub fn set_view_key(&mut self, draft: ViewKeyDraft) -> Result<(), SegmentError> {
        if self.role != SegmentRole::View {
            return Err(SegmentError::Role("view key"));
        }
        draft::check_view_key(&draft)?;
        self.view_key = Some(draft);
        Ok(())
    }

    /// Adds a view node. Refused on a fact builder.
    ///
    /// # Errors
    /// Refuses fact builders and invalid node fields.
    pub fn add_node(&mut self, draft: NodeDraft) -> Result<(), SegmentError> {
        if self.role != SegmentRole::Fact {
            draft::check_node(&draft)?;
            self.nodes.push(draft);
            return Ok(());
        }
        Err(SegmentError::Role("node"))
    }

    /// Adds a forward edge. Refused on a fact builder.
    ///
    /// # Errors
    /// Refuses fact builders and invalid edge endpoints or confidence.
    pub fn add_edge(&mut self, draft: EdgeDraft) -> Result<(), SegmentError> {
        if self.role != SegmentRole::View {
            return Err(SegmentError::Role("edge"));
        }
        draft::check_edge(&draft)?;
        self.edges.push(draft);
        Ok(())
    }

    /// Encodes the drafts. Equal drafts produce equal bytes, including checksums.
    ///
    /// # Errors
    /// Refuses missing role-required data, invalid cross-references, exceeded limits and unrepresentable section sizes or arithmetic.
    pub fn build(&self) -> Result<Vec<u8>, SegmentError> {
        let limits = self.limits.effective();
        let input = Input {
            role: self.role,
            facts: &self.facts,
            terms: &self.terms,
            lengths: self.lengths.as_deref(),
            view_key: self.view_key.as_ref(),
            nodes: &self.nodes,
            edges: &self.edges,
        };
        let sections = compile::sections(&input, &limits)?;
        directory::assemble(&sections, &limits)
    }

    fn new(role: SegmentRole) -> Self {
        Self {
            role,
            limits: DecoderLimits::standard(),
            facts: Vec::new(),
            terms: Vec::new(),
            lengths: None,
            view_key: None,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_fact_file_is_208_bytes_and_payload_hashes_are_sha256_of_empty() {
        let bytes = SegmentBuilder::fact().build().unwrap();
        assert_eq!(bytes.len(), 208);
        let empty = super::super::checksum::sha256(&[]).unwrap();
        assert_eq!(&bytes[112..144], &empty);
        assert_eq!(&bytes[176..208], &empty);
        let tight = DecoderLimits {
            max_segment_bytes: 80,
            ..DecoderLimits::standard()
        };
        assert_eq!(
            SegmentBuilder::fact()
                .with_limits(tight)
                .build()
                .unwrap_err(),
            SegmentError::Limit("segment")
        );
    }
}
