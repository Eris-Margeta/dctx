//! Application layer: the protection use case composed over the broker port (no IO of its own).
//! Implements PLUGINS.md §6.2 sequencing and the git.md outcome table on top of the pure domain.
//! Depends on domain, ports and dctx-plugin-protocol records; used by adapters::worker and the tests.

pub mod binding;
pub mod failure;
pub mod invoke;
pub mod protect;
pub mod stage;
pub mod transfer;
