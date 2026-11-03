// SPDX-License-Identifier: Apache-2.0
//! Plugin enablement, removal and bundled consent for PLUGINS.md §5.1.
//! Depends on distribution registry operations and the bundled catalog.
//! Used by the dispatcher, installation and execution operations.

use dctx_plugin_distribution::{ConsentApproval, ConsentScope, Confinement, DistributionError, Origin, PluginId};

use super::ops_output::{dist, done, fail, Outcome};
use super::{bundled, render, Env};

pub(super) fn enable(env: &mut Env<'_>, raw_id: &str, project: Option<&str>, approve: bool, json: bool) -> Outcome {
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

pub(super) fn disable(env: &mut Env<'_>, raw_id: &str, project: Option<&str>, json: bool) -> Outcome {
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

pub(super) fn remove(env: &mut Env<'_>, raw_id: &str, json: bool) -> Outcome {
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

pub(super) fn activate_and_enable(env: &Env<'_>, id: &PluginId, approve: bool, project: Option<&str>) -> Result<(), DistributionError> {
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

pub(super) fn prepare_bundled(env: &Env<'_>, id: &PluginId) -> Result<(), DistributionError> {
    if id.as_str() != bundled::catalog().id || !bundled::worker_linked() {
        return Ok(());
    }
    if dist(env).registry()?.get(id).is_some() {
        return Ok(());
    }
    dist(env).register_bundled(id, bundled::installed_version()?)
}

pub(super) fn is_bundled_selection(env: &Env<'_>, id: &PluginId) -> bool {
    if id.as_str() != "dctx/git" || !bundled::worker_linked() {
        return false;
    }
    match dist(env).registry() {
        Ok(registry) => registry.get(id).is_none_or(|record| record.origin == Origin::Bundled),
        Err(_) => false,
    }
}
