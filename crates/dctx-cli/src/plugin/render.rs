// SPDX-License-Identifier: Apache-2.0
//! Text and bounded JSON for plugin commands.
//! Implements PLUGINS.md §5.1 "`--json` equivalents" and escaped display text.
//! Depends on std. Used by the plugin entry and the operation results.

use dctx_plugin_distribution::{escape_display, Enablement, Origin, PluginStatus};
use std::fmt::Write as _;

/// Help for §5 commands. Printing it starts no worker and touches no registry.
pub fn help_text() -> String {
    "\
Usage:
  dctx plugin search [query] [--refresh] [--json]
  dctx plugin install <owner/repo>[@version] [--enable --approve] [--json]
  dctx plugin install --url <https-url> --id <publisher/name> [--json]
  dctx plugin install --file <bundle> --id <publisher/name> [--json]
  dctx plugin list [--json]
  dctx plugin info <id> [--json]
  dctx plugin enable <id> [--project <path>] [--approve] [--json]
  dctx plugin disable <id> [--project <path>] [--json]
  dctx plugin remove <id> [--json]
  dctx plugin update [<id>] [--version <version>] [--check] [--json]
  dctx plugin verify [<id>] [--refresh-trust] [--json]
  dctx plugin run <id> -- <args>
  dctx git <status|protect>

Search and list read cached metadata only. --refresh is the fetch.
Install stays inactive until enable. --enable requires --approve.
dctx git is the dctx/git alias, not a Git passthrough.
"
    .to_owned()
}

/// JSON string literal, including quotes.
pub fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => {
                let _ = write!(out, "\\u{:04x}", other as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// One list row, text or JSON.
pub fn status_line(
    status: &PluginStatus,
    alias: &str,
    consent: &str,
    worker: &str,
    json: bool,
) -> String {
    let version = status
        .version
        .as_ref()
        .map_or("-", dctx_plugin_distribution::Version::as_str);
    if json {
        format!(
            "{{\"id\":{},\"version\":{},\"origin\":{},\"alias\":{},\"enablement\":{},\"consent\":{},\"blocked\":{},\"worker\":{}}}",
            json_string(status.id.as_str()),
            json_string(version),
            json_string(origin_name(status.origin)),
            json_string(alias),
            json_string(enablement_name(status.enablement)),
            json_string(consent),
            if status.blocked { "true" } else { "false" },
            json_string(worker),
        )
    } else {
        format!(
            "{}\t{}\torigin={}\talias={}\t{}\t{}\tworker={}",
            status.id.as_str(),
            version,
            origin_name(status.origin),
            alias,
            enablement_name(status.enablement),
            consent,
            worker
        )
    }
}

/// Escapes index summary text for a terminal and for JSON.
pub fn safe_summary(summary: &str) -> String {
    escape_display(summary)
}

/// Stable origin name for text and JSON.
pub fn origin_name(origin: Origin) -> &'static str {
    match origin {
        Origin::Installed => "installed",
        Origin::Bundled => "bundled",
        Origin::Override => "override",
    }
}

/// Stable enablement name for text and JSON.
pub fn enablement_name(enablement: Enablement) -> &'static str {
    match enablement {
        Enablement::NeverEnabled => "inactive",
        Enablement::Enabled => "enabled",
        Enablement::Disabled => "disabled",
    }
}

/// Consent column. A sticky disable wins over a missing grant.
pub fn consent_name(needs_consent: bool, enablement: Enablement) -> &'static str {
    if enablement == Enablement::Disabled {
        "disabled"
    } else if needs_consent {
        "needs-consent"
    } else if enablement == Enablement::Enabled {
        "granted"
    } else {
        "inactive"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn help_names_every_management_command() {
        let help = help_text();
        for name in [
            "search", "install", "list", "info", "enable", "disable", "remove", "update", "verify",
            "run", "dctx git",
        ] {
            assert!(help.contains(name), "{name}");
        }
    }

    proptest! {
        #[test]
        fn json_strings_have_no_raw_controls(text in "\\PC{0,32}") {
            let encoded = json_string(&text);
            assert!(encoded.starts_with('"') && encoded.ends_with('"'));
            let body = &encoded[1..encoded.len() - 1];
            for character in body.chars() {
                assert!(!character.is_control());
            }
        }
    }
}
