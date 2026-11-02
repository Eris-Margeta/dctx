//! Target preparation of the map verbs, before any scan: a stable id stays as given, a place-relative file becomes `m:<path>`.
//! Moved unchanged from the parent module; refusals are argument errors (exit 2), mapped by the parent with a hint.

use std::path::Path;

use dctx_map_query::Verb as QueryVerb;

/// Keeps a stable id, or turns an existing place-relative file into `m:<path>` before any scan.
pub(super) fn prepare(verb: QueryVerb, target: &str, place: &Path) -> Result<String, String> {
    if verb == QueryVerb::Find || is_stable_target(target) {
        return Ok(target.to_string());
    }
    let relative = target.strip_prefix("./").unwrap_or(target);
    if !is_relative_file(relative) {
        return Err("target must be a stable id or a place-relative file path".to_string());
    }
    match std::fs::metadata(place.join(relative)) {
        Ok(meta) if meta.is_file() => Ok(format!("m:{relative}")),
        Ok(_) => Err(format!("{relative} is a directory; pass a file path or a stable id")),
        Err(_) => Err(format!(
            "{relative} is not an existing file; pass a stable id or a place-relative file path"
        )),
    }
}

/// True for `@<6 hex>` and `<p|k|m|s|t|d>:<rest>` ids.
fn is_stable_target(target: &str) -> bool {
    if let Some(hex) = target.strip_prefix('@') {
        return hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    }
    let Some((prefix, rest)) = target.split_once(':') else {
        return false;
    };
    matches!(prefix, "p" | "k" | "m" | "s" | "t" | "d") && !rest.is_empty()
}

/// True for a non-empty `/`-separated path with no `.`, `..`, empty or absolute part, and no backslash, colon or control character.
fn is_relative_file(target: &str) -> bool {
    !target.is_empty()
        && !target.starts_with('/')
        && !target.contains(['\\', ':'])
        && !target.chars().any(char::is_control)
        && target.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}
