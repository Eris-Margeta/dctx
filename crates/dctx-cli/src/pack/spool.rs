// SPDX-License-Identifier: Apache-2.0
//! Anonymous source-body spill. This is never a cache object or an output artifact.
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use dctx::Source as _;
use dctx_core::facts::EntryMeta;
use dctx_core::ids::RelativePath;

type Offsets = HashMap<String, (u64, usize)>;

pub(super) struct Spool {
    writer: BufWriter<File>,
    offsets: Offsets,
    position: u64,
    hidden: Option<RelativePath>,
    frozen_parent: Option<(RelativePath, EntryMeta)>,
    // Declared last so Windows closes the delete-on-close file before removing its directory.
    directory: tempfile::TempDir,
}

impl Spool {
    pub(super) fn new(root: &Path) -> io::Result<Self> {
        let temp_root = std::fs::canonicalize(std::env::temp_dir())?;
        let source_root = std::fs::canonicalize(root).ok();
        // On Windows an anonymous tempfile still has a name until its handle closes.
        // Keep it in one known directory, excluding that directory from the source view.
        // Freeze the original parent's facts so our temporary entry cannot alter MOD stamps.
        let frozen_parent = source_root.as_ref().and_then(|source_root| {
            let relative = relative_to(source_root, &temp_root)?;
            let metadata = dctx::FsSource::open(root).ok()?.metadata(&relative).ok()?;
            Some((relative, metadata))
        });
        let directory = tempfile::Builder::new()
            .prefix("dctx-pack-")
            .tempdir_in(&temp_root)?;
        let hidden = source_root
            .as_ref()
            .and_then(|root| relative_to(root, directory.path()));
        let file = tempfile::tempfile_in(directory.path())?;
        Ok(Self {
            writer: BufWriter::with_capacity(64 * 1024, file),
            offsets: HashMap::new(),
            position: 0,
            hidden,
            frozen_parent,
            directory,
        })
    }
    pub(super) fn hidden(&self) -> Option<&RelativePath> {
        self.hidden.as_ref()
    }
    pub(super) fn frozen_parent(&self) -> Option<&(RelativePath, EntryMeta)> {
        self.frozen_parent.as_ref()
    }
    pub(super) fn push(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.offsets
            .insert(path.to_string(), (self.position, bytes.len()));
        self.position += bytes.len() as u64;
        Ok(())
    }
    pub(super) fn finish(self) -> io::Result<Reader> {
        let file = self
            .writer
            .into_inner()
            .map_err(std::io::IntoInnerError::into_error)?;
        Ok(Reader {
            file: RefCell::new(file),
            offsets: self.offsets,
            _directory: self.directory,
        })
    }
}

pub(super) struct Reader {
    file: RefCell<File>,
    offsets: Offsets,
    _directory: tempfile::TempDir,
}

impl Reader {
    pub(super) fn load(&self, node: &dctx_template::MarkerNode) -> io::Result<Option<Vec<u8>>> {
        let Some(&(offset, len)) = self.offsets.get(&node.relative_path) else {
            return Ok(None);
        };
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; len];
        file.read_exact(&mut bytes)?;
        Ok(Some(bytes))
    }
}

fn relative_to(root: &Path, path: &Path) -> Option<RelativePath> {
    let suffix = path.strip_prefix(root).ok()?;
    if suffix.as_os_str().is_empty() {
        return Some(RelativePath::root());
    }
    let text = suffix.to_str()?;
    let text = if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.to_string()
    };
    RelativePath::parse(&text).ok()
}
