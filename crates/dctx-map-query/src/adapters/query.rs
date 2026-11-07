// SPDX-License-Identifier: Apache-2.0
//! Five-verb dispatch with lazy refresh and explicit identity outcomes.
//! Implements SPEC-MAP-MODE sections 3, 4, 6.4 and 7.
//! Uses the index graph and FTS5 store; renderers impose final wire budgets.
use super::Index;
use crate::{
    Answer, Error, Record, Request, Result, Verb,
    domain::{expression, quote, rank},
    ports::Source,
};
use dctx_map_model::{IdReference, IdentityStatus, StableId};
use dctx_map_store::MapStore;
use std::{collections::BTreeSet, time::Duration};

impl Index {
    /// Refreshes current sources before answering; stale files are always marked.
    ///
    /// # Errors
    /// Propagates request validation, source refresh, storage, revision and answer construction errors.
    pub fn query(&mut self, source: &impl Source, request: &Request) -> Result<Answer> {
        request.validate()?;
        if self.cancelled() {
            return Err(Error::Request("cancelled".into()));
        }
        self.refresh(
            source,
            if self.ready {
                Some(Duration::from_millis(50))
            } else {
                None
            },
        )?;
        self.answer(source, request)
    }

    /// Queries a just-refreshed index, checking the selected records against current source.
    /// Call refresh first when discovering new files or newly introduced search terms.
    ///
    /// # Errors
    /// Refuses invalid requests, an unavailable graph/revision or invalid read spans; propagates lookup, source and storage errors.
    #[allow(
        clippy::too_many_lines,
        reason = "Keep validated verb dispatch together with the answer metadata and revision checks shared by every branch."
    )]
    pub fn answer(&self, source: &impl Source, request: &Request) -> Result<Answer> {
        request.validate()?;
        if !self.ready {
            return Err(Error::Request("index has not been refreshed".into()));
        }
        if !self.graph_loaded {
            return Err(Error::Request("graph is not loaded".into()));
        }
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| Error::Request("index has not been refreshed".into()))?;
        if let Some(rev) = &request.revision
            && (rev.len() < 7 || !snapshot.revision.starts_with(rev) || !snapshot.dirty.is_empty())
        {
            return Err(Error::Revision(rev.clone()));
        }
        let mut answer = Answer {
            place: self.place.clone(),
            rev: snapshot.revision.clone(),
            dirty: snapshot.dirty.len(),
            age_seconds: self.age_seconds(),
            verb: request.verb,
            records: vec![],
            more: 0,
            next: request.command(request.offset),
            identity: None,
            reason: None,
        };
        answer.records = match request.verb {
            Verb::Find => self.find(request)?,
            Verb::Map
                if request.target.is_empty()
                    || !request.target.contains(':') && !request.target.starts_with('@') =>
            {
                self.outline(request, None)?
            }
            _ => {
                let (id, identity) = self.identity(&request.target)?;
                answer.identity = identity;
                if let Some(id) = id {
                    match request.verb {
                        Verb::Map => self.outline(request, Some(&id))?,
                        Verb::Neighbors => {
                            if let Some(IdentityStatus::Moved { old, new }) = &answer.identity {
                                answer.reason = Some(format!("moved: {old} -> {new}"));
                                answer.identity = None;
                            }
                            self.neighbors(request, &id)
                        }
                        Verb::Explain => vec![self.explain(&id)],
                        Verb::Read => self.read_record(source, request, &id, &mut answer)?,
                        Verb::Find => vec![],
                    }
                } else {
                    vec![]
                }
            }
        };
        answer.records.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.id.cmp(&b.id))
                .then_with(|| {
                    a.relation
                        .as_ref()
                        .map(|r| (r.edge, r.incoming))
                        .cmp(&b.relation.as_ref().map(|r| (r.edge, r.incoming)))
                })
        });
        if !matches!(request.verb, Verb::Read | Verb::Explain) {
            let total = answer.records.len();
            answer.records = answer
                .records
                .into_iter()
                .skip(request.offset)
                .take(request.limit)
                .collect();
            answer.more = total.saturating_sub(request.offset.saturating_add(answer.records.len()));
        }
        let mut checked = BTreeSet::new();
        let mut changed = BTreeSet::new();
        for record in &answer.records {
            if checked.insert(record.path.clone())
                && let Some(file) = snapshot.files.get(&record.path)
                && file.readable
                && !source.unchanged(&record.path, file).unwrap_or(false)
            {
                changed.insert(record.path.clone());
            }
        }
        for record in &mut answer.records {
            record.stale |=
                self.report.stale.contains_key(&record.path) || changed.contains(&record.path);
        }
        if request.verb == Verb::Read && answer.records.iter().any(|r| r.stale) {
            answer.records.clear();
            answer.identity = None;
            answer.reason =
                Some("stale source changed during query; refresh before reading".into());
            answer.next = request.command(0);
        }
        if !self.report.stale.is_empty() {
            answer.reason = Some(format!(
                "stale: {} file(s) could not be refreshed: {}",
                self.report.stale.len(),
                self.report
                    .stale
                    .keys()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if answer.records.is_empty() && answer.reason.is_none() {
            answer.reason = Some("no match".into());
            let shorter = request.target.split_whitespace().next().unwrap_or("map");
            answer.next = format!("dctx map find {}", quote(shorter));
        } else if answer.more > 0 {
            answer.next = request.command(request.offset.saturating_add(answer.records.len()));
        }
        Ok(answer)
    }

    fn find(&self, request: &Request) -> Result<Vec<Record>> {
        let Some(expression) = expression(&request.target) else {
            return Ok(vec![]);
        };
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| Error::Request("index not ready".into()))?;
        let mut records = Vec::new();
        let Some(store) = &self.store else {
            return Err(Error::Request("index storage unavailable".into()));
        };
        let hits: Vec<_> = store
            .search(&expression, 40)?
            .into_iter()
            .filter_map(|hit| {
                self.nodes.get(&hit.id).map(|node| {
                    (
                        node.clone(),
                        hit.score,
                        self.degree.get(&hit.id).copied().unwrap_or(0),
                    )
                })
            })
            .collect();
        for (node, score, degree) in hits {
            let mut record = Record::from(&node);
            let matches_kind = request.kinds.is_empty()
                || request.kinds.contains(&record.kind)
                || (request.kinds.iter().any(|k| k == "type")
                    && matches!(record.kind.as_str(), "struct" | "enum" | "trait" | "type"));
            if !matches_kind {
                continue;
            }
            if request
                .within
                .as_ref()
                .is_some_and(|p| !crate::domain::inside(&node.path, p))
            {
                continue;
            }
            record.score = rank(
                &node,
                score,
                request,
                degree,
                snapshot.recent.contains(&node.path),
            );
            records.push(record);
        }
        Ok(records)
    }

    fn identity(&self, value: &str) -> Result<(Option<StableId>, Option<IdentityStatus>)> {
        let original = match value.parse::<IdReference>()? {
            IdReference::Full(id) => id,
            IdReference::Alias(alias) => {
                let ids: BTreeSet<_> = self
                    .nodes
                    .keys()
                    .chain(self.moves.keys())
                    .filter(|id| id.alias().as_deref() == Some(&alias))
                    .cloned()
                    .collect();
                if ids.len() != 1 {
                    return Err(Error::Request(format!(
                        "unknown or ambiguous alias {alias}; try: dctx map find {alias}"
                    )));
                }
                ids.into_iter()
                    .next()
                    .ok_or_else(|| Error::Request("alias disappeared".into()))?
            }
        };
        let mut id = original.clone();
        let mut seen = BTreeSet::new();
        while !self.nodes.contains_key(&id) && seen.insert(id.clone()) {
            let Some(next) = self.moves.get(&id) else {
                break;
            };
            id = next.clone();
        }
        if self.nodes.contains_key(&id) {
            let status = (id != original).then(|| IdentityStatus::Moved {
                old: original,
                new: id.clone(),
            });
            Ok((Some(id), status))
        } else {
            Ok((None, Some(IdentityStatus::Gone { old: original })))
        }
    }
}
