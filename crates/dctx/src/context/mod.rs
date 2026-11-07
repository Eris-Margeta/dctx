// SPDX-License-Identifier: Apache-2.0
//! Engine-owned task-context selection and rendering, shared by CLI and RPC.
mod page;
mod terms;

use crate::{DctxError, ErrorCode, MapRequest, Session, SnapshotHandle, VecSink, Verb};

const NEIGHBOR_HITS: usize = 4;
const READ_HITS: usize = 3;

/// Task text and the budget of its source context page.
pub struct ContextRequest {
    /// Names and prose from the task.
    pub text: String,
    /// Estimated token budget, 100..=100000.
    pub budget: u32,
    /// 1 for text; 2 for a JSON document containing the complete text page.
    pub output_format: u32,
}

/// Engine-rendered context page and its match count.
pub struct ContextReport {
    /// Encoded page, including its final newline.
    pub text: String,
    /// Number of matching find records, before budget selection.
    pub hits: usize,
}

impl Session {
    /// Selects and renders task context against one pinned snapshot.
    /// # Errors
    /// Refuses an empty task, an invalid budget/format, or an index/query failure.
    pub fn context(
        &mut self,
        handle: &SnapshotHandle,
        request: &ContextRequest,
    ) -> Result<ContextReport, DctxError> {
        if !cfg!(feature = "map") {
            return Err(DctxError::unsupported("map"));
        }
        if request.text.trim().is_empty() || !(100..=100_000).contains(&request.budget) {
            return Err(DctxError::invalid(
                "context requires non-empty text and budget 100..=100000",
            ));
        }
        if !matches!(request.output_format, 1 | 2) {
            return Err(DctxError::invalid("context output format must be 1 or 2"));
        }
        let mut report = build(request, |query| {
            let mut sink = VecSink::new();
            self.query(handle, query, &mut sink)?;
            Ok(String::from_utf8_lossy(sink.as_bytes()).into_owned())
        })?;
        if request.output_format == 2 {
            let mut json = String::from("{\"v\":2,\"verb\":\"context\",\"text\":");
            crate::domain::quote_json(&mut json, &report.text);
            use std::fmt::Write as _;
            let _ = writeln!(json, ",\"hits\":{}}}", report.hits);
            report.text = json;
        }
        Ok(report)
    }
}

/// Builds the context page from typed facade map queries.
fn build(
    plan: &ContextRequest,
    mut query: impl FnMut(&MapRequest) -> Result<String, DctxError>,
) -> Result<ContextReport, DctxError> {
    let terms = terms::extract(&plan.text);
    let mut page = page::Page::new(
        plan.budget,
        &format!(
            "# dctx pack --for budget={} terms={}",
            plan.budget,
            terms.len()
        ),
    );
    let mut best: Vec<String> = Vec::new();
    let mut hits = 0;
    for term in &terms {
        let Some(found) = ask(&mut query, Verb::Find, term)? else {
            continue;
        };
        let records: Vec<&str> = body(&found)
            .into_iter()
            .filter(|line| line.contains('\t'))
            .collect();
        hits += records.len();
        if let Some(id) = best_hit(&records, term)
            && !best.contains(&id)
        {
            best.push(id);
        }
        page.section(&format!("## find {term}"), &records);
    }
    for id in best.iter().take(NEIGHBOR_HITS) {
        if let Some(answer) = ask(&mut query, Verb::Neighbors, id)? {
            page.section(&format!("## neighbors {id}"), &body(&answer));
        }
    }
    for id in best.iter().take(READ_HITS) {
        if let Some(answer) = ask(&mut query, Verb::Read, id)? {
            page.section(&format!("## read {id}"), &body(&answer));
        }
    }
    Ok(ContextReport {
        text: page.finish(),
        hits,
    })
}

/// One map query. A usage or no-result refusal skips that term or id; an index failure stops the pack.
fn ask(
    query: &mut impl FnMut(&MapRequest) -> Result<String, DctxError>,
    verb: Verb,
    target: &str,
) -> Result<Option<String>, DctxError> {
    let mut request = MapRequest::new(verb);
    if verb == Verb::Find {
        request.query = target.to_string();
    } else {
        request.target = target.to_string();
    }
    request.output_format = 1;
    request.limits.records = Some(match verb {
        Verb::Find => 3,
        Verb::Neighbors => 12,
        _ => 1,
    });
    if verb != Verb::Read {
        request.depth = Some(1);
        request.limits.depth = Some(1);
        request.limits.estimated_tokens = Some(500);
        request.limits.encoded_bytes = Some(1750);
    }
    match query(&request) {
        Ok(answer) => Ok(Some(answer)),
        Err(error)
            if matches!(
                error.code(),
                ErrorCode::InvalidRequest
                    | ErrorCode::BudgetTooSmall
                    | ErrorCode::StaleContinuation
                    | ErrorCode::Stale
                    | ErrorCode::UnsupportedCapability
                    | ErrorCode::UnsupportedVersion
                    | ErrorCode::PolicyDenied
                    | ErrorCode::PolicyUnavailable
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// The lines of a map text page after its `# <place> <verb>` header.
fn body(answer: &str) -> Vec<&str> {
    let mut lines = answer.lines();
    let first = answer.lines().next();
    if first.is_some_and(|line| line.starts_with("# ")) {
        lines.next();
    }
    lines.collect()
}

/// A file term selects its module. Otherwise prefer the named symbol, then the
/// first symbol id, then the first id.
fn best_hit(records: &[&str], term: &str) -> Option<String> {
    let ids: Vec<&str> = records
        .iter()
        .filter_map(|line| line.split('\t').next())
        .filter(|id| !id.is_empty())
        .collect();
    if term.contains('.') {
        let suffix = format!("/{term}");
        if let Some(path) = ids
            .iter()
            .filter_map(|id| {
                id.strip_prefix("s:")
                    .or_else(|| id.strip_prefix("m:"))
                    .map(|rest| rest.split('#').next().unwrap_or(rest))
            })
            .find(|path| *path == term || path.ends_with(&suffix))
        {
            return Some(format!("m:{path}"));
        }
    }
    let named = |id: &str| {
        let name = id.rsplit('#').next().unwrap_or(id);
        name == term || name.ends_with(&format!("::{term}")) || name.ends_with(&format!(".{term}"))
    };
    let chosen = ids
        .iter()
        .copied()
        .find(|&id| id.contains('#') && named(id))
        .or_else(|| ids.iter().copied().find(|&id| id.starts_with("s:")))
        .or_else(|| ids.first().copied());
    chosen.map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ContextRequest {
        ContextRequest {
            text: "fix `alpha_body` in BetaStore".into(),
            budget: 3000,
            output_format: 1,
        }
    }

    #[test]
    fn composition_uses_typed_queries_in_find_neighbors_read_order() {
        let mut calls = Vec::new();
        let report = build(&request(), |query| {
            calls.push((query.verb, query.query.clone(), query.target.clone()));
            assert_eq!(query.output_format, 1);
            match query.verb {
                Verb::Find if query.query == "alpha_body" => {
                    assert_eq!(query.limits.records, Some(3));
                    Ok("# p find\ns:a.rs#other\tfn\ns:a.rs#alpha_body\tfn\n".into())
                }
                Verb::Find => Ok("# p find\ns:b.rs#BetaStore\tstruct\n".into()),
                Verb::Neighbors => {
                    assert_eq!(query.limits.records, Some(12));
                    Ok("# p neighbors\ncalls:in:s:b.rs#caller\tfn\n".into())
                }
                Verb::Read => {
                    assert!(query.limits.estimated_tokens.is_none());
                    Ok("# p read\n# preserved source comment\nfn body() {}\n".into())
                }
                _ => unreachable!(),
            }
        })
        .expect("context");
        assert_eq!(report.hits, 3);
        assert_eq!(
            calls.iter().map(|call| call.0).collect::<Vec<_>>(),
            [
                Verb::Find,
                Verb::Find,
                Verb::Neighbors,
                Verb::Neighbors,
                Verb::Read,
                Verb::Read
            ]
        );
        assert_eq!(calls[2].2, "s:a.rs#alpha_body");
        assert!(
            report
                .text
                .contains("## read s:a.rs#alpha_body\n# preserved source comment\n")
        );
    }

    #[test]
    fn no_hits_and_refused_terms_remain_empty_but_index_failure_stops() {
        let empty = build(&request(), |_| Err(DctxError::invalid("term"))).expect("skip refusal");
        assert_eq!(empty.hits, 0);
        let error = build(&request(), |_| Err(DctxError::SourceUnavailable));
        assert!(matches!(error, Err(DctxError::SourceUnavailable)));
    }

    #[test]
    fn file_terms_prefer_the_module_and_source_comments_survive() {
        assert_eq!(
            best_hit(&["s:src/lib.rs#Alias\ttype"], "src/lib.rs").as_deref(),
            Some("m:src/lib.rs")
        );
        assert_eq!(
            body("# p read\nid\tfn\n# comment\n"),
            ["id\tfn", "# comment"]
        );
        assert_eq!(body("no header\n"), ["no header"]);
    }
}
