//! Closed-object reader, bounded value readers and object builder shared by every record codec.
//! Implements the structural layer of plugin-api README.md (closed shapes, `additionalProperties:false`)
//! and the integer/string/array bounds of semantics.md S-BOUNDS. Depends on json::value and refusal.
//! Used by records::* decode/encode functions; not part of the public API.

use super::value::{Json, MAX_SAFE_INTEGER};
use crate::refusal::Refusal;

/// Reads a closed object: every member must be consumed or `finish` refuses it.
pub(crate) struct Fields<'a> {
    members: &'a [(String, Json)],
    used: Vec<bool>,
}

impl<'a> Fields<'a> {
    pub(crate) fn of(value: &'a Json) -> Result<Self, Refusal> {
        let members = value.as_object().ok_or(Refusal::schema("expected-object"))?;
        Ok(Self { members, used: vec![false; members.len()] })
    }

    pub(crate) fn opt(&mut self, key: &str) -> Option<&'a Json> {
        let index = self.members.iter().position(|(name, _)| name == key)?;
        self.used[index] = true;
        Some(&self.members[index].1)
    }

    pub(crate) fn req(&mut self, key: &str) -> Result<&'a Json, Refusal> {
        self.opt(key).ok_or(Refusal::schema("missing-field"))
    }

    pub(crate) fn has(&self, key: &str) -> bool {
        self.members.iter().any(|(name, _)| name == key)
    }

    pub(crate) fn finish(self) -> Result<(), Refusal> {
        if self.used.iter().all(|used| *used) { Ok(()) } else { Err(Refusal::schema("unknown-field")) }
    }
}

/// Integer in `min..=max` (both within the safe range).
pub(crate) fn int_in(value: &Json, min: u64, max: u64) -> Result<u64, Refusal> {
    let number = value.as_int().ok_or(Refusal::schema("expected-integer"))?;
    let number = u64::try_from(number).map_err(|_| Refusal::schema("integer-range"))?;
    if (min..=max).contains(&number) { Ok(number) } else { Err(Refusal::schema("integer-range")) }
}

/// `uint`: 0 through 2^53 - 1.
pub(crate) fn uint(value: &Json) -> Result<u64, Refusal> {
    int_in(value, 0, MAX_SAFE_INTEGER)
}

/// `positive`: 1 through 2^53 - 1.
pub(crate) fn positive(value: &Json) -> Result<u64, Refusal> {
    int_in(value, 1, MAX_SAFE_INTEGER)
}

pub(crate) fn boolean(value: &Json) -> Result<bool, Refusal> {
    value.as_bool().ok_or(Refusal::schema("expected-boolean"))
}

pub(crate) fn string(value: &Json) -> Result<&str, Refusal> {
    value.as_str().ok_or(Refusal::schema("expected-string"))
}

/// Requires the exact string constant `expected`.
pub(crate) fn constant(value: &Json, expected: &str) -> Result<(), Refusal> {
    if value.as_str() == Some(expected) { Ok(()) } else { Err(Refusal::schema("constant-mismatch")) }
}

/// Requires `true` (schema `const: true`).
pub(crate) fn constant_true(value: &Json) -> Result<(), Refusal> {
    if value.as_bool() == Some(true) { Ok(()) } else { Err(Refusal::schema("constant-mismatch")) }
}

/// Requires the integer constant `expected`.
pub(crate) fn constant_int(value: &Json, expected: i64) -> Result<(), Refusal> {
    if value.as_int() == Some(expected) { Ok(()) } else { Err(Refusal::schema("constant-mismatch")) }
}

/// Decodes a bounded array item by item.
pub(crate) fn list<T>(
    value: &Json,
    min: usize,
    max: usize,
    item: impl Fn(&Json) -> Result<T, Refusal>,
) -> Result<Vec<T>, Refusal> {
    let items = value.as_array().ok_or(Refusal::schema("expected-array"))?;
    if items.len() < min || items.len() > max {
        return Err(Refusal::schema("array-length"));
    }
    items.iter().map(item).collect()
}

/// Like [`list`] but also enforces `uniqueItems` on the decoded values.
pub(crate) fn unique_list<T: PartialEq>(
    value: &Json,
    min: usize,
    max: usize,
    item: impl Fn(&Json) -> Result<T, Refusal>,
) -> Result<Vec<T>, Refusal> {
    let decoded = list(value, min, max, item)?;
    for (index, entry) in decoded.iter().enumerate() {
        if decoded[..index].contains(entry) {
            return Err(Refusal::schema("duplicate-item"));
        }
    }
    Ok(decoded)
}

/// Builds an object member by member, skipping absent optional fields.
#[derive(Default)]
pub(crate) struct Obj(Vec<(String, Json)>);

impl Obj {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn put(mut self, key: &str, value: Json) -> Self {
        self.0.push((key.to_owned(), value));
        self
    }

    pub(crate) fn maybe(self, key: &str, value: Option<Json>) -> Self {
        match value {
            Some(value) => self.put(key, value),
            None => self,
        }
    }

    pub(crate) fn build(self) -> Json {
        Json::Object(self.0)
    }
}

/// Encodes a slice with `item`.
pub(crate) fn array<T>(items: &[T], item: impl Fn(&T) -> Json) -> Json {
    Json::Array(items.iter().map(item).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(members: &[(&str, Json)]) -> Json {
        Json::Object(members.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect())
    }

    #[test]
    fn closed_objects_refuse_unknown_and_missing_fields() {
        let value = object(&[("a", Json::Int(1)), ("b", Json::Bool(true))]);
        let mut fields = Fields::of(&value).expect("object");
        assert_eq!(fields.req("a").and_then(uint), Ok(1));
        assert_eq!(fields.finish().unwrap_err().reason, "unknown-field");
        let mut fields = Fields::of(&value).expect("object");
        assert_eq!(fields.req("c").unwrap_err().reason, "missing-field");
        let mut fields = Fields::of(&value).expect("object");
        assert!(fields.has("b"));
        let _ = (fields.req("a"), fields.opt("b"));
        assert_eq!(fields.finish(), Ok(()));
        assert!(Fields::of(&Json::Int(1)).is_err());
    }

    #[test]
    fn integer_bounds_are_inclusive_and_signed_values_refused() {
        assert_eq!(uint(&Json::Int(9_007_199_254_740_991)), Ok(9_007_199_254_740_991));
        assert!(uint(&Json::Int(9_007_199_254_740_992)).is_err());
        assert!(uint(&Json::Int(-1)).is_err());
        assert!(positive(&Json::Int(0)).is_err());
        assert!(uint(&Json::str("1")).is_err());
    }

    #[test]
    fn arrays_enforce_length_and_uniqueness() {
        let value = Json::Array(vec![Json::Int(1), Json::Int(1)]);
        assert_eq!(list(&value, 0, 2, uint), Ok(vec![1, 1]));
        assert_eq!(unique_list(&value, 0, 2, uint).unwrap_err().reason, "duplicate-item");
        assert_eq!(list(&value, 0, 1, uint).unwrap_err().reason, "array-length");
        let built = Obj::new().put("a", array(&[1u64], |v| Json::uint(*v))).maybe("b", None).build();
        assert_eq!(built, object(&[("a", Json::Array(vec![Json::Int(1)]))]));
    }
}
