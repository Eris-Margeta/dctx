// SPDX-License-Identifier: Apache-2.0
//! Exit-code failures of the map verbs (docs/CLI-EXIT-CODES.md), each with a hint the user can act on.
//! Covers argument refusals, ids that climb out of the place, binary file targets, places that cannot be opened and ids the index does not hold.
//! Used by the parent module before the scan (arguments, targets) and after the facade query (outcomes, answer errors).

use std::path::Path;

use dctx_map_query::Verb as QueryVerb;

use super::args::Options;
use crate::exit::{self, Failure};

/// Bytes of a target file sniffed for a NUL byte, the indexer's binary test.
const SNIFF_BYTES: u64 = 8192;

/// The help command a hint names: `dctx map <verb> --help`, or `dctx map --help` when the first word is not a verb.
pub(super) fn help_command(arguments: &[String]) -> String {
    match arguments.first() {
        Some(verb) if super::verb_help::text(verb).is_some() => format!("dctx map {verb} --help"),
        _ => "dctx map --help".to_string(),
    }
}

/// An argument refusal: exit 2, with a hint fitted to a missing target or else naming the verb's help.
pub(super) fn usage(arguments: &[String], message: String) -> Failure {
    let verb = arguments.first().map_or("", String::as_str);
    let hint = if message.contains("requires a target") || message.contains("requires two targets")
    {
        match verb {
            "find" => "pass a non-empty name or phrase, for example: dctx map find alpha".to_string(),
            "grep" => "pass a non-empty literal, for example: dctx map grep TODO".to_string(),
            _ => "pass a stable id (list them with `dctx map find <name>`) or a place-relative file path".to_string(),
        }
    } else {
        format!(
            "run `{}` for the options and an example",
            help_command(arguments)
        )
    };
    Failure::usage(message, hint)
}

/// A place that cannot be opened: exit 3.
pub(super) fn place(place: &Path, message: &str) -> Failure {
    Failure::index(
        format!("cannot open the place {}: {message}", place.display()),
        "run dctx inside a project directory or pass --place <an existing directory>",
    )
}

/// A facade error from the snapshot build. No argument reaches it, so every refusal is an index error (exit 3).
pub(super) fn snapshot(error: &dctx::DctxError) -> Failure {
    let failure = exit::from_facade(error, "dctx map --help");
    if failure.code == exit::INDEX {
        return failure;
    }
    Failure::index(
        failure.message,
        "check that the place exists and is readable, then run the query again",
    )
}

/// An error text from the hubs, path or export answer: an unknown path end is no result (exit 1), the rest exit 3.
pub(super) fn answer(message: String) -> Failure {
    if message.starts_with("unknown id") {
        return Failure::no_result(message, "list current ids with `dctx map find <name>`");
    }
    Failure::index(message, "run the query again; if it keeps failing, drop the cache with `dctx cache purge --root <place> --yes`")
}

/// The exit-1 failure printed after a `gone` page: the id is not in this index.
pub(super) fn gone(id: &str) -> Failure {
    Failure::no_result(
        format!("{id} is not in the index"),
        "the file may be missing, ignored, sensitive or renamed; list current ids with `dctx map find <name>`",
    )
}

/// Checks the prepared targets: no end may leave the place (exit 2), and map, explain or read of a binary file is no result (exit 1).
pub(super) fn targets(options: &Options) -> Result<(), Failure> {
    if options.request.verb == QueryVerb::Find {
        return Ok(());
    }
    inside_place(&options.request.target)?;
    if let Some(to) = &options.to {
        inside_place(to)?;
        return Ok(());
    }
    match options.request.target.strip_prefix("m:") {
        Some(relative)
            if matches!(
                options.request.verb,
                QueryVerb::Map | QueryVerb::Explain | QueryVerb::Read
            ) =>
        {
            text_file(&options.place, relative)
        }
        _ => Ok(()),
    }
}

/// Refuses a stable id whose path part (before `#`) starts with `/` or has a `..` segment.
fn inside_place(target: &str) -> Result<(), Failure> {
    let Some((_, rest)) = target.split_once(':') else {
        return Ok(());
    };
    let path = rest.split('#').next().unwrap_or(rest);
    if path.starts_with('/') || path.split('/').any(|part| part == "..") {
        return Err(Failure::usage(
            format!("{target} points outside the place"),
            "ids and paths name files inside the place: drop `..` and any leading `/`, or pass --place <the other directory>",
        ));
    }
    Ok(())
}

/// Refuses a file whose first [`SNIFF_BYTES`] hold a NUL byte. A missing or unreadable file is left to the facade.
fn text_file(place: &Path, relative: &str) -> Result<(), Failure> {
    let Ok(Some(bytes)) = crate::host::read_included_file(place, relative) else {
        return Ok(());
    };
    if bytes
        .iter()
        .take(SNIFF_BYTES as usize)
        .any(|byte| *byte == 0)
    {
        return Err(Failure::no_result(
            format!("{relative} is a binary file; dctx maps and reads text files only"),
            "pass a source or text file; `dctx map find <name>` lists indexed ids",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn usage_hints_name_the_verb_help_or_the_missing_target() {
        let flag = usage(
            &words(&["find", "x", "--bogus"]),
            "unknown or inapplicable option --bogus".into(),
        );
        assert_eq!(flag.code, exit::USAGE);
        assert!(flag.hint.contains("dctx map find --help"), "{}", flag.hint);
        assert!(usage(&words(&["nope"]), "expected map".into())
            .hint
            .contains("`dctx map --help`"));
        assert!(usage(
            &words(&["find", ""]),
            "request: this verb requires a target".into()
        )
        .hint
        .contains("non-empty name"));
        assert!(usage(
            &words(&["neighbors"]),
            "request: this verb requires a target".into()
        )
        .hint
        .contains("stable id"));
    }

    #[test]
    fn ids_that_leave_the_place_are_usage_errors() {
        for target in [
            "m:../outside.py",
            "s:../a.py#f",
            "m:a/../../b.rs",
            "m:/etc/passwd",
        ] {
            let failure = inside_place(target).expect_err(target);
            assert_eq!(failure.code, exit::USAGE);
            assert!(
                failure.message.contains("outside the place"),
                "{}",
                failure.message
            );
        }
        for target in [
            "m:a.py",
            "s:a.py#f",
            "s:a.rs#Range..End",
            "@0a1b2c",
            "m:dir/..hidden.py",
        ] {
            assert!(inside_place(target).is_ok(), "{target}");
        }
    }

    #[test]
    fn a_binary_file_is_no_result_and_a_text_file_passes() {
        let place = tempfile::tempdir().expect("place");
        std::fs::write(
            place.path().join("logo.png"),
            [0x89, b'P', b'N', b'G', 0, 0, 1],
        )
        .expect("binary");
        std::fs::write(place.path().join("a.py"), b"def a():\n    return 1\n").expect("text");
        let failure = text_file(place.path(), "logo.png").expect_err("binary");
        assert_eq!(failure.code, exit::NO_RESULT);
        assert!(
            failure.message.contains("logo.png is a binary file"),
            "{}",
            failure.message
        );
        assert!(text_file(place.path(), "a.py").is_ok());
        assert!(text_file(place.path(), "missing.py").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn binary_prechecks_do_not_follow_links_or_sniff_ignored_content() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("binary"), b"\0OUTSIDE_SENTINEL").unwrap();
        std::os::unix::fs::symlink(outside.path().join("binary"), root.path().join("linked.rs"))
            .unwrap();
        assert!(
            text_file(root.path(), "linked.rs").is_ok(),
            "the outside binary marker must not be observed"
        );
        std::fs::write(root.path().join("ignored.rs"), b"\0PRIVATE_SENTINEL").unwrap();
        std::fs::write(root.path().join(".gitignore"), b"ignored.rs\n").unwrap();
        assert!(
            text_file(root.path(), "ignored.rs").is_ok(),
            "excluded content must not be sniffed"
        );
    }

    #[test]
    fn gone_ids_and_unknown_path_ends_are_no_result() {
        assert_eq!(gone("m:nope.py").code, exit::NO_RESULT);
        assert!(gone("m:nope.py")
            .message
            .contains("m:nope.py is not in the index"));
        assert_eq!(answer("unknown id: s:a.py#x".into()).code, exit::NO_RESULT);
        assert_eq!(answer("io: disk".into()).code, exit::INDEX);
        assert_eq!(
            snapshot(&dctx::DctxError::invalid("stale ledger")).code,
            exit::INDEX
        );
    }
}
