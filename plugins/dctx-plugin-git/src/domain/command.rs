// SPDX-License-Identifier: Apache-2.0
//! The `dctx git` command surface: verbs `status` and `protect`, and the sole flag `protect --stage`.
//! Implements plugin-api git.md "Command verbs are status/protect ... Unknown verbs/flags refuse; no Git
//! passthrough" (G-PROFILE). Pure; used by `app::command`.

/// A parsed command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Report local protection state; no writes, no Git.
    Status,
    /// Protect now; `stage` requests a new explicit staging action.
    Protect {
        /// `--stage` was given.
        stage: bool,
    },
}

/// Why a command was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandRefusal {
    /// The alias is not `git`.
    UnknownAlias,
    /// The verb is not declared.
    UnknownVerb,
    /// An argument is not accepted by the verb.
    UnknownArgument,
}

impl CommandRefusal {
    /// Stable safe reason name.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::UnknownAlias => "unknown-alias",
            Self::UnknownVerb => "unknown-verb",
            Self::UnknownArgument => "unknown-argument",
        }
    }
}

/// Parses `alias verb args...`.
/// # Errors
/// Refuses an unknown alias/verb or arguments outside the command profile.
pub fn parse(alias: &str, verb: &str, args: &[&str]) -> Result<Command, CommandRefusal> {
    if alias != "git" {
        return Err(CommandRefusal::UnknownAlias);
    }
    match (verb, args) {
        ("status", []) => Ok(Command::Status),
        ("protect", []) => Ok(Command::Protect { stage: false }),
        ("protect", ["--stage"]) => Ok(Command::Protect { stage: true }),
        ("status" | "protect", _) => Err(CommandRefusal::UnknownArgument),
        _ => Err(CommandRefusal::UnknownVerb),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_declared_verbs_and_the_stage_flag_parse() {
        assert_eq!(parse("git", "status", &[]), Ok(Command::Status));
        assert_eq!(
            parse("git", "protect", &[]),
            Ok(Command::Protect { stage: false })
        );
        assert_eq!(
            parse("git", "protect", &["--stage"]),
            Ok(Command::Protect { stage: true })
        );
        assert_eq!(
            parse("git", "protect", &["--stage", "--stage"]),
            Err(CommandRefusal::UnknownArgument)
        );
        assert_eq!(
            parse("git", "status", &["--stage"]),
            Err(CommandRefusal::UnknownArgument)
        );
        assert_eq!(
            parse("git", "add", &["."]),
            Err(CommandRefusal::UnknownVerb)
        );
        assert_eq!(
            parse("git", "commit", &[]),
            Err(CommandRefusal::UnknownVerb)
        );
        assert_eq!(
            parse("svn", "status", &[]),
            Err(CommandRefusal::UnknownAlias)
        );
    }
}
