// SPDX-License-Identifier: Apache-2.0
//! The CLI exit-code contract of docs/CLI-EXIT-CODES.md: 0 ok, 1 no result, 2 usage, 3 index error.
//! `map`, `pack`, `mcp`, `languages`, unknown commands and non-UTF-8 arguments return a [`Failure`]; `main` prints it and exits with its code.
//! Every failure carries a hint the user can act on. Cache and template command failures use exit 1.

use dctx::{DctxError, ErrorCode, Resource};

/// The command answered.
pub const OK: i32 = 0;
/// The command ran, but the named target or input has no answer: an id not in the index, a binary file, nothing to pack.
pub const NO_RESULT: i32 = 1;
/// The arguments are wrong: an unknown command or flag, a missing or malformed value, a bound out of range, a stale cursor.
pub const USAGE: i32 = 2;
/// The place, source, cache or index could not be opened, read, built or written.
pub const INDEX: i32 = 3;

/// One failed command: its exit code, what went wrong and what the user can do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// [`NO_RESULT`], [`USAGE`], [`INDEX`], or 1 for a cache or template command.
    pub code: i32,
    /// What went wrong, one line, no trailing newline.
    pub message: String,
    /// What to do next, one line; empty when there is nothing to suggest.
    pub hint: String,
}

impl Failure {
    /// A failure with an explicit code.
    pub fn new(code: i32, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: hint.into(),
        }
    }

    /// Exit 1: the target or input has no answer.
    pub fn no_result(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(NO_RESULT, message, hint)
    }

    /// Exit 2: the arguments are wrong.
    pub fn usage(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(USAGE, message, hint)
    }

    /// Exit 3: the place, source, cache or index failed.
    pub fn index(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(INDEX, message, hint)
    }

    /// Prefixes the message with the command name, as `map: …`.
    #[must_use]
    pub fn within(mut self, command: &str) -> Self {
        self.message = format!("{command}: {}", self.message);
        self
    }

    /// The stderr text: `❌ Error: <message>`, then `hint: <hint>` when there is a hint. Ends with a newline.
    #[allow(
        clippy::must_use_candidate,
        reason = "A rendering accessor; callers print it or compare it."
    )]
    pub fn render(&self) -> String {
        if self.hint.is_empty() {
            format!("❌ Error: {}\n", self.message)
        } else {
            format!("❌ Error: {}\nhint: {}\n", self.message, self.hint)
        }
    }
}

/// Maps a facade error to the contract: request refusals are usage, a denied target is no result, the rest is an index error.
/// `help` is the help command to name in a usage hint, such as `dctx map find --help`.
#[allow(
    clippy::must_use_candidate,
    reason = "A pure mapping; the caller returns the failure."
)]
pub fn from_facade(error: &DctxError, help: &str) -> Failure {
    let message = error.to_string();
    match error.code() {
        ErrorCode::StaleContinuation | ErrorCode::Stale => Failure::usage(
            message,
            "the cursor belongs to an older index or another query; run the query again without --cursor to start from the first page",
        ),
        ErrorCode::BudgetTooSmall => Failure::usage(message, "raise --budget (at most 2000 tokens)"),
        ErrorCode::InvalidRequest | ErrorCode::UnsupportedCapability | ErrorCode::UnsupportedVersion => {
            Failure::usage(message, format!("run `{help}` for the options and an example"))
        }
        ErrorCode::PolicyDenied | ErrorCode::PolicyUnavailable => Failure::no_result(
            message,
            "the target is excluded by .gitignore, .dctxignore or the sensitive-file rules; pick another target",
        ),
        ErrorCode::ResourceLimit => Failure::index(message, limit_hint(error)),
        ErrorCode::CacheLimit => Failure::index(message, "free cache space with `dctx cache gc`, then run the query again"),
        ErrorCode::Busy => Failure::index(message, "another dctx process holds the cache; run the query again when it finishes"),
        ErrorCode::CorruptIndex => Failure::index(
            message,
            "drop the cached index with `dctx cache purge --root <place> --yes`, then run the query again to rebuild it",
        ),
        ErrorCode::SourceChanged => Failure::index(message, "files changed while dctx read them; run the query again"),
        _ => Failure::index(message, "check that the place exists and is readable, then run the query again"),
    }
}

/// The hint for a bounded quantity: the source, record and entry caps have an env switch, the others need a smaller place.
fn limit_hint(error: &DctxError) -> &'static str {
    match error {
        DctxError::ResourceLimit { resource: Resource::SourceBytes, .. } => {
            "the place has more source bytes than one scan reads; raise DCTX_MAX_SOURCE_BYTES or pass --place <a smaller directory>"
        }
        DctxError::ResourceLimit { resource: Resource::Records, .. } => {
            "the place holds more records (files, symbols, edges) than one index keeps; raise DCTX_MAX_RECORDS (1..100000000) or pass --place <a smaller directory>"
        }
        DctxError::ResourceLimit { resource: Resource::Entries, .. } => {
            "the place has more files and directories than one scan visits; raise DCTX_MAX_ENTRIES (1..100000000) or pass --place <a smaller directory>"
        }
        _ => "the place or the answer exceeds a dctx limit; pass --place <a smaller directory> or narrow the query",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_codes_are_the_documented_ones() {
        assert_eq!((OK, NO_RESULT, USAGE, INDEX), (0, 1, 2, 3));
    }

    #[test]
    fn render_prints_the_message_then_the_hint() {
        let failure = Failure::usage("bad flag", "run `dctx map --help`").within("map");
        assert_eq!(
            failure.render(),
            "❌ Error: map: bad flag\nhint: run `dctx map --help`\n"
        );
        assert_eq!(Failure::new(1, "legacy", "").render(), "❌ Error: legacy\n");
    }

    #[test]
    fn facade_errors_map_to_the_contract() {
        assert_eq!(
            from_facade(&DctxError::StaleContinuation, "dctx map find --help").code,
            USAGE
        );
        assert!(from_facade(&DctxError::StaleContinuation, "x")
            .hint
            .contains("without --cursor"));
        assert_eq!(
            from_facade(
                &DctxError::invalid("target is required"),
                "dctx map read --help"
            )
            .code,
            USAGE
        );
        assert!(
            from_facade(&DctxError::invalid("x"), "dctx map read --help")
                .hint
                .contains("dctx map read --help")
        );
        assert_eq!(
            from_facade(&DctxError::BudgetTooSmall { minimum_bytes: 9 }, "x").code,
            USAGE
        );
        assert_eq!(from_facade(&DctxError::PolicyDenied, "x").code, NO_RESULT);
        assert_eq!(from_facade(&DctxError::SourceUnavailable, "x").code, INDEX);
        assert_eq!(
            from_facade(&DctxError::CorruptIndex { reason: "digest" }, "x").code,
            INDEX
        );
        assert_eq!(from_facade(&DctxError::Busy, "x").code, INDEX);
        let cap = DctxError::ResourceLimit {
            resource: Resource::SourceBytes,
            limit: 1,
        };
        assert!(from_facade(&cap, "x")
            .hint
            .contains("DCTX_MAX_SOURCE_BYTES"));
        let records = DctxError::ResourceLimit {
            resource: Resource::Records,
            limit: 1,
        };
        assert!(from_facade(&records, "x").hint.contains("DCTX_MAX_RECORDS"));
        let entries = DctxError::ResourceLimit {
            resource: Resource::Entries,
            limit: 1,
        };
        assert!(from_facade(&entries, "x").hint.contains("DCTX_MAX_ENTRIES"));
        for error in [
            DctxError::SourceUnavailable,
            DctxError::io("disk"),
            DctxError::StaleContinuation,
            DctxError::PolicyDenied,
        ] {
            assert!(!from_facade(&error, "x").hint.is_empty(), "{error}");
        }
    }
}
