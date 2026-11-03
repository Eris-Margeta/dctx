// SPDX-License-Identifier: Apache-2.0
//! JSON value tree produced by the strict decoder and consumed by record codecs.
//! Implements the value layer of plugin-api wire.md §Bytes and semantics.md S-BOUNDS.
//! Depends on nothing; used by `json::decode`, `json::encode`, `json::fields` and records.
//! Objects keep member order and never contain a duplicate key; numbers are integers only.

/// Largest integer the contract admits (2^53 - 1).
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// A decoded JSON value. Only integer numbers exist in this protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A decimal integer as spelled on the wire; schema layers enforce the safe range.
    Int(i64),
    /// A string without NUL or unpaired surrogates.
    Str(String),
    /// An ordered array.
    Array(Vec<Json>),
    /// An object in member order with unique keys.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Builds a string value.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn str(value: &str) -> Self {
        Self::Str(value.to_owned())
    }

    /// Builds an integer from an already bounded unsigned value.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn uint(value: u64) -> Self {
        Self::Int(i64::try_from(value).unwrap_or(i64::MAX))
    }

    /// Member lookup on an object; `None` for other values.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(members) => members.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    /// Object members, if this is an object.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Self::Object(members) => Some(members),
            _ => None,
        }
    }

    /// Array items, if this is an array.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// String contents, if this is a string.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    /// Integer, if this is a number.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// Boolean, if this is a boolean.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Structural equality that ignores object member order (JSON Schema `uniqueItems` semantics).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn same_value(&self, other: &Json) -> bool {
        match (self, other) {
            (Self::Object(left), Self::Object(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .all(|(key, value)| other.get(key).is_some_and(|found| value.same_value(found)))
            }
            (Self::Array(left), Self::Array(right)) => {
                left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a.same_value(b))
            }
            _ => self == other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_value_ignores_member_order_only() {
        let a = Json::Object(vec![("a".into(), Json::Int(1)), ("b".into(), Json::Bool(true))]);
        let b = Json::Object(vec![("b".into(), Json::Bool(true)), ("a".into(), Json::Int(1))]);
        let c = Json::Object(vec![("b".into(), Json::Bool(false)), ("a".into(), Json::Int(1))]);
        assert!(a.same_value(&b));
        assert!(!a.same_value(&c));
        let left = Json::Array(vec![Json::Int(1), Json::Int(2)]);
        let right = Json::Array(vec![Json::Int(2), Json::Int(1)]);
        assert!(!left.same_value(&right));
    }

    #[test]
    fn accessors_reject_other_kinds() {
        let value = Json::str("x");
        assert_eq!(value.as_str(), Some("x"));
        assert_eq!(value.as_int(), None);
        assert_eq!(value.get("x"), None);
        assert_eq!(Json::uint(MAX_SAFE_INTEGER).as_int(), Some(9_007_199_254_740_991));
    }
}
