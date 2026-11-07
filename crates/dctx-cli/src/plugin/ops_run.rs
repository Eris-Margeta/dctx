// SPDX-License-Identifier: Apache-2.0
//! Plugin verification and command admission for PLUGINS.md §5.1.
//! Depends on distribution admission, trust and bundled worker availability.
//! Used by the plugin operations dispatcher for verify, run and git.

use dctx_plugin_distribution::{DistributionError, PluginId};

use super::ops_enablement::is_bundled_selection;
use super::ops_output::{code_name, dist, done, fail, usage, Outcome};
use super::{bundled, render, Env};

pub(super) fn verify(
    env: &mut Env<'_>,
    raw_id: Option<&str>,
    refresh_trust: bool,
    json: bool,
) -> Outcome {
    let text = raw_id.unwrap_or("dctx/git");
    let id = match PluginId::parse(text) {
        Ok(id) => id,
        Err(error) => return fail("verify", &error, json),
    };
    if refresh_trust {
        if let Err(error) = env
            .trust
            .check_freshness(env.clock.now(), env.policy.freshness_ceiling_secs)
        {
            return fail("verify", &error, json);
        }
    }
    if is_bundled_selection(env, &id) {
        let worker = if bundled::worker_linked() {
            "same-binary"
        } else {
            "excluded"
        };
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
            format!(
                "{{\"digest\":{},\"result\":{result}}}",
                render::json_string(&outcome.digest.to_string())
            )
        })
        .collect();
    let mut body = done("verify", json, env, |fields| {
        format!(
            "{fields},\"id\":{},\"outcomes\":[{}]",
            render::json_string(id.as_str()),
            rows.join(",")
        )
    });
    if failed {
        body.code = 1;
    }
    body
}

pub(super) fn route(
    env: &mut Env<'_>,
    raw_id: &str,
    args: &[String],
    command: &str,
    json: bool,
) -> Outcome {
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
    let rest = args
        .iter()
        .skip(1)
        .map(|arg| render::json_string(arg))
        .collect::<Vec<_>>()
        .join(",");
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
