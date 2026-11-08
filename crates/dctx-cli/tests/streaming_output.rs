// SPDX-License-Identifier: Apache-2.0
//! Transport parity and closed-reader behavior, portable across Unix and Windows pipes.
use std::io::Read;
use std::process::{Command, Stdio};

fn command(root: &std::path::Path, cache: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
    command.current_dir(root).env("DCTX_CACHE_DIR", cache);
    command
}

#[test]
fn streaming_pack_matches_capture_and_closed_stdout_is_quiet() {
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path().join("place");
    let cache = fixture.path().join("cache");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("large.txt"), "payload\n".repeat(256 * 1024)).unwrap();
    let args = ["pack".to_string(), root.display().to_string()];
    let expected = dctx_cli::pack::output(&args).expect("capture");
    let output = command(&root, &cache).args(["pack", "."]).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(output.stdout, expected.stdout.as_bytes());
    let missing_output = fixture.path().join("absent").join("output.txt");
    let mut refused_args = args.to_vec();
    refused_args.extend(["--output".to_string(), missing_output.display().to_string()]);
    let failure = dctx_cli::pack::output(&refused_args).unwrap_err();
    let refused = command(&root, &cache).args(&refused_args).output().unwrap();
    assert_eq!(refused.status.code(), Some(failure.code));
    assert_eq!(refused.stderr, failure.render().as_bytes());
    let mut child = command(&root, &cache)
        .args(["pack", "."])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = child.stdout.take().unwrap();
    let mut first = [0; 1];
    reader.read_exact(&mut first).unwrap();
    drop(reader);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
}

#[test]
fn interrupted_export_leaves_a_complete_reusable_generation() {
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path().join("place");
    let cache = fixture.path().join("cache");
    std::fs::create_dir(&root).unwrap();
    for index in 0..300 {
        std::fs::write(
            root.join(format!("f{index}.py")),
            "def alpha():\n    return beta()\ndef beta():\n    return 1\n",
        )
        .unwrap();
    }
    let expected = command(&root, &cache)
        .args(["map", "export"])
        .output()
        .unwrap();
    assert!(expected.status.success(), "{:?}", expected);
    let mut child = command(&root, &cache)
        .args(["map", "export"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = child.stdout.take().unwrap();
    reader.read_exact(&mut [0; 1]).unwrap();
    drop(reader);
    let stopped = child.wait_with_output().unwrap();
    assert!(stopped.status.success(), "{:?}", stopped);
    assert!(stopped.stderr.is_empty(), "{:?}", stopped.stderr);
    let repeated = command(&root, &cache)
        .env("DCTX_REUSE_REPORT", "1")
        .args(["map", "export"])
        .output()
        .unwrap();
    assert!(repeated.status.success(), "{:?}", repeated);
    assert_eq!(repeated.stdout, expected.stdout);
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("generation reused"));
}

#[test]
fn a_temp_directory_inside_the_source_stays_invisible_and_preserves_tree_stamps() {
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path().join("place");
    let cache = fixture.path().join("cache");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("kept.txt"), "content\n").unwrap();
    for stamps in ["off", "on"] {
        let args = [
            "pack".to_string(),
            root.display().to_string(),
            format!("--tree-stamps={stamps}"),
        ];
        let expected = dctx_cli::pack::output(&args).expect("capture");
        let actual = command(&root, &cache)
            .env("TMPDIR", &root)
            .env("TEMP", &root)
            .env("TMP", &root)
            .args(&args)
            .output()
            .unwrap();
        assert!(actual.status.success(), "{:?}", actual);
        assert_eq!(actual.stdout, expected.stdout.as_bytes());
        let names: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("kept.txt")]);
    }
}
