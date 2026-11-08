// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
//! Property tests for the MCP JSON decoder. Kept out of `json.rs` because the
//! standard-library-only playground compiles that file directly.

use super::json::parse;
use proptest::prelude::*;

proptest! {
    #[test]
    fn robustness_json_decoder(text in ".{0,2048}") {
        let first = parse(&text);
        prop_assert_eq!(format!("{first:?}"), format!("{:?}", parse(&text)));
    }
}
