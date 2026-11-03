// SPDX-License-Identifier: Apache-2.0
//! One newline/byte-offset index per source file, used to resolve node spans.
//! Implements DCTX-COLD-SCAN C1 (`lex_dispatch` no longer rescans from byte 0).
//! Depends on the standard library. `from_nodes` builds one index for each file.
//! Empty lines, CRLF, UTF-8 byte offsets and a missing final newline stay exact.
//! Line lookup is O(1) by number, inside the required O(log n) bound.

/// Start and trimmed-end offsets for every `split_inclusive('\n')` row.
pub(super) struct SpanIndex {
    starts: Vec<u64>,
    ends: Vec<u64>,
    /// Span end when the requested end line is past the last row.
    file_len: u64,
}

impl SpanIndex {
    /// Indexes `text` in one forward pass.
    pub(super) fn build(text: &str) -> Self {
        let rows = text.bytes().filter(|byte| *byte == b'\n').count() + usize::from(!text.ends_with('\n'));
        let mut starts = Vec::with_capacity(rows);
        let mut ends = Vec::with_capacity(rows);
        let mut offset = 0usize;
        for line in text.split_inclusive('\n') {
            let start = u64::try_from(offset).unwrap_or(u64::MAX);
            let content = line.trim_end_matches(['\r', '\n']).len();
            let end = u64::try_from(offset.saturating_add(content)).unwrap_or(u64::MAX);
            starts.push(start);
            ends.push(end);
            offset = offset.saturating_add(line.len());
        }
        Self { starts, ends, file_len: u64::try_from(text.len()).unwrap_or(u64::MAX) }
    }

    /// `[start, end)` for inclusive 1-based `lines`, or `[0, 0]` when the start line is absent.
    ///
    /// An end line past the last row uses the file length. The end excludes that row's trailing CR/LF.
    pub(super) fn line_span(&self, lines: [u32; 2]) -> [u64; 2] {
        let start_line = lines[0].max(1);
        let end_line = lines[1].max(start_line);
        let Some(&start) = self.starts.get(row(start_line)) else {
            return [0, 0];
        };
        let end = self.ends.get(row(end_line)).copied().unwrap_or(self.file_len);
        [start, end]
    }
}

fn row(line: u32) -> usize {
    usize::try_from(line.saturating_sub(1)).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use super::super::lex_dispatch::line_span_oracle;
    use super::SpanIndex;

    fn assert_oracle(text: &str) {
        let index = SpanIndex::build(text);
        assert_eq!(index.starts.len(), text.split_inclusive('\n').count(), "{text:?}");
        assert_eq!(index.starts.len(), index.ends.len());
        let last = u32::try_from(index.starts.len()).unwrap_or(u32::MAX).saturating_add(2);
        for start in 0..=last {
            for end in 0..=last {
                let lines = [start, end];
                assert_eq!(index.line_span(lines), line_span_oracle(text, lines), "{text:?} {lines:?}");
            }
        }
        assert_eq!(index.line_span([u32::MAX, u32::MAX]), line_span_oracle(text, [u32::MAX, u32::MAX]));
    }

    #[test]
    fn spans_match_the_pre_index_scan() {
        let mut many = String::new();
        for number in 0..40 {
            let _ = write!(many, "é{number}\r\n\n");
        }
        many.push_str("tail");
        let samples = [
            "",
            "\n",
            "\n\n",
            "\r\n",
            "ab\r",
            "a\r\n\nb",
            "a\rb\nc\r",
            "é\nβ",
            "é",
            "{\"Alpha\":1,\"Beta\":{\"Gamma\":2},\"Delta\":[3]}",
            "{\n  \"Alpha\": 1,\n\n  \"Beta\": {\n    \"Gamma\": 2\n  }\n}\n",
            "{\r\n  \"Alpha\": 1,\r\n\r\n  \"Beta\": 2\r\n}\r\n",
            "{\n  \"名前\": \"café\",\n  \"Key\": \"🔑\"\n}\n",
            "{\n  \"Alpha\": 1,\n\n  \"Tail\": \"é\"}",
            many.as_str(),
        ];
        for text in samples {
            assert_oracle(text);
        }
    }

    #[test]
    fn pinned_edges_keep_crlf_empty_and_final_line_bytes() {
        let text = "a\r\n\nb";
        let index = SpanIndex::build(text);
        assert_eq!(index.line_span([1, 1]), [0, 1]);
        assert_eq!(index.line_span([2, 2]), [3, 3]);
        assert_eq!(index.line_span([3, 3]), [4, 5]);
        assert_eq!(index.line_span([1, 3]), [0, 5]);
        assert_eq!(index.line_span([1, 9]), [0, 5]);
        assert_eq!(index.line_span([4, 4]), [0, 0]);
        assert_eq!(index.line_span([0, 0]), [0, 1]);
        assert_eq!(index.line_span([3, 1]), [4, 5]);

        let cr = "ab\r";
        let index = SpanIndex::build(cr);
        assert_eq!(index.line_span([1, 1]), [0, 2]);
        assert_eq!(index.line_span([1, 2]), [0, 3]);

        let one = "ééé";
        let index = SpanIndex::build(one);
        assert_eq!(index.starts.len(), 1);
        assert_eq!(index.line_span([1, 1]), [0, 6]);
    }
}
