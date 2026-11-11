// SPDX-License-Identifier: Apache-2.0
//! Conservative identifier reference lines, excluding comments, strings and declaration names.
use super::generic::{Syntax, mask};

/// Identifier occurrences in masked source, retaining offsets and one-based lines.
/// Import aliases are included, so a renamed callee's call lines remain visible.
pub(crate) fn references(
    source: &str,
    path: &str,
    name: &str,
    mut visit: impl FnMut(usize, u32) -> bool,
) {
    let language = super::extract::language_of(path);
    let hash = matches!(language, "python" | "ruby" | "shell");
    let syntax = Syntax {
        line: if hash { &["#"] } else { &["//"] },
        block: if hash { None } else { Some(("/*", "*/")) },
        nested: language == "rust",
        quotes: if hash { b"\"'" } else { b"\"'`" },
        triple: matches!(language, "python" | "kotlin" | "java"),
    };
    let masked = mask(source, &syntax);
    let mut names = vec![name];
    for line in masked.lines() {
        if !(line.contains("import ") || line.contains("use ")) {
            continue;
        }
        let words: Vec<_> = line
            .split(|c: char| !word(c))
            .filter(|s| !s.is_empty())
            .collect();
        for chunk in words.windows(3) {
            if chunk[0] == name && chunk[1] == "as" {
                names.push(chunk[2]);
            }
        }
    }
    let mut offset = 0;
    for (index, line) in masked.split_inclusive('\n').enumerate() {
        for (at, token) in line
            .match_indices(|c: char| !word(c))
            .map(|(at, delimiter)| (at, delimiter.len()))
            .chain(std::iter::once((line.len(), 0)))
            .scan(0, |start, (end, width)| {
                let at = *start;
                *start = end + width;
                Some((at, line.get(at..end).unwrap_or("")))
            })
        {
            if names.contains(&token)
                && !visit(offset + at, u32::try_from(index + 1).unwrap_or(u32::MAX))
            {
                return;
            }
        }
        offset += line.len();
    }
}

fn word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_offsets_handle_utf8_aliases_and_masked_multiline_text() {
        let source = "from x import target as alias\n# target\n'''target\ntarget'''\nα = target; b = alias()\nretarget()\n";
        let mut found = Vec::new();
        references(source, "sample.py", "target", |offset, line| {
            found.push((
                source[offset..].split(|c: char| !word(c)).next().unwrap(),
                line,
            ));
            true
        });
        assert_eq!(
            found,
            [("target", 1), ("alias", 1), ("target", 5), ("alias", 5)]
        );
        let mut visited = 0;
        references(source, "sample.py", "target", |_, _| {
            visited += 1;
            false
        });
        assert_eq!(visited, 1);
    }
}
