//! Closed index page: identity, releases, and display text.
//! Implements index.schema.json, semantics.md S-BOUNDS, and PLUGINS.md §5.3.
//! Depends on the strict JSON parser and text checks. Signature verification calls [`validate_index`].
//! Inclusion in a page grants nothing. Alias fields and duplicate claims are refused.

use super::json::{Json, parse};
use super::text::{check_hex64, check_id, check_manifest_url, check_repository, check_semver, check_summary};
use crate::IndexError;

/// One signed discovery page. The byte sequence that verified is not reserialized here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    /// Monotonic page sequence. A later page replaces an older cached page only after verification.
    pub sequence: u64,
    /// Plugins on this page. Identifiers are unique.
    pub plugins: Vec<Plugin>,
}

/// Discovery record for one plugin. It is not a manifest and not consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    /// Canonical `publisher/name` id.
    pub id: String,
    /// `owner/name` locator. A repository is not a trust anchor.
    pub repository: String,
    /// Bounded display summary with no terminal controls.
    pub summary: String,
    /// Lowercase SHA-256 fingerprint of the publisher's 32-byte key.
    pub publisher_key_fingerprint: String,
    /// Release locators in increasing `release_sequence` order.
    pub releases: Vec<Release>,
}

/// One release locator. The manifest digest is not a signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// SemVer 2.0 version with no leading-zero numeric prerelease id.
    pub version: String,
    /// Publisher release counter. Unrequested rollback is not represented by a lower value.
    pub release_sequence: u64,
    /// HTTPS manifest locator without userinfo or a private host.
    pub manifest_url: String,
    /// Lowercase SHA-256 of the exact manifest bytes.
    pub manifest_sha256: String,
}

/// Validates exact index bytes against the schema and the semantic refusals.
///
/// # Errors
/// Returns [`IndexError`] for bounds, schema, duplicate ids, alias claims, and hostile text.
pub fn validate_index(bytes: &[u8]) -> Result<Index, IndexError> {
    let json = parse(bytes)?;
    let fields = object(&json)?;
    expect_only(fields, &["schema", "sequence", "plugins"])?;
    let schema = number(required(fields, "schema")?)?;
    if schema != 1 {
        return Err(IndexError::Schema("schema"));
    }
    let sequence = positive(required(fields, "sequence")?, "sequence")?;
    let plugins_json = array(required(fields, "plugins")?)?;
    if plugins_json.len() > 2048 {
        return Err(IndexError::Schema("plugins"));
    }
    let mut plugins = Vec::with_capacity(plugins_json.len());
    let mut ids = Vec::with_capacity(plugins_json.len());
    let mut repositories = Vec::with_capacity(plugins_json.len());
    for plugin_json in plugins_json {
        let plugin = parse_plugin(plugin_json)?;
        if ids.iter().any(|id: &String| id.eq_ignore_ascii_case(&plugin.id)) {
            return Err(IndexError::DuplicateId);
        }
        if repositories
            .iter()
            .any(|repository: &String| repository.eq_ignore_ascii_case(&plugin.repository))
        {
            return Err(IndexError::AliasClaim);
        }
        ids.push(plugin.id.clone());
        repositories.push(plugin.repository.clone());
        plugins.push(plugin);
    }
    Ok(Index { sequence, plugins })
}

fn parse_plugin(value: &Json) -> Result<Plugin, IndexError> {
    let fields = object(value)?;
    expect_only(
        fields,
        &["id", "repository", "summary", "publisher_key_fingerprint", "releases"],
    )?;
    let id = string(required(fields, "id")?)?;
    check_id(id)?;
    let repository = string(required(fields, "repository")?)?;
    check_repository(repository)?;
    let summary = string(required(fields, "summary")?)?;
    check_summary(summary)?;
    let publisher_key_fingerprint = string(required(fields, "publisher_key_fingerprint")?)?;
    if !check_hex64(publisher_key_fingerprint) {
        return Err(IndexError::Schema("publisher_key_fingerprint"));
    }
    let releases_json = array(required(fields, "releases")?)?;
    if releases_json.is_empty() || releases_json.len() > 128 {
        return Err(IndexError::Schema("releases"));
    }
    let mut releases = Vec::with_capacity(releases_json.len());
    let mut versions = Vec::with_capacity(releases_json.len());
    let mut last_sequence = 0u64;
    for release_json in releases_json {
        let release = parse_release(release_json)?;
        if release.release_sequence <= last_sequence || versions.iter().any(|version| version == &release.version)
        {
            return Err(IndexError::DuplicateRelease);
        }
        last_sequence = release.release_sequence;
        versions.push(release.version.clone());
        releases.push(release);
    }
    Ok(Plugin {
        id: id.to_owned(),
        repository: repository.to_owned(),
        summary: summary.to_owned(),
        publisher_key_fingerprint: publisher_key_fingerprint.to_owned(),
        releases,
    })
}

fn parse_release(value: &Json) -> Result<Release, IndexError> {
    let fields = object(value)?;
    expect_only(
        fields,
        &["version", "release_sequence", "manifest_url", "manifest_sha256"],
    )?;
    let version = string(required(fields, "version")?)?;
    if !check_semver(version) {
        return Err(IndexError::Schema("version"));
    }
    let release_sequence = positive(required(fields, "release_sequence")?, "release_sequence")?;
    let manifest_url = string(required(fields, "manifest_url")?)?;
    check_manifest_url(manifest_url)?;
    let manifest_sha256 = string(required(fields, "manifest_sha256")?)?;
    if !check_hex64(manifest_sha256) {
        return Err(IndexError::Schema("manifest_sha256"));
    }
    Ok(Release {
        version: version.to_owned(),
        release_sequence,
        manifest_url: manifest_url.to_owned(),
        manifest_sha256: manifest_sha256.to_owned(),
    })
}

fn expect_only(fields: &[(String, Json)], allowed: &[&str]) -> Result<(), IndexError> {
    for (key, _) in fields {
        if matches!(key.as_str(), "alias" | "aliases" | "commands") {
            return Err(IndexError::AliasClaim);
        }
        if matches!(key.as_str(), "secret" | "private_key" | "seed") {
            return Err(IndexError::SecretMaterial);
        }
        if !allowed.iter().any(|name| *name == key) {
            return Err(IndexError::UnsignedMetadata);
        }
    }
    if fields.len() != allowed.len() {
        return Err(IndexError::Schema("missing field"));
    }
    Ok(())
}

fn required<'a>(fields: &'a [(String, Json)], name: &str) -> Result<&'a Json, IndexError> {
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .ok_or(IndexError::Schema("missing field"))
}

fn object(value: &Json) -> Result<&[(String, Json)], IndexError> {
    match value {
        Json::Object(fields) => Ok(fields),
        _ => Err(IndexError::Schema("object")),
    }
}

fn array(value: &Json) -> Result<&[Json], IndexError> {
    match value {
        Json::Array(items) => Ok(items),
        _ => Err(IndexError::Schema("array")),
    }
}

fn string(value: &Json) -> Result<&str, IndexError> {
    match value {
        Json::String(text) => Ok(text),
        _ => Err(IndexError::Schema("string")),
    }
}

fn number(value: &Json) -> Result<u64, IndexError> {
    match value {
        Json::Number(number) => Ok(*number),
        _ => Err(IndexError::Schema("number")),
    }
}

fn positive(value: &Json, field: &'static str) -> Result<u64, IndexError> {
    let number = number(value)?;
    if number < 1 {
        return Err(IndexError::Schema(field));
    }
    Ok(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FINGERPRINT: &str = "21fe31dfa154a261626bf854046fd2271b7bed4b6abe45aa58877ef47f9721b9";
    const DIGEST: &str = "06103b8c8f458e406fc73a174110ea560970c4183ce930dfd642fe13013f4f44";

    fn page(plugins: &str) -> String {
        format!(r#"{{"schema":1,"sequence":1,"plugins":[{plugins}]}}"#)
    }

    fn git(summary: &str) -> String {
        format!(
            r#"{{"id":"dctx/git","repository":"fixture/dctx-plugin-git","summary":"{summary}","publisher_key_fingerprint":"{FINGERPRINT}","releases":[{{"version":"1.0.0","release_sequence":1,"manifest_url":"https://example.invalid/plugin.json","manifest_sha256":"{DIGEST}"}}]}}"#
        )
    }

    #[test]
    fn git_fixture_shape_is_the_positive_control() {
        let index = validate_index(page(&git(
            "Local fixture for dctx-plugin-git. Not a published release.",
        ))
        .as_bytes())
        .unwrap();
        assert_eq!(index.plugins.len(), 1);
        assert_eq!(index.plugins[0].id, "dctx/git");
        assert_eq!(index.plugins[0].repository, "fixture/dctx-plugin-git");
    }

    #[test]
    fn duplicate_alias_unsigned_and_hostile_have_positive_controls() {
        let ok = page(&git("Local fixture for dctx-plugin-git."));
        assert!(validate_index(ok.as_bytes()).is_ok());
        let duplicate = page(&format!("{},{}", git("one"), git("two").replace(
            "fixture/dctx-plugin-git",
            "fixture/dctx-plugin-git-copy",
        )));
        assert_eq!(validate_index(duplicate.as_bytes()), Err(IndexError::DuplicateId));
        let alias = git("Local fixture.").replace(
            r#""summary":"Local fixture.""#,
            r#""summary":"Local fixture.","alias":"git""#,
        );
        assert_eq!(validate_index(page(&alias).as_bytes()), Err(IndexError::AliasClaim));
        let unsigned = ok.replace(
            r#""plugins""#,
            r#""homepage":"https://example.invalid","plugins""#,
        );
        assert_eq!(
            validate_index(unsigned.as_bytes()),
            Err(IndexError::UnsignedMetadata)
        );
        let hostile = page(&git("hide\\u202eme"));
        assert_eq!(validate_index(hostile.as_bytes()), Err(IndexError::HostileDisplay));
    }
}
