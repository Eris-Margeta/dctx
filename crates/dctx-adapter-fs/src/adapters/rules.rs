//! Loads built-in, global, and project rule files.
//! Implements DX-B3 rule loading: `.dctxignore`, global rules, and `.gitignore` as plain text.
//! Depends on [`crate::ports::SourceFs`]. [`super::source`] calls this before the walk.
//! Global directories are caller-supplied. This adapter does not discover a home directory.

use super::error::SourceError;
use crate::domain::{Rule, SelectionPolicy, builtin_ignore, builtin_omit, parse_rule};
use crate::ports::SourceFs;
use std::{io, path::Path};

const IGNORE_FILES: [&str; 3] = [".gitignore", ".dctxignore", ".legacyignore"];
const OMIT_FILE: &str = ".dctxomitcontent";

pub(crate) fn load_policy<F: SourceFs>(
    root: &Path,
    global_dir: Option<&Path>,
    fs: &F,
) -> Result<SelectionPolicy, SourceError> {
    let mut ignore = builtin_ignore();
    let mut omit = builtin_omit();
    if let Some(dir) = global_dir {
        append_rules(fs, &dir.join("ignore"), &mut ignore)?;
        append_rules(fs, &dir.join("omitcontent"), &mut omit)?;
    }
    for name in IGNORE_FILES {
        append_rules(fs, &root.join(name), &mut ignore)?;
    }
    append_rules(fs, &root.join(OMIT_FILE), &mut omit)?;
    Ok(SelectionPolicy::from_rules(ignore, omit))
}

fn append_rules<F: SourceFs>(
    fs: &F,
    path: &Path,
    rules: &mut Vec<Rule>,
) -> Result<(), SourceError> {
    let Some(text) = read_rule(fs, path)? else {
        return Ok(());
    };
    for line in text.lines() {
        if let Some(rule) = parse_rule(line) {
            rules.push(rule);
        }
    }
    Ok(())
}

fn read_rule<F: SourceFs>(fs: &F, path: &Path) -> Result<Option<String>, SourceError> {
    match fs.read_policy(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SourceError::Rule {
            path: path.to_path_buf(),
            source: error,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FileHandlingDecision, ItemContext};
    use crate::ports::{DirEntry, EntryMeta, SourceFs};
    use std::{io, path::PathBuf};

    struct PolicyFs {
        fail: Option<PathBuf>,
        text: Vec<(PathBuf, String)>,
    }

    impl SourceFs for PolicyFs {
        fn metadata(&self, _: &Path) -> io::Result<EntryMeta> {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
        fn read_dir(&self, _: &Path) -> io::Result<Vec<DirEntry>> {
            Ok(Vec::new())
        }
        fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
            Err(io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
        }
        fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
            Err(io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
        }
        fn read_policy(&self, path: &Path) -> io::Result<Option<String>> {
            if self.fail.as_deref() == Some(path) {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
            }
            Ok(self
                .text
                .iter()
                .find(|(candidate, _)| candidate == path)
                .map(|(_, text)| text.clone()))
        }
    }

    #[test]
    fn project_and_global_lines_apply_in_order_and_unreadable_rules_refuse() {
        let root = Path::new("/proj");
        let global = Path::new("/cfg");
        let fs = PolicyFs {
            fail: None,
            text: vec![
                (global.join("ignore"), "*.log\n".to_string()),
                (root.join(".gitignore"), "!keep.log\n".to_string()),
                (root.join(".dctxignore"), "secret.txt\n".to_string()),
                (root.join(".dctxomitcontent"), "*.omit\n".to_string()),
            ],
        };
        let policy = load_policy(root, Some(global), &fs).unwrap();
        let decide = |name: &str| {
            policy.decide(&ItemContext {
                relative_path: name,
                name,
                is_directory: false,
                debug_mode: false,
            })
        };
        assert_eq!(decide("debug.log"), FileHandlingDecision::Exclude);
        assert_eq!(decide("keep.log"), FileHandlingDecision::Include);
        assert_eq!(decide("secret.txt"), FileHandlingDecision::Exclude);
        assert_eq!(decide("note.omit"), FileHandlingDecision::OmitContent);
        let denied = PolicyFs {
            fail: Some(root.join(".gitignore")),
            text: Vec::new(),
        };
        let error = load_policy(root, None, &denied).unwrap_err();
        match error {
            SourceError::Rule { path, source } => {
                assert_eq!(path, root.join(".gitignore"));
                assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
            }
            other => panic!("expected rule failure, got {other}"),
        }
    }
}
