// SPDX-License-Identifier: Apache-2.0
//! Node text owns its backing buffer. Decoded fields share the validated string
//! table, so graph materialization does not allocate a string per field.
use std::fmt;
use std::ops::{Deref, Range};
use std::sync::Arc;

struct Storage {
    buffer: String,
    // Empty for a newly built, independently owned field.
    ranges: Vec<Range<usize>>,
}

#[derive(Clone)]
pub(crate) struct NodeText {
    storage: Arc<Storage>,
    id: usize,
}

impl NodeText {
    pub(crate) fn table(buffer: String, ranges: Vec<Range<usize>>) -> Vec<Self> {
        let count = ranges.len();
        let storage = Arc::new(Storage { buffer, ranges });
        (0..count)
            .map(|id| Self {
                storage: storage.clone(),
                id,
            })
            .collect()
    }

    pub(crate) fn as_str(&self) -> &str {
        if self.storage.ranges.is_empty() {
            &self.storage.buffer
        } else {
            &self.storage.buffer[self.storage.ranges[self.id].clone()]
        }
    }
}

impl From<String> for NodeText {
    fn from(value: String) -> Self {
        Self {
            storage: Arc::new(Storage {
                buffer: value,
                ranges: Vec::new(),
            }),
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
