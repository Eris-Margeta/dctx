//! Who produced a fact, and which bytes that claim is bound to.
//! Implements ARCHITECTURE-PROPOSAL §3 tier rules and IDENTITY-v1 provenance.
//! Depends on [`super::ContentId`] and [`super::Precision`]. Answers and summaries read it.
//! Tier one requires both a model label and a body fingerprint. The library never calls a model.

use super::{ContentId, EvidenceLabel, Precision};

/// Monotonic index generation supplied by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(u64);

impl Generation {
    /// Caller-supplied generation. Zero is a valid first generation.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Raw generation number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Tier of a summary. Tier one is imported text, not a model call made here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SummaryTier {
    /// Doc, signature or heading text.
    TierZero,
    /// Caller-supplied model text bound to a body fingerprint.
    TierOne,
}

/// Provenance of one fact or summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    content: ContentId,
    precision: Precision,
    extractor: EvidenceLabel,
    extraction_plan: ContentId,
    generation: Generation,
    model: Option<EvidenceLabel>,
    body_fingerprint: Option<ContentId>,
}

impl Provenance {
    /// Binds file bytes, parser precision, the extractor and the extraction-plan digest.
    pub fn new(
        content: ContentId,
        precision: Precision,
        extractor: EvidenceLabel,
        extraction_plan: ContentId,
        generation: Generation,
    ) -> Self {
        Self {
            content,
            precision,
            extractor,
            extraction_plan,
            generation,
            model: None,
            body_fingerprint: None,
        }
    }

    /// Records the model that produced a tier-one summary.
    pub fn with_model(mut self, model: EvidenceLabel) -> Self {
        self.model = Some(model);
        self
    }

    /// Records the body fingerprint a tier-one summary is bound to.
    pub fn with_body_fingerprint(mut self, body: ContentId) -> Self {
        self.body_fingerprint = Some(body);
        self
    }

    /// Authorized file identity.
    pub const fn content(&self) -> ContentId {
        self.content
    }

    /// Parser precision for this fact.
    pub const fn precision(&self) -> &Precision {
        &self.precision
    }

    /// Extractor name and revision.
    pub const fn extractor(&self) -> &EvidenceLabel {
        &self.extractor
    }

    /// Digest of the extraction plan.
    pub const fn extraction_plan(&self) -> ContentId {
        self.extraction_plan
    }

    /// Index generation that published the fact.
    pub const fn generation(&self) -> Generation {
        self.generation
    }

    /// Model label when a caller imported generated text.
    pub const fn model(&self) -> Option<&EvidenceLabel> {
        self.model.as_ref()
    }

    /// Body fingerprint when a caller imported generated text.
    pub const fn body_fingerprint(&self) -> Option<ContentId> {
        self.body_fingerprint
    }

    /// Tier one only when both the model label and the body fingerprint are present.
    pub const fn summary_tier(&self) -> SummaryTier {
        if self.model.is_some() && self.body_fingerprint.is_some() {
            SummaryTier::TierOne
        } else {
            SummaryTier::TierZero
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Completion, Coverage, EvidenceLabel, Precision};

    fn sample() -> Provenance {
        Provenance::new(
            ContentId::of_bytes(b"fn main() {}").unwrap(),
            Precision::new(
                Coverage::Syntax,
                Completion::Complete,
                EvidenceLabel::new("syn:reviewed").unwrap(),
            ),
            EvidenceLabel::new("rust-items:1").unwrap(),
            ContentId::of_bytes(b"plan-v1").unwrap(),
            Generation::new(3),
        )
    }

    #[test]
    fn tier_one_requires_model_and_body_fingerprint() {
        let base = sample();
        assert_eq!(base.summary_tier(), SummaryTier::TierZero);
        assert_eq!(base.generation().get(), 3);
        assert_eq!(base.extractor().as_str(), "rust-items:1");
        assert_eq!(base.content().byte_length(), 12);
        let model_only = base
            .clone()
            .with_model(EvidenceLabel::new("fixture-model").unwrap());
        assert_eq!(model_only.summary_tier(), SummaryTier::TierZero);
        let body_only = sample().with_body_fingerprint(ContentId::of_bytes(b"body").unwrap());
        assert_eq!(body_only.summary_tier(), SummaryTier::TierZero);
        let tier_one = model_only.with_body_fingerprint(ContentId::of_bytes(b"body").unwrap());
        assert_eq!(tier_one.summary_tier(), SummaryTier::TierOne);
        assert_eq!(tier_one.model().unwrap().as_str(), "fixture-model");
        assert_eq!(tier_one.body_fingerprint().unwrap().byte_length(), 4);
    }
}
