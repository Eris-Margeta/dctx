//! Fixed environment for trusted bootstrap processes; never an inherited allowlist.
//! Implements S-ADMISSION and PL-R loader/credential isolation requirements.
//! Only prepares a Command; it is not permission to spawn an unconfined plugin.

use std::process::Command;

/// Empty ambient authority with deterministic locale; no caller-supplied values.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSecretEnvironment;

impl NoSecretEnvironment {
    /// Clears all inherited/previously configured variables before setting fixed locale.
    pub fn apply(self, command: &mut Command) {
        command.env_clear().env("LANG", "C").env("LC_ALL", "C");
    }

    /// Verifies the actual bootstrap environment before entry into restricted code.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn is_clean(self) -> bool {
        std::env::vars_os().all(|(key, value)| (key == "LANG" || key == "LC_ALL") && value == "C")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_preconfigured_secrets_loader_git_and_home_values() {
        let mut command = Command::new("unused");
        for key in [
            "HOME",
            "PATH",
            "TOKEN",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "GIT_CONFIG",
        ] {
            command.env(key, "fixture-secret");
        }
        NoSecretEnvironment.apply(&mut command);
        let vars: Vec<_> = command.get_envs().collect();
        assert_eq!(vars.len(), 2);
        assert!(
            vars.iter()
                .all(|(k, v)| (*k == "LANG" || *k == "LC_ALL") && *v == Some("C".as_ref()))
        );
    }
}
