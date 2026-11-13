// SPDX-License-Identifier: Apache-2.0
//! Direct CLI queries preserve source scope and ignore policy.

use std::process::Command;

#[test]
fn cli_reads_source_without_opening_an_unusable_cache_or_sibling_rules() {
    let fixture = tempfile::tempdir().expect("fixture");
    let place = fixture.path().join("place");
    std::fs::create_dir_all(place.join("src/nested")).expect("dirs");
    std::fs::create_dir_all(place.join("unrelated")).expect("sibling");
    std::fs::write(place.join(".gitignore"), "src/nested/blocked.py\n").expect("root rules");
    std::fs::write(place.join("src/.gitignore"), "*.py\n!kept.py\n").expect("ancestor rules");
    std::fs::write(
        place.join("src/nested/kept.py"),
        "def target():\n    return 'needle'\n",
    )
    .expect("file");
    std::fs::write(
        place.join("src/nested/blocked.py"),
        "def hidden():\n    return 'SECRET'\n",
    )
    .expect("hidden");
    std::fs::write(place.join("unrelated/.gitignore"), [0xff]).expect("invalid unrelated rule");
    let cache = fixture.path().join("unusable-cache");
    std::fs::write(&cache, "a file cannot be a cache directory").expect("bad cache");
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(&place)
            .env("XDG_CONFIG_HOME", fixture.path().join("config"))
            .env("DCTX_CACHE_DIR", &cache)
            .env_remove("DCTX_PROFILE")
            .env_remove("DCTX_REUSE_REPORT");
        command
    };
    let cli = command()
        .args(["read", "s:src/nested/kept.py#target"])
        .output()
        .expect("CLI");
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert!(cli.stderr.is_empty());
    let source = String::from_utf8(cli.stdout).expect("UTF8");
    assert!(source.contains("return 'needle'"), "{source}");
    assert!(!source.contains("SECRET"), "{source}");
    for args in [
        vec!["map", "find", "needle", "--in", "src/nested"],
        vec!["map", "grep", "needle", "--in", "src/nested"],
        vec!["map", "skeleton", "src/nested/kept.py"],
    ] {
        let output = command().args(args).output().expect("query");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).expect("UTF8");
        assert!(text.contains("kept.py"), "{text}");
        assert!(
            !text.contains("SECRET") && !text.contains("hidden"),
            "{text}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(cache).expect("cache unchanged"),
        "a file cannot be a cache directory"
    );
}
