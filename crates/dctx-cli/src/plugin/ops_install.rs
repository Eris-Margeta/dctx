// SPDX-License-Identifier: Apache-2.0
//! Plugin installation and update commands for PLUGINS.md §5.1.
//! Depends on distribution installs, the cached index and consent enablement.
//! Used by the plugin operations dispatcher.

use std::fs;
use std::path::Path;

use dctx_plugin_distribution::{DistributionError, HttpsUrl, PluginId, Version};

use super::ops_catalog::cached_index;
use super::ops_enablement::{activate_and_enable, is_bundled_selection};
use super::ops_output::{dist, done, fail, Outcome};
use super::parse::Install;
use super::{render, Env};

pub(super) fn install_cmd(env: &mut Env<'_>, install: &Install, json: bool) -> Outcome {
    let report = match install {
        Install::Index {
            repository,
            version,
            enable,
            approve,
        } => {
            let index = match cached_index(env) {
                Ok(index) => index,
                Err(error) => return fail("install", &error, json),
            };
            let Some(entry) = index
                .entries
                .iter()
                .find(|entry| entry.repository.eq_ignore_ascii_case(repository))
            else {
                return fail("install", &DistributionError::NotFound, json);
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
        Install::Url {
            url,
            id,
            enable,
            approve,
        } => {
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
        Install::File {
            path,
            id,
            enable,
            approve,
        } => {
            let Ok(bytes) = fs::read(Path::new(path)) else {
                return fail("install", &DistributionError::NotFound, json);
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

pub(super) fn update(
    env: &mut Env<'_>,
    raw_id: Option<&str>,
    version: Option<&str>,
    check: bool,
    json: bool,
) -> Outcome {
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
