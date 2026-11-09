// SPDX-License-Identifier: Apache-2.0
//! Plugin verification and command admission for PLUGINS.md §5.1.
//! Depends on distribution admission, trust and bundled worker availability.
//! Used by the plugin operations dispatcher for verify, run and git.

use dctx_plugin_distribution::{DistributionError, PluginId};

use super::ops_enablement::is_bundled_selection;
use super::ops_output::{code_name, dist, done, fail, fail_for, failure_text, usage, Outcome};
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
            return fail_for("verify", Some(id.as_str()), &error, json);
        }
    }
    if is_bundled_selection(env, &id) {
        let worker = if bundled::worker_linked() {
            "same-binary"
        } else {
            "excluded"
        };
        return done(
            "verify",
            json,
            env,
            |fields| {
                format!(
                    "{fields},\"id\":{},\"origin\":\"bundled\",\"worker\":{},\"executed\":false",
                    render::json_string(id.as_str()),
                    render::json_string(worker)
                )
            },
            || {
                format!(
                    "{} is bundled in this dctx binary (worker {worker}); nothing to verify on disk\n",
                    id.as_str()
                )
            },
        );
    }
    let outcomes = match dist(env).verify(&id) {
        Ok(outcomes) => outcomes,
        Err(error) => return fail_for("verify", Some(id.as_str()), &error, json),
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
    let lines: String = outcomes
        .iter()
        .map(|outcome| match &outcome.result {
            Ok(()) => format!("{}\tok\n", outcome.digest),
            Err(error) => format!("{}\t{}\n", outcome.digest, code_name(error)),
        })
        .collect();
    let mut body = done(
        "verify",
        json,
        env,
        |fields| {
            format!(
                "{fields},\"id\":{},\"outcomes\":[{}]",
                render::json_string(id.as_str()),
                rows.join(",")
            )
        },
        || {
            if failed {
                format!(
                    "{lines}{}\n",
                    failure_text(
                        "verify",
                        &format!(
                            "an installed version of {} failed verification",
                            id.as_str()
                        ),
                        &format!(
                            "remove it with `dctx plugin remove {}` and install a fresh copy",
                            id.as_str()
                        ),
                    )
                )
            } else if lines.is_empty() {
                format!("{}: no installed versions to verify\n", id.as_str())
            } else {
                lines.clone()
            }
        },
    );
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
                failure_text(
                    command,
                    &format!("{error} (StrictBuild)"),
                    "use a dctx build that includes the bundled git worker",
                )
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
        Err(error) => return fail_for(command, Some(id.as_str()), &error, json),
    };
    if !known && id.as_str() == "dctx/git" && bundled::worker_linked() {
        return fail_for(
            command,
            Some(id.as_str()),
            &DistributionError::ConsentRequired,
            json,
        );
    }
    let admission = match dist(env).admit(&id) {
        Ok(admission) => admission,
        Err(error) => return fail_for(command, Some(id.as_str()), &error, json),
    };
    if let Err(error) = dist(env).release(&admission) {
        return fail_for(command, Some(id.as_str()), &error, json);
    }
    let rest = args
        .iter()
        .skip(1)
        .map(|arg| render::json_string(arg))
        .collect::<Vec<_>>()
        .join(",");
    done(
        command,
        json,
        env,
        |fields| {
            format!(
                "{fields},\"id\":{},\"verb\":{},\"args\":[{rest}],\"worker\":\"same-binary\",\"spawned\":false,\"digest\":{}",
                render::json_string(id.as_str()),
                render::json_string(verb),
                render::json_string(&admission.digest.to_string())
            )
        },
        || {
            format!(
                "{} {verb}: admitted (sha256 {}); no worker was started\n",
                id.as_str(),
                admission.digest
            )
        },
    )
}
