// SPDX-License-Identifier: Apache-2.0
//! dctx plugin installer, trust and discovery client (PLUGINS.md §5, §7.2 row PL-I).
//! Implements docs/design/plugin-api/distribution.md and docs/formal/dctx-plugins/Install.tla:
//! signed manifests verified with dctx-ed25519, payload digests with dctx-hash, DCTXPLG1 offline
//! bundles, origin-checked bounded downloads, immutable versions and an atomic activation record.
//! Depends on dctx-hash, dctx-ed25519 and dctx-plugin-protocol; used by PL-CLI and PL-APP.
//! No code runs during install, verify, list or recovery: no port can execute a file.
//! **Unstable**: this crate has no API or ABI stability promise in dctx v1 and may change in any release.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod domain;
pub mod error;
pub mod fakes;
pub mod ports;
pub mod service;

pub use domain::admission::{Admission, PluginStatus};
pub use domain::bundle::OfflineBundle;
pub use domain::consent::{CapabilitySet, ConsentApproval, ConsentRecord, ConsentScope};
pub use domain::digest::{Fingerprint, Sha256Hex, fingerprint};
pub use domain::index::{IndexEntry, IndexRelease, VerifiedIndex, escape_display};
pub use domain::manifest::{Artifact, Manifest, Payload};
pub use domain::names::{KeyId, PluginId, SafeFileName, Target, Version};
pub use domain::origin::{HttpsUrl, OriginPolicy};
pub use domain::registry::{Confinement, Enablement, InstalledVersion, Origin, PluginRecord, Registry};
pub use domain::trust::{TrustSource, TrustStore, TrustedKey, VerifiedManifest};
pub use error::{DistributionError, Document, NetworkError, OriginRefusal};
pub use ports::{Body, Clock, InstallKey, LockToken, Network, Response, Store};
pub use service::{DEFAULT_MAX_ARTIFACT_BYTES, Distribution, InstallPolicy, InstallReport, RecoveryReport, VerifyOutcome};
