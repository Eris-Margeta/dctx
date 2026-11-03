// SPDX-License-Identifier: Apache-2.0
//! Pure host domain: bindings, grants, selection, authority, receipts, budgets, liveness, lifecycle, hooks.
//! Implements the PL-M models docs/formal/dctx-plugins/*.tla as synchronous IO-free state machines.
//! Depends on dctx-plugin-protocol records and dctx-hash only. Used by `service` and the facade (PL-F).

pub mod authority;
pub mod binding;
pub mod budget;
pub mod grant;
pub mod hooks;
pub mod ledger;
pub mod lifecycle;
pub mod liveness;
pub mod selection;
pub mod trace;
