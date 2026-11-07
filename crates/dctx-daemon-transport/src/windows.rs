// SPDX-License-Identifier: Apache-2.0
//! Synchronous named pipes with polling deadlines and an explicit current-user DACL.
use std::ffi::c_void;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Threading::*;

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: the pointer was allocated by a Win32 LocalAlloc-family API.
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// SID of the process token user. The aligned buffer owns the SID storage.
fn token_user() -> io::Result<Vec<usize>> {
    // SAFETY: all output pointers are writable and buffers stay alive across calls.
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = File::from_raw_handle(token);
        let mut needed = 0;
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut needed);
        let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(buffer)
    }
}

fn sid_text() -> io::Result<String> {
    let user = token_user()?;
    // SAFETY: token_user returned an aligned TOKEN_USER containing a valid SID.
    unsafe {
        let sid = (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid;
        let mut text = null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let allocation = Local(text.cast());
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        drop(allocation);
        Ok(result)
    }
}

fn descriptor() -> io::Result<Local> {
    let sid = sid_text()?;
    // Protected DACL: only this exact user, including when launched elevated.
    let text = wide(std::ffi::OsStr::new(&format!("O:{sid}D:P(A;;GA;;;{sid})")));
    let mut result = null_mut();
    // SAFETY: text is NUL-terminated and result is a writable output pointer.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut result,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(Local(result))
}

/// Creates a current-user-only directory, refusing symlinks and non-directories.
pub fn private_directory(path: &Path) -> io::Result<()> {
    let security = descriptor()?;
    let attrs = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    // SAFETY: path and security descriptor remain valid during this call.
    if unsafe { CreateDirectoryW(wide(path.as_os_str()).as_ptr(), &attrs) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32) {
            return Err(error);
        }
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe daemon directory",
        ));
    }
    Ok(())
}

/// Local machine pipe name, partitioned by user and cache identity.
pub fn endpoint(_directory: &Path, identity: &str) -> PathBuf {
    // Failure makes connection/binding unavailable, which the CLI handles by falling back.
    PathBuf::from(format!(
        r"\\.\pipe\dctx-{}-{identity}",
        sid_text().unwrap_or_default()
    ))
}

/// Connected pipe stream. I/O is bounded and never impersonates clients.
pub struct Stream {
    file: Arc<File>,
    timeout: Duration,
    active: Option<Arc<AtomicBool>>,
}

fn poll<T>(timeout: Duration, mut action: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let deadline = Instant::now() + timeout;
    loop {
        match action() {
            Err(error) if matches!(error.raw_os_error(), Some(x) if x == ERROR_NO_DATA as i32 || x == ERROR_PIPE_BUSY as i32) =>
            {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "daemon pipe timed out",
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

impl Read for Stream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        poll(self.timeout, || (&*self.file).read(bytes))
    }
}
impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        poll(self.timeout, || match (&*self.file).write(bytes) {
            // A nonblocking byte pipe can succeed without progress when its buffer is full.
            Ok(0) => Err(io::Error::from_raw_os_error(ERROR_NO_DATA as i32)),
            result => result,
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Connects only to a locally created pipe owned by this user, without impersonation rights.
pub fn connect(path: &Path, timeout: Duration) -> io::Result<Stream> {
    // SAFETY: NUL-terminated path and null optional parameters are valid; returned handle is owned.
    let handle = unsafe {
        CreateFileW(
            wide(path.as_os_str()).as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | READ_CONTROL,
            0,
            null(),
            OPEN_EXISTING,
            SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful CreateFileW returns a unique owned handle.
    let file = unsafe { File::from_raw_handle(handle) };
    let user = token_user()?;
    let mut owner = null_mut();
    let mut security = null_mut();
    // SAFETY: handle is live and all output pointers are writable.
    let result = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut security,
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let _security = Local(security);
    // SAFETY: both SIDs are valid while their owning allocations are live.
    if unsafe { EqualSid(owner, (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid) } == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "daemon pipe owner differs",
        ));
    }
    let mode = PIPE_READMODE_BYTE | PIPE_NOWAIT;
    // SAFETY: handle is a pipe and mode is a valid pointer to mode flags.
    if unsafe { SetNamedPipeHandleState(handle, &mode, null(), null()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Stream {
        file: Arc::new(file),
        timeout,
        active: None,
    })
}

/// One first-instance, local-only pipe listener with a protected user-only DACL.
pub struct Listener {
    pipe: Arc<File>,
    active: Arc<AtomicBool>,
}
impl Listener {
    /// Claims the pipe name, refusing a competing server.
    pub fn bind(path: &Path) -> io::Result<Self> {
        let security = descriptor()?;
        let attrs = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security.0,
            bInheritHandle: 0,
        };
        // SAFETY: all buffers/descriptors remain valid during the call; no handle inheritance.
        let handle = unsafe {
            CreateNamedPipeW(
                wide(path.as_os_str()).as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                65536,
                0,
                &attrs,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateNamedPipeW returned a unique owned handle.
        let pipe = Arc::new(unsafe { File::from_raw_handle(handle) });
        Ok(Self {
            pipe,
            active: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Accepts without blocking; ownership of the first pipe instance persists between clients.
    pub fn accept(&mut self, timeout: Duration) -> io::Result<Stream> {
        if self.active.load(Ordering::Acquire) {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        // SAFETY: live pipe handle in nonblocking mode, with no overlapped operation.
        if unsafe { ConnectNamedPipe(self.pipe.as_raw_handle(), null_mut()) } == 0 {
            let error = io::Error::last_os_error();
            match error.raw_os_error().map(|code| code as u32) {
                Some(ERROR_PIPE_CONNECTED) => {}
                Some(ERROR_PIPE_LISTENING) => return Err(io::ErrorKind::WouldBlock.into()),
                Some(ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED) => {
                    // A client disconnected before accept. Reset the existing instance;
                    // closing/recreating it would expose a listener ownership race.
                    // SAFETY: the handle belongs to this server's live pipe instance.
                    unsafe {
                        DisconnectNamedPipe(self.pipe.as_raw_handle());
                    }
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                _ => return Err(error),
            }
        }
        self.active.store(true, Ordering::Release);
        Ok(Stream {
            file: Arc::clone(&self.pipe),
            timeout,
            active: Some(Arc::clone(&self.active)),
        })
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            // The protocol acknowledges the full response before this point. Disconnect
            // without closing the listener handle, even while the old client is closing.
            // SAFETY: this stream owns the sole accepted connection to the live pipe.
            unsafe {
                DisconnectNamedPipe(self.file.as_raw_handle());
            }
            active.store(false, Ordering::Release);
        }
    }
}

/// Changes the read/write deadline after the handshake.
pub fn set_timeout(stream: &mut Stream, timeout: Duration) -> io::Result<()> {
    stream.timeout = timeout;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_descriptor_grants_only_the_process_token_user() {
        let descriptor = descriptor().expect("descriptor");
        let user = token_user().expect("token");
        // SAFETY: descriptors, aligned token buffer and ACE outputs remain valid throughout.
        unsafe {
            let mut present = 0;
            let mut defaulted = 0;
            let mut acl = null_mut();
            assert_ne!(
                GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted),
                0
            );
            assert_ne!(present, 0);
            assert!(!acl.is_null());
            assert_eq!((*acl).AceCount, 1);
            let mut ace = null_mut();
            assert_ne!(GetAce(acl, 0, &mut ace), 0);
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            assert_eq!(ace.Header.AceType, 0); // ACCESS_ALLOWED_ACE_TYPE
            assert_eq!(ace.Mask, GENERIC_ALL);
            assert_ne!(
                EqualSid(
                    std::ptr::addr_of!(ace.SidStart).cast_mut().cast(),
                    (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid
                ),
                0
            );
            let mut control = 0;
            let mut revision = 0;
            assert_ne!(
                GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision),
                0
            );
            assert_ne!(control & SE_DACL_PROTECTED, 0);
        }
    }

    #[test]
    fn pipe_roundtrip_exercises_full_buffers_and_exclusive_listener() {
        let identity = format!(
            "test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let path = endpoint(Path::new(""), &identity);
        let mut listener = Listener::bind(&path).expect("listen");
        assert!(Listener::bind(&path).is_err());
        let server_path = path.clone();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept(Duration::from_secs(3)) {
                        Ok(stream) => break stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline);
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("accept: {error}"),
                    }
                };
                assert!(Listener::bind(&server_path).is_err());
                assert!(listener.accept(Duration::from_secs(1)).is_err());
                let mut ping = [0; 4];
                stream.read_exact(&mut ping).expect("ping");
                assert_eq!(&ping, b"ping");
                stream
                    .write_all(&vec![b'x'; 256 * 1024])
                    .expect("large response");
                let mut ack = [0];
                stream.read_exact(&mut ack).expect("ack");
                assert_eq!(ack, [1]);
                drop(stream);
                // Name ownership persists even while the disconnected client retains its handle.
                assert!(Listener::bind(&server_path).is_err());
            }
        });
        let mut old_clients = Vec::new();
        for _ in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut client = loop {
                match connect(&path, Duration::from_secs(3)) {
                    Ok(client) => break client,
                    Err(error) => {
                        assert!(Instant::now() < deadline, "connect: {error}");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            };
            client.write_all(b"ping").expect("request");
            let mut response = vec![0; 256 * 1024];
            client.read_exact(&mut response).expect("response");
            assert!(response.iter().all(|byte| *byte == b'x'));
            client.write_all(&[1]).expect("ack");
            old_clients.push(client);
        }
        server.join().expect("server");
    }
}
