//! Content access guarded before opening any path.
//! Implements the ADR-014 never-read guarantee for excluded and omitted files.
//! Uses ContentSource so tests can observe every attempted open.

use crate::{FileHandlingDecision, ports::ContentSource};
use std::{fs, io, path::Path};

/// Filesystem implementation of authorized content reads.
pub struct FileContentSource;
impl ContentSource for FileContentSource {
    fn read_content(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }
}

/// Reads only when the decision permits content; otherwise never touches IO.
///
/// # Errors
/// Propagates the content source read error when the decision permits a read.
pub fn read_content_if_allowed(
    source: &impl ContentSource,
    path: &Path,
    decision: FileHandlingDecision,
) -> io::Result<Option<Vec<u8>>> {
    if decision.should_read_content() {
        source.read_content(path).map(Some)
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IgnoreEngine, ItemContext, domain::ignore::parse_ignore_pattern_line};
    use std::cell::Cell;

    struct Spy(Cell<usize>);
    impl ContentSource for Spy {
        fn read_content(&self, _: &Path) -> io::Result<Vec<u8>> {
            self.0.set(self.0.get() + 1);
            Ok(b"public content".to_vec())
        }
    }
    #[test]
    fn excluded_debug_visible_and_omitted_files_never_open() {
        let spy = Spy(Cell::new(0));
        let engine = IgnoreEngine::from_rules(
            vec![parse_ignore_pattern_line(".env").unwrap()],
            vec![parse_ignore_pattern_line("*.lock").unwrap()],
        );
        for (name, debug, expected) in [
            (".env", false, FileHandlingDecision::Exclude),
            (".env", true, FileHandlingDecision::DebugVisible),
            ("deps.lock", false, FileHandlingDecision::OmitContent),
        ] {
            let decision = engine.decide(&ItemContext {
                relative_path: name,
                name,
                is_directory: false,
                debug_mode: debug,
            });
            assert_eq!(decision, expected);
            assert!(
                read_content_if_allowed(&spy, Path::new(name), decision)
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(spy.0.get(), 0);
        let decision = engine.decide(&ItemContext {
            relative_path: "main.rs",
            name: "main.rs",
            is_directory: false,
            debug_mode: false,
        });
        assert!(
            read_content_if_allowed(&spy, Path::new("main.rs"), decision)
                .unwrap()
                .is_some()
        );
        assert_eq!(spy.0.get(), 1);
    }
}
