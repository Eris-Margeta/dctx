//! Host services composing the pure domain with ports: broker, effects, supervisor and session.
//! Implements PLUGINS.md §3.3, §3.4 and §4.4 over `ports`. Depends on domain and ports.
//! Used by the facade (PL-F); tests drive it through `fakes`.

pub mod broker;
mod effects;
pub mod handles;
pub mod session;
pub mod supervisor;
