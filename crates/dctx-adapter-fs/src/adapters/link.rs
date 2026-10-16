//! Symlink classification against the source root.
//! Implements DX-B3 escape refusal before a content read.
//! Depends on [`crate::domain::confine`] and [`crate::ports`]. Walk and lookup both use it.

use super::error::SourceError;
use crate::domain::{is_inside, join_lexical};
use crate::ports::SourceFs;
use std::path::{Path, PathBuf};

pub(crate) enum LinkTarget {
    Outside { path: PathBuf, target: PathBuf },
    Inside,
}

pub(crate) fn classify_link<F: SourceFs>(
    root: &Path,
    link: &Path,
    fs: &F,
) -> Result<LinkTarget, SourceError> {
    let linked = fs.read_link(link).map_err(|source| SourceError::io(link, source))?;
    let parent = link.parent().unwrap_or(link);
    // The name `raw` is rejected here: `&raw` is a Rust 2024 raw-borrow prefix.
    let resolved = join_lexical(parent, &linked);
    if is_inside(root, &resolved) {
        Ok(LinkTarget::Inside)
    } else {
        Ok(LinkTarget::Outside {
            path: link.to_path_buf(),
            target: resolved,
        })
    }
}
