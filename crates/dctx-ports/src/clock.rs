// SPDX-License-Identifier: Apache-2.0
//! `Clock` port: injected wall-clock time.
//! Implements contracts/API-PORTS-v1.md `Clock.wall`: engines never read the system clock,
//! so a frozen clock replays a run exactly (P6).
//! Depends on dctx-core `Timestamp`. Used by dctx-pack to stamp snapshots.

use dctx_core::ids::Timestamp;

/// Wall-clock time supplied by the host.
pub trait Clock {
    /// Current wall-clock time.
    fn now(&self) -> Timestamp;
}
