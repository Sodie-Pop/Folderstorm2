//! The bridge's listener: 127.0.0.1 on a port the OS picks, a fresh token
//! and a discovery file each time it starts. Its threads authenticate a
//! call, check its permission class, then hand it to the viewer's frame
//! thread through a bounded channel ([`Pending`]) and wait for the answer
//! with a timeout. They never touch viewer state themselves.
//!
//! The frame thread tells the bridge the login it is on
//! ([`Bridge::set_login`]); the bridge gives that login's `viewer_session`
//! in `/v1/health`, refuses at once a bound call pinned to another one (the
//! frame thread checks again: it is the authority), and answers each call
//! with only the error codes its sidecar declares
//! ([`ToolError::for_sidecar`]).

use crate::binding::{self, Binding};
use crate::discovery::{self, Discovery};
use crate::http::{self, HttpError};
use crate::policy::{Class, Decision, Permissions};
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
/// The header the sidecar passes the MCP client's own name in
/// (`clientInfo.name`), for the Ask dialog.
pub const CLIENT_HEADER: &str = "x-folderstorm-client";
/// The longest client name kept.
pub const CLIENT_NAME_MAX: usize = 48;

pub struct Config {
    /// Where the discovery file goes; None writes none (tests).
    pub discovery_dir: Option<PathBuf>,
    pub pid: u32,
    pub version: String,
    pub permissions: Permissions,
    pub viewer_timeout: Duration,
}

/// Who the viewer is, for `/v1/health` (session routing,
/// folderstorm/plans/mcp-routing.md): told by the frame thread.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    /// Its `--profile`; None for the main profile.
    pub profile: Option<String>,
    /// A session host manages it.
    pub managed: bool,
    /// The name it shows for its own avatar while logged in; health gives
    /// it only while Read is Allow.
    pub character: Option<String>,
}

/// A call waiting for the viewer's answer.
pub struct Pending {
    pub tool: String,
    pub arguments: Value,
    /// The user's permissions say Ask: the viewer asks before it runs.
    pub ask: bool,
    /// The name the MCP client gives itself, cleaned; None when it gave
    /// none. Any local program can claim any name.
    pub client: Option<String>,
    /// The `viewer_session` the sidecar pinned; None: unbound (an older
    /// sidecar, or nothing pinned yet). The frame thread refuses a bound
    /// tool pinned to another login than its own.
    pub session: Option<String>,
    reply: SyncSender<Result<Value, ToolError>>,
}

impl Pending {
    /// A call made in the process (the viewer's tests), and where its
    /// answer arrives.
    pub fn detached(tool: &str, arguments: Value, ask: bool, client: Option<&str>) -> (Pending, Receiver<Result<Value, ToolError>>) {
        let (reply, answer) = mpsc::sync_channel(1);
        (
            Pending {
                tool: tool.to_owned(),
                arguments,
                ask,
                client: client.and_then(clean_client),
                session: None,
                reply,
            },
            answer,
        )
    }

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
    /// The login the frame thread is on, and its `viewer_session`.
    binding: Mutex<Binding>,
    /// Session routing: the profile, managed or not, the character.
    identity: Mutex<Identity>,
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
            binding: Mutex::new(Binding::default()),
            identity: Mutex::new(Identity::default()),
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

    /// The viewer's login now, from the frame thread once per frame: login
    /// generation `generation`, logged in (or the demo) or not. A new
    /// generation gets a new `viewer_session`.
    pub fn set_login(&self, generation: u64, logged_in: bool) {
        self.shared
            .binding
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .update(generation, logged_in);
    }

    /// The `viewer_session` of the login now; None while logged out.
    pub fn viewer_session(&self) -> Option<String> {
        self.shared.viewer_session()
    }

    /// Who the viewer is (its profile, managed or not, its avatar's name),
    /// from the frame thread; health gives the name only while Read is
    /// Allow.
    pub fn set_identity(&self, identity: Identity) {
        let mut held = self.shared.identity.lock().unwrap_or_else(PoisonError::into_inner);
        if *held != identity {
            *held = identity;
        }
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
            let _ = TcpStream::connect_timeout(
                &SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.shared.port).into(),
                Duration::from_secs(1),
            );
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

impl Shared {
    fn viewer_session(&self) -> Option<String> {
        self.binding
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .session()
            .map(str::to_owned)
    }
}

/// What an accept loop serves: the bridge, or the host's MCP controller
/// (`crate::controller`). Both share the bounds of [`accept_loop`].
pub(crate) trait Served: Send + Sync + 'static {
    fn stopping(&self) -> bool;
    fn connections(&self) -> &AtomicUsize;
    fn serve(&self, stream: &mut TcpStream);
}

impl Served for Shared {
    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    fn connections(&self) -> &AtomicUsize {
        &self.connections
    }

    fn serve(&self, stream: &mut TcpStream) {
        serve(stream, self);
    }
}

/// Decrements the connection count when a connection's thread ends.
struct Slot<S: Served>(Arc<S>);

impl<S: Served> Drop for Slot<S> {
    fn drop(&mut self) {
        self.0.connections().fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) fn accept_loop<S: Served>(listener: TcpListener, shared: Arc<S>) {
    for stream in listener.incoming() {
        if shared.stopping() {
            break;
        }
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        if shared.connections().fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            shared.connections().fetch_sub(1, Ordering::SeqCst);
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
            slot.0.serve(&mut stream);
        });
        if let Err(e) = spawned {
            log::warn!("local assistant bridge: no thread for a connection: {e}");
        }
    }
}

pub(crate) fn reply(stream: &mut TcpStream, status: u16, body: &Value) {
    let _ = http::write_response(stream, status, body.to_string().as_bytes());
}

pub(crate) fn reply_error(stream: &mut TcpStream, error: &ToolError) {
    reply(stream, error.code.status(), &json!({"ok": false, "error": error}));
}

fn serve(stream: &mut TcpStream, shared: &Shared) {
    match handle(stream, shared) {
        Ok(result) => reply(stream, 200, &json!({"ok": true, "result": result})),
        Err(error) => reply_error(stream, &error),
    }
}

/// Read one request, bounded.
pub(crate) fn read_request(stream: &mut TcpStream) -> Result<(http::Head, Vec<u8>), ToolError> {
    http::read_message(stream, http::MAX_REQUEST_BODY).map_err(|e| match e {
        HttpError::TooLarge => ToolError::new(ErrorCode::TooLarge, "request too large"),
        HttpError::Malformed(what) => ToolError::new(ErrorCode::BadRequest, what),
        HttpError::Io(_) => ToolError::new(ErrorCode::BadRequest, "request not received"),
    })
}

/// The checks every request passes before anything else, the bridge's and
/// the host controller's: no web page (`Origin`), no DNS rebinding (the
/// `Host` must be the loopback address and our port), the bearer token.
pub(crate) fn check_head(head: &http::Head, port: u16, token: &Token) -> Result<(), ToolError> {
    // A browser page sends Origin: no web page may drive the bridge.
    if head.header("origin").is_some() {
        return Err(ToolError::new(ErrorCode::Forbidden, "requests from web pages are refused"));
    }
    // DNS rebinding: the Host must be the loopback address and our port.
    let host_ok = head.header("host").is_some_and(|host| {
        let port = port.to_string();
        host.rsplit_once(':')
            .is_some_and(|(name, p)| p == port && (name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost")))
    });
    if !host_ok {
        return Err(ToolError::new(ErrorCode::Forbidden, "unexpected Host header"));
    }
    let authorized = head
        .header("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| token.matches(t.trim()));
    if !authorized {
        return Err(ToolError::new(ErrorCode::Unauthorized, "missing or wrong bearer token"));
    }
    Ok(())
}

fn handle(stream: &mut TcpStream, shared: &Shared) -> Result<Value, ToolError> {
    let (head, body) = read_request(stream)?;
    check_head(&head, shared.port, &shared.token)?;
    if shared.stopping.load(Ordering::SeqCst) {
        return Err(ToolError::new(ErrorCode::Unavailable, "the bridge is stopping"));
    }
    let probe = head.header(PROBE_HEADER).is_some();
    let client = head.header(CLIENT_HEADER).and_then(clean_client);
    let method = head.first[0].as_str();
    let path = head.first[1].as_str();
    match (method, path) {
        ("GET", "/v1/health") => {
            let permissions = *shared.permissions.lock().unwrap_or_else(PoisonError::into_inner);
            let read_allowed = permissions.check(Class::Read).is_ok();
            let viewer_session = shared.viewer_session();
            let identity = shared.identity.lock().unwrap_or_else(PoisonError::into_inner).clone();
            let health = Health {
                api: crate::API_VERSION,
                product: folderstorm_brand::PRODUCT.to_owned(),
                version: shared.version.clone(),
                read_allowed,
                capabilities: Some(binding::capabilities()),
                // (a name only for a login, and only with Read)
                character: identity.character.filter(|_| read_allowed && viewer_session.is_some()),
                viewer_session,
                profile: identity.profile,
                managed: identity.managed,
            };
            serde_json::to_value(health).map_err(|_| ToolError::new(ErrorCode::Internal, "health not serialized"))
        }
        ("POST", "/v1/call") => {
            let call: CallRequest =
                serde_json::from_slice(&body).map_err(|_| ToolError::new(ErrorCode::BadRequest, "the body is not a tool call"))?;
            let result = call_tool(shared, &call, client);
            if !probe {
                record(shared, &call.tool, &result);
            }
            result.map_err(|e| e.for_sidecar(call.features.as_ref()))
        }
        (_, "/v1/health" | "/v1/call") => Err(ToolError::new(ErrorCode::BadRequest, "wrong method")),
        _ => Err(ToolError::new(ErrorCode::NotFound, "no such endpoint")),
    }
}

/// A client's name as the dialog may show it: printable, one line, short.
pub(crate) fn clean_client(name: &str) -> Option<String> {
    let name: String = name.chars().filter(|c| !c.is_control()).take(CLIENT_NAME_MAX).collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn call_tool(shared: &Shared, call: &CallRequest, client: Option<String>) -> Result<Value, ToolError> {
    if protocol::NEVER.contains(&call.tool.as_str()) {
        return Err(ToolError::new(ErrorCode::PermissionDenied, protocol::NEVER_MESSAGE));
    }
    let Some(spec) = protocol::tool(&call.tool) else {
        // a sidecar that knows capabilities only sends a tool it believes
        // this viewer has: say plainly that the viewer is the older side
        return Err(if call.features.is_some() {
            ToolError::new(
                ErrorCode::Unsupported,
                format!(
                    "the running {product} does not know the tool `{}`: update {product}; nothing was sent",
                    call.tool,
                    product = folderstorm_brand::PRODUCT
                ),
            )
        } else {
            ToolError::new(ErrorCode::UnknownTool, format!("unknown tool `{}`", call.tool))
        });
    };
    // a fast refusal only: the frame thread decides again
    if spec.bound()
        && let Some(pinned) = &call.session
    {
        let current = shared.viewer_session();
        if current.as_deref() != Some(pinned.as_str()) {
            return Err(binding::session_changed(current.as_deref()));
        }
    }
    let permissions = *shared.permissions.lock().unwrap_or_else(PoisonError::into_inner);
    let ask = match permissions.decide_all(spec.classes()) {
        // a picture with the interface asks even on Allow
        Decision::Run => spec.class.is_some() && protocol::always_asks(&call.tool, &call.arguments),
        Decision::Ask => true,
        Decision::Refuse => {
            return Err(ToolError::new(
                ErrorCode::PermissionDenied,
                "the user does not allow this in Preferences › Privacy › Local assistant",
            ));
        }
    };
    let (pending, answer) = Pending::detached(&call.tool, call.arguments.clone(), ask, None);
    let pending = Pending {
        client,
        session: call.session.clone(),
        ..pending
    };
    match shared.queue.try_send(pending) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => return Err(ToolError::new(ErrorCode::Busy, "too many calls waiting for the viewer")),
        Err(TrySendError::Disconnected(_)) => return Err(ToolError::new(ErrorCode::Unavailable, "the bridge is stopping")),
    }
    // the dialog's time, then the tool's own, on top of the usual wait
    let wait = shared.viewer_timeout + spec.wait + if ask { protocol::ASK_TIMEOUT } else { Duration::ZERO };
    match answer.recv_timeout(wait) {
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
