//! One bounded answer for each of the five current map verbs.
//! Implements ARCHITECTURE-PROPOSAL §3 and SPEC-MAP-MODE §4 for Stage 0.
//! Depends on dctx-map-query; the measure adapter stores the JSON answers.

use crate::report::VerbAnswer;
use dctx_map_query::{FileSource, Format, Index, Request, Verb, render_answer_at};
use serde_json::Value;

/// Map, find, and the three id-following verbs. A missing id is a recorded refusal.
pub(crate) fn five_verbs(
    index: &Index,
    source: &FileSource,
    place: &str,
    map_request: &Request,
    map_answer: &dctx_map_query::Answer,
    find_target: &str,
) -> Vec<VerbAnswer> {
    let map_verb = rendered("map", "", || {
        render_answer_at(map_answer, map_request, Format::Json, Some(place))
    });
    let find_verb = ask(index, source, place, Verb::Find, find_target);
    let identity = map_verb
        .answer
        .as_ref()
        .and_then(first_id)
        .or_else(|| find_verb.answer.as_ref().and_then(first_id));
    let mut verbs = vec![map_verb, find_verb];
    match identity {
        Some(id) => {
            verbs.push(ask(index, source, place, Verb::Neighbors, &id));
            verbs.push(ask(index, source, place, Verb::Explain, &id));
            verbs.push(ask(index, source, place, Verb::Read, &id));
        }
        None => {
            for verb in ["neighbors", "explain", "read"] {
                verbs.push(VerbAnswer {
                    verb: verb.into(),
                    target: String::new(),
                    ok: false,
                    answer: None,
                    error: Some("no stable id in the map or find answer".into()),
                });
            }
        }
    }
    verbs
}

fn ask(index: &Index, source: &FileSource, place: &str, verb: Verb, target: &str) -> VerbAnswer {
    let name = verb_name(verb);
    let request = Request::new(verb, target);
    rendered(name, target, || {
        let answer = index.answer(source, &request)?;
        render_answer_at(&answer, &request, Format::Json, Some(place))
    })
}

fn rendered(
    verb: &str,
    target: &str,
    render: impl FnOnce() -> Result<String, dctx_map_query::Error>,
) -> VerbAnswer {
    match render().and_then(|text| {
        serde_json::from_str::<Value>(&text).map_err(|error| {
            dctx_map_query::Error::Request(format!("verb JSON was not an object: {error}"))
        })
    }) {
        Ok(answer) => VerbAnswer {
            verb: verb.into(),
            target: target.into(),
            ok: true,
            answer: Some(answer),
            error: None,
        },
        Err(error) => VerbAnswer {
            verb: verb.into(),
            target: target.into(),
            ok: false,
            answer: None,
            error: Some(error.to_string()),
        },
    }
}

fn verb_name(verb: Verb) -> &'static str {
    match verb {
        Verb::Map => "map",
        Verb::Find => "find",
        Verb::Neighbors => "neighbors",
        Verb::Explain => "explain",
        Verb::Read => "read",
    }
}

fn first_id(value: &Value) -> Option<String> {
    value.get("records")?.as_array()?.iter().find_map(|record| {
        record
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
    })
}
