//! Property checks for display text and duplicate identity.
//! Implements the PL-X refusal that hostile summaries and duplicate ids do not pass.
//! Depends on proptest and the pure validator. A plain summary is the positive control.
//! No signature or filesystem is required for these lexical refusals.

#![forbid(unsafe_code)]

use plugin_index::{IndexError, validate_index};
use proptest::prelude::*;

const FINGERPRINT: &str = "21fe31dfa154a261626bf854046fd2271b7bed4b6abe45aa58877ef47f9721b9";
const DIGEST: &str = "bf74b6b3ff523cb08461bff8bbaa48dba16d91026d17ecd59e683beb165cbf9c";

fn page(summary: &str, second_id: Option<&str>) -> String {
    let second = match second_id {
        None => String::new(),
        Some(id) => format!(
            r#",{{"id":"{id}","repository":"fixture/dctx-plugin-other","summary":"other","publisher_key_fingerprint":"{FINGERPRINT}","releases":[{{"version":"1.0.0","release_sequence":1,"manifest_url":"https://example.invalid/other.json","manifest_sha256":"{DIGEST}"}}]}}"#
        ),
    };
    format!(
        r#"{{"schema":1,"sequence":1,"plugins":[{{"id":"dctx/git","repository":"fixture/dctx-plugin-git","summary":"{summary}","publisher_key_fingerprint":"{FINGERPRINT}","releases":[{{"version":"1.0.0","release_sequence":1,"manifest_url":"https://example.invalid/plugin.json","manifest_sha256":"{DIGEST}"}}]}}{second}]}}"#
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn plain_summaries_validate_and_controls_are_hostile(
        suffix in "[a-z]{0,24}",
        control in 1u8..32u8,
    ) {
        let plain = page(&format!("Local fixture {suffix}"), None);
        prop_assert!(validate_index(plain.as_bytes()).is_ok());
        let hostile = page(&format!("Local\\u{control:04x}"), None);
        prop_assert_eq!(
            validate_index(hostile.as_bytes()),
            Err(IndexError::HostileDisplay)
        );
    }

}

#[test]
fn a_repeated_id_is_a_duplicate_even_with_another_repository() {
    let duplicate = page("Local fixture", Some("dctx/git"));
    assert_eq!(
        validate_index(duplicate.as_bytes()),
        Err(IndexError::DuplicateId)
    );
    let distinct = page("Local fixture", Some("dctx/other"));
    assert!(validate_index(distinct.as_bytes()).is_ok());
}
