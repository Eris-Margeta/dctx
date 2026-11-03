// SPDX-License-Identifier: Apache-2.0
//! Hit text of the grep verb, shared by the index that writes it and the wire encoder that reads it.
//! A grep record keeps its hits in `source_utf8`: one `L<n>: <line>` row per hit, then at most one omission note.
//! Depends on [`super::answer::AnswerRecord`] only. No file is read here.
//! The text page indents each row by two spaces and ends with `<hits> hits in <symbols> symbols`.

use super::answer::AnswerRecord;

/// Characters kept from one source line.
const LINE_CHARS: usize = 200;

/// One hit row: the line number and the line trimmed of surrounding blanks and cut to 200 characters.
pub(crate) fn hit_row(line: u32, text: &str) -> String {
    let clipped: String = text.trim().chars().take(LINE_CHARS).collect();
    format!("L{line}: {clipped}")
}

/// The row that stands for `count` hits dropped from a group to fit the page budget.
pub(crate) fn omission_note(count: u64) -> String {
    format!("+{count} more hits in this symbol; narrow with --in")
}

/// The dropped-hit count when `row` is an omission note.
fn omitted_in(row: &str) -> Option<u64> {
    let rest = row.strip_prefix('+')?;
    let (digits, _) = rest.split_once(' ')?;
    digits.parse().ok()
}

/// Hits a group stands for: its rows plus the hits an omission note counts.
fn group_hits(source: &str) -> u64 {
    source
        .lines()
        .map(|row| omitted_in(row).unwrap_or(1))
        .sum()
}

/// Keeps the first half of a group's hits and notes the rest. `None` when one hit or fewer is left to drop.
pub(crate) fn shrink(source: &str) -> Option<String> {
    let mut rows: Vec<&str> = Vec::new();
    let mut omitted = 0u64;
    for row in source.lines() {
        match omitted_in(row) {
            Some(count) => omitted = omitted.saturating_add(count),
            None => rows.push(row),
        }
    }
    if rows.len() < 2 {
        return None;
    }
    let keep = rows.len() / 2;
    omitted = omitted.saturating_add(u64::try_from(rows.len() - keep).unwrap_or(u64::MAX));
    let mut out = rows[..keep].join("\n");
    out.push('\n');
    out.push_str(&omission_note(omitted));
    Some(out)
}

/// Appends the indented rows of one grep record.
pub(crate) fn append_rows(out: &mut String, record: &AnswerRecord) {
    for row in record.source_utf8.as_deref().unwrap_or("").lines() {
        out.push_str("  ");
        out.push_str(row);
        out.push('\n');
    }
}

/// Appends the last line of a grep page. A page with no group says `0 hits`.
pub(crate) fn append_summary(out: &mut String, records: &[AnswerRecord]) {
    if records.is_empty() {
        out.push_str("0 hits\n");
        return;
    }
    let hits: u64 = records.iter().map(|record| group_hits(record.source_utf8.as_deref().unwrap_or(""))).sum();
    out.push_str(&format!("{hits} hits in {} symbols\n", records.len()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_trimmed_clipped_and_counted() {
        assert_eq!(hit_row(7, "   let x = 1;  "), "L7: let x = 1;");
        assert_eq!(hit_row(1, &"y".repeat(300)).len(), "L1: ".len() + 200);
        let mut record = AnswerRecord::basic("s:a.rs#a", "fn", "a.rs");
        record.source_utf8 = Some("L1: a\nL2: b".to_string());
        let mut out = String::new();
        append_rows(&mut out, &record);
        append_summary(&mut out, &[record]);
        assert_eq!(out, "  L1: a\n  L2: b\n2 hits in 1 symbols\n");
        let mut empty = String::new();
        append_summary(&mut empty, &[]);
        assert_eq!(empty, "0 hits\n");
    }

    #[test]
    fn shrinking_halves_the_rows_and_keeps_the_hit_total() {
        let source = "L1: a\nL2: b\nL3: c\nL4: d";
        let once = shrink(source).expect("shrunk");
        assert_eq!(once, "L1: a\nL2: b\n+2 more hits in this symbol; narrow with --in");
        assert_eq!(group_hits(&once), 4);
        let twice = shrink(&once).expect("shrunk again");
        assert_eq!(twice, "L1: a\n+3 more hits in this symbol; narrow with --in");
        assert_eq!(group_hits(&twice), 4);
        assert!(shrink(&twice).is_none());
    }
}
