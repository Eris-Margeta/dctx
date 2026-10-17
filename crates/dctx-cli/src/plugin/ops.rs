//! §5.1 operations over the distribution facade and the dctx facade catalog.
//! Implements search, install, list, info, enable, disable, remove, update, verify and run.
//! Depends on `dctx_plugin_distribution::Distribution` and the bundled git catalog.
//! Used by the plugin entry. List and help never call this module's mutating functions.

use std::fs;
use std::path::Path;

use dctx::Capabilities;
use dctx_plugin_distribution::domain::index::INDEX_LIMITS;
use dctx_plugin_distribution::{
    ConsentApproval, ConsentScope, Confinement, Distribution, DistributionError, Enablement, HttpsUrl, Origin,
    PluginId, PluginStatus, SafeFileName, VerifiedIndex, Version,
};

use super::bundled;
use super::fetch;
use super::parse::{Action, Install, Request};
use super::render;
use super::Env;

/// Rendered command result. `code` is the process status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Zero is success. One is a typed refusal. Two is usage.
    pub code: u8,
    /// Text or one JSON object.
    pub body: String,
}

/// Runs `request` against `env`.
pub fn execute(request: &Request, env: &mut Env<'_>) -> Outcome {
    match &request.action {
        Action::Help => ok("help", &render::help_text(), request.json, env),
        Action::Search { query, refresh } => search(env, query, *refresh, request.json),
        Action::Install(install) => install_cmd(env, install, request.json),
        Action::List => list(env, request.json),
        Action::Info { id } => info(env, id, request.json),
        Action::Enable { id, project, approve } => enable(env, id, project.as_deref(), *approve, request.json),
        Action::Disable { id, project } => disable(env, id, project.as_deref(), request.json),
        Action::Remove { id } => remove(env, id, request.json),
        Action::Update { id, version, check } => update(env, id.as_deref(), version.as_deref(), *check, request.json),
        Action::Verify { id, refresh_trust } => verify(env, id.as_deref(), *refresh_trust, request.json),
        Action::Run { id, args } => route(env, id, args, "run", request.json),
        Action::Git { args } => {
            let catalog = bundled::catalog();
            if catalog.alias != "git" {
                return usage("git", "the facade catalog has no git alias", request.json);
            }
            route(env, &catalog.id, args, "git", request.json)
        }
    }
}

fn dist<'a>(env: &'a Env<'_>) -> Distribution<'a> {
    Distribution::new(env.network, env.store, env.clock, env.trust, env.policy)
}

fn search(env: &mut Env<'_>, query: &str, refresh: bool, json: bool) -> Outcome {
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
    let age = env.index.fetched_at.map(|at| env.clock.now().saturating_sub(at));
    let hits: Vec<String> = index.search(query).iter().map(|entry| hit(entry)).collect();
    let age_json = age.map(|age| age.to_string()).unwrap_or_else(|| "null".to_owned());
    done("search", json, env, |fields| {
        format!(
            "{fields},\"cache_age_secs\":{age_json},\"trust\":\"verified\",\"query\":{},\"hits\":[{}]",
            render::json_string(query),
            hits.join(",")
        )
    })
}

fn hit(entry: &dctx_plugin_distribution::IndexEntry) -> String {
    let version = entry.releases.first().map(|release| release.version.as_str()).unwrap_or("");
    format!(
        "{{\"id\":{},\"repository\":{},\"summary\":{},\"version\":{}}}",
        render::json_string(entry.id.as_str()),
        render::json_string(&entry.repository),
        render::json_string(&render::safe_summary(&entry.summary)),
        render::json_string(version)
    )
}

fn refresh_index(env: &mut Env<'_>) -> Result<(), DistributionError> {
    let key = env.index_key.ok_or(DistributionError::UnknownKey)?;
    let url = HttpsUrl::parse(env.index_url)?;
    let limit = INDEX_LIMITS.max_bytes as u64;
    let bytes = fetch::fetch_all(env.network, &env.policy.origin, &url, limit)?;
    let signature_name = SafeFileName::parse("index.json.sig")?;
    let signature = fetch::fetch_all(env.network, &env.policy.origin, &url.sibling(&signature_name), limit)?;
    let index = VerifiedIndex::verify(&bytes, &signature, &key)?;
    index.check_newer_than(env.index.sequence)?;
    env.index.sequence = Some(index.sequence);
    env.index.fetched_at = Some(env.clock.now());
    env.index.bytes = Some(bytes);
    env.index.signature = Some(signature);
    Ok(())
}

fn cached_index(env: &Env<'_>) -> Result<VerifiedIndex, DistributionError> {
    let key = env.index_key.ok_or(DistributionError::UnknownKey)?;
    let bytes = env.index.bytes.as_deref().ok_or(DistributionError::NotFound)?;
    let signature = env.index.signature.as_deref().ok_or(DistributionError::NotFound)?;
    VerifiedIndex::verify(bytes, signature, &key)
}

fn install_cmd(env: &mut Env<'_>, install: &Install, json: bool) -> Outcome {
    let report = match install {
        Install::Index { repository, version, enable, approve } => {
            let index = match cached_index(env) {
                Ok(index) => index,
                Err(error) => return fail("install", &error, json),
            };
            let entry = match index.entries.iter().find(|entry| entry.repository.eq_ignore_ascii_case(repository)) {
                Some(entry) => entry,
                None => return fail("install", &DistributionError::NotFound, json),
            };
            let parsed = match version.as_deref().map(Version::parse).transpose() {
                Ok(version) => version,
                Err(error) => return fail("install", &error, json),
            };
            let id = entry.id.clone();
            match dist(env).install_from_index(&index, &id, parsed.as_ref()) {
                Ok(report) => (report, *enable, *approve, id),
                Err(error) => return fail("install", &error, json),
            }
        }
        Install::Url { url, id, enable, approve } => {
            let parsed_url = match HttpsUrl::parse(url) {
                Ok(url) => url,
                Err(error) => return fail("install", &DistributionError::from(error), json),
            };
            let parsed_id = match PluginId::parse(id) {
                Ok(id) => id,
                Err(error) => return fail("install", &error, json),
            };
            match dist(env).install_url(&parsed_id, &parsed_url) {
                Ok(report) => (report, *enable, *approve, parsed_id),
                Err(error) => return fail("install", &error, json),
            }
        }
        Install::File { path, id, enable, approve } => {
            let bytes = match fs::read(Path::new(path)) {
                Ok(bytes) => bytes,
                Err(_) => return fail("install", &DistributionError::NotFound, json),
            };
            let parsed_id = match PluginId::parse(id) {
                Ok(id) => id,
                Err(error) => return fail("install", &error, json),
            };
            match dist(env).install_bundle(&parsed_id, &bytes) {
                Ok(report) => (report, *enable, *approve, parsed_id),
                Err(error) => return fail("install", &error, json),
            }
        }
    };
    let (report, enable, approve, id) = report;
    if enable {
        if let Err(error) = activate_and_enable(env, &id, approve, None) {
            return fail("install", &error, json);
        }
    }
    done("install", json, env, |fields| {
        format!(
            "{fields},\"id\":{},\"version\":{},\"origin\":{},\"activated\":{},\"capabilities\":{}",
            render::json_string(report.key.id.as_str()),
            render::json_string(report.key.version.as_str()),
            render::json_string(render::origin_name(report.origin)),
            if enable { "true" } else { "false" },
            report.capabilities.len()
        )
    })
}

fn list(env: &Env<'_>, json: bool) -> Outcome {
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
        done("list", true, env, |fields| format!("{fields},\"plugins\":[{}]", rendered.join(",")))
    } else {
        let mut text = rendered.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        let _ = facade_note(env);
        Outcome { code: 0, body: text }
    }
}

fn info(env: &Env<'_>, raw_id: &str, json: bool) -> Outcome {
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
    let labels = if status.id.as_str() == "dctx/git" { bundled::capability_labels() } else { Vec::new() };
    let grants = labels.iter().map(|label| render::json_string(label)).collect::<Vec<_>>().join(",");
    let hooks = if status.id.as_str() == "dctx/git" {
        bundled::catalog().hooks.into_iter().map(|hook| render::json_string(&hook)).collect::<Vec<_>>().join(",")
    } else {
        String::new()
    };
    done("info", json, env, |fields| {
        format!("{fields},\"plugin\":{},\"capabilities\":[{grants}],\"hooks\":[{hooks}],\"receipts\":[]", render_status(&status, true))
    })
}

fn enable(env: &mut Env<'_>, raw_id: &str, project: Option<&str>, approve: bool, json: bool) -> Outcome {
    let id = match PluginId::parse(raw_id) {
        Ok(id) => id,
        Err(error) => return fail("enable", &error, json),
    };
    let present = match dist(env).registry() {
        Ok(registry) => registry.get(&id).is_some(),
        Err(error) => return fail("enable", &error, json),
    };
    if !present && !approve {
        return fail("enable", &DistributionError::ConsentRequired, json);
    }
    if let Err(error) = prepare_bundled(env, &id) {
        return fail("enable", &error, json);
    }
    if let Err(error) = activate_and_enable(env, &id, approve, project) {
        return fail("enable", &error, json);
    }
    done("enable", json, env, |fields| {
        format!("{fields},\"id\":{},\"enablement\":\"enabled\"", render::json_string(id.as_str()))
    })
}

fn disable(env: &mut Env<'_>, raw_id: &str, project: Option<&str>, json: bool) -> Outcome {
    let id = match PluginId::parse(raw_id) {
        Ok(id) => id,
        Err(error) => return fail("disable", &error, json),
    };
    if let Err(error) = prepare_bundled(env, &id) {
        return fail("disable", &error, json);
    }
    let leases = match dist(env).disable(&id) {
        Ok(leases) => leases,
        Err(error) => return fail("disable", &error, json),
    };
    let project = project.unwrap_or("");
    done("disable", json, env, |fields| {
        format!(
            "{fields},\"id\":{},\"enablement\":\"disabled\",\"leases\":{leases},\"project\":{}",
            render::json_string(id.as_str()),
            render::json_string(project)
        )
    })
}

fn remove(env: &mut Env<'_>, raw_id: &str, json: bool) -> Outcome {
    let id = match PluginId::parse(raw_id) {
        Ok(id) => id,
        Err(error) => return fail("remove", &error, json),
    };
    if let Err(error) = prepare_bundled(env, &id) {
        return fail("remove", &error, json);
    }
    if let Err(error) = dist(env).remove(&id) {
        return fail("remove", &error, json);
    }
    done("remove", json, env, |fields| format!("{fields},\"id\":{},\"masked\":true", render::json_string(id.as_str())))
}

fn update(env: &mut Env<'_>, raw_id: Option<&str>, version: Option<&str>, check: bool, json: bool) -> Outcome {
    let text = raw_id.unwrap_or("dctx/git");
    let id = match PluginId::parse(text) {
        Ok(id) => id,
        Err(error) => return fail("update", &error, json),
    };
    if is_bundled_selection(env, &id) {
        return done("update", json, env, |fields| {
            format!(
                "{fields},\"id\":{},\"check\":{},\"path\":\"containing-release\",\"downloaded\":false",
                render::json_string(id.as_str()),
                if check { "true" } else { "false" }
            )
        });
    }
    let index = match cached_index(env) {
        Ok(index) => index,
        Err(error) => return fail("update", &error, json),
    };
    let parsed = match version.map(Version::parse).transpose() {
        Ok(version) => version,
        Err(error) => return fail("update", &error, json),
    };
    let entry = match index.entry(&id, env.trust) {
        Ok(entry) => entry.clone(),
        Err(error) => return fail("update", &error, json),
    };
    let release = match entry.select(parsed.as_ref()) {
        Ok(release) => release.clone(),
        Err(error) => return fail("update", &error, json),
    };
    if check {
        return done("update", json, env, |fields| {
            format!(
                "{fields},\"id\":{},\"check\":true,\"version\":{},\"downloaded\":false",
                render::json_string(id.as_str()),
                render::json_string(release.version.as_str())
            )
        });
    }
    let report = match dist(env).install_from_index(&index, &id, Some(&release.version)) {
        Ok(report) => report,
        Err(error) => return fail("update", &error, json),
    };
    let activated = match dist(env).activate(&id, &report.key.digest) {
        Ok(()) => true,
        Err(DistributionError::ConsentRequired) => false,
        Err(error) => return fail("update", &error, json),
    };
    done("update", json, env, |fields| {
        format!(
            "{fields},\"id\":{},\"version\":{},\"activated\":{}",
            render::json_string(id.as_str()),
            render::json_string(release.version.as_str()),
            if activated { "true" } else { "false" }
        )
    })
}

fn verify(env: &mut Env<'_>, raw_id: Option<&str>, refresh_trust: bool, json: bool) -> Outcome {
    let text = raw_id.unwrap_or("dctx/git");
    let id = match PluginId::parse(text) {
        Ok(id) => id,
        Err(error) => return fail("verify", &error, json),
    };
    if refresh_trust {
        if let Err(error) = env.trust.check_freshness(env.clock.now(), env.policy.freshness_ceiling_secs) {
            return fail("verify", &error, json);
        }
    }
    if is_bundled_selection(env, &id) {
        let worker = if bundled::worker_linked() { "same-binary" } else { "excluded" };
        return done("verify", json, env, |fields| {
            format!(
                "{fields},\"id\":{},\"origin\":\"bundled\",\"worker\":{},\"executed\":false",
                render::json_string(id.as_str()),
                render::json_string(worker)
            )
        });
    }
    let outcomes = match dist(env).verify(&id) {
        Ok(outcomes) => outcomes,
        Err(error) => return fail("verify", &error, json),
    };
    let failed = outcomes.iter().any(|outcome| outcome.result.is_err());
    let rows: Vec<String> = outcomes
        .iter()
        .map(|outcome| {
            let result = match &outcome.result {
                Ok(()) => "\"ok\"".to_owned(),
                Err(error) => render::json_string(code_name(error)),
            };
            format!("{{\"digest\":{},\"result\":{result}}}", render::json_string(&outcome.digest.to_string()))
        })
        .collect();
    let mut body = done("verify", json, env, |fields| {
        format!("{fields},\"id\":{},\"outcomes\":[{}]", render::json_string(id.as_str()), rows.join(","))
    });
    if failed {
        body.code = 1;
    }
    body
}

fn route(env: &mut Env<'_>, raw_id: &str, args: &[String], command: &str, json: bool) -> Outcome {
    if raw_id == "dctx/git" && !bundled::worker_linked() {
        let error = super::worker::WorkerError::StrictBuild;
        return Outcome {
            code: 1,
            body: if json {
                format!(
                    "{{\"ok\":false,\"command\":{},\"error\":\"StrictBuild\",\"detail\":{},\"spawned\":false}}",
                    render::json_string(command), render::json_string(&error.to_string())
                )
            } else {
                format!("{command}: {error}")
            },
        };
    }
    let verb = match args.first() {
        Some(verb) => verb.as_str(),
        None => return usage(command, "a verb is required", json),
    };
    if raw_id == "dctx/git" && !bundled::VERBS.contains(&verb) {
        return usage(command, "unknown-verb", json);
    }
    let id = match PluginId::parse(raw_id) {
        Ok(id) => id,
        Err(error) => return fail(command, &error, json),
    };
    let known = match dist(env).registry() {
        Ok(registry) => registry.get(&id).is_some(),
        Err(error) => return fail(command, &error, json),
    };
    if !known && id.as_str() == "dctx/git" && bundled::worker_linked() {
        return fail(command, &DistributionError::ConsentRequired, json);
    }
    let admission = match dist(env).admit(&id) {
        Ok(admission) => admission,
        Err(error) => return fail(command, &error, json),
    };
    if let Err(error) = dist(env).release(&admission) {
        return fail(command, &error, json);
    }
    let rest = args.iter().skip(1).map(|arg| render::json_string(arg)).collect::<Vec<_>>().join(",");
    let spawns = env.host.spawns();
    let git_children = env.host.git_children();
    done(command, json, env, |fields| {
        format!(
            "{fields},\"id\":{},\"verb\":{},\"args\":[{rest}],\"worker\":\"same-binary\",\"spawned\":false,\"facade_spawns\":{spawns},\"facade_git_children\":{git_children},\"digest\":{}",
            render::json_string(id.as_str()),
            render::json_string(verb),
            render::json_string(&admission.digest.to_string())
        )
    })
}

fn activate_and_enable(env: &Env<'_>, id: &PluginId, approve: bool, project: Option<&str>) -> Result<(), DistributionError> {
    let registry = dist(env).registry()?;
    let record = registry.get(id).ok_or(DistributionError::NotInstalled)?;
    if record.origin != Origin::Bundled {
        return Err(DistributionError::ConfinementRequired);
    }
    let version = record.active_version().ok_or(DistributionError::NotActive)?;
    let covered = record.consent.as_ref().is_some_and(|consent| consent.covers(&version.signer, &version.capabilities));
    if !covered && !approve {
        return Err(DistributionError::ConsentRequired);
    }
    let approval = approve.then(|| ConsentApproval {
        digest: version.digest,
        capabilities: version.capabilities.clone(),
        scope: match project {
            Some(path) => ConsentScope::Project(path.to_owned()),
            None => ConsentScope::User,
        },
    });
    dist(env).enable(id, approval.as_ref(), Confinement::Unavailable)
}

fn prepare_bundled(env: &Env<'_>, id: &PluginId) -> Result<(), DistributionError> {
    if id.as_str() != bundled::catalog().id || !bundled::worker_linked() {
        return Ok(());
    }
    if dist(env).registry()?.get(id).is_some() {
        return Ok(());
    }
    dist(env).register_bundled(id, bundled::installed_version()?)
}

fn is_bundled_selection(env: &Env<'_>, id: &PluginId) -> bool {
    if id.as_str() != "dctx/git" || !bundled::worker_linked() {
        return false;
    }
    match dist(env).registry() {
        Ok(registry) => registry.get(id).map(|record| record.origin == Origin::Bundled).unwrap_or(true),
        Err(_) => false,
    }
}

fn synthetic(env: &Env<'_>) -> Option<PluginStatus> {
    let version = bundled::installed_version().ok()?;
    Some(PluginStatus {
        id: bundled::id().ok()?,
        origin: Origin::Bundled,
        version: Some(version.version),
        enablement: if bundled::worker_linked() { Enablement::Enabled } else { Enablement::Disabled },
        needs_consent: bundled::worker_linked(),
        blocked: !bundled::worker_linked(),
        revocation_age_secs: env.trust.revocation_age(env.clock.now()),
    })
}

fn render_status(status: &PluginStatus, json: bool) -> String {
    let alias = if status.id.as_str() == "dctx/git" { bundled::catalog().alias } else { String::new() };
    if status.id.as_str() == "dctx/git" && !bundled::worker_linked() {
        return render::status_line(status, &alias, "unavailable", "excluded", json);
    }
    let worker = if status.origin == Origin::Bundled && bundled::worker_linked() { "same-binary" } else { "host-target" };
    render::status_line(status, &alias, render::consent_name(status.needs_consent, status.enablement), worker, json)
}

fn facade_note(env: &Env<'_>) -> String {
    let caps = Capabilities::current();
    format!("facade_git={} spawns={} git_children={}", caps.git, env.host.spawns(), env.host.git_children())
}

fn done(command: &str, json: bool, env: &Env<'_>, fill: impl FnOnce(String) -> String) -> Outcome {
    let note = facade_note(env);
    if json {
        let fields = format!(
            "\"ok\":true,\"command\":{},\"facade_git\":{},\"facade_spawns\":{},\"facade_git_children\":{}",
            render::json_string(command),
            if Capabilities::current().git { "true" } else { "false" },
            env.host.spawns(),
            env.host.git_children()
        );
        Outcome { code: 0, body: format!("{{{}}}", fill(fields)) }
    } else {
        let extra = fill(String::new());
        Outcome { code: 0, body: format!("{command} {note} {extra}") }
    }
}

fn ok(command: &str, text: &str, json: bool, env: &Env<'_>) -> Outcome {
    if json {
        done(command, true, env, |fields| format!("{fields},\"text\":{}", render::json_string(text)))
    } else {
        let _ = facade_note(env);
        Outcome { code: 0, body: text.to_owned() }
    }
}

fn fail(command: &str, error: &DistributionError, json: bool) -> Outcome {
    let name = code_name(error);
    if json {
        Outcome {
            code: 1,
            body: format!(
                "{{\"ok\":false,\"command\":{},\"error\":{},\"detail\":{}}}",
                render::json_string(command),
                render::json_string(name),
                render::json_string(&error.to_string())
            ),
        }
    } else {
        Outcome { code: 1, body: format!("{command}: {error}") }
    }
}

fn usage(command: &str, reason: &str, json: bool) -> Outcome {
    if json {
        Outcome {
            code: 2,
            body: format!(
                "{{\"ok\":false,\"command\":{},\"error\":\"Usage\",\"detail\":{}}}",
                render::json_string(command),
                render::json_string(reason)
            ),
        }
    } else {
        Outcome { code: 2, body: format!("{command}: {reason}") }
    }
}

fn code_name(error: &DistributionError) -> &'static str {
    match error {
        DistributionError::Malformed { .. } => "Malformed",
        DistributionError::Signature(_) => "Signature",
        DistributionError::UnknownKey => "UnknownKey",
        DistributionError::FingerprintMismatch => "FingerprintMismatch",
        DistributionError::KeyRevoked => "KeyRevoked",
        DistributionError::UnknownPublisher => "UnknownPublisher",
        DistributionError::SignerChanged => "SignerChanged",
        DistributionError::IdentityMismatch => "IdentityMismatch",
        DistributionError::ArtifactRevoked => "ArtifactRevoked",
        DistributionError::Downgrade { .. } => "Downgrade",
        DistributionError::Replay { .. } => "Replay",
        DistributionError::UnsupportedTarget => "UnsupportedTarget",
        DistributionError::TargetMismatch => "TargetMismatch",
        DistributionError::SizeMismatch { .. } => "SizeMismatch",
        DistributionError::IntegrityFailure => "IntegrityFailure",
        DistributionError::SumsMismatch => "SumsMismatch",
        DistributionError::ManifestDigestMismatch => "ManifestDigestMismatch",
        DistributionError::LimitExceeded { .. } => "LimitExceeded",
        DistributionError::Origin(_) => "Origin",
        DistributionError::Network(_) => "Network",
        DistributionError::Busy => "Busy",
        DistributionError::Interrupted => "Interrupted",
        DistributionError::NotInstalled => "NotInstalled",
        DistributionError::NotActive => "NotActive",
        DistributionError::Quarantined => "Quarantined",
        DistributionError::Disabled => "Disabled",
        DistributionError::ConsentRequired => "ConsentRequired",
        DistributionError::ConfinementRequired => "ConfinementRequired",
        DistributionError::StaleRevocation { .. } => "StaleRevocation",
        DistributionError::Leased => "Leased",
        DistributionError::NotFound => "NotFound",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::parse::parse_argv;
    use crate::plugin::IndexCache;
    use dctx::PluginHost;
    use dctx_plugin_distribution::domain::signature::{INDEX_DOMAIN, MANIFEST_DOMAIN};
    use dctx_plugin_distribution::fakes::{FakeNetwork, FixedClock, MemoryStore};
    use dctx_plugin_distribution::{
        fingerprint, InstallPolicy, KeyId, OfflineBundle, PluginId, Sha256Hex, Store, Target, TrustStore,
    };
    use ed25519_dalek::{Signer, SigningKey};

    const NOW: u64 = 1_000_000;
    const BASE: &str = "https://releases.example.org/dctx-git";
    const INDEX: &str = "https://releases.example.org/index.json";

    struct World {
        net: FakeNetwork,
        store: MemoryStore,
        clock: FixedClock,
        trust: TrustStore,
        policy: InstallPolicy,
        key: SigningKey,
        index_key: [u8; 32],
    }

    impl World {
        fn new() -> Self {
            let key = SigningKey::from_bytes(&[7; 32]);
            let index_key = key.verifying_key().to_bytes();
            let mut trust = TrustStore::new();
            let signer = trust.pin_official(KeyId::parse("official-1").unwrap(), index_key).unwrap();
            trust.pin_publisher(PluginId::parse("dctx/git").unwrap(), signer);
            trust.record_revocations([], [], NOW);
            Self {
                net: FakeNetwork::new(),
                store: MemoryStore::new(),
                clock: FixedClock::at(NOW),
                trust,
                policy: InstallPolicy::new(Target::AppleArm64),
                key,
                index_key,
            }
        }

        fn sign(&self, domain: &[u8], document: &[u8]) -> Vec<u8> {
            let mut message = domain.to_vec();
            message.extend_from_slice(document);
            self.key.sign(&message).to_bytes().to_vec()
        }

        fn release(&self, version: &str, sequence: u64) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
            let executable = b"#!/bin/sh\necho fixture\n".to_vec();
            let digest = Sha256Hex::of(&executable).unwrap();
            let manifest = format!(
                concat!(
                    "{{\"manifest_version\":1,\"id\":\"dctx/git\",\"name\":\"dctx-plugin-git\",\"version\":\"{version}\",",
                    "\"release_sequence\":{sequence},\"publisher_key\":\"official-1\",\"license\":\"Apache-2.0\",",
                    "\"protocol\":[{{\"major\":1,\"min_minor\":0,\"max_minor\":0}}],\"host_api\":{{\"major\":1,\"min_minor\":0}},",
                    "\"hooks\":[],\"commands\":[],\"capabilities\":{caps},\"config\":{{}},\"limits\":{{}},",
                    "\"reason_codes\":[],\"diagnostic_codes\":[],\"artifacts\":[{{\"target\":\"aarch64-apple-darwin\",",
                    "\"file\":\"dctx-plugin-git-{version}\",\"bytes\":{len},\"sha256\":\"{digest}\",",
                    "\"format\":\"native-executable\",\"min_os\":\"14.0\",\"runtime_resources\":[]}}]}}"
                ),
                version = version,
                sequence = sequence,
                caps = bundled::CAPABILITIES_JSON,
                len = executable.len(),
                digest = digest,
            )
            .into_bytes();
            let signature = self.sign(MANIFEST_DOMAIN, &manifest);
            (manifest, signature, executable)
        }

        fn publish(&self, version: &str, sequence: u64) {
            let (manifest, signature, executable) = self.release(version, sequence);
            let dir = format!("{BASE}/{version}");
            self.net.serve(&format!("{dir}/plugin.json"), &manifest);
            self.net.serve(&format!("{dir}/plugin.json.sig"), &signature);
            self.net.serve(&format!("{dir}/dctx-plugin-git-{version}"), &executable);
        }

        fn publish_index(&self, summary: &str, version: &str, sequence: u64) {
            let (manifest, _, _) = self.release(version, sequence);
            let entry = format!(
                "{{\"id\":\"dctx/git\",\"repository\":\"fixture/dctx-plugin-git\",\"summary\":{summary},\"publisher_key_fingerprint\":\"{fp}\",\"releases\":[{{\"version\":\"{version}\",\"release_sequence\":{sequence},\"manifest_url\":\"{BASE}/{version}/plugin.json\",\"manifest_sha256\":\"{sha}\"}}]}}",
                summary = render::json_string(summary),
                fp = fingerprint(&self.index_key),
                sha = Sha256Hex::of(&manifest).unwrap(),
            );
            let bytes = format!("{{\"schema\":1,\"sequence\":{sequence},\"plugins\":[{entry}]}}").into_bytes();
            let signature = self.sign(INDEX_DOMAIN, &bytes);
            self.net.serve(INDEX, &bytes);
            self.net.serve("https://releases.example.org/index.json.sig", &signature);
        }

        fn bundle(&self, version: &str, sequence: u64) -> Vec<u8> {
            let (manifest, signature, executable) = self.release(version, sequence);
            let file = format!("dctx-plugin-git-{version}");
            OfflineBundle::encode(&[
                ("plugin.json", manifest.as_slice()),
                ("plugin.json.sig", signature.as_slice()),
                (file.as_str(), executable.as_slice()),
            ])
        }
    }

    fn run(world: &World, index: &mut IndexCache, args: &[&str]) -> Outcome {
        let host = PluginHost::fake();
        let mut env = Env {
            network: &world.net,
            store: &world.store,
            clock: &world.clock,
            trust: &world.trust,
            policy: &world.policy,
            index,
            index_url: INDEX,
            index_key: Some(world.index_key),
            host: &host,
        };
        let owned: Vec<String> = std::iter::once("dctx").chain(args.iter().copied()).map(str::to_owned).collect();
        let request = parse_argv(&owned).unwrap();
        let outcome = execute(&request, &mut env);
        assert_eq!(host.spawns(), 0);
        assert_eq!(host.git_children(), 0);
        outcome
    }

    #[cfg(feature = "git-worker")]
    #[test]
    fn list_and_help_have_no_effects_and_show_the_bundled_plugin() {
        let world = World::new();
        let before = world.store.load_registry().unwrap();
        let mut index = IndexCache::default();
        let listed = run(&world, &mut index, &["plugin", "list"]);
        assert_eq!(listed.code, 0);
        assert!(listed.body.contains("dctx/git"));
        assert!(listed.body.contains("enabled"));
        assert!(listed.body.contains("needs-consent"));
        assert!(listed.body.contains("same-binary"));
        let json = run(&world, &mut index, &["plugin", "list", "--json"]);
        assert!(json.body.contains("\"facade_git\":false"));
        assert!(json.body.contains("\"origin\":\"bundled\""));
        assert!(json.body.contains("needs-consent"));
        let info = run(&world, &mut index, &["plugin", "info", "dctx/git", "--json"]);
        assert_eq!(info.code, 0);
        for label in ["file.read:.gitignore", "file.write:.gitignore", "program:git.add-ignore", "resource:git-admin:stage-ignore"] {
            assert!(info.body.contains(label), "{label} in {}", info.body);
        }
        let help = run(&world, &mut index, &["plugin", "help"]);
        assert_eq!(help.code, 0);
        assert!(help.body.contains("plugin search"));
        assert!(world.net.requests().is_empty());
        assert_eq!(world.store.load_registry().unwrap(), before);
        let search = run(&world, &mut index, &["plugin", "search", "git", "--json"]);
        assert_eq!(search.code, 0);
        assert!(search.body.contains("\"trust\":\"absent\""));
        assert!(world.net.requests().is_empty());
    }

    #[cfg(not(feature = "git-worker"))]
    #[test]
    fn strict_list_reports_unavailable_and_help_still_works_without_effects() {
        let world = World::new();
        let before = world.store.load_registry().unwrap();
        let mut index = IndexCache::default();
        for args in [vec!["plugin", "list"], vec!["plugin", "list", "--json"]] {
            let listed = run(&world, &mut index, &args);
            assert_eq!(listed.code, 0, "{}", listed.body);
            for label in ["dctx/git", "bundled", "unavailable", "excluded"] {
                assert!(listed.body.contains(label), "{}", listed.body);
            }
            assert!(!listed.body.contains("same-binary"));
            assert!(!listed.body.contains("needs-consent"));
        }
        let info = run(&world, &mut index, &["plugin", "info", "dctx/git", "--json"]);
        assert_eq!(info.code, 0, "{}", info.body);
        assert!(info.body.contains("unavailable"));
        for args in [vec!["plugin", "help"], vec!["plugin", "help", "--json"]] {
            let help = run(&world, &mut index, &args);
            assert_eq!(help.code, 0, "{}", help.body);
            assert!(help.body.contains("plugin search"));
        }
        assert!(world.net.requests().is_empty());
        assert_eq!(world.store.load_registry().unwrap(), before);
    }

    #[test]
    fn search_refresh_install_enable_and_refusals() {
        let world = World::new();
        world.publish("1.0.0", 1);
        world.publish_index("Protect exports", "1.0.0", 1);
        let mut index = IndexCache::default();
        let missing = run(&world, &mut index, &["plugin", "search", "--refresh", "--json"]);
        assert_eq!(missing.code, 0, "{}", missing.body);
        assert!(missing.body.contains("Protect exports"));
        assert!(!world.net.requests().is_empty());
        let requests = world.net.requests().len();
        let cached = run(&world, &mut index, &["plugin", "search", "git", "--json"]);
        assert_eq!(cached.code, 0);
        assert_eq!(world.net.requests().len(), requests, "search without refresh does not fetch");

        let refused = run(&world, &mut index, &["plugin", "install", "missing/repo", "--json"]);
        assert_eq!(refused.code, 1);
        assert!(refused.body.contains("NotFound"));
        let bad_url = run(
            &world,
            &mut index,
            &["plugin", "install", "--url", "http://releases.example.org/plugin.json", "--id", "dctx/git", "--json"],
        );
        assert_eq!(bad_url.code, 1);
        assert!(bad_url.body.contains("Origin"));

        let installed = run(&world, &mut index, &["plugin", "install", "fixture/dctx-plugin-git@1.0.0", "--json"]);
        assert_eq!(installed.code, 0, "{}", installed.body);
        assert!(installed.body.contains("\"activated\":false"));
        let listed = run(&world, &mut index, &["plugin", "list", "--json"]);
        assert!(listed.body.contains("inactive") || listed.body.contains("override") || listed.body.contains("installed"));

        let blocked = run(&world, &mut index, &["plugin", "enable", "dctx/git", "--approve", "--json"]);
        assert_eq!(blocked.code, 1, "{}", blocked.body);
        assert!(
            blocked.body.contains("ConfinementRequired") || blocked.body.contains("NotActive") || blocked.body.contains("ConsentRequired"),
            "{}",
            blocked.body
        );
    }

    #[cfg(feature = "git-worker")]
    #[test]
    fn bundled_enable_routes_git_and_canonical_run_then_disable_sticks() {
        let world = World::new();
        let mut index = IndexCache::default();
        let refused = run(&world, &mut index, &["plugin", "enable", "dctx/git", "--json"]);
        assert_eq!(refused.code, 1);
        assert!(refused.body.contains("ConsentRequired"));
        assert!(world.store.load_registry().unwrap().is_none());
        let enabled = run(&world, &mut index, &["plugin", "enable", "dctx/git", "--approve", "--project", "/work", "--json"]);
        assert_eq!(enabled.code, 0, "{}", enabled.body);
        let git = run(&world, &mut index, &["git", "status", "--json"]);
        let run_cmd = run(&world, &mut index, &["plugin", "run", "dctx/git", "--", "status"]);
        assert_eq!(git.code, 0, "{}", git.body);
        assert_eq!(run_cmd.code, 0, "{}", run_cmd.body);
        assert!(git.body.contains("\"verb\":\"status\""));
        assert!(run_cmd.body.contains("\"verb\":\"status\""));
        assert!(git.body.contains("\"spawned\":false"));
        assert!(git.body.contains("same-binary"));
        let unknown = run(&world, &mut index, &["git", "add", "--json"]);
        assert_eq!(unknown.code, 2);
        assert!(unknown.body.contains("unknown-verb"));
        let updated = run(&world, &mut index, &["plugin", "update", "dctx/git", "--check", "--json"]);
        assert_eq!(updated.code, 0, "{}", updated.body);
        assert!(updated.body.contains("containing-release"));
        assert!(updated.body.contains("\"downloaded\":false"));
        assert!(world.net.requests().is_empty());
        let verified = run(&world, &mut index, &["plugin", "verify", "dctx/git", "--json"]);
        assert_eq!(verified.code, 0, "{}", verified.body);
        assert!(verified.body.contains("\"executed\":false"));
        let disabled = run(&world, &mut index, &["plugin", "disable", "dctx/git", "--json"]);
        assert_eq!(disabled.code, 0, "{}", disabled.body);
        let again = run(&world, &mut index, &["git", "protect", "--json"]);
        assert_eq!(again.code, 1);
        assert!(again.body.contains("Disabled"));
        let removed = run(&world, &mut index, &["plugin", "remove", "dctx/git", "--json"]);
        assert_eq!(removed.code, 0, "{}", removed.body);
        let listed = run(&world, &mut index, &["plugin", "list"]);
        assert!(listed.body.contains("disabled"));
        assert!(!listed.body.contains("needs-consent"));
    }

    #[cfg(not(feature = "git-worker"))]
    #[test]
    fn strict_alias_and_canonical_run_refuse_without_effects() {
        let world = World::new();
        let mut index = IndexCache::default();
        for installed in [false, true] {
            if installed {
                world.publish("1.0.0", 1);
                let outcome = run(&world, &mut index, &[
                    "plugin", "install", "--url", "https://releases.example.org/dctx-git/1.0.0/plugin.json",
                    "--id", "dctx/git", "--json",
                ]);
                assert_eq!(outcome.code, 0, "{}", outcome.body);
            }
            let before = world.store.load_registry().unwrap();
            let requests = world.net.requests().len();
            for verb in ["status", "protect"] {
                for json in [false, true] {
                    let mut alias = vec!["git", verb];
                    let mut canonical = vec!["plugin", "run", "dctx/git"];
                    if json {
                        alias.push("--json");
                        canonical.push("--json");
                    }
                    canonical.extend(["--", verb]);
                    for args in [alias, canonical] {
                        let refused = run(&world, &mut index, &args);
                        assert_eq!(refused.code, 1, "{}", refused.body);
                        assert!(refused.body.contains("StrictBuild"), "{}", refused.body);
                        if json {
                            assert!(refused.body.contains("\"error\":\"StrictBuild\""));
                            assert!(refused.body.contains("\"spawned\":false"));
                        }
                    }
                }
            }
            assert_eq!(world.net.requests().len(), requests);
            assert_eq!(world.store.load_registry().unwrap(), before);
        }
    }

    #[test]
    fn offline_install_verifies_and_a_stale_trust_refresh_refuses() {
        let world = World::new();
        let bytes = world.bundle("1.0.0", 1);
        let path = std::env::temp_dir().join(format!("dctx-cli-bundle-{}.dctx-plugin", std::process::id()));
        fs::write(&path, &bytes).unwrap();
        let mut index = IndexCache::default();
        let args_path = path.to_string_lossy().to_string();
        let installed = run(&world, &mut index, &["plugin", "install", "--file", args_path.as_str(), "--id", "dctx/git", "--json"]);
        assert_eq!(installed.code, 0, "{}", installed.body);
        let _ = fs::remove_file(&path);
        let verified = run(&world, &mut index, &["plugin", "verify", "dctx/git", "--json"]);
        assert_eq!(verified.code, 0, "{}", verified.body);
        let mut policy = world.policy.clone();
        policy.freshness_ceiling_secs = Some(10);
        let host = PluginHost::fake();
        world.clock.set(NOW + 11);
        let mut env = Env {
            network: &world.net,
            store: &world.store,
            clock: &world.clock,
            trust: &world.trust,
            policy: &policy,
            index: &mut index,
            index_url: INDEX,
            index_key: Some(world.index_key),
            host: &host,
        };
        let request = parse_argv(&["dctx".into(), "plugin".into(), "verify".into(), "--refresh-trust".into(), "--json".into()]).unwrap();
        let stale = execute(&request, &mut env);
        assert_eq!(stale.code, 1, "{}", stale.body);
        assert!(stale.body.contains("StaleRevocation"));
        let positive = run(&world, &mut index, &["plugin", "verify", "dctx/git", "--json"]);
        assert_eq!(positive.code, 0, "{}", positive.body);
    }

    #[test]
    fn hostile_search_text_is_escaped_and_unsigned_refresh_refuses() {
        let world = World::new();
        let summary = format!("ok{}", '\u{202e}');
        world.publish("1.0.0", 1);
        world.publish_index(&summary, "1.0.0", 1);
        let mut index = IndexCache::default();
        let found = run(&world, &mut index, &["plugin", "search", "--refresh", "--json"]);
        assert_eq!(found.code, 0, "{}", found.body);
        assert!(!found.body.contains('\u{202e}'));
        assert!(found.body.contains("\\\\u{202e}") || found.body.contains("\\u{202e}"));
        world.net.unroute(INDEX);
        world.net.serve(INDEX, b"not-an-index");
        let mut index = IndexCache::default();
        let refused = run(&world, &mut index, &["plugin", "search", "--refresh", "--json"]);
        assert_eq!(refused.code, 1);
        assert!(refused.body.contains("Signature") || refused.body.contains("Malformed"));
    }
}
