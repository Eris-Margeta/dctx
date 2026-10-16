//! dctx/git: the bundled git plugin. Local `.gitignore` protection and the brokered `git add .gitignore`.
//! Implements PLUGINS.md §6 and the frozen plugin-api git.md profile (decision 078/decision 079: no Git in the core;
//! the behaviour lives only here). Every effect is a broker request (decision 080); the worker has no direct
//! filesystem, process or network access. Depends on dctx-plugin-protocol; used by main.rs and hosts.
//! Layers: `domain` (pure rules), `ports` (broker), `app` (use case), `adapters` (framed stdio worker).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adapters;
pub mod app;
pub mod domain;
pub mod ports;
