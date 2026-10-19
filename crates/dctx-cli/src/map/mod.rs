//! Read-only map verbs through the facade session.
//! Implements SPEC-MAP-MODE sections 4 and 9.1 and ARCHITECTURE-PROPOSAL §7 row DX-D2.
//! Depends on map-query for request bounds and on the `dctx` facade for the snapshot and the query.
//! The command does not open a home-directory sqlite cache.

mod args;

use dctx::{Direction, MapRequest, SnapshotRequest, VecSink, Verb};
use dctx_map_query::{Direction as QueryDirection, Format, Request, Verb as QueryVerb};

use crate::host;

/// Runs one map verb. Admin verbs are refused before a source is opened.
pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    let page = execute(arguments)?;
    print!("{page}");
    Ok(())
}

/// Same as [`run`], returning the page instead of printing it.
pub(crate) fn execute(arguments: &[String]) -> Result<String, String> {
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["-h"] {
        return Ok(args::HELP.to_string());
    }
    let mut options = args::parse(arguments)?;
    if let Ok(focus) = std::env::var("DCTX_MAP_FOCUS") {
        options.request.focus = focus
            .split([',', '\n'])
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect();
        options.request.validate().map_err(|error| error.to_string())?;
    }
    if options.request.revision.is_some() {
        return Err("git revisions are not available; this build has no git capability".to_string());
    }
    if options.request.offset > 0 {
        return Err("the facade pages with the next cursor, not --offset".to_string());
    }
    let mut session = host::open_dir(&options.place)?;
    let handle = session.snapshot(&SnapshotRequest::strict()).map_err(|error| error.to_string())?;
    let request = facade_request(&options.request, options.format);
    let mut sink = VecSink::new();
    session.query(&handle, &request, &mut sink).map_err(|error| error.to_string())?;
    Ok(String::from_utf8_lossy(sink.as_bytes()).into_owned())
}

fn facade_request(request: &Request, format: Format) -> MapRequest {
    let mut mapped = MapRequest::new(match request.verb {
        QueryVerb::Map => Verb::Map,
        QueryVerb::Find => Verb::Find,
        QueryVerb::Neighbors => Verb::Neighbors,
        QueryVerb::Explain => Verb::Explain,
        QueryVerb::Read => Verb::Read,
    });
    if request.verb == QueryVerb::Find {
        mapped.query = request.target.clone();
    } else {
        mapped.target = request.target.clone();
    }
    mapped.kinds = request.kinds.clone();
    mapped.edges = request.edges.iter().map(|edge| edge.as_str().to_string()).collect();
    mapped.depth = Some(u32::from(request.depth));
    mapped.direction = match request.direction {
        QueryDirection::In => Direction::In,
        QueryDirection::Out => Direction::Out,
        QueryDirection::Both => Direction::Both,
    };
    mapped.limits.records = Some(u64::try_from(request.limit).unwrap_or(u64::MAX));
    mapped.limits.estimated_tokens = Some(u64::from(request.budget.tokens()));
    mapped.limits.encoded_bytes = Some(u64::try_from(request.budget.bytes()).unwrap_or(u64::MAX));
    mapped.limits.depth = Some(u32::from(request.depth));
    mapped.output_format = if matches!(format, Format::Json) { 2 } else { 1 };
    mapped.within = request.within.clone();
    if let Some([start, end]) = request.lines {
        mapped.lines = Some(end.saturating_sub(start).saturating_add(1));
    }
    mapped.full = request.full;
    mapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_commands_are_refused_before_any_cache_write() {
        assert!(execute(&["index".into()]).is_err());
        assert!(execute(&["stats".into()]).is_err());
        assert!(execute(&["summaries".into()]).is_err());
    }

    #[test]
    fn git_revision_and_offset_are_refused_before_a_scan() {
        let revision = execute(&["find".into(), "name".into(), "--rev".into(), "abc".into()]).expect_err("rev");
        assert!(revision.contains("git"));
        let offset = execute(&["find".into(), "name".into(), "--offset".into(), "1".into()]).expect_err("offset");
        assert!(offset.contains("--offset"));
    }

    #[test]
    fn find_uses_the_facade_and_writes_no_project_text() {
        let path = std::env::temp_dir().join(format!("dctx-cli-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp");
        std::fs::write(path.join("kept.rs"), b"fn kept() {}\n").expect("source");
        let page = execute(&[
            "find".into(),
            "kept".into(),
            "--json".into(),
            "--place".into(),
            path.display().to_string(),
        ])
        .expect("find");
        assert!(page.contains("\"id\":\"s:kept.rs#kept\""), "{page}");
        assert!(
            page.contains(
                "\"precision\":{\"coverage\":\"syntax\",\"completion\":\"complete\",\"parser\":\"dctx-lex-rust/syn-3.0.9\"}"
            ),
            "{page}"
        );
        assert!(!path.join(".dctx").exists());
        let listed: Vec<_> = std::fs::read_dir(&path).expect("list").flatten().collect();
        assert_eq!(listed.len(), 1);
        let _ = std::fs::remove_dir_all(&path);
    }
}
