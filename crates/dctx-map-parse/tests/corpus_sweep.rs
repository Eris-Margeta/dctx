//! Opt-in, read-only robustness sweep over real source trees.
//! Checks SPEC-MAP-MODE sections 5 and 6.2 through all PureParser adapters.
//! Uses public parser records, std threads, and common_sweep's store checks.
//! Run with DCTX_SWEEP_ROOTS and --ignored --nocapture; ordinary tests skip it.
#![forbid(unsafe_code)]

mod common_sweep;

use common_sweep::{CorpusFile, assert_store_accepts, collect_files, read_source};
use dctx_map_parse::{Language, ParseError, ParsedFile, Parser, PureParser};
use std::any::Any;
use std::collections::BTreeMap;
use std::panic::catch_unwind;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const STACK_BYTES: usize = 16 * 1024 * 1024;
const SLOW_AFTER: Duration = Duration::from_secs(5);
const LANGUAGES: [Language; 15] = [
    Language::Rust,
    Language::JavaScript,
    Language::TypeScript,
    Language::Tsx,
    Language::Markdown,
    Language::Python,
    Language::Go,
    Language::Java,
    Language::C,
    Language::Config,
    Language::CSharp,
    Language::Kotlin,
    Language::Ruby,
    Language::Php,
    Language::Swift,
];

#[derive(Default)]
struct FileReport {
    ok: bool,
    refusal: Option<&'static str>,
    violations: Vec<String>,
}

#[derive(Default)]
struct LanguageReport {
    files: usize,
    ok: usize,
    refused: usize,
    reasons: BTreeMap<&'static str, usize>,
    slowest: Option<(Duration, PathBuf)>,
}

fn panic_reason(payload: &(dyn Any + Send)) -> String {
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("non-string panic payload");
    message
        .lines()
        .next()
        .unwrap_or("empty panic payload")
        .to_owned()
}

fn refusal_reason(error: &ParseError) -> &'static str {
    match error {
        ParseError::InvalidId(_) => "InvalidId",
        ParseError::InvalidUtf8(_) => "InvalidUtf8",
        ParseError::TooLarge => "TooLarge",
        ParseError::Grammar(_) => "Grammar",
        ParseError::NoTree => "NoTree",
        ParseError::Label(_) => "Label",
        ParseError::Syntax { .. } => "Syntax",
    }
}

fn same_result(
    first: &Result<ParsedFile, ParseError>,
    second: &Result<ParsedFile, ParseError>,
) -> bool {
    match (first, second) {
        (Ok(first), Ok(second)) => first == second,
        // ParseError has no PartialEq. Debug retains variant and all error fields.
        (Err(first), Err(second)) => format!("{first:?}") == format!("{second:?}"),
        _ => false,
    }
}

fn checked_parse(
    path: &str,
    source: &[u8],
    attempt: &str,
    report: &mut FileReport,
) -> Option<Result<ParsedFile, ParseError>> {
    match catch_unwind(|| PureParser.parse(path, source)) {
        Ok(result) => {
            if let Ok(facts) = &result {
                let checked = catch_unwind(|| {
                    assert_eq!(facts.path, path, "parsed file has wrong path");
                    assert_eq!(facts.language, Language::for_path(path), "wrong language");
                    assert_store_accepts(facts, source);
                });
                if let Err(payload) = checked {
                    report
                        .violations
                        .push(format!("{attempt} invariant: {}", panic_reason(&*payload)));
                }
            }
            Some(result)
        }
        Err(payload) => {
            report.violations.push(format!(
                "{attempt} parser panic: {}",
                panic_reason(&*payload)
            ));
            None
        }
    }
}

fn inspect_file(path: &str, source: &[u8], repeat: bool) -> FileReport {
    let mut report = FileReport::default();
    let first = checked_parse(path, source, "first parse", &mut report);
    match &first {
        Some(Ok(_)) => report.ok = true,
        Some(Err(error)) => report.refusal = Some(refusal_reason(error)),
        None => {}
    }
    if repeat {
        let second = checked_parse(path, source, "repeat parse", &mut report);
        if let (Some(first), Some(second)) = (&first, &second)
            && !same_result(first, second)
        {
            report
                .violations
                .push("determinism: repeated parse returned different facts or refusal".into());
        }
    }
    report
}

fn run_file(file: &CorpusFile, source: Vec<u8>, repeat: bool) -> Option<(FileReport, Duration)> {
    let parser_path = file.parser_path.clone();
    let (finished, completion) = mpsc::channel();
    let started = Instant::now();
    let worker = match thread::Builder::new()
        .name("corpus-sweep".into())
        .stack_size(STACK_BYTES)
        .spawn(move || {
            let report = inspect_file(&parser_path, &source, repeat);
            let elapsed = started.elapsed();
            let _ = finished.send(());
            (report, elapsed)
        }) {
        Ok(worker) => worker,
        Err(error) => {
            println!("skip {:?}: cannot start parser thread: {error}", file.path);
            return None;
        }
    };
    let flagged = matches!(
        completion.recv_timeout(SLOW_AFTER),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    if flagged {
        println!(
            "SLOW {:?} | {:?} | over 5 s; waiting for result",
            file.path, file.language
        );
    }
    // Timing is diagnostic: join even after the guard fires, so no result is lost
    // and no detached parser threads accumulate while the corpus advances.
    let (report, elapsed) = match worker.join() {
        Ok(result) => result,
        Err(payload) => (
            FileReport {
                violations: vec![format!("worker panic: {}", panic_reason(&*payload))],
                ..FileReport::default()
            },
            started.elapsed(),
        ),
    };
    if elapsed > SLOW_AFTER {
        println!(
            "SLOW {:?} | {:?} | {:.3} s",
            file.path,
            file.language,
            elapsed.as_secs_f64()
        );
    }
    Some((report, elapsed))
}

fn print_table(reports: &BTreeMap<String, LanguageReport>) {
    println!("language | files | ok | refused | refusal reasons | slowest file (wall seconds)");
    for (language, report) in reports {
        let reasons = report
            .reasons
            .iter()
            .map(|(reason, count)| format!("{reason}={count}"))
            .collect::<Vec<_>>()
            .join(", ");
        let slowest = report
            .slowest
            .as_ref()
            .map(|(elapsed, path)| format!("{path:?} ({:.3})", elapsed.as_secs_f64()))
            .unwrap_or_else(|| "-".into());
        println!(
            "{language} | {} | {} | {} | {} | {slowest}",
            report.files,
            report.ok,
            report.refused,
            if reasons.is_empty() { "-" } else { &reasons }
        );
    }
}

#[test]
#[ignore = "needs DCTX_SWEEP_ROOTS"]
fn corpus_sweep() {
    let Some(value) = std::env::var_os("DCTX_SWEEP_ROOTS").filter(|value| !value.is_empty()) else {
        println!("no roots");
        return;
    };
    // Empty components must not silently authorize scanning the current directory.
    let roots = std::env::split_paths(&value)
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();
    if roots.is_empty() {
        println!("no roots");
        return;
    }
    let files = collect_files(roots);
    let mut reports = BTreeMap::<String, LanguageReport>::new();
    for language in LANGUAGES {
        reports.insert(format!("{language:?}"), LanguageReport::default());
    }
    let mut offenders = Vec::new();
    let mut parsed = 0_usize;
    let mut repeated = 0_usize;
    for file in files {
        let Some(source) = read_source(&file.path) else {
            continue;
        };
        let repeat = parsed.is_multiple_of(20);
        let Some((result, elapsed)) = run_file(&file, source, repeat) else {
            continue;
        };
        parsed += 1;
        repeated += usize::from(repeat);
        let report = reports.entry(format!("{:?}", file.language)).or_default();
        report.files += 1;
        report.ok += usize::from(result.ok);
        if let Some(reason) = result.refusal {
            report.refused += 1;
            *report.reasons.entry(reason).or_default() += 1;
        }
        if report
            .slowest
            .as_ref()
            .is_none_or(|(time, _)| elapsed > *time)
        {
            report.slowest = Some((elapsed, file.path.clone()));
        }
        for reason in result.violations {
            let offender = format!(
                "{:?} | {:?} | {}",
                file.path,
                file.language,
                reason.lines().next().unwrap_or("unknown violation")
            );
            println!("OFFENDER {offender}");
            offenders.push(offender);
        }
    }
    print_table(&reports);
    println!(
        "parsed {parsed} files; repeated {repeated}; {} violations",
        offenders.len()
    );
    if !offenders.is_empty() {
        println!(
            "QUESTION: Cut lexer fixes for these corpus offenders:\n{}",
            offenders.join("\n")
        );
    }
    assert!(
        offenders.is_empty(),
        "corpus violations:\n{}",
        offenders.join("\n")
    );
}
