// SPDX-License-Identifier: Apache-2.0
//! Exercise every registered extension, including heuristic and document skeletons.
use super::extract::extract_structural;
use crate::languages::LANGUAGES;
use proptest::prelude::*;

proptest! {
    #[test]
    fn robustness_all_registered_skeletons(
        bytes in prop_oneof![
            prop::collection::vec(any::<u8>(), 0..2048),
            prop::collection::vec(any::<char>(), 0..256)
                .prop_map(|chars| chars.into_iter().collect::<String>().into_bytes()),
            "[a-zA-Z0-9_ \\n\\r\\t{}()\\[\\]<>:;#/'\"=éλ🦀$|~\\\\-]{0,512}".prop_map(String::into_bytes),
        ],
    ) {
        for language in LANGUAGES {
            let paths = language.extensions.iter().map(|ext| format!("sample.{ext}"))
                .chain(language.files.iter().map(|file| file.replace('*', "sample")));
            for path in paths {
                let first = extract_structural(&path, &bytes);
                prop_assert_eq!(&first, &extract_structural(&path, &bytes), "{}", path);
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    for symbol in first {
                        let [start, end] = symbol.span;
                        prop_assert!(start <= end && end <= bytes.len() as u64, "{}: {:?}", path, symbol);
                        prop_assert!(text.is_char_boundary(usize::try_from(start).unwrap()));
                        prop_assert!(text.is_char_boundary(usize::try_from(end).unwrap()));
                        prop_assert!(symbol.lines[0] >= 1 && symbol.lines[0] <= symbol.lines[1]);
                    }
                } else {
                    prop_assert!(first.is_empty());
                }
            }
        }
    }
}
