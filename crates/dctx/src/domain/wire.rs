//! Canonical JSON v2 and a plain text page. No insignificant whitespace, LF terminated.
//! Implements contracts/API-PORTS-v1.md ("JSON v2 and compatibility").
//! Depends on [`super::answer`]. The session writes the returned string as one sink page.
//! `score` is a sort key and is not a JSON field. The object has no `vcs` field and no git id.

use std::fmt::Write as _;
use super::answer::{Answer, AnswerRecord};
use super::error::DctxError;
use super::request::Verb;

/// Encodes `answer` as one JSON v2 document plus a trailing LF, without `snapshot`, `generation` or `freshness.age_s`.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn encode_answer(place: &str, answer: &Answer) -> String {
    encode_answer_with(place, answer, false)
}

/// Like [`encode_answer`]. `meta` restores `snapshot`, `generation` and `freshness.age_s`.
#[must_use]
pub fn encode_answer_with(place: &str, answer: &Answer, meta: bool) -> String {
    let mut out = String::new();
    out.push('{');
    field_raw(&mut out, "v", "2", true);
    field_str(&mut out, "place", place, false);
    if meta {
        field_str(&mut out, "snapshot", &answer.snapshot, false);
        field_raw(&mut out, "generation", &answer.generation.to_string(), false);
    }
    field_str(&mut out, "verb", answer.verb.as_str(), false);
    out.push_str(",\"freshness\":{");
    field_str(&mut out, "mode", &answer.freshness.mode, true);
    field_str(&mut out, "state", &answer.freshness.state, false);
    if meta {
        field_raw(&mut out, "age_s", &answer.freshness.age_s.to_string(), false);
    }
    out.push('}');
    out.push_str(",\"precision\":{");
    field_str(&mut out, "coverage", &answer.precision.coverage, true);
    field_str(&mut out, "completion", &answer.precision.completion, false);
    field_str(&mut out, "parser", &answer.precision.parser, false);
    out.push('}');
    out.push_str(",\"records\":[");
    for (index, record) in answer.records.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        encode_record(&mut out, record, answer.verb == Verb::Neighbors);
    }
    out.push(']');
    field_raw(&mut out, "more", &answer.more.to_string(), false);
    match &answer.next {
        Some(next) => field_str(&mut out, "next", next, false),
        None => field_raw(&mut out, "next", "null", false),
    }
    if let Some(outcome) = &answer.outcome {
        field_str(&mut out, "outcome", outcome.as_str(), false);
        field_str(&mut out, "outcome_id", outcome.id(), false);
    }
    out.push('}');
    out.push('\n');
    out
}

/// Encodes `answer` as text. The header is `# <place> <verb>`; with `meta` it also carries the own digest and generation, never a git revision.
pub fn encode_text(place: &str, answer: &Answer, meta: bool) -> String {
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(place);
    out.push(' ');
    out.push_str(answer.verb.as_str());
    if meta {
        out.push(' ');
        out.push_str(&answer.snapshot);
        out.push_str(" g=");
        out.push_str(&answer.generation.to_string());
    }
    out.push('\n');
    for record in &answer.records {
        if answer.verb == Verb::Skeleton {
            push_skeleton_line(&mut out, record);
            continue;
        }
        out.push_str(&record.id);
        out.push('\t');
        out.push_str(&record.kind);
        out.push('\t');
        out.push_str(&record.path);
        out.push('\n');
        if answer.verb == Verb::Grep {
            super::append_rows(&mut out, record);
        }
        // A read carries the node text; the text form prints it after the record line, as the JSON form does.
        if answer.verb == Verb::Read
            && let Some(source) = record.source_utf8.as_deref()
        {
            out.push_str(source);
            if !source.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    if answer.verb == Verb::Grep {
        super::append_summary(&mut out, &answer.records);
    }
    if let Some(outcome) = &answer.outcome {
        out.push_str(outcome.as_str());
        out.push('\t');
        out.push_str(outcome.id());
        out.push('\n');
    }
    out
}

/// One skeleton line: `<id>\t<kind>\tL<start>-<end>\t<signature>`. The module line carries its path, and tabs or newlines inside a signature become spaces.
fn push_skeleton_line(out: &mut String, record: &AnswerRecord) {
    let _ = write!(out, "{}\t{}\tL{}-{}\t", record.id, record.kind, record.lines[0], record.lines[1]);
    match record.signature.as_deref() {
        Some(signature) => out.extend(signature.chars().map(|ch| if matches!(ch, '\t' | '\n' | '\r') { ' ' } else { ch })),
        None if record.kind == "module" => out.push_str(&record.path),
        None => {}
    }
    out.push('\n');
}

/// One record object. A `compact` record (neighbors) leaves out null fields and a `target` that its id already ends with.
fn encode_record(out: &mut String, record: &AnswerRecord, compact: bool) {
    out.push('{');
    field_str(out, "id", &record.id, true);
    field_str(out, "kind", &record.kind, false);
    field_str(out, "path", &record.path, false);
    out.push_str(",\"lines\":[");
    out.push_str(&record.lines[0].to_string());
    out.push(',');
    out.push_str(&record.lines[1].to_string());
    out.push(']');
    field_str(out, "summary", &record.summary, false);
    field_str(out, "summary_source", &record.summary_source, false);
    field_opt_in(out, "signature", record.signature.as_deref(), compact);
    match record.span {
        Some([start, end]) => {
            out.push_str(",\"span\":[");
            out.push_str(&start.to_string());
            out.push(',');
            out.push_str(&end.to_string());
            out.push(']');
        }
        None if compact => {}
        None => field_raw(out, "span", "null", false),
    }
    field_opt_in(out, "source_utf8", record.source_utf8.as_deref(), compact);
    field_opt_in(out, "edge", record.edge.as_deref(), compact);
    field_opt_in(out, "direction", record.direction.as_deref(), compact);
    let target = record.target.as_deref().filter(|target| !(compact && record.id.ends_with(target)));
    field_opt_in(out, "target", target, compact);
    field_opt_in(out, "confidence", record.confidence.as_deref(), compact);
    out.push('}');
}

/// Like [`field_opt`], and with `compact` a null value is left out.
fn field_opt_in(out: &mut String, key: &str, value: Option<&str>, compact: bool) {
    if value.is_some() || !compact {
        field_opt(out, key, value);
    }
}

fn field_opt(out: &mut String, key: &str, value: Option<&str>) {
    match value {
        Some(text) => field_str(out, key, text, false),
        None => field_raw(out, key, "null", false),
    }
}

fn field_str(out: &mut String, key: &str, value: &str, first: bool) {
    if !first {
        out.push(',');
    }
    push_string(out, key);
    out.push(':');
    push_string(out, value);
}

fn field_raw(out: &mut String, key: &str, raw: &str, first: bool) {
    if !first {
        out.push(',');
    }
    push_string(out, key);
    out.push(':');
    out.push_str(raw);
}

fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                let code = u32::from(control);
                let _ = write!(out, "\\u{code:04x}");
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Cursor bound to one generation, verb and last id.
pub(crate) fn cursor(generation: u64, verb: Verb, id: &str) -> String {
    format!("v1|{generation}|{}|{id}", verb.as_str())
}

/// Returns the last id when the cursor matches `generation` and `verb`.
pub(crate) fn parse_cursor(cursor: &str, generation: u64, verb: Verb) -> Result<String, DctxError> {
    let mut parts = cursor.splitn(4, '|');
    let version = parts.next();
    let found_generation = parts.next();
    let found_verb = parts.next();
    let id = parts.next();
    if version != Some("v1") {
        return Err(DctxError::StaleContinuation);
    }
    let found_generation = found_generation.and_then(|text| text.parse::<u64>().ok()).ok_or(DctxError::StaleContinuation)?;
    if found_generation != generation {
        return Err(DctxError::StaleContinuation);
    }
    if found_verb != Some(verb.as_str()) {
        return Err(DctxError::invalid("continuation verb does not match the request"));
    }
    match id {
        Some(id) if !id.is_empty() => Ok(id.to_string()),
        _ => Err(DctxError::StaleContinuation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::answer::{Freshness, IdentityOutcome, Precision};

    #[test]
    fn quotes_and_controls_are_escaped() {
        let mut answer = sample();
        answer.records[0].summary = "say \"hi\"\\".to_string();
        let json = encode_answer("fixture", &answer);
        assert!(json.ends_with('\n'));
        assert!(json.contains("say \\\"hi\\\"\\\\"));
        assert!(!json.contains("score"));
        assert!(json.contains("\"v\":2"));
        let quoted = {
            let mut out = String::new();
            push_string(&mut out, "\u{0001}");
            out
        };
        assert_eq!(quoted, "\"\\u0001\"");
    }

    #[test]
    fn answers_from_different_snapshots_are_equal_bytes_without_meta() {
        let first = sample();
        let mut second = sample();
        second.snapshot = "v1:sha256:cd:7".to_string();
        second.generation = 7;
        second.freshness.age_s = 99;
        assert_eq!(encode_answer("fixture", &first), encode_answer("fixture", &second));
        assert_eq!(encode_text("fixture", &first, false), encode_text("fixture", &second, false));
        assert!(encode_text("fixture", &first, false).starts_with("# fixture find\n"));
        let json = encode_answer("fixture", &first);
        assert!(!json.contains("\"snapshot\"") && !json.contains("generation") && !json.contains("age_s"));
        assert!(json.contains("\"mode\":\"immutable_snapshot\"") && json.contains("\"state\":\"verified\""));
        assert_ne!(encode_answer_with("fixture", &first, true), encode_answer_with("fixture", &second, true));
        assert!(encode_answer_with("fixture", &first, true).contains("\"snapshot\":\"v1:sha256:ab:1\",\"generation\":1"));
        assert!(encode_answer_with("fixture", &first, true).contains("\"age_s\":0"));
        assert!(encode_text("fixture", &first, true).starts_with("# fixture find v1:sha256:ab:1 g=1\n"));
    }

    #[test]
    fn skeleton_text_lines_carry_id_kind_lines_and_signature() {
        let mut answer = sample();
        answer.verb = Verb::Skeleton;
        answer.outcome = None;
        let mut method = AnswerRecord::basic("s:a.py#Repo::get", "fn", "a.py");
        method.lines = [2, 3];
        method.signature = Some("def get(self,\tkey):".to_string());
        let mut module = AnswerRecord::basic("m:a.py", "module", "a.py");
        module.lines = [1, 9];
        answer.records = vec![module, method];
        let text = encode_text("fixture", &answer, false);
        assert_eq!(text, "# fixture skeleton\nm:a.py\tmodule\tL1-9\ta.py\ns:a.py#Repo::get\tfn\tL2-3\tdef get(self, key):\n");
        assert!(encode_answer("fixture", &answer).contains("\"signature\":\"def get(self,\\tkey):\""));
    }

    fn sample() -> Answer {
        Answer {
            generation: 1,
            snapshot: "v1:sha256:ab:1".to_string(),
            verb: Verb::Find,
            records: vec![AnswerRecord::basic("s:src/lib.rs#ping", "fn", "src/lib.rs")],
            more: 0,
            next: None,
            freshness: Freshness { mode: "immutable_snapshot".to_string(), state: "verified".to_string(), age_s: 0 },
            precision: Precision {
                coverage: "structural".to_string(),
                completion: "complete".to_string(),
                parser: "dctx-structural/2".to_string(),
            },
            outcome: Some(IdentityOutcome::Gone { id: "s:src/lib.rs#removed".to_string() }),
        }
    }
}
