// SPDX-License-Identifier: Apache-2.0
//! §5.1 operations over the distribution facade and the dctx facade catalog.
//! Implements search, install, list, info, enable, disable, remove, update, verify and run.
//! Depends on `dctx_plugin_distribution::Distribution` and the bundled git catalog.
//! Used by the plugin entry. List and help never call this module's mutating functions.

#[cfg(test)]
use std::fs;

use super::ops_catalog::{info, list, search};
use super::ops_enablement::{disable, enable, remove};
use super::ops_install::{install_cmd, update};
use super::ops_output::{ok, usage};
use super::ops_run::{route, verify};
use super::parse::{Action, Request};
use super::{bundled, render, Env};

pub use super::ops_output::Outcome;

/// Runs `request` against `env`.
pub fn execute(request: &Request, env: &mut Env<'_>) -> Outcome {
    match &request.action {
        Action::Help => ok("help", &render::help_text(), request.json, env),
        Action::Search { query, refresh } => search(env, query, *refresh, request.json),
        Action::Install(install) => install_cmd(env, install, request.json),
        Action::List => list(env, request.json),
        Action::Info { id } => info(env, id, request.json),
        Action::Enable {
            id,
            project,
            approve,
        } => enable(env, id, project.as_deref(), *approve, request.json),
        Action::Disable { id, project } => disable(env, id, project.as_deref(), request.json),
        Action::Remove { id } => remove(env, id, request.json),
        Action::Update { id, version, check } => {
            update(env, id.as_deref(), version.as_deref(), *check, request.json)
        }
        Action::Verify { id, refresh_trust } => {
            verify(env, id.as_deref(), *refresh_trust, request.json)
        }
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

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
