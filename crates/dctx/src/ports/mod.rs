// SPDX-License-Identifier: Apache-2.0
//! Ports the facade uses. The traits themselves live in `dctx-ports`.
//! Implements ARCHITECTURE-PROPOSAL §2: engines depend on ports, never on an adapter.
//! Depends on `dctx-ports`. The session and the filesystem adapter use these traits.
//! A host supplies every port. The facade does not discover one.

pub use dctx_ports::{Cache, Cancellation, Clock, FactStore, Source};
