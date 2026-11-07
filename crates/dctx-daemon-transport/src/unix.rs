// SPDX-License-Identifier: Apache-2.0
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Connected local byte stream.
pub type Stream = UnixStream;

/// Creates or verifies a directory owned by this user with mode 0700.
pub fn private_directory(path: &Path) -> io::Result<()> {
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let meta = fs::symlink_metadata(path)?;
    // SAFETY: geteuid takes no arguments and does not access Rust memory.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "daemon directory must be user-owned and mode 0700",
        ));
    }
    Ok(())
}

/// Unix socket endpoint inside the private directory.
pub fn endpoint(directory: &Path, _identity: &str) -> PathBuf {
    directory.join("socket")
}

/// Connects with bounded reads and writes.
pub fn connect(path: &Path, timeout: Duration) -> io::Result<Stream> {
    let meta = fs::symlink_metadata(path)?;
    // SAFETY: geteuid takes no arguments and does not access Rust memory.
    if !meta.file_type().is_socket()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe daemon socket",
        ));
    }
    let stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    Ok(stream)
}

/// Sole owner of the socket name. Its OS file lock survives stale socket files and crashes.
pub struct Listener {
    listener: UnixListener,
    path: PathBuf,
    _lock: fs::File,
}

impl Listener {
    /// Claims the endpoint. Another live daemon prevents binding; stale sockets are recovered.
    pub fn bind(path: &Path) -> io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path.with_extension("lock"))?;
        lock.try_lock()
            .map_err(|error| io::Error::new(io::ErrorKind::AddrInUse, error))?;
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_socket() => fs::remove_file(path)?,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "daemon endpoint is not a socket",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            path: path.to_owned(),
            _lock: lock,
        })
    }

    /// Accepts without blocking. A stream has bounded frame reads and writes.
    pub fn accept(&mut self, timeout: Duration) -> io::Result<Stream> {
        let (stream, _) = self.listener.accept()?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        Ok(stream)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Changes the read/write deadline after the handshake.
pub fn set_timeout(stream: &mut Stream, timeout: Duration) -> io::Result<()> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))
}
