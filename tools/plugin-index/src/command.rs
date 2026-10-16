//! Argument dispatch for the local checker.
//! Implements PLUGINS.md §7.2 PL-X. Depends on the pure domain functions.
//! The binary supplies a filesystem reader. Tests pass bytes in memory.
//! `publish` and `sign` are refused. This module performs no IO of its own.

use crate::domain::{PublisherPins, check_signed_index, decode_index_key, validate_index};
use crate::IndexError;

/// Runs `validate` or `check`. `read` loads each named local artifact.
///
/// # Errors
/// Returns a typed refusal. Publication commands never read their arguments.
pub fn execute<R>(args: &[String], mut read: R) -> Result<(), IndexError>
where
    R: FnMut(&str) -> Result<Vec<u8>, IndexError>,
{
    match args.first().map(String::as_str) {
        Some("publish" | "sign" | "push" | "release") => Err(IndexError::PublicationRefused),
        Some("validate") if args.len() == 2 => {
            let bytes = read(&args[1])?;
            validate_index(&bytes)?;
            Ok(())
        }
        Some("check") if args.len() == 5 => {
            let index = read(&args[1])?;
            let signature = read(&args[2])?;
            let key = decode_index_key(&read(&args[3])?)?;
            let pins = PublisherPins::parse(&read(&args[4])?)?;
            check_signed_index(&index, &signature, &key, &pins)?;
            Ok(())
        }
        _ => Err(IndexError::Usage),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_commands_are_refused_before_any_read() {
        let error = execute(&["publish".to_owned()], |_| unreachable!());
        assert_eq!(error, Err(IndexError::PublicationRefused));
        let error = execute(&["sign".to_owned(), "index.json".to_owned()], |_| unreachable!());
        assert_eq!(error, Err(IndexError::PublicationRefused));
    }
}
