// SPDX-License-Identifier: Apache-2.0
//! `dctx map export`: the whole graph as one deterministic document, graph JSON or a Mermaid flowchart.
//! Parses its own few flags, so the five verbs and their bounds in `args` stay as they are.
//! It is an export, not an answer: no target, no 2,000-token cap; `--output PATH` writes a file instead of stdout.

use dctx::{MapRequest, Session, SnapshotHandle, Verb};
use dctx_map_query::{Format, Request, Verb as QueryVerb};
use std::path::{Path, PathBuf};

use super::args::Options;

/// What `export` was asked for.
#[derive(Debug, Clone)]
pub(super) struct Export {
    /// `json` or `mermaid`.
    pub format: String,
    /// File to write instead of stdout.
    pub output: Option<PathBuf>,
}

/// Parses `export [--format json|mermaid] [--output PATH] [--place PATH]`. The default format is `json`.
pub(super) fn parse(args: &[String]) -> Result<Options, String> {
    let request = Request::new(QueryVerb::Map, "");
    let (mut place, mut format, mut output) = (PathBuf::from("."), "json".to_string(), None);
    let mut index = 1;
    while index < args.len() {
        let flag = &args[index];
        index += 1;
        match flag.as_str() {
            "--format" | "--output" | "--place" => {
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                index += 1;
                match flag.as_str() {
                    "--format" => format = value.clone(),
                    "--output" => output = Some(PathBuf::from(value)),
                    _ => place = PathBuf::from(value),
                }
            }
            _ => {
                return Err(format!(
                    "export takes no target and only --format, --output and --place; got {flag}"
                ))
            }
        }
    }
    if !matches!(format.as_str(), "json" | "mermaid") {
        return Err(format!(
            "unknown export format: {format}; expected json or mermaid"
        ));
    }
    request.validate().map_err(|e| e.to_string())?;
    Ok(Options {
        request,
        format: Format::Text,
        place,
        continuation: None,
        budget_set: false,
        profile: false,
        meta: false,
        hubs: false,
        grep: false,
        ignore_case: false,
        skeleton: false,
        to: None,
        export: Some(Export { format, output }),
    })
}

/// Writes an export to stdout or the explicitly requested file.
pub(super) fn write(
    session: &mut Session,
    handle: &SnapshotHandle,
    options: &Options,
    output: &mut dyn std::io::Write,
) -> Result<(), String> {
    let export = options
        .export
        .as_ref()
        .ok_or("export requires its options")?;
    let request = MapRequest::new(Verb::Neighbors);
    match &export.output {
        Some(path) => {
            let file = create_output(path)
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);
            session
                .export_graph_to(handle, &export.format, &request, &mut writer)
                .map_err(|error| error.to_string())?;
            std::io::Write::flush(&mut writer).map_err(|error| error.to_string())
        }
        None => session
            .export_graph_to(handle, &export.format, &request, output)
            .map_err(|error| error.to_string()),
    }
}

/// Opens `--output` for writing without following a final symlink, and only when it is (or becomes) a
/// regular file: a link planted at the output path never redirects or truncates another file, and a FIFO
/// or device is refused. The file is truncated only after the opened handle is checked.
fn create_output(path: &Path) -> std::io::Result<std::fs::File> {
    use std::io::{Error, ErrorKind};
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "refusing to write through a symbolic link",
            ))
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(Error::new(ErrorKind::InvalidInput, "not a regular file"))
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::new(ErrorKind::InvalidInput, "not a regular file"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT: a link swapped in after the check above.
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "refusing to write through a symbolic link",
            ));
        }
    }
    file.set_len(0)?;
    Ok(file)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
const NOFOLLOW: i32 = 0x20000;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
const NOFOLLOW: i32 = 0x100;
/// Other Unix targets rely on the `symlink_metadata` check before opening.
#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))
))]
const NOFOLLOW: i32 = 0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn export_takes_a_format_an_output_and_a_place_only() {
        let plain = parse(&strings(&["export"])).expect("plain");
        assert_eq!(
            plain.export.as_ref().map(|e| e.format.as_str()),
            Some("json")
        );
        let full = parse(&strings(&[
            "export", "--format", "mermaid", "--output", "g.mmd",
        ]))
        .expect("full");
        assert_eq!(
            full.export.and_then(|e| e.output),
            Some(PathBuf::from("g.mmd"))
        );
        for bad in [
            vec!["export", "--format", "nope"],
            vec!["export", "src"],
            vec!["export", "--json"],
            vec!["export", "--format"],
        ] {
            assert!(parse(&strings(&bad)).is_err(), "{bad:?}");
        }
        assert!(super::super::args::HELP.contains("export"));
    }

    #[test]
    fn export_writes_the_graph_to_stdout_or_a_file_and_repeats_byte_for_byte() {
        let fixture = tempfile::tempdir().expect("fixture");
        let (place, cache) = (fixture.path().join("place"), fixture.path().join("cache"));
        std::fs::create_dir_all(&place).expect("place");
        std::fs::write(
            place.join("a.py"),
            "def alpha():\n    return beta()\n\ndef beta():\n    return 1\n",
        )
        .expect("source");
        let run = |extra: &[&str]| {
            let mut args = strings(&["export", "--place", &place.display().to_string()]);
            args.extend(strings(extra));
            super::super::execute_with(&args, |at, _| host::open_mapped_at(at, Some(&cache)))
        };
        let json = run(&["--format", "json"]).expect("json");
        assert!(
            json.contains("{\"src\":\"s:a.py#alpha\",\"edge\":\"calls\",\"dst\":\"s:a.py#beta\""),
            "{json}"
        );
        let mermaid = run(&["--format", "mermaid"]).expect("mermaid");
        assert!(
            mermaid.starts_with("flowchart LR\n") && mermaid.contains("-->|calls|"),
            "{mermaid}"
        );
        assert_eq!(mermaid, run(&["--format", "mermaid"]).expect("again"));
        let file = fixture.path().join("out.json");
        assert_eq!(
            run(&["--output", &file.display().to_string()]).expect("file"),
            ""
        );
        assert_eq!(std::fs::read_to_string(&file).expect("written"), json);
        // A shorter export into the same file leaves no stale tail.
        std::fs::write(&file, "x".repeat(json.len() * 2)).expect("longer");
        run(&["--output", &file.display().to_string()]).expect("rewrite");
        assert_eq!(std::fs::read_to_string(&file).expect("rewritten"), json);
        assert!(run(&["--format", "nope"]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn export_output_never_follows_a_symlink_or_opens_a_special_file() {
        let fixture = tempfile::tempdir().expect("fixture");
        let (place, cache) = (fixture.path().join("place"), fixture.path().join("cache"));
        std::fs::create_dir_all(&place).expect("place");
        std::fs::write(place.join("a.py"), "def alpha():\n    return 1\n").expect("source");
        let victim = fixture.path().join("victim");
        std::fs::write(&victim, "keep me").expect("victim");
        let link = fixture.path().join("graph.json");
        std::os::unix::fs::symlink(&victim, &link).expect("symlink");
        let dangling = fixture.path().join("dangling.json");
        std::os::unix::fs::symlink(fixture.path().join("created-through-link"), &dangling)
            .expect("dangling");
        let run = |output: &Path| {
            let args = strings(&[
                "export",
                "--place",
                &place.display().to_string(),
                "--output",
                &output.display().to_string(),
            ]);
            super::super::execute_with(&args, |at, _| host::open_mapped_at(at, Some(&cache)))
        };
        let refused = run(&link).expect_err("symlink");
        assert!(refused.contains("symbolic link"), "{refused}");
        assert_eq!(std::fs::read_to_string(&victim).expect("victim"), "keep me");
        assert!(run(&dangling).is_err());
        assert!(!fixture.path().join("created-through-link").exists());
        let refused = run(fixture.path()).expect_err("directory");
        assert!(refused.contains("not a regular file"), "{refused}");
    }
}
