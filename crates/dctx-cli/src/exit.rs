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
    /// Zero marks a disconnected output consumer for a quiet CLI return.
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
    from_facade_scoped(error, help, "pass --place <a smaller directory>")
}

/// Uses the caller's actual scope syntax in capacity hints (map has `--place`, pack has PATH).
pub(crate) fn from_facade_scoped(error: &DctxError, help: &str, narrower: &str) -> Failure {
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
        ErrorCode::ResourceLimit => Failure::index(message, limit_hint(error, narrower)),
        ErrorCode::CacheLimit => Failure::index(message, format!("cache capacity is insufficient for this request; {narrower} (the product cache cap is 2147483648 bytes)")),
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
fn limit_hint(error: &DctxError, narrower: &str) -> String {
    let cause = match error {
        DctxError::ResourceLimit { resource: Resource::SourceBytes, .. } => {
            "the place has more source bytes than one scan reads; raise DCTX_MAX_SOURCE_BYTES (1..17179869184 bytes) or"
        }
        DctxError::ResourceLimit { resource: Resource::Records, .. } => {
            "the place holds more records (files, symbols, edges) than one index keeps; raise DCTX_MAX_RECORDS (1..100000000) or"
        }
        DctxError::ResourceLimit { resource: Resource::Entries, .. } => {
            "the place has more files and directories than one scan visits; raise DCTX_MAX_ENTRIES (1..100000000) or"
        }
        DctxError::ResourceLimit { resource: Resource::ObjectBytes, .. } => {
            "the generation exceeds the cache object read limit;"
        }
        _ => "the place or the answer exceeds a dctx limit; narrow the query or",
    };
    format!("{cause} {narrower}")
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

    #[test]
    fn capacity_hints_distinguish_objects_and_cache_without_assuming_gc_can_help() {
        let object = from_facade(
            &DctxError::ResourceLimit {
                resource: Resource::ObjectBytes,
                limit: 64,
            },
            "x",
        );
        assert!(object.message.contains("object_bytes > 64"));
        assert!(object.hint.contains("object read limit"));
        assert!(object.hint.contains("--place"));
        let cache = from_facade(
            &DctxError::CacheLimit {
                requested: 100,
                available: 0,
            },
            "x",
        );
        assert!(cache.hint.contains("cache capacity"));
        assert!(cache.hint.contains("--place <a smaller directory>"));
        assert!(!cache.hint.contains("cache gc"));
    }
}

/// Captured engine stdout and an optional failure reported after that output.
#[derive(Debug)]
pub struct CommandOutput {
    /// Exact bytes printed by the command, represented as UTF-8.
    pub stdout: String,
    /// The CLI failure, including a no-result outcome after an emitted page.
    pub failure: Option<Failure>,
}

impl CommandOutput {
    /// Prints the captured output and returns its original CLI status.
    /// # Errors
    /// Returns the failure attached to the emitted output.
    pub fn emit(self) -> Result<(), Failure> {
        self.write_to(&mut std::io::stdout().lock())
            .or_else(|failure| {
                if failure.code == 0 {
                    Ok(())
                } else {
                    Err(failure)
                }
            })
    }
}

/// Maps output failures uniformly, including Windows closed-pipe errors.
/// A closed transport has code zero so CLI entry points can finish quietly.
pub fn write_failure(error: std::io::Error) -> Failure {
    if error.kind() == std::io::ErrorKind::BrokenPipe
        || cfg!(windows) && matches!(error.raw_os_error(), Some(109 | 232))
    {
        Failure::new(0, "stream closed", "")
    } else {
        Failure::index(error.to_string(), "check the output destination")
    }
}

impl CommandOutput {
    /// Transports captured bytes to a writer and preserves the original status.
    /// # Errors
    /// Returns an output failure or the command failure after flushing its page.
    pub fn write_to(self, output: &mut dyn std::io::Write) -> Result<(), Failure> {
        output
            .write_all(self.stdout.as_bytes())
            .map_err(write_failure)?;
        output.flush().map_err(write_failure)?;
        self.failure.map_or(Ok(()), Err)
    }
}

/// Remembers the original I/O error while engine error types cross the transport boundary.
pub struct TrackedWriter<'a> {
    inner: &'a mut dyn std::io::Write,
    error: Option<std::io::Error>,
}

impl<'a> TrackedWriter<'a> {
    /// Wraps a writer without buffering its output.
    pub fn new(inner: &'a mut dyn std::io::Write) -> Self {
        Self { inner, error: None }
    }
    /// Takes the first underlying writer failure, retaining its platform error code.
    pub fn take_error(&mut self) -> Option<std::io::Error> {
        self.error.take()
    }
    fn record<T>(&mut self, result: std::io::Result<T>) -> std::io::Result<T> {
        result.map_err(|error| {
            let returned = std::io::Error::new(error.kind(), error.to_string());
            if error.kind() != std::io::ErrorKind::Interrupted && self.error.is_none() {
                self.error = Some(error);
            }
            returned
        })
    }
}

impl std::io::Write for TrackedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let result = self.inner.write(bytes);
        self.record(result)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let result = self.inner.flush();
        self.record(result)
    }
}

#[cfg(test)]
mod writer_tests {
    use super::*;
    use std::io::{self, Write};

    #[test]
    fn interrupted_writes_retry_without_leaving_a_transport_failure() {
        struct InterruptOnce {
            interrupted: bool,
            bytes: Vec<u8>,
        }
        impl Write for InterruptOnce {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut output = InterruptOnce {
            interrupted: false,
            bytes: Vec::new(),
        };
        let mut tracked = TrackedWriter::new(&mut output);
        tracked.write_all(b"complete").expect("retry");
        assert!(tracked.take_error().is_none());
        assert_eq!(output.bytes, b"complete");
    }
}
