// SPDX-License-Identifier: Apache-2.0
//! Closed-object field access over the protocol's strict JSON value tree.
//! Implements `additionalProperties: false` and required-member rules of the plugin-api schemas.
//! Depends on dctx-plugin-protocol's `Json`; used by manifest, bundle, index and registry codecs.
//! Every failure names the document and a static rule; no untrusted text escapes.

use dctx_plugin_protocol::Json;
use dctx_plugin_protocol::json::value::MAX_SAFE_INTEGER;

use crate::error::{DistributionError, Document};

/// A JSON object checked against its allowed and required member names.
#[derive(Clone, Copy, Debug)]
pub struct Closed<'a> {
    members: &'a [(String, Json)],
    document: Document,
}

impl<'a> Closed<'a> {
    /// Refuses non-objects, unknown members and missing required members.
    pub fn new(
        value: &'a Json,
        document: Document,
        allowed: &[&str],
        required: &[&str],
    ) -> Result<Self, DistributionError> {
        let members = value.as_object().ok_or(DistributionError::malformed(document, "not-object"))?;
        if members.iter().any(|(name, _)| !allowed.contains(&name.as_str())) {
            return Err(DistributionError::malformed(document, "unknown-field"));
        }
        if required.iter().any(|name| !members.iter().any(|(member, _)| member == name)) {
            return Err(DistributionError::malformed(document, "missing-field"));
        }
        Ok(Self { members, document })
    }

    /// Optional member.
    pub fn get(&self, name: &str) -> Option<&'a Json> {
        self.members.iter().find(|(member, _)| member == name).map(|(_, value)| value)
    }

    /// Required member (already checked by [`Closed::new`]).
    pub fn req(&self, name: &str) -> Result<&'a Json, DistributionError> {
        self.get(name).ok_or(self.refuse("missing-field"))
    }

    /// Required string.
    pub fn str(&self, name: &str) -> Result<&'a str, DistributionError> {
        self.req(name)?.as_str().ok_or(self.refuse("not-string"))
    }

    /// Required integer in `0..=2^53-1`.
    pub fn uint(&self, name: &str) -> Result<u64, DistributionError> {
        uint(self.req(name)?, self.document)
    }

    /// Required integer in `1..=2^53-1`.
    pub fn positive(&self, name: &str) -> Result<u64, DistributionError> {
        let value = self.uint(name)?;
        if value == 0 { Err(self.refuse("not-positive")) } else { Ok(value) }
    }

    /// Required array with an item count in `min..=max`.
    pub fn array(&self, name: &str, min: usize, max: usize) -> Result<&'a [Json], DistributionError> {
        let items = self.req(name)?.as_array().ok_or(self.refuse("not-array"))?;
        if items.len() < min || items.len() > max {
            return Err(self.refuse("array-length"));
        }
        Ok(items)
    }

    /// Required object member, unchecked beyond its type.
    pub fn object(&self, name: &str) -> Result<&'a Json, DistributionError> {
        let value = self.req(name)?;
        value.as_object().map(|_| value).ok_or(self.refuse("not-object"))
    }

    /// Refusal in this object's document.
    pub fn refuse(&self, reason: &'static str) -> DistributionError {
        DistributionError::malformed(self.document, reason)
    }
}

/// Integer in the contract's safe range.
pub fn uint(value: &Json, document: Document) -> Result<u64, DistributionError> {
    value
        .as_int()
        .and_then(|number| u64::try_from(number).ok())
        .filter(|number| *number <= MAX_SAFE_INTEGER)
        .ok_or(DistributionError::malformed(document, "not-uint"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(members: Vec<(&str, Json)>) -> Json {
        Json::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    #[test]
    fn closed_objects_refuse_unknown_and_missing_members() {
        let value = object(vec![("a", Json::Int(1)), ("b", Json::str("x"))]);
        let closed = Closed::new(&value, Document::Index, &["a", "b"], &["a"]).unwrap();
        assert_eq!(closed.uint("a"), Ok(1));
        assert_eq!(closed.str("b"), Ok("x"));
        assert!(closed.positive("b").is_err());
        assert!(Closed::new(&value, Document::Index, &["a"], &["a"]).is_err());
        assert!(Closed::new(&value, Document::Index, &["a", "b", "c"], &["c"]).is_err());
        assert!(Closed::new(&Json::Int(1), Document::Index, &[], &[]).is_err());
    }

    #[test]
    fn integers_stay_in_the_safe_range() {
        assert!(uint(&Json::Int(-1), Document::Index).is_err());
        assert!(uint(&Json::Int(9_007_199_254_740_992), Document::Index).is_err());
        assert_eq!(uint(&Json::Int(9_007_199_254_740_991), Document::Index), Ok(9_007_199_254_740_991));
    }
}
