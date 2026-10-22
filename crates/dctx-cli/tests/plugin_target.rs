//! REL-P closed target selection and refusal before filesystem access.
//! Exercises the pure platform mapper and the product command dispatcher on one host.
//! Imports the binary module directly to test crate-private seams without a public API.

#![forbid(unsafe_code)]
#![allow(dead_code)]

#[path = "../src/plugin/mod.rs"]
mod plugin;

use dctx_plugin_distribution::Target;
use plugin::UnsupportedTarget;
use plugin::disk::target_for;

#[test]
#[allow(clippy::no_effect_underscore_binding, reason = "Type-check the linked worker entry without executing it during target-mapping tests.")]
fn every_supported_triple_maps_to_its_manifest_target() {
    let _worker_entry = plugin::enter_worker;
    for (os, arch, env, expected) in [
        ("macos", "aarch64", "", Target::AppleArm64),
        ("macos", "x86_64", "", Target::AppleX64),
        ("linux", "x86_64", "gnu", Target::LinuxX64),
        ("linux", "aarch64", "gnu", Target::LinuxArm64),
        ("windows", "x86_64", "msvc", Target::WindowsX64),
    ] {
        assert_eq!(target_for(os, arch, env), Ok(expected));
        assert_eq!(Target::parse(expected.as_str()), Ok(expected));
    }
}

#[test]
fn unsupported_operating_systems_have_supported_controls() {
    for os in ["freebsd", "android", "ios", "unknown", ""] {
        assert_eq!(target_for(os, "x86_64", "gnu"),
            Err(UnsupportedTarget::Os(format!("{os}-x86_64-gnu"))));
        assert_eq!(target_for("linux", "x86_64", "gnu"), Ok(Target::LinuxX64));
    }
}

#[test]
fn unsupported_cpus_have_supported_controls() {
    for (os, env, control) in [
        ("macos", "", Target::AppleX64),
        ("linux", "gnu", Target::LinuxX64),
        ("windows", "msvc", Target::WindowsX64),
    ] {
        for arch in ["arm", "riscv64", "powerpc64", "unknown", ""] {
            assert_eq!(target_for(os, arch, env),
                Err(UnsupportedTarget::Cpu(format!("{os}-{arch}-{env}"))));
            assert_eq!(target_for(os, "x86_64", env), Ok(control));
        }
    }
    assert_eq!(target_for("windows", "aarch64", "msvc"),
        Err(UnsupportedTarget::Cpu("windows-aarch64-msvc".into())));
    assert_eq!(target_for("windows", "x86_64", "msvc"), Ok(Target::WindowsX64));
}

#[test]
fn unsupported_libcs_and_abis_have_supported_controls() {
    for (os, arch, supported, control) in [
        ("linux", "x86_64", "gnu", Target::LinuxX64),
        ("linux", "aarch64", "gnu", Target::LinuxArm64),
        ("windows", "x86_64", "msvc", Target::WindowsX64),
        ("macos", "x86_64", "", Target::AppleX64),
        ("macos", "aarch64", "", Target::AppleArm64),
    ] {
        for env in ["musl", "uclibc", "gnu", "msvc", "", "unknown"] {
            if env == supported { continue; }
            assert_eq!(target_for(os, arch, env),
                Err(UnsupportedTarget::Libc(format!("{os}-{arch}-{env}"))));
            assert_eq!(target_for(os, arch, supported), Ok(control));
        }
    }
}

fn args(command: &[&str], json: bool) -> Vec<String> {
    let mut args = vec!["dctx".to_owned(), "plugin".to_owned()];
    args.extend(command.iter().map(|s| (*s).to_owned()));
    if json { args.push("--json".to_owned()); }
    args
}

fn root() -> std::path::PathBuf {
    std::fs::canonicalize(std::env::temp_dir()).unwrap()
        .join(format!("dctx-target-{}", std::process::id()))
}

#[test]
fn list_and_help_never_resolve_the_target_or_create_the_store() {
    let root = root().join("readonly");
    assert!(!root.exists());
    for command in ["list", "help"] {
        for json in [false, true] {
            let (_, outcome) = plugin::outcome_at(&args(&[command], json), &root,
                || panic!("{command} must not resolve the host target")).unwrap();
            assert_eq!(outcome.code, 0, "{}", outcome.body);
            assert!(outcome.body.contains(if command == "list" { "dctx/git" } else { "Usage:" }));
            assert!(!root.exists());
        }
    }
}

#[test]
fn mutating_commands_refuse_all_unsupported_platform_kinds_before_store_access() {
    let root = root().join("refused");
    assert!(!root.exists());
    for platform in [("freebsd", "x86_64", "gnu"), ("linux", "riscv64", "gnu"), ("linux", "x86_64", "musl")] {
        for command in [vec!["install", "owner/repo"], vec!["update"], vec!["enable", "dctx/git", "--approve"]] {
            for json in [false, true] {
                let mut called = false;
                let (_, outcome) = plugin::outcome_at(&args(&command, json), &root, || {
                    called = true;
                    target_for(platform.0, platform.1, platform.2)
                }).unwrap();
                assert!(called);
                assert_eq!(outcome.code, 1);
                let detail = format!("unsupported platform: {}-{}-{}", platform.0, platform.1, platform.2);
                assert!(outcome.body.contains(&detail), "{}", outcome.body);
                if json {
                    assert!(outcome.body.contains("\"error\":\"UnsupportedTarget\""));
                    assert!(outcome.body.contains(&format!("\"command\":\"{}\"", command[0])));
                }
                assert!(!root.exists());
            }
        }
    }
    let (_, outcome) = plugin::outcome_at(&args(&["update", "--check"], true), &root,
        || target_for("linux", "x86_64", "gnu")).unwrap();
    assert!(!outcome.body.contains("unsupported platform"));
    assert!(!root.exists());
}
