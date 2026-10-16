//! Relative object names under one store root.
//! Implements SEGMENTS-v1 immutable digest names and the ban on `.git` paths (decision 078).
//! Depends on [`super::error::StoreError`]. The filesystem adapters refuse any other shape.
//! Names are content addresses. Publication renames into them and never overwrites bytes.

use super::error::StoreError;

const HEX: &[u8; 16] = b"0123456789abcdef";

pub(crate) fn hex_encode(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

pub(crate) fn hex_decode(text: &str) -> Result<[u8; 32], StoreError> {
    if text.len() != 64 {
        return Err(StoreError::CorruptIndex { reason: "name" });
    }
    let raw = text.as_bytes();
    let mut out = [0u8; 32];
    for index in 0..32 {
        let high = nibble(raw[index * 2])?;
        let low = nibble(raw[index * 2 + 1])?;
        out[index] = (high << 4) | low;
    }
    Ok(out)
}

fn nibble(byte: u8) -> Result<u8, StoreError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(StoreError::CorruptIndex { reason: "name" }),
    }
}

/// Rejects absolute paths, `..`, empty components, backslash, and a `.git` component.
pub(crate) fn check_rel(path: &str) -> Result<(), StoreError> {
    if path.is_empty() || path.len() > 512 || path.starts_with('/') || path.contains('\\') {
        return Err(StoreError::InvalidRequest { reason: "path" });
    }
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." || part == ".git" {
            return Err(StoreError::InvalidRequest { reason: "path" });
        }
    }
    Ok(())
}

pub(crate) fn check_root(root: &str) -> Result<(), StoreError> {
    if root.is_empty() || root.len() > 64 || root == ".git" {
        return Err(StoreError::InvalidRequest { reason: "root" });
    }
    let ok = root.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if !ok {
        return Err(StoreError::InvalidRequest { reason: "root" });
    }
    Ok(())
}

pub(crate) fn check_token(token: &str) -> Result<(), StoreError> {
    if token.is_empty() || token.len() > 64 || token == ".git" {
        return Err(StoreError::InvalidRequest { reason: "namespace" });
    }
    let ok = token
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if !ok {
        return Err(StoreError::InvalidRequest { reason: "namespace" });
    }
    Ok(())
}

pub(crate) fn segment_path(digest: &[u8; 32]) -> String {
    format!("segments/{}.seg", hex_encode(digest))
}

pub(crate) fn manifest_path(digest: &[u8; 32]) -> String {
    format!("manifests/{}.man", hex_encode(digest))
}

pub(crate) fn current_path(root: &str) -> String {
    format!("roots/{root}/current")
}

pub(crate) fn previous_path(root: &str) -> String {
    format!("roots/{root}/previous")
}

pub(crate) fn tmp_path(name: &str) -> String {
    format!("tmp/{name}")
}

pub(crate) const fn cursor_path() -> &'static str {
    "maintenance/cursor"
}

pub(crate) fn object_digest(path: &str) -> Option<[u8; 32]> {
    let name = path.rsplit('/').next()?;
    let hex = name.strip_suffix(".seg").or_else(|| name.strip_suffix(".man"))?;
    hex_decode(hex).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_git_components_are_refused() {
        let bytes = [0xabu8; 32];
        let text = hex_encode(&bytes);
        assert_eq!(text.len(), 64);
        assert_eq!(hex_decode(&text).unwrap(), bytes);
        assert!(hex_decode("abcd").is_err());
        assert!(check_rel("segments/.git/objects").is_err());
        assert!(check_rel("../segments/aa.seg").is_err());
        assert!(check_root(".git").is_err());
        assert!(check_root("main").is_ok());
        assert_eq!(
            object_digest(&segment_path(&bytes)).unwrap(),
            bytes
        );
    }
}
