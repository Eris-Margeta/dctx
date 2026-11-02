//! Hidden-name, extension and sensitive-name helpers for pack file filters and every map verb.
//! Implements the name rules used by [`super::include`] (leading dot, last extension) and the strict
//! sensitive-file deny of ADR-014 (no override flag in v1).
//! Depends on nothing; pure over a file or directory name.
//! Used by [`super::include`] and, for the sensitive rule, by the pack scan, the map policy, map read and map grep.
//! A leading-dot name such as `.gitignore` has extension `gitignore`.

/// Check if a file/folder name is hidden (starts with '.')
pub(super) fn is_hidden(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

/// Whole final path components that are always sensitive (compared after ASCII lowercasing).
const SENSITIVE_NAMES: &[&str] =
    &[".env", ".envrc", ".netrc", "_netrc", ".npmrc", ".pypirc", ".pgpass", ".htpasswd", ".git-credentials", "credentials"];

/// Extensions (text after the last dot, non-empty stem) of env, key, certificate and keystore files.
const SENSITIVE_EXTENSIONS: &[&str] = &["env", "pem", "key", "p12", "pfx", "jks", "keystore"];

/// Data-file extensions that make `credentials.<ext>` and `secrets.<ext>` sensitive; `credentials.py` stays source.
const DATA_EXTENSIONS: &[&str] = &["json", "yaml", "yml", "toml", "ini", "xml"];

/// SSH private-key base names. The bare name or a `_`/`-` suffix without a dot matches; `id_rsa.pub` does not.
const SSH_KEYS: &[&str] = &["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"];

/// True for sensitive file names that no dctx command ever reads, indexes or prints; the deny has no override.
/// Covers `.env`, `.env.*`, `*.env`, `.envrc`; `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.jks`, `*.keystore`;
/// `id_rsa`, `id_dsa`, `id_ecdsa`, `id_ed25519` (and `_`/`-` suffixed variants); `credentials`, `credentials.<data>`,
/// `*.credentials.json`, `secrets.<data>` (data = json, yaml, yml, toml, ini, xml); `.netrc`, `_netrc`, `.npmrc`,
/// `.pypirc`, `.pgpass`, `.htpasswd` and `.git-credentials`.
/// The match is ASCII case-insensitive and looks at the final path component only.
#[must_use]
pub fn is_secret_shaped_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if SENSITIVE_NAMES.contains(&lower.as_str()) || lower.starts_with(".env.") {
        return true;
    }
    if lower.strip_suffix("credentials.json").is_some_and(|head| head.ends_with('.')) {
        return true;
    }
    if let Some((stem, extension)) = lower.rsplit_once('.') {
        if !stem.is_empty() && SENSITIVE_EXTENSIONS.contains(&extension) {
            return true;
        }
        if (stem == "credentials" || stem == "secrets") && DATA_EXTENSIONS.contains(&extension) {
            return true;
        }
    }
    SSH_KEYS.iter().any(|&key| {
        lower.strip_prefix(key).is_some_and(|rest| rest.is_empty() || (rest.starts_with(['_', '-']) && !rest.contains('.')))
    })
}

/// True when any `/`-separated segment of `path` is exactly `.git`. Such paths are never listed or read,
/// whatever the ignore rules say: `!.git/` in `.gitignore` or `.dctxignore` cannot re-include them.
#[must_use]
pub fn has_git_segment(path: &str) -> bool {
    path.split('/').any(|segment| segment == ".git")
}

/// Extract file extension from filename
/// For files like ".gitignore", returns "gitignore"
/// For files like "file.txt", returns "txt"
/// For files like "archive.tar.gz", returns "gz"
pub(super) fn get_extension(file_name: &str) -> Option<&str> {
    // Find the last dot
    if let Some(pos) = file_name.rfind('.') {
        // If dot is at position 0 (e.g., ".gitignore"), treat everything after as extension
        if pos == 0 && file_name.len() > 1 {
            return Some(&file_name[1..]);
        }
        // Normal case: dot is not at the start or end
        if pos > 0 && pos < file_name.len() - 1 {
            return Some(&file_name[pos + 1..]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_hidden_function() {
        assert!(is_hidden(".gitignore"));
        assert!(is_hidden(".env"));
        assert!(is_hidden(".config"));
        assert!(!is_hidden("file.txt"));
        assert!(!is_hidden(".")); // Current dir marker
        assert!(!is_hidden("..")); // Parent dir marker
    }

    #[test]
    fn secret_shaped_names_cover_env_files() {
        for name in [".env", ".env.local", ".env.production", "settings.env", ".envrc", ".ENV", "prod.ENV"] {
            assert!(is_secret_shaped_name(name), "{name}");
        }
        for name in ["environment.rs", "env.rs", "envrc", "src", ".environment", "app.py", "myenv"] {
            assert!(!is_secret_shaped_name(name), "{name}");
        }
    }

    #[test]
    fn sensitive_names_cover_keys_credentials_and_token_files() {
        for name in [
            "server.pem", "TLS.KEY", "client.p12", "store.pfx", "app.jks", "release.keystore", "id_rsa", "id_ed25519",
            "id_ecdsa", "id_dsa", "id_rsa_deploy", "id_ed25519-old", "credentials", "credentials.json", "Credentials.YAML",
            "gcp.credentials.json", "secrets.yaml", "secrets.yml", "secrets.json", ".npmrc", ".netrc", "_netrc", ".pypirc",
            ".pgpass", ".htpasswd", ".git-credentials",
        ] {
            assert!(is_secret_shaped_name(name), "{name}");
        }
        for name in [
            "credentials.py", "credentials.rs", "secrets.rs", "secret.yaml", "id_rsa.pub", "id_rsa.py", "keys.rs", "pem.rs",
            ".key", "npmrc", "app.yaml", "config.json", "README.md", "key.txt", "my_id_rsa", "credentialsx",
        ] {
            assert!(!is_secret_shaped_name(name), "{name}");
        }
    }

    #[test]
    fn git_segments_are_found_at_any_depth_and_only_exactly() {
        for path in [".git", ".git/config", "nested/.git", "nested/.git/objects/ab"] {
            assert!(has_git_segment(path), "{path}");
        }
        for path in ["", "src/lib.rs", ".github/workflows/ci.yml", ".gitignore", "a.git/config", "git/config", ".gitmodules"] {
            assert!(!has_git_segment(path), "{path}");
        }
    }

    #[test]
    fn test_get_extension_function() {
        assert_eq!(get_extension("file.txt"), Some("txt"));
        assert_eq!(get_extension("archive.tar.gz"), Some("gz"));
        assert_eq!(get_extension("Makefile"), None);
        assert_eq!(get_extension(".gitignore"), Some("gitignore"));
        assert_eq!(get_extension("file."), None);
        assert_eq!(get_extension(".hidden"), Some("hidden"));
    }
}
