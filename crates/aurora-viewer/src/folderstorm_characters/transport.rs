//! The private endpoint a managed viewer reports to its session host on
//! (session-host plan, section 8; the messages and their framing are
//! `folderstorm_session::ipc`). One endpoint per launch of a viewer, made
//! by the host, one-way (viewer to host), for one client. The other
//! direction is the viewer's standard input, which needs nothing here.
//!
//! Nothing of it leaves the PC or the user's logon:
//! - **Windows**: a named pipe `\\.\pipe\folderstorm2-session-<session>-<host
//!   pid>-<slot>-<random>`, made through the `secondlife://` hand-over's
//!   own code (`slurl_handoff/windows.rs`): inbound, first instance only (a
//!   name someone else made first is an error, never joined), remote
//!   clients refused, write access for this logon alone. The host accepts
//!   only the process it started (`GetNamedPipeClientProcessId`); the
//!   viewer writes only to a pipe served by the host that started it
//!   (`GetNamedPipeServerProcessId`).
//! - **Elsewhere**: a socket in a folder made for it with access for the
//!   user alone (mode 0700), in `$XDG_RUNTIME_DIR` or the system's
//!   temporary folder. On Linux the host also checks that the client is
//!   the user's process it started (`SO_PEERCRED`).
//!
//! Past the endpoint, the host still asks for the launch's one-time token
//! before it takes anything a client says (`host/process.rs`).

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// How many clients that are not the viewer are turned away before the
/// host gives the endpoint up.
const STRANGERS: usize = 8;

/// Random hexadecimal digits of an endpoint's name (48 bits: the name is
/// not what keeps others out, and a socket path is short).
const NAME_DIGITS: usize = 12;

/// Fresh random hexadecimal digits from the system (two version 4 UUIDs).
pub fn random_hex(digits: usize) -> String {
    let mut hex = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    hex.truncate(digits);
    hex
}

/// The host's end, before the viewer connected.
pub struct Listener {
    inner: imp::Listener,
    endpoint: String,
}

impl Listener {
    /// Makes the endpoint of one launch of the viewer in `slot`.
    pub fn bind(slot: u8) -> io::Result<Self> {
        let (inner, endpoint) = imp::bind(slot, &random_hex(NAME_DIGITS))?;
        Ok(Self { inner, endpoint })
    }

    /// What the bootstrap tells the viewer.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Waits for the viewer, the process `pid`, and gives what it writes.
    /// Blocks until a client comes: [`unblock`] ends the wait once
    /// `cancelled` is set.
    pub fn accept(self, pid: u32, cancelled: &AtomicBool) -> io::Result<Box<dyn Read + Send>> {
        self.inner.accept(pid, cancelled)
    }
}

/// Ends an [`Listener::accept`] whose viewer will not come (it ended, or
/// the host closes): a client that is turned away.
pub fn unblock(endpoint: &str) {
    drop(imp::open(endpoint));
}

/// The viewer's end: opens the endpoint its host named, if it has the
/// shape of one and is served by that host.
pub fn connect(endpoint: &str, host_pid: u32) -> io::Result<Box<dyn Write + Send>> {
    if !imp::is_endpoint(endpoint) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a session endpoint"));
    }
    imp::connect(endpoint, host_pid)
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, "the viewer did not come")
}

fn strangers_error() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "only clients that are not the viewer came")
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};

    /// The socket's name inside its own folder.
    const SOCKET: &str = "s";

    pub struct Listener {
        socket: UnixListener,
        /// The folder made for the socket, removed with it.
        dir: PathBuf,
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.dir.join(SOCKET));
            let _ = std::fs::remove_dir(&self.dir);
        }
    }

    /// The longest socket path taken: under every system's limit (104
    /// bytes on macOS, 108 on Linux, the terminator included).
    const MAX_PATH: usize = 100;

    /// Where the endpoint's folder is made: the user's runtime folder
    /// (already theirs alone), or the temporary folder when there is none
    /// or its path leaves no room for a socket's.
    fn folder(name: &str) -> io::Result<PathBuf> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute() && dir.is_dir());
        runtime
            .into_iter()
            .chain([std::env::temp_dir()])
            .map(|base| base.join(name))
            .find(|dir| dir.join(SOCKET).as_os_str().len() <= MAX_PATH)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder short enough for a socket"))
    }

    pub fn bind(slot: u8, random: &str) -> io::Result<(Listener, String)> {
        let name = ::folderstorm_session::ipc::endpoint_name(0, std::process::id(), slot, random)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "endpoint name"))?;
        let dir = folder(&name)?;
        // made new and for the user alone in one step: a folder already
        // there (someone else's) is an error, never used
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let path = dir.join(SOCKET);
        let made = UnixListener::bind(&path).and_then(|socket| {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            Ok(socket)
        });
        match made {
            Ok(socket) => {
                let endpoint = path.to_string_lossy().into_owned();
                Ok((Listener { socket, dir }, endpoint))
            }
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_dir(&dir);
                Err(e)
            }
        }
    }

    impl Listener {
        pub fn accept(self, pid: u32, cancelled: &AtomicBool) -> io::Result<Box<dyn Read + Send>> {
            for _ in 0..STRANGERS {
                let (stream, _) = self.socket.accept()?;
                if cancelled.load(Ordering::Acquire) {
                    return Err(cancelled_error());
                }
                if is_viewer(&stream, pid) {
                    // one-way: nothing is ever written to the viewer here
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    return Ok(Box::new(stream));
                }
                log::warn!("session host: a client that is not the viewer was turned away");
            }
            Err(strangers_error())
        }
    }

    /// Whether the client is the user's process `pid` (Linux; elsewhere the
    /// folder's permissions alone keep other users out).
    #[cfg(target_os = "linux")]
    fn is_viewer(stream: &UnixStream, pid: u32) -> bool {
        use std::os::fd::AsRawFd;
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: a live socket, and an output buffer and length of exactly
        // one `ucred`, both valid for the call.
        let ok = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut cred).cast(),
                &raw mut len,
            )
        } == 0;
        // SAFETY: no argument, cannot fail.
        let me = unsafe { libc::geteuid() };
        ok && cred.uid == me && u32::try_from(cred.pid).is_ok_and(|peer| peer == pid)
    }

    #[cfg(not(target_os = "linux"))]
    fn is_viewer(_stream: &UnixStream, _pid: u32) -> bool {
        true
    }

    pub fn open(endpoint: &str) -> io::Result<UnixStream> {
        UnixStream::connect(endpoint)
    }

    /// `<folder>/folderstorm2-session-…/s`, absolute.
    pub fn is_endpoint(endpoint: &str) -> bool {
        let path = Path::new(endpoint);
        path.is_absolute()
            && path.file_name().is_some_and(|n| n == SOCKET)
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n.to_string_lossy().starts_with(folderstorm_brand::SESSION_PIPE_PREFIX))
    }

    pub fn connect(endpoint: &str, _host_pid: u32) -> io::Result<Box<dyn Write + Send>> {
        let stream = open(endpoint)?;
        // one-way: nothing is ever read from the host here
        let _ = stream.shutdown(std::net::Shutdown::Read);
        Ok(Box::new(stream))
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::io::{AsRawHandle, OwnedHandle};

    const PIPES: &str = r"\\.\pipe\";

    pub struct Listener {
        pipe: OwnedHandle,
    }

    pub fn bind(slot: u8, random: &str) -> io::Result<(Listener, String)> {
        use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
        let pid = std::process::id();
        let mut session = 0u32;
        // SAFETY: a valid pointer to a u32 the call fills; on failure it stays 0.
        unsafe {
            ProcessIdToSessionId(pid, &mut session);
        }
        let name = ::folderstorm_session::ipc::endpoint_name(session, pid, slot, random)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "endpoint name"))?;
        let endpoint = format!("{PIPES}{name}");
        let pipe = crate::slurl_handoff::folderstorm_session_pipe(&endpoint)?;
        Ok((Listener { pipe }, endpoint))
    }

    impl Listener {
        pub fn accept(self, pid: u32, cancelled: &AtomicBool) -> io::Result<Box<dyn Read + Send>> {
            use windows_sys::Win32::Foundation::{ERROR_NO_DATA, ERROR_PIPE_CONNECTED, GetLastError};
            use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, DisconnectNamedPipe, GetNamedPipeClientProcessId};
            let handle = self.pipe.as_raw_handle();
            for _ in 0..STRANGERS {
                // SAFETY: a live, synchronous server handle owned by `self`.
                if unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) } == 0 {
                    // SAFETY: this thread's error, read right after the call.
                    let error = unsafe { GetLastError() };
                    // a client that connected before this call is connected;
                    // one that also closed already (ERROR_NO_DATA) is
                    // checked and turned away below like any other
                    if !matches!(error, ERROR_PIPE_CONNECTED | ERROR_NO_DATA) {
                        return Err(io::Error::from_raw_os_error(error as i32));
                    }
                }
                if cancelled.load(Ordering::Acquire) {
                    return Err(cancelled_error());
                }
                let mut client = 0u32;
                // SAFETY: the same live handle and a valid output pointer.
                let known = unsafe { GetNamedPipeClientProcessId(handle, &mut client) } != 0;
                if known && client == pid {
                    return Ok(Box::new(std::fs::File::from(self.pipe)));
                }
                log::warn!("session host: a client that is not the viewer was turned away");
                // SAFETY: the same live handle; ends that client's connection.
                unsafe { DisconnectNamedPipe(handle) };
            }
            Err(strangers_error())
        }
    }

    /// How long a client waits, once, for the pipe's one instance to be
    /// free (another client is being turned away).
    const BUSY_WAIT_MS: u32 = 2000;

    pub fn open(endpoint: &str) -> io::Result<std::fs::File> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
        use windows_sys::Win32::Storage::FileSystem::{FILE_READ_ATTRIBUTES, FILE_WRITE_DATA, SECURITY_IDENTIFICATION, SYNCHRONIZE};
        use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
        let open = || {
            std::fs::OpenOptions::new()
                // what the pipe's security gives this logon, no more
                .access_mode(FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
                // whoever serves the pipe cannot act as this process
                .security_qos_flags(SECURITY_IDENTIFICATION)
                .open(endpoint)
        };
        match open() {
            // the one instance is connected to another client (one the
            // host is turning away): wait for it once, then try again
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                let name: Vec<u16> = endpoint.encode_utf16().chain(std::iter::once(0)).collect();
                // SAFETY: `name` is a NUL-terminated UTF-16 string that
                // lives through the call; the call keeps no pointer.
                let free = unsafe { WaitNamedPipeW(name.as_ptr(), BUSY_WAIT_MS) } != 0;
                if free { open() } else { Err(e) }
            }
            other => other,
        }
    }

    /// `\\.\pipe\folderstorm2-session-…`, a local pipe and nothing else (no
    /// other computer, no file).
    pub fn is_endpoint(endpoint: &str) -> bool {
        endpoint.strip_prefix(PIPES).is_some_and(|name| {
            name.starts_with(folderstorm_brand::SESSION_PIPE_PREFIX) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    }

    pub fn connect(endpoint: &str, host_pid: u32) -> io::Result<Box<dyn Write + Send>> {
        use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
        let pipe = open(endpoint)?;
        let mut server = 0u32;
        // SAFETY: a live client pipe handle and a valid output pointer.
        if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut server) } == 0 || server != host_pid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the endpoint is not served by the host",
            ));
        }
        Ok(Box::new(pipe))
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use super::*;

    pub struct Listener;

    fn unsupported<T>() -> io::Result<T> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub fn bind(_slot: u8, _random: &str) -> io::Result<(Listener, String)> {
        unsupported()
    }

    impl Listener {
        pub fn accept(self, _pid: u32, _cancelled: &AtomicBool) -> io::Result<Box<dyn Read + Send>> {
            unsupported()
        }
    }

    pub fn open(_endpoint: &str) -> io::Result<std::fs::File> {
        unsupported()
    }

    pub fn is_endpoint(_endpoint: &str) -> bool {
        false
    }

    pub fn connect(_endpoint: &str, _host_pid: u32) -> io::Result<Box<dyn Write + Send>> {
        unsupported()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use ::folderstorm_session::ipc::{ToHost, read_frame, write_frame};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    #[test]
    fn the_endpoint_is_the_user_s_alone_and_removed_after_use() {
        let listener = Listener::bind(3).expect("bound");
        let path = Path::new(listener.endpoint()).to_owned();
        let dir = path.parent().expect("folder").to_owned();
        assert!(imp::is_endpoint(listener.endpoint()));
        let mode = |p: &Path| std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&path), 0o600);
        drop(listener);
        assert!(!dir.exists());
    }

    #[test]
    fn only_endpoints_of_the_right_shape_are_opened() {
        for bad in [
            "",
            "s",
            "/etc/passwd",
            "/tmp/s",
            "/tmp/other-session-1/s",
            "relative/folderstorm2-session-0-1-0-ab/s",
            "/tmp/folderstorm2-session-0-1-0-ab/t",
        ] {
            assert!(connect(bad, 1).is_err(), "{bad}");
        }
    }

    #[test]
    fn reports_cross_the_real_socket_from_the_viewer_s_process_only() {
        let listener = Listener::bind(0).expect("bound");
        let endpoint = listener.endpoint().to_owned();
        let cancelled = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let host = scope.spawn(|| {
                // the "viewer" is this very process
                let mut from_viewer = listener.accept(std::process::id(), &cancelled).expect("accepted");
                let first: ToHost = read_frame(&mut from_viewer).expect("first frame");
                let second: ToHost = read_frame(&mut from_viewer).expect("second frame");
                // the viewer ended: the link says closed, between frames
                let end = read_frame::<ToHost>(&mut from_viewer);
                (first, second, matches!(end, Err(::folderstorm_session::ipc::FrameError::Closed)))
            });
            let mut to_host = connect(&endpoint, std::process::id()).expect("connected");
            write_frame(&mut to_host, &ToHost::Heartbeat { loop_count: 1 }).expect("written");
            write_frame(&mut to_host, &ToHost::Heartbeat { loop_count: 2 }).expect("written");
            drop(to_host);
            let (first, second, closed) = host.join().expect("host thread");
            assert_eq!(first, ToHost::Heartbeat { loop_count: 1 });
            assert_eq!(second, ToHost::Heartbeat { loop_count: 2 });
            assert!(closed);
        });
        assert!(!Path::new(&endpoint).exists(), "the socket is removed once the viewer is connected");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_client_that_is_not_the_viewer_is_turned_away() {
        let listener = Listener::bind(1).expect("bound");
        let endpoint = listener.endpoint().to_owned();
        let cancelled = AtomicBool::new(false);
        std::thread::scope(|scope| {
            // the host waits for a process that is not this one
            let host = scope.spawn(|| listener.accept(std::process::id().wrapping_add(1), &cancelled).is_err());
            for _ in 0..STRANGERS {
                let Ok(mut stranger) = connect(&endpoint, 0) else {
                    break;
                };
                let _ = write_frame(&mut stranger, &ToHost::Heartbeat { loop_count: 9 });
            }
            assert!(host.join().expect("host thread"), "nothing a stranger wrote was read");
        });
    }

    #[test]
    fn a_viewer_that_never_comes_does_not_hold_the_host() {
        let listener = Listener::bind(2).expect("bound");
        let endpoint = listener.endpoint().to_owned();
        let cancelled = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let host = scope.spawn(|| listener.accept(std::process::id(), &cancelled).err().map(|e| e.kind()));
            cancelled.store(true, Ordering::Release);
            unblock(&endpoint);
            assert_eq!(host.join().expect("host thread"), Some(io::ErrorKind::ConnectionAborted));
        });
    }

    #[test]
    fn random_names_are_hexadecimal_and_differ() {
        let (a, b) = (random_hex(64), random_hex(64));
        assert!(::folderstorm_session::ipc::token_shaped(&a));
        assert_ne!(a, b);
        assert_eq!(random_hex(NAME_DIGITS).len(), NAME_DIGITS);
    }
}
