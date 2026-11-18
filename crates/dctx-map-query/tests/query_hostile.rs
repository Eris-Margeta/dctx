// SPDX-License-Identifier: Apache-2.0

use dctx_map_query::{Answer, Error, FileSource, Index, Request, Verb, ports::Source};
use std::{
    fs,
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(10);

fn checked(case: impl FnOnce() + Send + 'static) {
    let (done, finished) = mpsc::sync_channel(0);
    let start = Instant::now();
    let worker = thread::spawn(move || {
        case();
        let _ = done.send(());
    });
    assert_ne!(
        finished.recv_timeout(DEADLINE),
        Err(mpsc::RecvTimeoutError::Timeout),
        "hostile case exceeded {DEADLINE:?}"
    );
    if let Err(panic) = worker.join() {
        std::panic::resume_unwind(panic);
    }
    assert!(
        start.elapsed() < DEADLINE,
        "hostile case exceeded {DEADLINE:?}"
    );

    #[cfg(target_os = "linux")]
    {
        let status = fs::read_to_string("/proc/self/status").unwrap();
        let peak_kib: usize = status
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            peak_kib < 512 * 1024,
            "test process peaked at {peak_kib} KiB"
        );
    }
}

fn repeated(root: &Path, request: &Request) -> Result<Answer, String> {
    let run = || {
        let source = FileSource::new(root).unwrap();
        let mut index = Index::in_memory("hostile").unwrap();
        index
            .query(&source, request)
            .map(|mut answer| {
                // Age is runtime metadata; all remaining ordered output must match.
                answer.age_seconds = 0;
                answer
            })
            .map_err(|error| error.to_string())
    };
    let expected = run();
    for _ in 1..3 {
        assert_eq!(run(), expected);
    }
    expected
}

fn write_symbols(root: &Path) {
    fs::write(root.join("z.rs"), "pub fn needle() {}\n").unwrap();
    fs::write(
        root.join("a.rs"),
        "pub fn needle() {}\npub fn unrelated() {}\n",
    )
    .unwrap();
}

fn assert_needles(answer: &Answer) {
    let ids: Vec<_> = answer
        .records
        .iter()
        .map(|record| record.id.as_str())
        .collect();
    assert_eq!(ids, ["s:a.rs#needle", "s:z.rs#needle"]);
    assert_eq!(answer.more, 0);
}

#[cfg(unix)]
#[test]
fn symlinks_in_cycles_and_outside_the_root_are_skipped() {
    use std::os::unix::fs::symlink;

    checked(|| {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        fs::create_dir(&root).unwrap();
        write_symbols(&root);
        fs::write(fixture.path().join("outside.rs"), "pub fn outside() {}\n").unwrap();
        symlink("loop-b.rs", root.join("loop-a.rs")).unwrap();
        symlink("loop-a.rs", root.join("loop-b.rs")).unwrap();
        symlink(".", root.join("recursive")).unwrap();
        symlink("..", root.join("outside-dir")).unwrap();
        symlink("../outside.rs", root.join("outside-file.rs")).unwrap();

        let request = Request::new(Verb::Map, "");
        let answer = repeated(&root, &request).unwrap();
        let ids: Vec<_> = answer
            .records
            .iter()
            .map(|record| record.id.as_str())
            .collect();
        assert_eq!(ids, ["m:a.rs", "m:z.rs"]);
        for _ in 0..3 {
            let source = FileSource::new(&root).unwrap();
            let snapshot = source.snapshot().unwrap();
            assert!(
                snapshot
                    .files
                    .keys()
                    .map(String::as_str)
                    .eq(["a.rs", "z.rs"])
            );
            let before = source.content_reads();
            for path in [
                "loop-a.rs",
                "recursive/a.rs",
                "outside-dir/outside.rs",
                "outside-file.rs",
            ] {
                assert!(matches!(source.read(path), Err(Error::Denied(_))), "{path}");
            }
            assert_eq!(source.content_reads(), before);
        }
    });
}

#[test]
fn parent_components_and_nul_paths_are_refused_before_opening_content() {
    checked(|| {
        let root = tempfile::tempdir().unwrap();
        write_symbols(root.path());
        for path in ["../a.rs", "nested/../a.rs", "a\0.rs"] {
            let mut request = Request::new(Verb::Find, "needle");
            request.within = Some(path.into());
            let expected = Error::Request("path must be relative to the place".into()).to_string();
            assert_eq!(repeated(root.path(), &request), Err(expected.clone()));
            for _ in 0..3 {
                let source = FileSource::new(root.path()).unwrap();
                assert!(matches!(source.read(path), Err(Error::Request(_))));
                assert_eq!(source.read(path).unwrap_err().to_string(), expected);
                assert_eq!(source.content_reads(), 0);
            }
        }
    });
}

#[test]
fn oversized_path_component_is_safe_to_filter_and_cannot_be_opened() {
    checked(|| {
        let root = tempfile::tempdir().unwrap();
        write_symbols(root.path());
        let path = format!("{}.rs", "x".repeat(4096));
        let mut request = Request::new(Verb::Find, "needle");
        request.within = Some(path.clone());
        assert!(repeated(root.path(), &request).unwrap().records.is_empty());
        let mut expected = None;
        for _ in 0..3 {
            let source = FileSource::new(root.path()).unwrap();
            let error = source.read(&path).unwrap_err();
            assert!(matches!(error, Error::Io(_)));
            let message = error.to_string();
            assert_eq!(expected.get_or_insert_with(|| message.clone()), &message);
            assert_eq!(source.content_reads(), 0);
        }
    });
}

/// True when the filesystem itself refuses a hostile name (EILSEQ, ENAMETOOLONG, EINVAL).
fn fs_refuses(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(92 | 84 | 63 | 36 | 22))
        || error.kind() == std::io::ErrorKind::InvalidFilename
}

#[cfg(unix)]
#[test]
fn non_utf8_filename_does_not_hide_valid_neighbors() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    checked(|| {
        let root = tempfile::tempdir().unwrap();
        write_symbols(root.path());
        if let Err(error) = fs::write(
            root.path().join(OsStr::from_bytes(b"bad-\xff.rs")),
            "pub fn hidden() {}\n",
        ) {
            assert!(fs_refuses(&error), "{error}");
            eprintln!("skip: fs refuses non-UTF-8 filename: {error}");
        }
        fs::write(root.path().join("bad-\u{fffd}.rs"), "pub fn visible() {}\n").unwrap();
        let answer = repeated(root.path(), &Request::new(Verb::Map, "")).unwrap();
        let ids: Vec<_> = answer
            .records
            .iter()
            .map(|record| record.id.as_str())
            .collect();
        assert_eq!(ids, ["m:a.rs", "m:bad-\u{fffd}.rs", "m:z.rs"]);
        assert!(
            repeated(root.path(), &Request::new(Verb::Find, "hidden"))
                .unwrap()
                .records
                .is_empty()
        );
    });
}

#[test]
fn empty_repository_and_zero_byte_file() {
    checked(|| {
        let root = tempfile::tempdir().unwrap();
        let request = Request::new(Verb::Map, "");
        let empty = repeated(root.path(), &request).unwrap();
        assert!(empty.records.is_empty());
        assert_eq!(empty.more, 0);

        fs::write(root.path().join("empty.rs"), []).unwrap();
        let answer = repeated(root.path(), &request).unwrap();
        let ids: Vec<_> = answer
            .records
            .iter()
            .map(|record| record.id.as_str())
            .collect();
        assert_eq!(ids, ["m:empty.rs"]);
        let result = repeated(root.path(), &Request::new(Verb::Find, "needle")).unwrap();
        assert!(result.records.is_empty());
        assert_eq!(result.more, 0);
    });
}

#[test]
fn empty_and_huge_query_strings() {
    checked(|| {
        let root = tempfile::tempdir().unwrap();
        write_symbols(root.path());
        for (target, reason) in [
            (String::new(), "this verb requires a target"),
            (
                "x".repeat(1024 * 1024),
                "target is too long or contains control characters",
            ),
        ] {
            assert_eq!(
                repeated(root.path(), &Request::new(Verb::Find, target)),
                Err(Error::Request(reason.into()).to_string())
            );
        }
        let result = repeated(root.path(), &Request::new(Verb::Find, "x".repeat(4096))).unwrap();
        assert!(result.records.is_empty());
        assert_needles(&repeated(root.path(), &Request::new(Verb::Find, "needle")).unwrap());
    });
}

#[test]
fn regex_metacharacters_are_lexical_punctuation() {
    checked(|| {
        let root = tempfile::tempdir().unwrap();
        write_symbols(root.path());
        for pattern in [r"^(needle){1,1000000}$|.*", r"[needle]+(?=needle)\\"] {
            assert_needles(&repeated(root.path(), &Request::new(Verb::Find, pattern)).unwrap());
        }
        let result = repeated(root.path(), &Request::new(Verb::Find, r"[.*?^$+(){}|\\]")).unwrap();
        assert!(result.records.is_empty());
    });
}
