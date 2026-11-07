// SPDX-License-Identifier: Apache-2.0
//! Read-only map verbs through the facade session.
//! Implements SPEC-MAP-MODE sections 4 and 9.1 and ARCHITECTURE-PROPOSAL §7 row DX-D2.
//! Depends on map-query for request bounds and on the `dctx` facade for the snapshot and the query.
//! The command reuses a verified generation from the shared cache root.
//! It does not write inside the scanned project. A bad target is refused before the scan.

mod args;
mod export_verb;
mod failure;
mod file_page;
mod hubs;
mod path_verb;
mod skeleton;
mod target;
mod verb_help;

use dctx::{Direction, IdentityOutcome, MapRequest, SnapshotRequest, VecSink, Verb};
use dctx_map_query::{Direction as QueryDirection, Format, Request, Verb as QueryVerb};

use crate::exit::{self, Failure};
use crate::host;

/// Runs one map verb and prints its page. A `gone` page is printed, then the run fails with exit 1 (docs/CLI-EXIT-CODES.md).
pub(crate) fn run(arguments: &[String]) -> Result<(), Failure> {
    let answered =
        answer_with(arguments, host::open_mapped).map_err(|failure| failure.within("map"))?;
    print!("{}", answered.page);
    match answered.missing {
        Some(failure) => Err(failure.within("map")),
        None => Ok(()),
    }
}

/// Same as [`run`], returning the page instead of printing it. A `gone` page is a page here, not an error.
pub(crate) fn execute(arguments: &[String]) -> Result<String, String> {
    execute_with(arguments, host::open_mapped)
}

/// Same as [`execute`] for `dctx pack --for`: the page, or the failure with its exit code. A `gone` page is a page here.
pub(crate) fn answer(arguments: &[String]) -> Result<String, Failure> {
    answer_with(arguments, host::open_mapped).map(|answered| answered.page)
}

fn execute_with(
    arguments: &[String],
    open: impl FnOnce(&std::path::Path) -> Result<(dctx::Session, String), String>,
) -> Result<String, String> {
    answer_with(arguments, open)
        .map(|answered| answered.page)
        .map_err(|failure| {
            if failure.hint.is_empty() {
                failure.message
            } else {
                format!("{}\nhint: {}", failure.message, failure.hint)
            }
        })
}

/// One answered map verb: the page, and the exit-1 failure that follows a `gone` page.
struct Answered {
    /// The page for stdout.
    page: String,
    /// Set when the facade answered that the target is not in the index.
    missing: Option<Failure>,
}

impl Answered {
    /// A page with no failure after it.
    fn page(page: String) -> Self {
        Self {
            page,
            missing: None,
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the ordered parse, target, snapshot and query transaction with its existing error ordering."
)]
fn answer_with(
    arguments: &[String],
    open: impl FnOnce(&std::path::Path) -> Result<(dctx::Session, String), String>,
) -> Result<Answered, Failure> {
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["-h"] {
        return Ok(Answered::page(args::HELP.to_string()));
    }
    if arguments[1..]
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        if let Some(help) = verb_help::text(&arguments[0]) {
            return Ok(Answered::page(help));
        }
    }
    let usage = |message: String| failure::usage(arguments, message);
    let mut options = args::parse(arguments).map_err(usage)?;
    if let Ok(focus) = std::env::var("DCTX_MAP_FOCUS") {
        options.request.focus = focus
            .split([',', '\n'])
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect();
        options
            .request
            .validate()
            .map_err(|error| usage(error.to_string()))?;
    }
    if options.request.revision.is_some() {
        return Err(usage(
            "git revisions are not available; this build has no git capability".to_string(),
        ));
    }
    if options.request.offset > 0 {
        return Err(usage(
            "the facade pages with the next cursor, not --offset".to_string(),
        ));
    }
    if !options.hubs && options.export.is_none() {
        options.request.target = target::prepare(
            options.request.verb,
            &options.request.target,
            &options.place,
        )
        .map_err(usage)?;
        if let Some(to) = options.to.take() {
            options.to =
                Some(target::prepare(options.request.verb, &to, &options.place).map_err(usage)?);
        }
        failure::targets(&options)?;
    }
    file_page::widen(&mut options).map_err(usage)?;
    let profiling = profile_requested(options.profile);
    let (mut session, key) =
        open(&options.place).map_err(|message| failure::place(&options.place, &message))?;
    if profiling {
        session.enable_profile();
    }
    // Reuse checks content with digest_text, the same comparison as read_verified, except for a file whose size, mtime
    // (older than 2 s) and inode are all unchanged: that stamp is trusted. DCTX_FRESHNESS=strict_live hashes every file.
    let built = if profiling {
        session.snapshot_reusable_for_profile(&SnapshotRequest::strict(), &key)
    } else {
        session.snapshot_reusable(&SnapshotRequest::strict(), &key)
    };
    let handle = match built {
        Ok(handle) => handle,
        Err(error) => {
            if profiling {
                session.discard_profile();
            }
            return Err(failure::snapshot(&error));
        }
    };
    if host::reuse_report_enabled() {
        let status = if handle.reused() {
            "generation reused"
        } else {
            "generation built"
        };
        eprintln!("{status}");
        eprintln!("content_reads {}", session.content_reads());
        eprintln!("cached_content_reads {}", session.cached_content_reads());
        eprintln!("stamp_skipped_reads {}", session.stamp_skipped_reads());
    }
    if options.hubs {
        return hubs::answer(&mut session, &handle, &options)
            .map(Answered::page)
            .map_err(failure::answer);
    }
    if options.export.is_some() {
        return export_verb::answer(&mut session, &handle, &options)
            .map(Answered::page)
            .map_err(failure::answer);
    }
    if options.to.is_some() {
        return path_verb::answer(&mut session, &handle, &options)
            .map(Answered::page)
            .map_err(failure::answer);
    }
    let mut request = facade_request(
        &options.request,
        options.format,
        options.continuation,
        options.budget_set,
    );
    request.meta = options.meta;
    if options.grep {
        // The CLI parses grep as a find-shaped request; the facade knows the real verb.
        request.verb = Verb::Grep;
        request.ignore_case = options.ignore_case;
    }
    if options.skeleton {
        skeleton::retarget(&mut request, options.budget_set);
    }
    let mut sink = VecSink::new();
    let queried = if profiling {
        session.query_for_profile(&handle, &request, &mut sink)
    } else {
        session.query(&handle, &request, &mut sink)
    };
    let answer = match queried {
        Ok(answer) => answer,
        Err(error) => {
            if profiling {
                session.discard_profile();
            }
            return Err(exit::from_facade(&error, &failure::help_command(arguments)));
        }
    };
    if profiling {
        if let Some(report) = session.profile_report() {
            eprint!("{report}");
        }
    }
    let missing = match &answer.outcome {
        Some(IdentityOutcome::Gone { id }) => Some(failure::gone(id)),
        _ => None,
    };
    Ok(Answered {
        page: String::from_utf8_lossy(sink.as_bytes()).into_owned(),
        missing,
    })
}

/// `--profile` or `DCTX_PROFILE=1`. Map switches are flags; this env matches `DCTX_REUSE_REPORT`.
fn profile_requested(flag: bool) -> bool {
    flag || std::env::var("DCTX_PROFILE").ok().as_deref() == Some("1")
}

fn facade_request(
    request: &Request,
    format: Format,
    continuation: Option<String>,
    budget_set: bool,
) -> MapRequest {
    let single = matches!(request.verb, QueryVerb::Explain | QueryVerb::Read);
    let mut mapped = MapRequest::new(match request.verb {
        QueryVerb::Map => Verb::Map,
        QueryVerb::Find => Verb::Find,
        QueryVerb::Neighbors => Verb::Neighbors,
        QueryVerb::Explain => Verb::Explain,
        QueryVerb::Read => Verb::Read,
    });
    if request.verb == QueryVerb::Find {
        mapped.query.clone_from(&request.target);
    } else {
        mapped.target.clone_from(&request.target);
    }
    mapped.kinds.clone_from(&request.kinds);
    mapped.edges = request
        .edges
        .iter()
        .map(|edge| edge.as_str().to_string())
        .collect();
    mapped.direction = match request.direction {
        QueryDirection::In => Direction::In,
        QueryDirection::Out => Direction::Out,
        QueryDirection::Both => Direction::Both,
    };
    // Explain and read hard-cap at one record and depth 0. The query default is 60 records and depth 1.
    mapped.limits.records = if single {
        Some(1)
    } else {
        Some(u64::try_from(request.limit).unwrap_or(u64::MAX))
    };
    // map-query's read default is 500 tokens (1,750 bytes). A module page is larger, and the
    // facade drops the one record instead of cutting it. Leave the budget unset so the facade
    // read default (its hard cap) applies. An explicit --budget is forwarded unchanged.
    if request.verb != QueryVerb::Read || budget_set {
        mapped.limits.estimated_tokens = Some(u64::from(request.budget.tokens()));
        mapped.limits.encoded_bytes =
            Some(u64::try_from(request.budget.bytes()).unwrap_or(u64::MAX));
    }
    mapped.output_format = if matches!(format, Format::Json) { 2 } else { 1 };
    mapped.within.clone_from(&request.within);
    mapped.continuation = continuation;
    mapped.full = request.full;
    if let Some([start, end]) = request.lines {
        mapped.lines = Some(end.saturating_sub(start).saturating_add(1));
        mapped.line_start = Some(start);
    }
    // Explain and read hard-cap depth at 0. The absolute line stays in line_start.
    if single {
        mapped.depth = None;
        mapped.limits.depth = None;
    } else {
        mapped.depth = Some(u32::from(request.depth));
        mapped.limits.depth = Some(u32::from(request.depth));
    }
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
    fn grep_finds_a_literal_and_groups_hits_by_symbol() {
        let fixture = tempfile::tempdir().expect("fixture");
        let path = fixture.path().join("place");
        let cache = fixture.path().join("cache");
        std::fs::create_dir_all(&path).expect("temp");
        std::fs::write(
            path.join("a.py"),
            b"def alpha():\n    x = \"NEEDLE one\"\n\ndef beta():\n    return \"needle two\"\n",
        )
        .expect("source");
        std::fs::write(path.join(".env"), b"NEEDLE=secret\n").expect("env");
        let run = |extra: &[&str]| {
            let mut arguments: Vec<String> = vec![
                "grep".into(),
                "NEEDLE".into(),
                "--place".into(),
                path.display().to_string(),
            ];
            arguments.extend(extra.iter().map(ToString::to_string));
            execute_with(&arguments, |place| {
                host::open_mapped_at(place, Some(&cache))
            })
            .expect("grep")
        };
        let page = run(&[]);
        assert!(page.starts_with("# "), "{page}");
        assert!(page.contains(" grep\n"), "{page}");
        assert!(
            page.contains("s:a.py#alpha\t") && page.contains("\ta.py\n  L2: x = \"NEEDLE one\"\n"),
            "{page}"
        );
        assert!(!page.contains("beta") && !page.contains("secret"), "{page}");
        assert!(page.ends_with("1 hits in 1 symbols\n"), "{page}");
        assert!(run(&["-i"]).contains("s:a.py#beta"));
        assert!(run(&["--in", "nowhere"]).ends_with("0 hits\n"));
    }

    #[test]
    fn git_revision_and_offset_are_refused_before_a_scan() {
        let revision = execute(&["find".into(), "name".into(), "--rev".into(), "abc".into()])
            .expect_err("rev");
        assert!(revision.contains("git"));
        let offset = execute(&["find".into(), "name".into(), "--offset".into(), "1".into()])
            .expect_err("offset");
        assert!(offset.contains("--offset"));
    }

    #[test]
    fn map_without_a_target_is_refused_before_a_scan() {
        let error = execute(&[
            "map".into(),
            "--place".into(),
            "/no/such/dctx-e02c-place".into(),
        ])
        .expect_err("target");
        assert!(error.contains("requires a target"), "{error}");
        assert!(!error.contains("os error"), "{error}");
    }

    #[test]
    fn explain_and_read_stay_inside_the_facade_caps() {
        let explain = Request::new(QueryVerb::Explain, "s:a.rs#kept");
        let mapped = facade_request(&explain, Format::Json, None, false);
        assert_eq!(mapped.limits.records, Some(1));
        assert!(mapped.depth.is_none());
        assert!(mapped.limits.depth.is_none());
        assert_eq!(mapped.limits.estimated_tokens, Some(400));

        let mut read = Request::new(QueryVerb::Read, "m:a.rs");
        read.lines = Some([35, 36]);
        let mapped = facade_request(&read, Format::Json, Some("v1|1|read|m:a.rs".into()), false);
        assert_eq!(mapped.limits.records, Some(1));
        assert_eq!(mapped.lines, Some(2));
        assert_eq!(mapped.line_start, Some(35));
        assert!(
            mapped.depth.is_none(),
            "absolute lines must leave depth unset"
        );
        assert!(mapped.limits.depth.is_none());
        assert_eq!(mapped.continuation.as_deref(), Some("v1|1|read|m:a.rs"));
        assert!(mapped.limits.estimated_tokens.is_none());
        assert!(mapped.limits.encoded_bytes.is_none());
    }

    #[test]
    fn an_explicit_read_budget_is_forwarded_and_the_default_is_not_500() {
        let plain = args::parse(&["read".into(), "m:a.rs".into()]).expect("parse");
        let mapped = facade_request(
            &plain.request,
            plain.format,
            plain.continuation,
            plain.budget_set,
        );
        assert!(
            mapped.limits.estimated_tokens.is_none(),
            "default read must not send 500 tokens"
        );
        assert!(mapped.limits.encoded_bytes.is_none());

        let tiny = args::parse(&[
            "read".into(),
            "m:a.rs".into(),
            "--budget".into(),
            "8".into(),
        ])
        .expect("tiny");
        let mapped = facade_request(
            &tiny.request,
            tiny.format,
            tiny.continuation,
            tiny.budget_set,
        );
        assert_eq!(mapped.limits.estimated_tokens, Some(8));
        assert_eq!(
            mapped.limits.encoded_bytes,
            Some(u64::try_from(tiny.request.budget.bytes()).unwrap_or(0))
        );

        let map = args::parse(&["map".into(), "m:a.rs".into(), "--depth".into(), "2".into()])
            .expect("map");
        let mapped = facade_request(&map.request, map.format, map.continuation, map.budget_set);
        assert_eq!(mapped.limits.estimated_tokens, Some(500));
        assert_eq!(mapped.depth, Some(2));
        assert!(mapped.line_start.is_none());
    }

    #[test]
    fn an_existing_file_path_maps_like_its_module_id() {
        let fixture = tempfile::tempdir().expect("fixture");
        let path = fixture.path().join("place");
        let cache = fixture.path().join("cache");
        let execute = |arguments: &[String]| {
            execute_with(arguments, |place| host::open_mapped_at(place, Some(&cache)))
        };
        std::fs::create_dir_all(path.join("nested")).expect("temp");
        std::fs::write(path.join("kept.rs"), b"fn kept() {}\n").expect("source");
        let place = path.display().to_string();
        let by_id = execute(&[
            "map".into(),
            "m:kept.rs".into(),
            "--json".into(),
            "--place".into(),
            place.clone(),
        ])
        .expect("id");
        let by_path = execute(&[
            "map".into(),
            "kept.rs".into(),
            "--json".into(),
            "--place".into(),
            place,
        ])
        .expect("path");
        let without_age = |page: &str| {
            use dctx_plugin_protocol::{decode_object, DecodeLimits, Json};

            let Json::Object(mut fields) =
                decode_object(page.as_bytes(), DecodeLimits::WIRE).expect("JSON page")
            else {
                panic!("page must be an object");
            };
            let (_, Json::Object(freshness)) = fields
                .iter_mut()
                .find(|(key, _)| key == "freshness")
                .expect("freshness")
            else {
                panic!("freshness must be an object");
            };
            // Separate calls can straddle a second boundary; all other page fields must match.
            freshness.retain(|(key, _)| key != "age_s");
            Json::Object(fields)
        };
        assert_eq!(without_age(&by_path), without_age(&by_id));
        assert!(by_path.contains("\"id\":\"s:kept.rs#kept\""), "{by_path}");
        assert!(!by_path.contains("gone"), "{by_path}");
        let directory = execute(&[
            "map".into(),
            "nested".into(),
            "--place".into(),
            path.display().to_string(),
        ])
        .expect_err("dir");
        assert!(directory.contains("directory"), "{directory}");
    }

    #[test]
    fn find_uses_the_facade_and_writes_no_project_text() {
        let fixture = tempfile::tempdir().expect("fixture");
        let path = fixture.path().join("place");
        let cache = fixture.path().join("cache");
        std::fs::create_dir_all(&path).expect("temp");
        std::fs::write(path.join("kept.rs"), b"fn kept() {}\n").expect("source");
        // A temp place still uses the user-wide ledger unless the cache is injected.
        // Parallel nextest processes must not contend on that ambient ledger lock.
        let page = execute_with(
            &[
                "find".into(),
                "kept".into(),
                "--json".into(),
                "--place".into(),
                path.display().to_string(),
            ],
            |place| host::open_mapped_at(place, Some(&cache)),
        )
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
        assert!(
            cache.join("ledger.v1").is_file(),
            "the facade must use the isolated persistent cache"
        );
    }
}
