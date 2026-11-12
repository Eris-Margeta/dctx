// SPDX-License-Identifier: Apache-2.0
//! Node text owns its backing buffer. Decoded fields share the validated string
//! table, so graph materialization does not allocate a string per field.
use std::fmt;
use std::ops::{Deref, Range};
use std::sync::Arc;

// Repeated graph vocabulary needs neither a heap allocation nor a shared owner.
const STATIC: [&str; 18] = [
    "",
    "sig",
    "doc",
    "module",
    "fn",
    "method",
    "struct",
    "enum",
    "trait",
    "impl",
    "const",
    "type",
    "class",
    "interface",
    "heading",
    "key",
    "symbol",
    "test",
];

fn static_id(value: &str) -> Option<usize> {
    match value {
        "" => Some(0),
        "sig" => Some(1),
        "doc" => Some(2),
        "module" => Some(3),
        "fn" => Some(4),
        "method" => Some(5),
        "struct" => Some(6),
        "enum" => Some(7),
        "trait" => Some(8),
        "impl" => Some(9),
        "const" => Some(10),
        "type" => Some(11),
        "class" => Some(12),
        "interface" => Some(13),
        "heading" => Some(14),
        "key" => Some(15),
        "symbol" => Some(16),
        "test" => Some(17),
        _ => None,
    }
}

struct Storage {
    buffer: String,
    // Empty for an independently owned field. In a table, end == 0 encodes
    // a STATIC index in start; all other entries are byte ranges in buffer.
    ranges: Vec<Range<usize>>,
}

#[derive(Clone)]
pub(crate) struct NodeText {
    // Static vocabulary (including empty fields) needs no reference counting.
    storage: Option<Arc<Storage>>,
    id: usize,
}

/// Decode-time string table. Materialize owners only for referenced, nonstatic fields.
/// Paths and edge labels use the same wire table without needing a `NodeText`.
pub(crate) struct NodeTextTable(Arc<Storage>);

impl NodeTextTable {
    pub(crate) fn new(buffer: String, mut ranges: Vec<Range<usize>>) -> Self {
        for range in &mut ranges {
            if let Some(id) = static_id(&buffer[range.clone()]) {
                *range = id..0;
            }
        }
        Self(Arc::new(Storage { buffer, ranges }))
    }

    pub(crate) fn get(&self, id: usize) -> Option<NodeText> {
        let range = self.0.ranges.get(id)?;
        Some(NodeText {
            storage: (range.end != 0).then(|| self.0.clone()),
            id: if range.end == 0 { range.start } else { id },
        })
    }
}

impl NodeText {
    pub(crate) fn as_str(&self) -> &str {
        let Some(storage) = &self.storage else {
            return STATIC[self.id];
        };
        if storage.ranges.is_empty() {
            &storage.buffer
        } else {
            &storage.buffer[storage.ranges[self.id].clone()]
        }
    }
}

impl From<String> for NodeText {
    fn from(value: String) -> Self {
        if let Some(id) = static_id(&value) {
            return Self { storage: None, id };
        }
        Self {
            storage: Some(Arc::new(Storage {
                buffer: value,
                ranges: Vec::new(),
            })),
            id: 0,
        }
    }
}

impl Deref for NodeText {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for NodeText {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl Eq for NodeText {}
impl PartialEq<str> for NodeText {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}
impl PartialEq<&str> for NodeText {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}
impl PartialEq<String> for NodeText {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}
impl fmt::Debug for NodeText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}
impl fmt::Display for NodeText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_fields_keep_their_owner_without_materializing_unused_or_empty_entries() {
        let table = NodeTextTable::new("firstsecond".into(), vec![0..0, 0..5, 5..11]);
        assert!(table.get(3).is_none());
        let empty = table.get(0).unwrap();
        assert!(empty.storage.is_none());
        let first = table.get(1).unwrap();
        let second = table.get(2).unwrap();
        assert_eq!(Arc::strong_count(&table.0), 3);
        let copy = first.clone();
        drop(table);
        drop(first);
        assert_eq!(copy, "first");
        assert_eq!(second, "second");
        assert_eq!(empty, "");
        assert_eq!(NodeText::from(String::new()), empty);
        assert_eq!(NodeText::from("first".to_string()), copy);
    }

    #[test]
    fn static_vocabulary_is_identical_for_owned_and_decoded_fields() {
        let buffer = STATIC.concat();
        let mut offset = 0;
        let ranges = STATIC
            .iter()
            .map(|value| {
                let start = offset;
                offset += value.len();
                start..offset
            })
            .collect();
        let table = NodeTextTable::new(buffer, ranges);
        for (id, value) in STATIC.iter().enumerate() {
            assert_eq!(static_id(value), Some(id));
            let decoded = table.get(id).unwrap();
            assert!(decoded.storage.is_none());
            assert_eq!(decoded, *value);
            assert_eq!(decoded, NodeText::from((*value).to_owned()));
            assert_eq!(decoded.clone().to_string(), *value);
        }
        assert_eq!(Arc::strong_count(&table.0), 1);
    }
}
