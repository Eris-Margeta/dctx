use super::*;

fn project(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("dctx-cli-pack-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("temp");
    std::fs::write(path.join("kept.rs"), b"fn kept() {}\n").expect("source");
    path
}

#[test]
fn explicit_pack_writes_no_project_text_file() {
    let path = project("stdout");
    let outcome = execute(&["pack".into(), path.display().to_string()]).expect("pack");
    assert!(outcome.wrote.is_none());
    assert!(!outcome.clipboard);
    assert!(!path.join(".dctx").exists());
    let text = std::str::from_utf8(&outcome.body).expect("utf8");
    assert!(text.contains("PATH=\"kept.rs\""), "{text}");
    assert!(text.contains("fn kept() {}"), "{text}");
    assert!(outcome.report_line.contains("retention=stdout"));
    assert!(
        outcome
            .report_line
            .contains(&format!("bytes={}", outcome.body.len()))
    );
    let listed: Vec<_> = std::fs::read_dir(&path).expect("list").flatten().collect();
    assert_eq!(listed.len(), 1);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn env_shaped_files_are_never_printed_by_default() {
    let path = project("envfiles");
    for name in [".env", ".env.local", "settings.env", ".envrc"] {
        std::fs::write(path.join(name), b"K=CANARYSECRET\n").expect("env");
    }
    let outcome = execute(&["pack".into(), path.display().to_string()]).expect("pack");
    let text = std::str::from_utf8(&outcome.body).expect("utf8");
    assert!(!text.contains("CANARYSECRET"), "{text}");
    assert!(text.contains("fn kept() {}"), "{text}");
    let json =
        execute(&["pack".into(), path.display().to_string(), "--json".into()]).expect("json");
    assert!(!String::from_utf8_lossy(&json.body).contains("CANARYSECRET"));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn output_flag_writes_only_the_requested_file() {
    let path = project("file");
    let output = path.join("explicit.txt");
    let outcome = execute(&[
        "pack".into(),
        path.display().to_string(),
        "--output".into(),
        output.display().to_string(),
        "--json".into(),
    ])
    .expect("pack");
    assert_eq!(outcome.wrote.as_deref(), Some(output.as_path()));
    assert!(!path.join(".dctx").exists());
    let saved = std::fs::read(&output).expect("saved");
    assert_eq!(saved, outcome.body);
    assert!(
        std::str::from_utf8(&saved)
            .expect("utf8")
            .contains("\"files\"")
    );
    assert!(outcome.report_line.contains("retention=explicit"));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn unknown_flag_is_refused_before_a_scan() {
    let error = execute(&["pack".into(), "--tree".into()]).expect_err("flag");
    assert!(error.contains("unknown pack flag"));
}

#[test]
fn help_names_every_pack_flag() {
    let help = flag_help();
    for flag in [
        "--ext",
        "--binary",
        "--empty",
        "--non-empty",
        "--large",
        "--small",
        "--hidden",
        "--debug-ignored",
        "--include",
        "--exclude",
        "--output",
        "-o",
        "--clipboard",
        "-c",
        "--json",
        "--for",
        "--budget",
    ] {
        assert!(help.contains(flag), "missing {flag}");
    }
}

#[test]
fn empty_and_non_empty_are_refused() {
    let error =
        execute(&["pack".into(), "--empty".into(), "--non-empty".into()]).expect_err("both");
    assert!(error.contains("--empty"));
}

#[test]
fn default_omits_binary_and_ext_keeps_rust() {
    let path = std::env::temp_dir().join(format!("dctx-cli-pack-filter-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("temp");
    std::fs::write(path.join("kept.rs"), b"fn kept() {}\n").expect("rs");
    std::fs::write(path.join("notes.md"), b"notes\n").expect("md");
    std::fs::write(path.join("logo.png"), b"\x89PNG\0SENTINEL").expect("png");
    let packed = execute(&["pack".into(), path.display().to_string()]).expect("pack");
    let text = std::str::from_utf8(&packed.body).expect("utf8");
    assert!(text.contains("PATH=\"kept.rs\""));
    assert!(text.contains("PATH=\"notes.md\""));
    assert!(!text.contains("logo.png"), "{text}");
    assert!(!text.contains("SENTINEL"));
    let only = execute(&[
        "pack".into(),
        path.display().to_string(),
        "--ext".into(),
        "rs".into(),
    ])
    .expect("ext");
    let narrowed = std::str::from_utf8(&only.body).expect("utf8");
    assert!(narrowed.contains("PATH=\"kept.rs\""));
    assert!(!narrowed.contains("notes.md"));
    assert!(only.report_line.contains("files=1"));
    let _ = std::fs::remove_dir_all(&path);
}
