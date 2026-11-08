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
    verify_peer(&stream)?;
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
        private_directory(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
        use std::os::unix::fs::OpenOptionsExt;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path.with_extension("lock"))?;
        let metadata = lock.metadata()?;
        // SAFETY: geteuid takes no arguments and does not access Rust memory.
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unsafe daemon lock",
            ));
        }
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
        verify_peer(&stream)?;
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

fn require_uid(found: libc::uid_t, expected: libc::uid_t) -> io::Result<()> {
    if found != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "daemon peer is not the current user",
        ));
    }
    Ok(())
}

fn verify_peer(stream: &UnixStream) -> io::Result<()> {
    let uid = peer_uid(stream)?;
    // SAFETY: geteuid takes no arguments and does not access Rust memory.
    require_uid(uid, unsafe { libc::geteuid() })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_uid(stream: &UnixStream) -> io::Result<libc::uid_t> {
    use std::os::fd::AsRawFd;
    // SAFETY: ucred is a plain C structure whose all-zero representation is valid.
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: stream owns a live socket; credentials and length are valid output buffers.
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::from_mut(&mut credentials).cast(),
            &mut length,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(credentials.uid)
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn peer_uid(stream: &UnixStream) -> io::Result<libc::uid_t> {
    use std::os::fd::AsRawFd;
    let mut uid = 0;
    let mut gid = 0;
    // SAFETY: stream owns a live socket; uid and gid are valid output pointers.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
fn peer_uid(_stream: &UnixStream) -> io::Result<libc::uid_t> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(all(
    test,
    any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )
))]
mod tests {
    use super::*;

    #[test]
    fn kernel_peer_credentials_are_checked_and_other_users_are_refused() {
        let (a, b) = UnixStream::pair().unwrap();
        verify_peer(&a).unwrap();
        verify_peer(&b).unwrap();
        assert_eq!(
            require_uid(123, 124).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn daemon_directory_socket_and_lock_must_be_private() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("daemon");
        private_directory(&directory).unwrap();
        assert_eq!(fs::metadata(&directory).unwrap().mode() & 0o777, 0o700);
        let path = directory.join("socket");
        let mut listener = Listener::bind(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        let client = connect(&path, Duration::from_secs(1)).unwrap();
        let server = listener.accept(Duration::from_secs(1)).unwrap();
        drop((client, server));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        assert_eq!(
            connect(&path, Duration::from_secs(1)).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        drop(listener);
        fs::set_permissions(
            directory.join("socket.lock"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(Listener::bind(&path).is_err());
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(private_directory(&directory).is_err());
        assert!(Listener::bind(&path).is_err());
    }
}
