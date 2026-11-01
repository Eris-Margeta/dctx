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

fn page(path: &std::path::Path, extra: &[&str]) -> Result<String, String> {
    let mut args = vec!["pack".to_string(), path.display().to_string()];
    args.extend(extra.iter().map(|word| (*word).to_string()));
    execute(&args).map(|outcome| String::from_utf8(outcome.body).expect("utf8"))
}

fn touch(file: &std::path::Path, seconds: u64) {
    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds);
    std::fs::File::options()
        .write(true)
        .open(file)
        .expect("open")
        .set_times(std::fs::FileTimes::new().set_modified(when))
        .expect("touch");
}

#[test]
fn tree_stamps_flag_selects_the_template_and_off_survives_a_touch() {
    let path = project("stamps");
    touch(&path.join("kept.rs"), 1_000_000);
    let off = page(&path, &["--tree-stamps=off"]).expect("off");
    let spaced = page(&path, &["--tree-stamps", "off"]).expect("off, two words");
    let on = page(&path, &["--tree-stamps=on"]).expect("on");
    assert_eq!(off, spaced);
    assert_eq!(off.matches("MOD").count(), 0, "{off}");
    assert!(
        off.contains("(ID:ROOT)") && off.contains("kept.rs (ID:F001, SIZE:13)"),
        "{off}"
    );
    assert!(
        on.contains("(ID:ROOT, MOD:") && on.contains("kept.rs (ID:F001, MOD:1000000000, SIZE:13)"),
        "{on}"
    );
    assert!(
        on.contains("MOD:TIMESTAMP"),
        "the stamped instructions explain MOD"
    );
    touch(&path.join("kept.rs"), 2_000_000);
    assert_eq!(
        page(&path, &["--tree-stamps=off"]).expect("off again"),
        off,
        "a bare touch changes nothing"
    );
    assert_ne!(
        page(&path, &["--tree-stamps=on"]).expect("on again"),
        on,
        "the stamped page follows the mtime"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn tree_stamps_default_is_the_documented_one_and_bad_values_are_refused() {
    let path = project("stamps-default");
    let default = page(&path, &[]).expect("default");
    assert_eq!(
        default,
        page(&path, &[&format!("--tree-stamps={}", DOCUMENTED_DEFAULT)]).expect("explicit")
    );
    for (words, wanted) in [
        (&["--tree-stamps=maybe"][..], "on or off"),
        (&["--tree-stamps"][..], "requires on or off"),
    ] {
        let message = page(&path, words).expect_err("refused");
        assert!(message.contains(wanted), "{message}");
    }
    let _ = std::fs::remove_dir_all(&path);
}

/// The value of `--tree-stamps` when the flag is absent.
const DOCUMENTED_DEFAULT: &str = "on";
