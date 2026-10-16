//! Platform admission gates, separate from installation and broker consent.
//! Implements Lifecycle EnableOrRefuse and PP4; signatures never imply confinement.
//! Linux building blocks cannot mint a proven-launch disposition on their own.

use crate::{PlatformError, Unsupported};
use std::process::Child;

/// Native target profiles from PL-R; matching a triple does not imply support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Linux x86-64 GNU.
    LinuxX86_64,
    /// Linux AArch64 GNU, independently qualified.
    LinuxAarch64,
    /// macOS Apple silicon.
    MacosAarch64,
    /// macOS Intel.
    MacosX86_64,
    /// Windows x86-64 MSVC.
    WindowsX86_64,
    /// Any target outside the dossier.
    Other,
}

impl Target {
    /// Target compiled into this adapter; musl and other runtimes remain unqualified.
    pub const fn current() -> Self {
        if cfg!(all(
            target_os = "linux",
            target_arch = "x86_64",
            target_env = "gnu"
        )) {
            Self::LinuxX86_64
        } else if cfg!(all(
            target_os = "linux",
            target_arch = "aarch64",
            target_env = "gnu"
        )) {
            Self::LinuxAarch64
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Self::MacosAarch64
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Self::MacosX86_64
        } else if cfg!(all(
            target_os = "windows",
            target_arch = "x86_64",
            target_env = "msvc"
        )) {
            Self::WindowsX86_64
        } else {
            Self::Other
        }
    }
}

/// Requested native operation; no worker-controlled provenance exemption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchKind {
    /// Installed plugin, including an official standalone build or override.
    ThirdParty,
    /// Host-authenticated bundled code; decision 080 does not waive image identity.
    BundledGit,
    /// The independently confined `git.add-ignore` broker recipe.
    GitRecipe,
}

/// Honest admission result; primitives are never represented as a proven provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Support {
    /// Target whose evidence is required.
    pub target: Target,
    /// First unresolved guarantee blocking execution.
    pub missing: Unsupported,
}

/// Facade port for native subprocess admission.
pub trait ConfinementProvider {
    /// Reports missing evidence before enablement or handshake.
    fn support(&self, kind: LaunchKind) -> Support;
    /// Starts only a qualified confined worker, otherwise returns UnsupportedPlatform.
    /// No pathname/argv escape hatch exists while launch is unsupported.
    fn launch(&self, kind: LaunchKind) -> Result<Child, PlatformError>;
}

/// Default provider. General launch remains closed on every target (see README).
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeProvider;

impl ConfinementProvider for NativeProvider {
    fn support(&self, kind: LaunchKind) -> Support {
        support_for(Target::current(), kind)
    }

    fn launch(&self, kind: LaunchKind) -> Result<Child, PlatformError> {
        Err(PlatformError::Unsupported(self.support(kind).missing))
    }
}

fn support_for(target: Target, kind: LaunchKind) -> Support {
    let missing = match (target, kind) {
        (_, LaunchKind::GitRecipe) => Unsupported::ProgramRecipe,
        (_, LaunchKind::BundledGit) => Unsupported::BundledLaunch,
        (Target::LinuxX86_64 | Target::LinuxAarch64, _) => Unsupported::OneShotExec,
        (Target::MacosAarch64 | Target::MacosX86_64, _) => Unsupported::Seatbelt,
        _ => Unsupported::Target,
    };
    Support { target, missing }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_target_or_origin_can_claim_the_bundled_exception() {
        for target in [
            Target::LinuxX86_64,
            Target::LinuxAarch64,
            Target::MacosAarch64,
            Target::MacosX86_64,
            Target::WindowsX86_64,
            Target::Other,
        ] {
            for kind in [
                LaunchKind::ThirdParty,
                LaunchKind::BundledGit,
                LaunchKind::GitRecipe,
            ] {
                let evidence = support_for(target, kind);
                assert_eq!(evidence.target, target);
                assert!(matches!(
                    NativeProvider.launch(kind),
                    Err(PlatformError::Unsupported(_))
                ));
            }
        }
    }
}
