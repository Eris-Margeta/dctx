// SPDX-License-Identifier: Apache-2.0
//! One-call bounded trace and impact pages, shared by CLI and tool transports.

use super::args::Options;
use dctx::{
    AnswerLocation, IdentityOutcome, ImpactAnswer, MapRequest, Session, SnapshotHandle,
    TraceAnswer, VecSink, Verb,
};
use dctx_map_query::Format;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(super) struct Bounds {
    pub depth: usize,
    pub paths: usize,
    pub sites: usize,
    pub bytes: usize,
    pub trace: bool,
}

pub(super) fn parse(args: &[String]) -> Result<Options, String> {
    let trace = args[0] == "trace";
    let mut bounds = Bounds {
        depth: 8,
        paths: 10,
        sites: 1000,
        bytes: 65536,
        trace,
    };
    let mut common = vec!["neighbors".to_string()];
    let mut targets = Vec::new();
    let mut index = 1;
    let mut literal = false;
    while index < args.len() {
        let flag = &args[index];
        index += 1;
        if literal {
            targets.push(flag.clone());
            continue;
        }
        match flag.as_str() {
            "--" => literal = true,
            "--json" => common.push(flag.clone()),
            "--place" => {
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                index += 1;
                common.extend([flag.clone(), value.clone()]);
            }
            "--max-depth" | "--depth" | "--max-paths" | "--max-sites" | "--max-bytes" => {
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                index += 1;
                let (slot, min, max) = match flag.as_str() {
                    "--max-depth" | "--depth" => (&mut bounds.depth, 1, 16),
                    "--max-paths" if trace => (&mut bounds.paths, 1, 100),
                    "--max-sites" if !trace => (&mut bounds.sites, 1, 10000),
                    "--max-bytes" => (&mut bounds.bytes, 256, 1048576),
                    _ => {
                        return Err(format!(
                            "unknown or inapplicable option {flag} for {}",
                            args[0]
                        ))
                    }
                };
                *slot = value
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (min..=max).contains(n))
                    .ok_or_else(|| {
                        format!(
                            "{flag} {value} for {} (allowed range {min}..={max})",
                            args[0]
                        )
                    })?;
            }
            _ if flag.starts_with('-') => {
                return Err(format!(
                    "unknown or inapplicable option {flag} for {}",
                    args[0]
                ))
            }
            _ => targets.push(flag.clone()),
        }
    }
    if targets.len() != if trace { 2 } else { 1 } || targets.iter().any(String::is_empty) {
        return Err(if trace {
            "trace requires two targets: trace <A> <B>"
        } else {
            "impact requires a target: impact <SYMBOL>"
        }
        .to_string());
    }
    common.extend(["--".to_string(), targets.remove(0)]);
    let mut options = super::args::parse(&common)?;
    options.to = targets.pop();
    options.answer = Some(bounds);
    Ok(options)
}

pub(super) fn answer(
    session: &mut Session,
    handle: &SnapshotHandle,
    options: &Options,
    also: &[std::path::PathBuf],
    imports: &crate::host::Imports,
) -> Result<String, String> {
    let bounds = options.answer.as_ref().ok_or("missing answer bounds")?;
    let target = &options.request.target;
    let json = options.format == Format::Json;
    let page = if bounds.trace {
        let to = options.to.as_deref().ok_or("trace requires two targets")?;
        let mut answer = session
            .trace_paths(handle, target, to, bounds.depth, bounds.paths)
            .map_err(|e| e.to_string())?;
        // Outside notices count towards the same byte cap. Probe once; trimming never rescans.
        let notices = notices(session, handle, options, also, imports, &[target, to])?;
        loop {
            let page = with_notices(render_trace(&answer, bounds, json), &notices, json)?;
            if page.len() <= bounds.bytes {
                break page;
            }
            answer.truncated = true;
            if answer.paths.pop().is_none() {
                return Err(format!(
                    "--max-bytes {} cannot fit trace metadata and outside-index notices",
                    bounds.bytes
                ));
            }
        }
    } else {
        let answer = session
            .symbol_impact(handle, target, bounds.depth, bounds.sites)
            .map_err(|e| e.to_string())?;
        let notices = notices(session, handle, options, also, imports, &[target])?;
        let render =
            |keep| with_notices(render_impact(&answer, bounds, json, keep), &notices, json);
        let total = answer.direct.len() + answer.callers.len();
        let full = render(total)?;
        if full.len() <= bounds.bytes {
            return Ok(full);
        }
        // Keep the largest prefix, with direct sites before transitive callers.
        let (mut low, mut high) = (0, total);
        while low < high {
            let mid = low + (high - low).div_ceil(2);
            if render(mid)?.len() <= bounds.bytes {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        let page = render(low)?;
        if page.len() > bounds.bytes {
            return Err(format!(
                "--max-bytes {} cannot fit impact metadata and outside-index notices",
                bounds.bytes
            ));
        }
        page
    };
    Ok(page)
}

fn location(loc: &AnswerLocation) -> Value {
    json!({"id": loc.id, "file": loc.file, "line": loc.line, "symbol": loc.symbol, "depth": loc.depth})
}

fn render_trace(answer: &TraceAnswer, bounds: &Bounds, json_output: bool) -> String {
    if json_output {
        let paths: Vec<_> = answer
            .paths
            .iter()
            .map(|p| p.iter().map(location).collect::<Vec<_>>())
            .collect();
        return format!(
            "{}\n",
            json!({
                "v": dctx::WIRE_VERSION,
                "verb": "trace",
                "paths": paths,
                "counts": {"paths": answer.paths.len()},
                "truncated": answer.truncated,
                "bounds": {"max_depth": bounds.depth, "max_paths": bounds.paths, "max_bytes": bounds.bytes}
            })
        );
    }
    let mut page = String::new();
    for path in &answer.paths {
        page.push_str(
            &path
                .iter()
                .map(|loc| format!("{}:{} {}", loc.file, loc.line, loc.symbol))
                .collect::<Vec<_>>()
                .join(" -> "),
        );
        page.push('\n');
    }
    if answer.paths.is_empty() {
        page.push_str("no path within bounds\n");
    }
    if answer.truncated {
        page.push_str("truncated: depth, paths or bytes bound reached\n");
    }
    page
}

fn render_impact(answer: &ImpactAnswer, bounds: &Bounds, json_output: bool, keep: usize) -> String {
    let direct = &answer.direct[..keep.min(answer.direct.len())];
    let callers = &answer.callers[..keep
        .saturating_sub(answer.direct.len())
        .min(answer.callers.len())];
    let truncated = answer.truncated || keep < answer.direct.len() + answer.callers.len();
    let mut files: BTreeMap<&str, (Vec<&AnswerLocation>, Vec<&AnswerLocation>)> = BTreeMap::new();
    for loc in direct {
        files.entry(&loc.file).or_default().0.push(loc);
    }
    for loc in callers {
        files.entry(&loc.file).or_default().1.push(loc);
    }
    if json_output {
        let groups: Vec<_> = files
            .iter()
            .map(|(file, (direct, callers))| {
                json!({
                    "file": file,
                    "direct": direct.iter().map(|l| location(l)).collect::<Vec<_>>(),
                    "callers": callers.iter().map(|l| location(l)).collect::<Vec<_>>(),
                    "counts": {"direct_sites": direct.len(), "transitive_callers": callers.len()}
                })
            })
            .collect();
        return format!(
            "{}\n",
            json!({
                "v": dctx::WIRE_VERSION,
                "verb": "impact",
                "files": groups,
                "counts": {"files": files.len(), "direct_sites": direct.len(), "transitive_callers": callers.len()},
                "truncated": truncated,
                "bounds": {"max_depth": bounds.depth, "max_sites": bounds.sites, "max_bytes": bounds.bytes}
            })
        );
    }
    let mut page = format!(
        "impact: {} direct sites, {} transitive callers, {} files\n",
        direct.len(),
        callers.len(),
        files.len()
    );
    for (file, (direct, callers)) in files {
        page.push_str(&format!(
            "{file} ({} direct sites, {} transitive callers)\n",
            direct.len(),
            callers.len()
        ));
        for loc in direct {
            page.push_str(&format!(
                "  {}:{} {} [direct]\n",
                loc.file, loc.line, loc.symbol
            ));
        }
        for loc in callers {
            page.push_str(&format!(
                "  {}:{} {} [caller depth {}]\n",
                loc.file, loc.line, loc.symbol, loc.depth
            ));
        }
    }
    if direct.is_empty() && callers.is_empty() {
        page.push_str("no references or callers within bounds\n");
    }
    if truncated {
        page.push_str("truncated: depth, sites or bytes bound reached\n");
    }
    page
}

fn notices(
    session: &mut Session,
    handle: &SnapshotHandle,
    options: &Options,
    also: &[std::path::PathBuf],
    imports: &crate::host::Imports,
    targets: &[&str],
) -> Result<Value, String> {
    let mut notices = Vec::new();
    for target in targets {
        // Names use a metadata-only find; stable ids use explain so they are not
        // mistaken for missing names. Neither probe reads source or invokes fallback.
        let indexed = if target.contains(':') || target.starts_with('@') {
            let mut request = MapRequest::new(Verb::Explain);
            request.target = (*target).to_owned();
            session
                .query(handle, &request, &mut VecSink::new())
                .map_or(true, |answer| {
                    !matches!(answer.outcome, Some(IdentityOutcome::Gone { .. }))
                })
        } else {
            crate::host::indexed_name(session, handle, target, None)
        };
        if indexed {
            continue;
        }
        // A reusable generation need not read any source while answering trace.
        // On a miss, reuse find's literal lookup to capture imports from the
        // indexed reference sites before probing their outside definitions.
        let mut find = MapRequest::new(Verb::Find);
        find.query = target
            .rsplit_once('#')
            .map_or(*target, |(_, name)| name)
            .split('~')
            .next()
            .unwrap_or(target)
            .to_owned();
        session
            .query(handle, &find, &mut VecSink::new())
            .map_err(|error| error.to_string())?;
        let candidates = imports.candidates(None);
        if candidates.is_empty() {
            continue;
        }
        let page = crate::host::outside_notice(
            &options.place,
            also,
            target,
            "{}".to_string(),
            true,
            &candidates,
        );
        if let Ok(value) = serde_json::from_str::<Value>(&page) {
            for notice in value["defined_outside_index"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if !notices.contains(notice) {
                    notices.push(notice.clone());
                }
            }
        }
    }
    Ok(Value::Array(notices))
}

fn with_notices(mut page: String, notices: &Value, json_output: bool) -> Result<String, String> {
    if notices.as_array().is_none_or(Vec::is_empty) {
        return Ok(page);
    }
    if json_output {
        let mut value: Value = serde_json::from_str(&page).map_err(|e| e.to_string())?;
        value["defined_outside_index"] = notices.clone();
        return Ok(format!("{value}\n"));
    }
    for notice in notices.as_array().into_iter().flatten() {
        page.push_str(&format!(
            "defined outside index: {}\n{}\n",
            notice["path"].as_str().unwrap_or(""),
            notice["hint"].as_str().unwrap_or("")
        ));
    }
    Ok(page)
}
