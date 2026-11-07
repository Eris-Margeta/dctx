// SPDX-License-Identifier: Apache-2.0
//! Stable-toolchain fuzz targets for public engine decoders and language lexers.
//! Run with PROPTEST_CASES/PROPTEST_RNG_SEED to extend or replay a campaign.
#![forbid(unsafe_code)]

use dctx_core::ids::{RelativePath, ViewKey};
use dctx_core::settings::{AppConfig, parse_config, parse_config_line};
use dctx_map_model::{ContentId, StableId};
use proptest::prelude::*;

fn bytes() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        prop::collection::vec(any::<u8>(), 0..4096),
        prop::collection::vec(any::<char>(), 0..512)
            .prop_map(|chars| chars.into_iter().collect::<String>().into_bytes()),
        "[a-zA-Z0-9_ \\n\\r\\t{}()\\[\\]<>:;#/'\"=éλ🦀|~\\\\-]{0,1024}"
            .prop_map(String::into_bytes),
    ]
}

macro_rules! lexers {
    ($($target:ident, $krate:ident, $lexer:ident, $path:literal;)*) => {
        $(proptest! {
            #[test]
            fn $target(input in bytes()) {
                let lexer = $krate::$lexer::default();
                let first = lexer.extract($path, &input);
                prop_assert_eq!(format!("{first:?}"), format!("{:?}", lexer.extract($path, &input)));
            }
        })*
    };
}

lexers! {
    robustness_js, dctx_lex_js, JsLexer, "sample.js";
    robustness_tsx, dctx_lex_js, JsLexer, "sample.tsx";
    robustness_python, dctx_lex_py, PyLexer, "sample.py";
    robustness_go, dctx_lex_go, GoLexer, "sample.go";
    robustness_java, dctx_lex_java, JavaLexer, "Sample.java";
    robustness_c, dctx_lex_c, CLexer, "sample.c";
    robustness_cpp, dctx_lex_c, CLexer, "sample.cpp";
    robustness_json, dctx_lex_cfg, CfgLexer, "sample.json";
    robustness_jsonc, dctx_lex_cfg, CfgLexer, "sample.jsonc";
    robustness_toml, dctx_lex_cfg, CfgLexer, "sample.toml";
    robustness_yaml, dctx_lex_cfg, CfgLexer, "sample.yaml";
    robustness_csharp, dctx_lex_cs, CsLexer, "Sample.cs";
    robustness_kotlin, dctx_lex_kt, KotlinLexer, "sample.kt";
    robustness_ruby, dctx_lex_rb, RubyLexer, "sample.rb";
    robustness_php, dctx_lex_php, PhpLexer, "sample.php";
    robustness_swift, dctx_lex_swift, SwiftLexer, "sample.swift";
}

proptest! {
    #[test]
    fn robustness_paths_and_selectors(input in bytes()) {
        if let Ok(text) = std::str::from_utf8(&input) {
            for target in [text.to_owned(), format!("s:{text}#symbol"), format!("m:{text}"),
                format!("s:src/lib.rs#{text}"), format!("s:a.rs#f~{text}")] {
                if let Ok(id) = target.parse::<StableId>() {
                    prop_assert_eq!(id.as_str(), &target);
                    prop_assert_eq!(id.as_str().parse::<StableId>(), Ok(id.clone()));
                    let _ = id.alias();
                }
                let _ = ContentId::parse(&target);
                if let Ok(path) = RelativePath::parse(&target) {
                    prop_assert_eq!(path.as_str(), &target);
                    prop_assert!(!target.contains('\\') && !target.contains('\0'));
                    prop_assert!(target.is_empty() || target.split('/').all(|part| !matches!(part, "" | "." | "..")));
                    let _ = path.child(text);
                }
                let _ = ViewKey::parse(&target);
            }
        }
    }

    #[test]
    fn robustness_config_and_ignore_rules(input in bytes()) {
        if let Ok(text) = std::str::from_utf8(&input) {
            let mut incremental = AppConfig::default();
            for line in text.lines() {
                parse_config_line(line, &mut incremental);
                let _ = dctx_core::selection::parse_ignore_pattern_line(line);
            }
            prop_assert_eq!(format!("{:?}", parse_config(text)), format!("{incremental:?}"));
            let _ = dctx_core::selection::parse_rules(text);
            let _ = dctx_core::version::Version::parse(text);
        }
    }

    #[test]
    fn robustness_templates_and_legacy(input in bytes()) {
        let host = dctx_template::HostCeiling::base();
        let _ = dctx_template::load(&input, &host);
        if let Ok(text) = std::str::from_utf8(&input) {
            let _ = dctx_template::Template::parse(text).validate();
            let _ = dctx_template::compile(text, &host);
            let _ = dctx_template::compile(&format!("[DTS]\nversion = 2\n{text}"), &host);
        }
        for mode in [dctx_pack::LegacyBodies::Include, dctx_pack::LegacyBodies::MetadataOnly] {
            let _ = dctx_pack::decode_legacy(&input, mode, dctx_pack::LegacyLimits::default());
            let mut framed = b"DIRCTXTV\x01\0\0\0\0\0\0\0".to_vec();
            framed.extend_from_slice(&input);
            let _ = dctx_pack::decode_legacy(&framed, mode, dctx_pack::LegacyLimits::default());
        }
    }
}

#[cfg(feature = "rpc")]
proptest! {
    #[test]
    fn robustness_rpc_requests(input in bytes()) {
        if let Ok(request) = serde_json::from_slice::<dctx::rpc::Request>(&input) {
            let encoded = serde_json::to_vec(&request).unwrap();
            let again: dctx::rpc::Request = serde_json::from_slice(&encoded).unwrap();
            prop_assert_eq!(request.id, again.id);
            prop_assert_eq!(request.verb, again.verb);
            prop_assert_eq!(request.args, again.args);
        }
    }

    #[test]
    fn robustness_rpc_valid_requests(
        id in prop_oneof![any::<i64>().prop_map(dctx::rpc::Id::Integer), ".{0,64}".prop_map(dctx::rpc::Id::String)],
        verb in prop::sample::select(dctx::rpc::Verb::ALL.to_vec()),
        args in prop::collection::vec(".{0,64}", 0..12),
    ) {
        let request = dctx::rpc::Request { id, verb, args };
        let mut encoded = serde_json::to_vec(&request).unwrap();
        let decoded: dctx::rpc::Request = serde_json::from_slice(&encoded).unwrap();
        prop_assert_eq!(&decoded.args, &request.args);
        // Every truncation of a real request must be an ordinary decode error.
        for cut in 0..encoded.len() {
            prop_assert!(serde_json::from_slice::<dctx::rpc::Request>(&encoded[..cut]).is_err());
        }
        encoded.extend_from_slice(b" {}");
        prop_assert!(serde_json::from_slice::<dctx::rpc::Request>(&encoded).is_err());
    }
}

proptest! {
    #[test]
    fn robustness_checksum_valid_template_mutations(
        changes in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
    ) {
        let host = dctx_template::HostCeiling::base();
        let plan = dctx_template::compile(
            "[DTS]\nversion = 2\n[SELECT:source]\nroots = [\"src\"]\n[INDEX:source]\nfacts = [\"symbols\"]\nparser = \"structural\"\n[BUDGET:small]\nrecords = 10\n[VIEW:map]\nselection = \"source\"\nbudget = \"small\"\nlayout = \"AGENT_LINES\"\n", &host,
        ).unwrap();
        let mut bytes = dctx_template::to_bytes(&plan).unwrap();
        let end = bytes.len() - 32;
        for (index, value) in changes {
            bytes[index.index(end)] = value;
        }
        let sum = dctx_hash::Sha256::digest(&bytes[..end]).unwrap();
        bytes[end..].copy_from_slice(sum.as_bytes());
        if let Ok(decoded) = dctx_template::from_bytes(&bytes) {
            let canonical = dctx_template::to_bytes(&decoded).unwrap();
            prop_assert!(dctx_template::from_bytes(&canonical).is_ok());
        }
    }
}

proptest! {
    #[test]
    fn robustness_template_variable_graphs(
        targets in prop::collection::vec(0_usize..24, 1..24),
    ) {
        let mut source = String::from("[VARIABLES]\n");
        for (index, target) in targets.iter().enumerate() {
            source.push_str(&format!("v{index} = {{{{v{}}}}}\n", target % targets.len()));
        }
        source.push_str("[LAYOUT]\n{{v0}}\n");
        let first = dctx_template::Template::parse(&source);
        let again = dctx_template::Template::parse(&source);
        prop_assert_eq!(&first.cycles, &again.cycles);
        let _ = first.validate();
    }
}
