// SPDX-License-Identifier: Apache-2.0
//! Lexical checks for index strings.
//! Implements the PL-C index schema patterns and semantics.md S-BOUNDS / S-MANIFEST.
//! Depends on [`crate::IndexError`]. The index walker calls these before accepting a page.
//! Display text that can spoof a terminal is refused here, not escaped into the page.

use crate::IndexError;

const RESERVED_ALIASES: &[&str] = &[
    "plugin", "pack", "map", "cache", "template", "help", "version",
];

pub(crate) fn check_summary(summary: &str) -> Result<(), IndexError> {
    if summary.len() > 256 {
        return Err(IndexError::Schema("summary"));
    }
    if hostile_display(summary) {
        return Err(IndexError::HostileDisplay);
    }
    Ok(())
}

/// Terminal controls, bidi overrides, and invisible characters are hostile display data.
pub(crate) fn hostile_display(text: &str) -> bool {
    text.chars().any(|ch| {
        let code = u32::from(ch);
        ch.is_control()
            || (0x202A..=0x202E).contains(&code)
            || (0x2066..=0x2069).contains(&code)
            || matches!(
                code,
                0x00AD | 0x200B | 0x200C | 0x200D | 0x200E | 0x200F | 0x2028 | 0x2029 | 0xFEFF
            )
    })
}

pub(crate) fn check_id(id: &str) -> Result<(), IndexError> {
    if id.len() > 128 {
        return Err(IndexError::Schema("id"));
    }
    let Some((publisher, name)) = id.split_once('/') else {
        return Err(IndexError::Schema("id"));
    };
    if !id_component(publisher) || !id_component(name) {
        return Err(IndexError::Schema("id"));
    }
    if publisher.ends_with('.') || name.ends_with('.') || is_windows_device(publisher) || is_windows_device(name)
    {
        return Err(IndexError::Schema("id"));
    }
    if RESERVED_ALIASES.contains(&name) {
        return Err(IndexError::AliasClaim);
    }
    Ok(())
}

pub(crate) fn check_repository(repository: &str) -> Result<(), IndexError> {
    if repository.is_empty() || repository.len() > 256 {
        return Err(IndexError::Schema("repository"));
    }
    let Some((owner, name)) = repository.split_once('/') else {
        return Err(IndexError::Schema("repository"));
    };
    if !repo_component(owner) || !repo_component(name) || name.contains('/') {
        return Err(IndexError::Schema("repository"));
    }
    if matches!(owner, "." | "..") || matches!(name, "." | "..") || owner.ends_with('.') || name.ends_with('.')
    {
        return Err(IndexError::Schema("repository"));
    }
    Ok(())
}

pub(crate) fn check_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(is_lower_hex)
}

pub(crate) fn check_semver(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 {
        return false;
    }
    let (main, build) = split_once(value, '+');
    if let Some(build) = build {
        if !dot_idents(build, false) {
            return false;
        }
    }
    let (core, pre) = split_once(main, '-');
    let mut parts = core.split('.');
    let (Some(major), Some(minor), Some(patch), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    if !numeric(major) || !numeric(minor) || !numeric(patch) {
        return false;
    }
    match pre {
        Some(pre) => dot_idents(pre, true),
        None => true,
    }
}

pub(crate) fn check_manifest_url(url: &str) -> Result<(), IndexError> {
    if url.is_empty()
        || url.len() > 2048
        || url.chars().any(|ch| ch.is_whitespace() || ch.is_control())
        || !url.starts_with("https://")
    {
        return Err(IndexError::Schema("manifest_url"));
    }
    let rest = &url["https://".len()..];
    if rest.contains('#') || rest.contains('@') {
        return Err(IndexError::Origin);
    }
    let authority = rest.split(['/', '?']).next().unwrap_or("");
    let (host, port) = split_host(authority)?;
    if let Some(port) = port {
        if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(IndexError::Origin);
        }
    }
    if host.is_empty() || forbidden_host(host) {
        return Err(IndexError::Origin);
    }
    Ok(())
}

fn id_component(value: &str) -> bool {
    let mut bytes = value.bytes();
    match bytes.next() {
        Some(byte) if byte.is_ascii_lowercase() || byte.is_ascii_digit() => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-'))
}

fn repo_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn is_windows_device(component: &str) -> bool {
    let base = component.split('.').next().unwrap_or(component);
    matches!(base, "con" | "prn" | "aux" | "nul")
        || (base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn split_once(value: &str, separator: char) -> (&str, Option<&str>) {
    match value.split_once(separator) {
        Some((left, right)) => (left, Some(right)),
        None => (value, None),
    }
}

fn numeric(value: &str) -> bool {
    !value.is_empty()
        && !(value.len() > 1 && value.starts_with('0'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn dot_idents(value: &str, prerelease: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && (!prerelease || !part.bytes().all(|byte| byte.is_ascii_digit()) || numeric(part))
        })
}

fn split_host(authority: &str) -> Result<(&str, Option<&str>), IndexError> {
    if authority.is_empty() {
        return Err(IndexError::Origin);
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, after)) = rest.split_once(']') else {
            return Err(IndexError::Origin);
        };
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port),
            None if after.is_empty() => None,
            None => return Err(IndexError::Origin),
        };
        return Ok((host, port));
    }
    if authority.matches(':').count() > 1 {
        return Err(IndexError::Origin);
    }
    Ok(match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    })
}

fn forbidden_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return true;
    }
    if let Some(octets) = parse_ipv4(&host) {
        return forbidden_ipv4(octets);
    }
    host.contains(':') && forbidden_ipv6(&host)
}

fn parse_ipv4(host: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut parts = host.split('.');
    for octet in &mut octets {
        let part = parts.next()?;
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) || part.len() > 3 {
            return None;
        }
        if !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        *octet = part.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(octets)
}

fn forbidden_ipv4(octets: [u8; 4]) -> bool {
    octets[0] == 0
        || octets[0] == 10
        || octets[0] == 127
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
        || octets == [255, 255, 255, 255]
}

fn forbidden_ipv6(host: &str) -> bool {
    if host == "::" || host == "::1" || host == "0:0:0:0:0:0:0:1" {
        return true;
    }
    let first = host.split(':').next().unwrap_or("");
    let Ok(value) = u16::from_str_radix(first, 16) else {
        return false;
    };
    (0xfe80..=0xfebf).contains(&value) || (0xfc00..=0xfdff).contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_summary_is_kept_and_hostile_text_is_refused() {
        assert!(check_summary("Local fixture for dctx-plugin-git.").is_ok());
        assert_eq!(
            check_summary("hide\u{202e}me"),
            Err(IndexError::HostileDisplay)
        );
        assert_eq!(check_summary("\u{1b}[31m"), Err(IndexError::HostileDisplay));
        assert_eq!(check_id("dctx/map"), Err(IndexError::AliasClaim));
        assert!(check_id("dctx/git").is_ok());
        assert!(check_semver("1.0.0"));
        assert!(!check_semver("1.0.0-01"));
        assert_eq!(
            check_manifest_url("https://user:password@example.invalid/plugin.json"),
            Err(IndexError::Origin)
        );
        assert_eq!(
            check_manifest_url("http://example.invalid/plugin.json"),
            Err(IndexError::Schema("manifest_url"))
        );
        assert!(check_manifest_url("https://example.invalid/plugin.json").is_ok());
        assert!(forbidden_host("127.0.0.1"));
        assert!(forbidden_host("10.1.2.3"));
        assert!(forbidden_host("fe80::1"));
    }
}
