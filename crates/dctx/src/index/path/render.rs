// SPDX-License-Identifier: Apache-2.0
use std::fmt::Write as _;

use super::super::hubs::{PageHead, quote};
use super::model::Hop;

/// Encodes one page. `hops` is `None` for "no path". `json` selects the JSON v2 style object, otherwise the text page.
pub(crate) fn encode(head: &PageHead<'_>, hops: Option<&[Hop]>, json: bool) -> String {
    if json {
        encode_json(head, hops)
    } else {
        encode_text(head, hops)
    }
}

/// Text page: `# <place> path`, then `<edge>:<dir>\t<id>\t<kind>\t<path>` per hop (`start` for the first), or `no path`.
fn encode_text(head: &PageHead<'_>, hops: Option<&[Hop]>) -> String {
    let mut out = format!("# {} path", head.place);
    if head.meta {
        let _ = write!(out, " {} g={}", head.snapshot, head.generation);
    }
    out.push('\n');
    let Some(hops) = hops else {
        out.push_str("no path\n");
        return out;
    };
    for hop in hops {
        let label = if hop.edge.is_empty() {
            "start".to_string()
        } else {
            format!("{}:{}", hop.edge, hop.direction)
        };
        let _ = writeln!(out, "{label}\t{}\t{}\t{}", hop.id, hop.kind, hop.path);
    }
    out
}

/// JSON page with the envelope of the other verbs, `found`, and `records` of hops.
fn encode_json(head: &PageHead<'_>, hops: Option<&[Hop]>) -> String {
    let mut out = String::from("{\"v\":2,\"place\":");
    quote(&mut out, head.place);
    if head.meta {
        out.push_str(",\"snapshot\":");
        quote(&mut out, head.snapshot);
        let _ = write!(out, ",\"generation\":{}", head.generation);
    }
    out.push_str(",\"verb\":\"path\",\"freshness\":{\"mode\":");
    quote(&mut out, head.mode);
    out.push_str(",\"state\":");
    quote(&mut out, head.state);
    if head.meta {
        let _ = write!(out, ",\"age_s\":{}", head.age_s);
    }
    out.push_str("},\"precision\":{\"coverage\":");
    quote(&mut out, head.coverage);
    out.push_str(",\"completion\":");
    quote(&mut out, head.completion);
    out.push_str(",\"parser\":");
    quote(&mut out, head.parser);
    let _ = write!(out, "}},\"found\":{},\"records\":[", hops.is_some());
    for (index, hop) in hops.unwrap_or_default().iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        quote(&mut out, &hop.id);
        out.push_str(",\"kind\":");
        quote(&mut out, &hop.kind);
        out.push_str(",\"path\":");
        quote(&mut out, &hop.path);
        if !hop.edge.is_empty() {
            out.push_str(",\"edge\":");
            quote(&mut out, &hop.edge);
            out.push_str(",\"direction\":");
            quote(&mut out, hop.direction);
        }
        out.push('}');
    }
    out.push_str("]}\n");
    out
}
