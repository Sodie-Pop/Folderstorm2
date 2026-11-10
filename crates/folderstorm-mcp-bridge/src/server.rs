//! The bridge's listener: 127.0.0.1 on a port the OS picks, a fresh token
//! and a discovery file each time it starts. Its threads authenticate a
//! call, check its permission class, then hand it to the viewer's frame
//! thread through a bounded channel ([`Pending`]) and wait for the answer
//! with a timeout. They never touch viewer state themselves.

use crate::discovery::{self, Discovery};
use crate::http::{self, HttpError};
use crate::policy::{Class, Level, Permissions};
use crate::protocol::{self, CallRequest, ErrorCode, Health, ToolError};
use crate::token::Token;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

/// Connections served at once; more are answered 503 and closed.
pub const MAX_CONNECTIONS: usize = 4;
/// Calls waiting for the frame thread at once.
pub const MAX_QUEUED: usize = 16;
/// Socket read and write timeout.
pub const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a call waits for the frame thread's answer by default.
pub const VIEWER_TIMEOUT: Duration = Duration::from_secs(10);
/// Entries of the recent activity kept for Preferences.
pub const ACTIVITY: usize = 20;
/// The header the sidecar's `--diagnose` sends, so that its probes do not
/// show as an assistant's activity.
pub const PROBE_HEADER: &str = "x-folderstorm-probe";

pub struct Config {
    /// Where the discovery file goes; None writes none (tests).
    pub discovery_dir: Option<PathBuf>,
    pub pid: u32,
    pub version: String,
    pub permissions: Permissions,
    pub viewer_timeout: Duration,
}

/// A call waiting for the viewer's answer.
pub struct Pending {
    pub tool: String,
    pub arguments: Value,
    reply: SyncSender<Result<Value, ToolError>>,
}

impl Pending {
    /// Answer the call; an answer after the timeout is dropped.
    pub fn answer(self, result: Result<Value, ToolError>) {
        let _ = self.reply.try_send(result);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Refused,
    Failed(ErrorCode),
}

/// One authenticated call, for "Recent activity": the tool and how it
/// ended, never its arguments or result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub at: SystemTime,
    pub tool: String,
    pub outcome: Outcome,
}

struct Shared {
    token: Token,
    port: u16,
    version: String,
    viewer_timeout: Duration,
    permissions: Mutex<Permissions>,
    stopping: AtomicBool,
    connections: AtomicUsize,
    activity: Mutex<VecDeque<Activity>>,
    queue: SyncSender<Pending>,
}

/// The running bridge. Dropping it stops the listener and deletes the
/// discovery file.
pub struct Bridge {
    shared: Arc<Shared>,
    discovery: Option<PathBuf>,
    accept: Option<JoinHandle<()>>,
}

impl Bridge {
    /// Bind 127.0.0.1 on a free port, make a token, write the discovery
    /// file and start listening. The receiver gives the calls to answer.
    pub fn start(config: Config) -> std::io::Result<(Bridge, Receiver<Pending>)> {
        let token = Token::generate()?;
        // loopback only, never 0.0.0.0 or ::
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let (queue, calls) = mpsc::sync_channel(MAX_QUEUED);
        let shared = Arc::new(Shared {
            token,
            port,
            version: config.version,
            viewer_timeout: config.viewer_timeout,
            permissions: Mutex::new(config.permissions),
            stopping: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
            activity: Mutex::new(VecDeque::new()),
            queue,
        });
        let discovery = match &config.discovery_dir {
            Some(dir) => Some(discovery::write(dir, &Discovery::new(config.pid, port, &shared.token))?),
            None => None,
        };
        let accepting = shared.clone();
        let accept = std::thread::Builder::new()
            .name("mcp-bridge-accept".into())
            .spawn(move || accept_loop(listener, accepting));
        let accept = match accept {
            Ok(handle) => handle,
            Err(e) => {
                if let Some(path) = &discovery {
                    let _ = discovery::remove(path);
                }
                return Err(e);
            }
        };
        log::info!("local assistant bridge listening on 127.0.0.1:{port}");
        Ok((
            Bridge {
                shared,
                discovery,
                accept: Some(accept),
            },
            calls,
        ))
    }

    pub fn port(&self) -> u16 {
        self.shared.port
    }

    pub fn discovery_path(&self) -> Option<&Path> {
        self.discovery.as_deref()
    }

    /// The token, for tests and the in-process client only.
    pub fn token(&self) -> &Token {
        &self.shared.token
    }

    pub fn set_permissions(&self, permissions: Permissions) {
        *self.shared.permissions.lock().unwrap_or_else(PoisonError::into_inner) = permissions;
    }

    pub fn permissions(&self) -> Permissions {
        *self.shared.permissions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The recent calls, newest last.
    pub fn activity(&self) -> Vec<Activity> {
        self.shared
            .activity
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// Stop listening and delete the discovery file (also on drop).
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        if let Some(path) = self.discovery.take()
            && let Err(e) = discovery::remove(&path)
        {
            log::warn!("local assistant bridge: discovery file not deleted: {e}");
        }
        if let Some(accept) = self.accept.take() {
            // wake the blocking accept
            let _ = TcpStream::connect_timeout(&SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.shared.port).into(), Duration::from_secs(1));
            if accept.join().is_err() {
                log::warn!("local assistant bridge: the listener thread panicked");
            }
            log::info!("local assistant bridge stopped");
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Decrements the connection count when a connection's thread ends.
struct Slot(Arc<Shared>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::SeqCst);
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            break;
        }
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        if shared.connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            shared.connections.fetch_sub(1, Ordering::SeqCst);
            // take the request in first: closing on unread bytes resets the
            // connection before the client reads the answer
            let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
            let _ = std::io::Read::read(&mut stream, &mut [0u8; http::MAX_HEAD]);
            reply_error(&mut stream, &ToolError::new(ErrorCode::Busy, "too many connections"));
            continue;
        }
        let slot = Slot(shared.clone());
        let spawned = std::thread::Builder::new().name("mcp-bridge-call".into()).spawn(move || {
            let slot = slot;
            serve(&mut stream, &slot.0);
        });
        if let Err(e) = spawned {
            log::warn!("local assistant bridge: no thread for a connection: {e}");
        }
    }
}

fn reply(stream: &mut TcpStream, status: u16, body: &Value) {
    let _ = http::write_response(stream, status, body.to_string().as_bytes());
}

fn reply_error(stream: &mut TcpStream, error: &ToolError) {
    reply(stream, error.code.status(), &json!({"ok": false, "error": error}));
}

fn serve(stream: &mut TcpStream, shared: &Shared) {
    match handle(stream, shared) {
        Ok(result) => reply(stream, 200, &json!({"ok": true, "result": result})),
        Err(error) => reply_error(stream, &error),
    }
}

fn handle(stream: &mut TcpStream, shared: &Shared) -> Result<Value, ToolError> {
    let (head, body) = http::read_message(stream, http::MAX_REQUEST_BODY).map_err(|e| match e {
        HttpError::TooLarge => ToolError::new(ErrorCode::TooLarge, "request too large"),
        HttpError::Malformed(what) => ToolError::new(ErrorCode::BadRequest, what),
        HttpError::Io(_) => ToolError::new(ErrorCode::BadRequest, "request not received"),
    })?;
    // A browser page sends Origin: no web page may drive the bridge.
    if head.header("origin").is_some() {
        return Err(ToolError::new(ErrorCode::Forbidden, "requests from web pages are refused"));
    }
    // DNS rebinding: the Host must be the loopback address and our port.
    let host_ok = head.header("host").is_some_and(|host| {
        let port = shared.port.to_string();
        host.rsplit_once(':')
            .is_some_and(|(name, p)| p == port && (name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost")))
    });
    if !host_ok {
        return Err(ToolError::new(ErrorCode::Forbidden, "unexpected Host header"));
    }
    let authorized = head
        .header("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| shared.token.matches(t.trim()));
    if !authorized {
        return Err(ToolError::new(ErrorCode::Unauthorized, "missing or wrong bearer token"));
    }
    if shared.stopping.load(Ordering::SeqCst) {
        return Err(ToolError::new(ErrorCode::Unavailable, "the bridge is stopping"));
    }
    let probe = head.header(PROBE_HEADER).is_some();
    let method = head.first[0].as_str();
    let path = head.first[1].as_str();
    match (method, path) {
        ("GET", "/v1/health") => {
            let permissions = *shared.permissions.lock().unwrap_or_else(PoisonError::into_inner);
            let health = Health {
                api: crate::API_VERSION,
                product: folderstorm_brand::PRODUCT.to_owned(),
                version: shared.version.clone(),
                read_allowed: permissions.check(Class::Read).is_ok(),
            };
            serde_json::to_value(health).map_err(|_| ToolError::new(ErrorCode::Internal, "health not serialized"))
        }
        ("POST", "/v1/call") => {
            let call: CallRequest =
                serde_json::from_slice(&body).map_err(|_| ToolError::new(ErrorCode::BadRequest, "the body is not a tool call"))?;
            let result = call_tool(shared, &call);
            if !probe {
                record(shared, &call.tool, &result);
            }
            result
        }
        (_, "/v1/health" | "/v1/call") => Err(ToolError::new(ErrorCode::BadRequest, "wrong method")),
        _ => Err(ToolError::new(ErrorCode::NotFound, "no such endpoint")),
    }
}

fn call_tool(shared: &Shared, call: &CallRequest) -> Result<Value, ToolError> {
    let spec = protocol::tool(&call.tool).ok_or_else(|| ToolError::new(ErrorCode::UnknownTool, format!("unknown tool `{}`", call.tool)))?;
    if let Some(class) = spec.class {
        let permissions = *shared.permissions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(level) = permissions.check(class) {
            let why = match level {
                Level::Ask => "this kind of change needs the user's approval, which this version cannot ask for",
                _ => "the user does not allow this in Preferences › Privacy › Local assistant",
            };
            return Err(ToolError::new(ErrorCode::PermissionDenied, why));
        }
    }
    let (reply, answer) = mpsc::sync_channel(1);
    let pending = Pending {
        tool: call.tool.clone(),
        arguments: call.arguments.clone(),
        reply,
    };
    match shared.queue.try_send(pending) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => return Err(ToolError::new(ErrorCode::Busy, "too many calls waiting for the viewer")),
        Err(TrySendError::Disconnected(_)) => return Err(ToolError::new(ErrorCode::Unavailable, "the bridge is stopping")),
    }
    match answer.recv_timeout(shared.viewer_timeout) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err(ToolError::new(ErrorCode::Timeout, "the viewer did not answer in time")),
        Err(RecvTimeoutError::Disconnected) => Err(ToolError::new(ErrorCode::Unavailable, "the call was dropped")),
    }
}

fn record(shared: &Shared, tool: &str, result: &Result<Value, ToolError>) {
    let outcome = match result {
        Ok(_) => Outcome::Done,
        Err(e) if e.code == ErrorCode::PermissionDenied => Outcome::Refused,
        Err(e) => Outcome::Failed(e.code),
    };
    // an unknown name is the client's text: keep it short and printable
    let tool: String = tool.chars().filter(|c| c.is_ascii_graphic()).take(48).collect();
    let mut activity = shared.activity.lock().unwrap_or_else(PoisonError::into_inner);
    if activity.len() == ACTIVITY {
        activity.pop_front();
    }
    activity.push_back(Activity {
        at: SystemTime::now(),
        tool,
        outcome,
    });
}

#[cfg(test)]
mod tests;
