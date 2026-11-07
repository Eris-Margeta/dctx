// SPDX-License-Identifier: Apache-2.0
//! Plugin discovery and installed status queries for PLUGINS.md §5.1.
//! Depends on the signed index, distribution registry and bundled catalog.
//! Used by the operations dispatcher and installation commands.

use dctx_plugin_distribution::domain::index::INDEX_LIMITS;
use dctx_plugin_distribution::{
    DistributionError, Enablement, HttpsUrl, Origin, PluginId, PluginStatus, SafeFileName,
    VerifiedIndex,
};

use super::ops_output::{dist, done, facade_note, fail, Outcome};
use super::{bundled, fetch, render, Env};

pub(super) fn search(env: &mut Env<'_>, query: &str, refresh: bool, json: bool) -> Outcome {
    if refresh {
        if let Err(error) = refresh_index(env) {
            return fail("search", &error, json);
        }
    }
    let index = match cached_index(env) {
        Ok(index) => index,
        Err(_) if !refresh && env.index.bytes.is_none() => {
            return done("search", json, env, |fields| {
                format!("{fields},\"cache_age_secs\":null,\"trust\":\"absent\",\"hits\":[]")
            });
        }
        Err(error) => return fail("search", &error, json),
    };
    let age = env
        .index
        .fetched_at
        .map(|at| env.clock.now().saturating_sub(at));
    let hits: Vec<String> = index.search(query).iter().map(|entry| hit(entry)).collect();
    let age_json = age.map_or_else(|| "null".to_owned(), |age| age.to_string());
    done("search", json, env, |fields| {
        format!(
            "{fields},\"cache_age_secs\":{age_json},\"trust\":\"verified\",\"query\":{},\"hits\":[{}]",
            render::json_string(query),
            hits.join(",")
        )
    })
}

pub(super) fn hit(entry: &dctx_plugin_distribution::IndexEntry) -> String {
    let version = entry
        .releases
        .first()
        .map_or("", |release| release.version.as_str());
    format!(
        "{{\"id\":{},\"repository\":{},\"summary\":{},\"version\":{}}}",
        render::json_string(entry.id.as_str()),
        render::json_string(&entry.repository),
        render::json_string(&render::safe_summary(&entry.summary)),
        render::json_string(version)
    )
}

pub(super) fn refresh_index(env: &mut Env<'_>) -> Result<(), DistributionError> {
    let key = env.index_key.ok_or(DistributionError::UnknownKey)?;
    let url = HttpsUrl::parse(env.index_url)?;
    let limit = INDEX_LIMITS.max_bytes as u64;
    let bytes = fetch::fetch_all(env.network, &env.policy.origin, &url, limit)?;
    let signature_name = SafeFileName::parse("index.json.sig")?;
    let signature = fetch::fetch_all(
        env.network,
        &env.policy.origin,
        &url.sibling(&signature_name),
        limit,
    )?;
    let index = VerifiedIndex::verify(&bytes, &signature, &key)?;
    index.check_newer_than(env.index.sequence)?;
    env.index.sequence = Some(index.sequence);
    env.index.fetched_at = Some(env.clock.now());
    env.index.bytes = Some(bytes);
    env.index.signature = Some(signature);
    Ok(())
}

pub(super) fn cached_index(env: &Env<'_>) -> Result<VerifiedIndex, DistributionError> {
    let key = env.index_key.ok_or(DistributionError::UnknownKey)?;
    let bytes = env
        .index
        .bytes
        .as_deref()
        .ok_or(DistributionError::NotFound)?;
    let signature = env
        .index
        .signature
        .as_deref()
        .ok_or(DistributionError::NotFound)?;
    VerifiedIndex::verify(bytes, signature, &key)
}

pub(super) fn list(env: &Env<'_>, json: bool) -> Outcome {
    let mut rows = match dist(env).list() {
        Ok(rows) => rows,
        Err(error) => return fail("list", &error, json),
    };
    if let Some(row) = synthetic(env) {
        if !rows.iter().any(|existing| existing.id == row.id) {
            rows.push(row);
        }
    }
    let rendered: Vec<String> = rows.iter().map(|row| render_status(row, json)).collect();
    if json {
        done("list", true, env, |fields| {
            format!("{fields},\"plugins\":[{}]", rendered.join(","))
        })
    } else {
        let mut text = rendered.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        let _ = facade_note(env);
        Outcome {
            code: 0,
            body: text,
        }
    }
}

pub(super) fn info(env: &Env<'_>, raw_id: &str, json: bool) -> Outcome {
    let id = match PluginId::parse(raw_id) {
        Ok(id) => id,
        Err(error) => return fail("info", &error, json),
    };
    let status = match dist(env).registry() {
        Ok(registry) => registry.status(&id, env.trust, env.clock.now()).ok(),
        Err(error) => return fail("info", &error, json),
    };
    let status = match status {
        Some(status) => status,
        None => match synthetic(env).filter(|row| row.id == id) {
            Some(status) => status,
            None => return fail("info", &DistributionError::NotInstalled, json),
        },
    };
    let labels = if status.id.as_str() == "dctx/git" {
        bundled::capability_labels()
    } else {
        Vec::new()
    };
    let grants = labels
        .iter()
        .map(|label| render::json_string(label))
        .collect::<Vec<_>>()
        .join(",");
    let hooks = if status.id.as_str() == "dctx/git" {
        bundled::catalog()
            .hooks
            .into_iter()
            .map(|hook| render::json_string(&hook))
            .collect::<Vec<_>>()
            .join(",")
    } else {
        String::new()
    };
    done("info", json, env, |fields| {
        format!("{fields},\"plugin\":{},\"capabilities\":[{grants}],\"hooks\":[{hooks}],\"receipts\":[]", render_status(&status, true))
    })
}

pub(super) fn synthetic(env: &Env<'_>) -> Option<PluginStatus> {
    let version = bundled::installed_version().ok()?;
    Some(PluginStatus {
        id: bundled::id().ok()?,
        origin: Origin::Bundled,
        version: Some(version.version),
        enablement: if bundled::worker_linked() {
            Enablement::Enabled
        } else {
            Enablement::Disabled
        },
        needs_consent: bundled::worker_linked(),
        blocked: !bundled::worker_linked(),
        revocation_age_secs: env.trust.revocation_age(env.clock.now()),
    })
}

pub(super) fn render_status(status: &PluginStatus, json: bool) -> String {
    let alias = if status.id.as_str() == "dctx/git" {
        bundled::catalog().alias
    } else {
        String::new()
    };
    if status.id.as_str() == "dctx/git" && !bundled::worker_linked() {
        return render::status_line(status, &alias, "unavailable", "excluded", json);
    }
    let worker = if status.origin == Origin::Bundled && bundled::worker_linked() {
        "same-binary"
    } else {
        "host-target"
    };
    render::status_line(
        status,
        &alias,
        render::consent_name(status.needs_consent, status.enablement),
        worker,
        json,
    )
}
