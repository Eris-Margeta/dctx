//! Opt-in, read-only robustness sweep over real source trees.
//! Checks SPEC-MAP-MODE sections 5 and 6.2 through all `PureParser` adapters.
//! Uses public parser records, std threads, and `common_sweep`'s store checks.
//! Opt-in by environment: without `DCTX_SWEEP_ROOTS` the test prints `no roots` and returns. Run with it and --nocapture.
//! `DCTX_SWEEP_LANGS` limits parsing to report language names. `DCTX_SWEEP_REFUSAL_BUDGET`
//! fails the run when a listed language refuses more files than its budget.
#![forbid(unsafe_code)]

mod common_sweep;

use common_sweep::{CorpusFile, assert_store_accepts, collect_files, read_source};
use dctx_map_parse::{Language, ParseError, ParsedFile, Parser, PureParser};
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::catch_unwind;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const STACK_BYTES: usize = 16 * 1024 * 1024;
const SLOW_AFTER: Duration = Duration::from_secs(5);
const MAX_REFUSAL_SAMPLES: usize = 6;
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
    refusal: Option<Refusal>,
    violations: Vec<String>,
}

#[derive(Default)]
struct LanguageReport {
    files: usize,
    ok: usize,
    refused: usize,
    reasons: BTreeMap<String, RefusalReport>,
    slowest: Option<(Duration, PathBuf)>,
}

struct Refusal {
    message: String,
    line: Option<u32>,
}

#[derive(Default)]
struct RefusalReport {
    count: usize,
    samples: Vec<String>,
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

fn refusal_reason(error: &ParseError) -> Refusal {
    // Syntax carries only a location; keep it in the sample, not the group key.
    if let ParseError::Syntax { line, .. } = error {
        return Refusal {
            message: "Syntax: invalid syntax".into(),
            line: Some(*line),
        };
    }
    let kind = match error {
        ParseError::InvalidId(_) => "InvalidId",
        ParseError::InvalidUtf8(_) => "InvalidUtf8",
        ParseError::TooLarge => "TooLarge",
        ParseError::Grammar(_) => "Grammar",
        ParseError::NoTree => "NoTree",
        ParseError::Label(_) => "Label",
        ParseError::Syntax { .. } => "Syntax",
    };
    Refusal {
        message: format!("{kind}: {error}"),
        line: None,
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

#[allow(
    clippy::unnecessary_debug_formatting,
    reason = "Debug formatting quotes and escapes arbitrary corpus paths so diagnostic records remain unambiguous, including paths with control characters."
)]
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

fn optional_env_var(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Some(value),
        Ok(_) | Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => panic!("{name} is not valid Unicode"),
    }
}

fn parse_sweep_langs(value: &str) -> BTreeSet<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_refusal_budget(value: &str) -> Result<BTreeMap<String, usize>, String> {
    let mut budgets = BTreeMap::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((language, limit)) = part.split_once('=') else {
            return Err(format!("refusal budget entry {part:?} must use Lang=N"));
        };
        let language = language.trim();
        let limit = limit.trim();
        if language.is_empty() || limit.is_empty() {
            return Err(format!("refusal budget entry {part:?} must use Lang=N"));
        }
        let parsed = limit.parse::<usize>().map_err(|_| {
            format!("refusal budget for {language} has a non-numeric limit {limit:?}")
        })?;
        if budgets.insert(language.to_owned(), parsed).is_some() {
            return Err(format!("duplicate refusal budget for {language}"));
        }
    }
    Ok(budgets)
}

fn unknown_languages<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let known = LANGUAGES
        .iter()
        .map(|language| format!("{language:?}"))
        .collect::<BTreeSet<_>>();
    let mut unknown = names
        .into_iter()
        .filter(|name| !known.contains(*name))
        .collect::<Vec<_>>();
    unknown.sort_unstable();
    unknown.dedup();
    unknown
}

fn language_selected(selected: Option<&BTreeSet<String>>, language: &str) -> bool {
    selected.is_none_or(|names| names.contains(language))
}

fn format_budget_line(language: &str, refused: usize, budget: usize) -> String {
    // Equal to the budget still passes. Only a greater refusal count exceeds it.
    let status = if refused > budget { "exceeded" } else { "ok" };
    format!("BUDGET {language} refused={refused} budget={budget} {status}")
}

fn budget_failures(
    reports: &BTreeMap<String, LanguageReport>,
    budgets: &BTreeMap<String, usize>,
) -> Vec<String> {
    // Languages with no budget entry are omitted, so their refusals never fail the run.
    budgets
        .iter()
        .filter_map(|(language, budget)| {
            let refused = reports.get(language).map_or(0, |report| report.refused);
            (refused > *budget)
                .then(|| format!("{language} refused {refused} files; budget is {budget}"))
        })
        .collect()
}

#[allow(
    clippy::unnecessary_debug_formatting,
    reason = "Debug formatting quotes and escapes arbitrary corpus paths so diagnostic records remain unambiguous, including paths with control characters."
)]
fn print_table(reports: &BTreeMap<String, LanguageReport>, budgets: &BTreeMap<String, usize>) {
    println!("language | files | ok | refused | slowest file (wall seconds)");
    for (language, report) in reports {
        let slowest = report.slowest.as_ref().map_or_else(
            || "-".into(),
            |(elapsed, path)| format!("{path:?} ({:.3})", elapsed.as_secs_f64()),
        );
        println!(
            "{language} | {} | {} | {} | {slowest}",
            report.files, report.ok, report.refused
        );
        for (message, refusal) in &report.reasons {
            println!(
                "REFUSAL {language} | {message:?} | count={} | samples={}",
                refusal.count,
                refusal.samples.join(", ")
            );
        }
        if let Some(budget) = budgets.get(language).copied() {
            println!("{}", format_budget_line(language, report.refused, budget));
        }
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the corpus traversal, refusal budgets and final report in one test so the acceptance sequence is visible together."
)]
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
    let selected = optional_env_var("DCTX_SWEEP_LANGS").map(|value| parse_sweep_langs(&value));
    if let Some(names) = &selected {
        let unknown = unknown_languages(names.iter().map(String::as_str));
        assert!(
            unknown.is_empty(),
            "DCTX_SWEEP_LANGS has unknown languages: {}",
            unknown.join(", ")
        );
    }
    let budgets = match optional_env_var("DCTX_SWEEP_REFUSAL_BUDGET") {
        Some(value) => parse_refusal_budget(&value)
            .unwrap_or_else(|error| panic!("invalid DCTX_SWEEP_REFUSAL_BUDGET: {error}")),
        None => BTreeMap::new(),
    };
    let unknown = unknown_languages(budgets.keys().map(String::as_str));
    assert!(
        unknown.is_empty(),
        "DCTX_SWEEP_REFUSAL_BUDGET has unknown languages: {}",
        unknown.join(", ")
    );
    let files = collect_files(roots);
    let mut reports = BTreeMap::<String, LanguageReport>::new();
    for language in LANGUAGES {
        reports.insert(format!("{language:?}"), LanguageReport::default());
    }
    let mut offenders = Vec::new();
    let mut parsed = 0_usize;
    let mut repeated = 0_usize;
    for file in files {
        // Discovery order and the per-language cap stay as collect_files left them.
        if !language_selected(selected.as_ref(), &format!("{:?}", file.language)) {
            continue;
        }
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
            let refusal = report.reasons.entry(reason.message).or_default();
            refusal.count += 1;
            if refusal.samples.len() < MAX_REFUSAL_SAMPLES {
                let location = match reason.line {
                    Some(line) => format!("{:?}:{line}", file.path),
                    None => format!("{:?}", file.path),
                };
                refusal.samples.push(location);
            }
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
    print_table(&reports, &budgets);
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
    let exceeded = budget_failures(&reports, &budgets);
    assert!(
        exceeded.is_empty(),
        "refusal budget exceeded:\n{}",
        exceeded.join("\n")
    );
}

#[test]
fn refusal_messages_preserve_detail_and_separate_syntax_locations() {
    let first = refusal_reason(&ParseError::Syntax {
        path: "first.py".into(),
        line: 7,
    });
    let second = refusal_reason(&ParseError::Syntax {
        path: "second.py".into(),
        line: 19,
    });
    assert_eq!(first.message, second.message);
    assert_eq!(first.line, Some(7));
    assert_eq!(second.line, Some(19));

    let first = refusal_reason(&ParseError::Grammar("first diagnostic".into()));
    let second = refusal_reason(&ParseError::Grammar("second diagnostic".into()));
    assert_ne!(first.message, second.message);
    assert!(first.message.contains("first diagnostic"));
    assert_eq!(first.line, None);
}

#[test]
fn sweep_lang_filter_and_refusal_budget_parse() {
    assert_eq!(
        parse_sweep_langs("Ruby,C"),
        BTreeSet::from(["Ruby".to_owned(), "C".to_owned()])
    );
    assert_eq!(
        parse_sweep_langs(" Ruby, C, Ruby ,, "),
        BTreeSet::from(["C".to_owned(), "Ruby".to_owned()])
    );
    assert!(parse_sweep_langs("").is_empty());
    assert!(parse_sweep_langs("  ,  , ").is_empty());

    assert!(language_selected(None, "Swift"));
    let ruby_and_c = parse_sweep_langs("Ruby,C");
    assert!(language_selected(Some(&ruby_and_c), "Ruby"));
    assert!(!language_selected(Some(&ruby_and_c), "Swift"));
    assert!(!language_selected(Some(&BTreeSet::new()), "Ruby"));
    assert_eq!(
        unknown_languages(ruby_and_c.iter().map(String::as_str)),
        [] as [&str; 0]
    );
    assert_eq!(
        unknown_languages(["Unknown", "Swift", "COBOL", "COBOL"]),
        vec!["COBOL", "Unknown"]
    );

    let budgets = parse_refusal_budget("Ruby=120,C=60").unwrap();
    assert_eq!(budgets.get("Ruby").copied(), Some(120));
    assert_eq!(budgets.get("C").copied(), Some(60));
    assert_eq!(
        parse_refusal_budget(" Ruby = 120 , C = 60 , ").unwrap(),
        budgets
    );
    assert_eq!(
        parse_refusal_budget("Swift=0")
            .unwrap()
            .get("Swift")
            .copied(),
        Some(0)
    );
    assert!(parse_refusal_budget("").unwrap().is_empty());
    assert!(parse_refusal_budget(" , ").unwrap().is_empty());
    assert!(parse_refusal_budget("Ruby").unwrap_err().contains("Lang=N"));
    assert!(parse_refusal_budget("=12").unwrap_err().contains("Lang=N"));
    assert!(
        parse_refusal_budget("Ruby=")
            .unwrap_err()
            .contains("Lang=N")
    );
    assert!(
        parse_refusal_budget("Ruby=many")
            .unwrap_err()
            .contains("non-numeric")
    );
    assert!(
        parse_refusal_budget("Ruby=-1")
            .unwrap_err()
            .contains("non-numeric")
    );
    assert!(
        parse_refusal_budget("Ruby=120,Ruby=60")
            .unwrap_err()
            .contains("duplicate")
    );
    assert_eq!(
        unknown_languages(budgets.keys().map(String::as_str)),
        [] as [&str; 0]
    );

    let mut reports = BTreeMap::new();
    reports.insert(
        "C".to_owned(),
        LanguageReport {
            refused: 61,
            ..LanguageReport::default()
        },
    );
    reports.insert(
        "Ruby".to_owned(),
        LanguageReport {
            refused: 120,
            ..LanguageReport::default()
        },
    );
    reports.insert(
        "Swift".to_owned(),
        LanguageReport {
            refused: 500,
            ..LanguageReport::default()
        },
    );
    assert_eq!(
        format_budget_line("C", 61, 60),
        "BUDGET C refused=61 budget=60 exceeded"
    );
    assert_eq!(
        format_budget_line("Ruby", 120, 120),
        "BUDGET Ruby refused=120 budget=120 ok"
    );
    assert_eq!(
        budget_failures(&reports, &budgets),
        vec!["C refused 61 files; budget is 60".to_owned()]
    );
    let mut within = budgets.clone();
    within.insert("C".to_owned(), 61);
    assert_eq!(
        budget_failures(&reports, &within),
        [] as [std::string::String; 0]
    );
    assert_eq!(
        budget_failures(&reports, &BTreeMap::new()),
        [] as [std::string::String; 0]
    );
    assert_eq!(
        format_budget_line("Swift", 0, 100),
        "BUDGET Swift refused=0 budget=100 ok"
    );
}
