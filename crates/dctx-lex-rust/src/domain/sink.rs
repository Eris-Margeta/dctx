// SPDX-License-Identifier: Apache-2.0
//! Bounded accumulator of facts shared by the syn adapter and the line scan.
//! Implements the PARSERS-v1 record bound: overflow drops later facts and reports it.
//! Pure; finishing it yields the public Extraction.
use super::{
    Completion, Coverage, Declaration, Extraction, Limits, RawImport, RawReference, Refusal,
};

pub(crate) struct Sink {
    limit: usize,
    truncated: bool,
    declarations: Vec<Declaration>,
    imports: Vec<RawImport>,
    references: Vec<RawReference>,
}

impl Sink {
    pub fn new(limits: &Limits) -> Self {
        Self {
            limit: limits.max_records(),
            truncated: false,
            declarations: Vec::new(),
            imports: Vec::new(),
            references: Vec::new(),
        }
    }

    fn admit(&mut self) -> bool {
        let used = self.declarations.len() + self.imports.len() + self.references.len();
        if used >= self.limit {
            self.truncated = true;
        }
        !self.truncated
    }

    /// Index of the stored declaration, or `None` when the record limit dropped it.
    pub fn declare(&mut self, declaration: Declaration) -> Option<usize> {
        self.admit().then(|| {
            self.declarations.push(declaration);
            self.declarations.len() - 1
        })
    }

    pub fn import(&mut self, import: RawImport) {
        if self.admit() {
            self.imports.push(import);
        }
    }

    pub fn reference(&mut self, reference: RawReference) {
        if self.admit() {
            self.references.push(reference);
        }
    }

    /// A prior refusal wins over truncation as the reported reason.
    pub fn finish(
        self,
        coverage: Coverage,
        refusal: Option<Refusal>,
        module_doc: Option<String>,
    ) -> Extraction {
        let refusal = refusal.or(self
            .truncated
            .then_some(Refusal::TooManyRecords { limit: self.limit }));
        Extraction {
            completion: refusal.map_or(Completion::Complete, Completion::Partial),
            coverage,
            module_doc,
            declarations: self.declarations,
            imports: self.imports,
            references: self.references,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ImportKind, Precision, SourceSpan};

    fn import() -> RawImport {
        RawImport {
            kind: ImportKind::Use,
            path: "a".into(),
            alias: None,
            public: false,
            scope: String::new(),
            span: SourceSpan {
                start: 0,
                end: 1,
                lines: [1, 1],
            },
            precision: Precision::Syntax,
        }
    }

    #[test]
    fn overflow_drops_later_records_and_reports_truncation() {
        let mut sink = Sink::new(&Limits::narrowed(100, 10, 2));
        for _ in 0..3 {
            sink.import(import());
        }
        let facts = sink.finish(Coverage::Syntax, None, None);
        assert_eq!(facts.imports.len(), 2);
        assert_eq!(
            facts.completion,
            Completion::Partial(Refusal::TooManyRecords { limit: 2 })
        );
    }

    #[test]
    fn within_limit_is_complete_and_prior_refusal_wins() {
        let mut sink = Sink::new(&Limits::narrowed(100, 10, 2));
        sink.import(import());
        assert_eq!(
            sink.finish(Coverage::Syntax, None, None).completion,
            Completion::Complete
        );
        let refused = Sink::new(&Limits::default()).finish(
            Coverage::Opaque,
            Some(Refusal::Syntax { line: 1 }),
            None,
        );
        assert_eq!(
            refused.completion,
            Completion::Partial(Refusal::Syntax { line: 1 })
        );
    }
}
