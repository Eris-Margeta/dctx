// SPDX-License-Identifier: Apache-2.0
//! Documented macOS Seatbelt candidate path; never executes sandbox-exec as a fallback.
//! PL-R macOS requires an approved safe API, immutable loader resources and one-shot launch.
//! The deny-all baseline is a review artifact, not a qualified runnable worker profile.

use crate::{PlatformError, Unsupported};

/// Checked-in deny-default SBPL baseline; contains no filesystem, Mach or process allows.
pub const DENY_ALL_PROFILE: &str = include_str!("../profiles/macos-deny-all.sb");

/// Refuses launch on both macOS architectures; no unsafe FFI or deprecated helper fallback.
///
/// # Errors
/// Always returns the unsupported Seatbelt error because no macOS provider is qualified.
pub fn require_provider() -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported(Unsupported::Seatbelt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_is_deny_all_and_cannot_advertise_a_provider_on_linux() {
        assert_eq!(DENY_ALL_PROFILE, "(version 1)\n(deny default)\n");
        assert!(matches!(
            require_provider(),
            Err(PlatformError::Unsupported(Unsupported::Seatbelt))
        ));
    }
}
