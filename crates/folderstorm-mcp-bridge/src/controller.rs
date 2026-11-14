//! The session host's MCP controller (folderstorm/plans/mcp-routing.md,
//! section 3): a separate authority from every viewer's local assistant.
//! The host (`--session-host`) runs this listener only while its user turned
//! "Enable controller MCP" on; it has its own token, its own discovery file
//! (`folderstorm2-controller-<pid>.json`, [`discovery::Kind::Controller`]),
//! its own instance (`host_session`) and its own permission classes
//! ([`HostClass`]). Nothing here reads a viewer's settings or token.
//!
//! The listener's threads authenticate a call (the bridge's checks,
//! [`crate::server::check_head`]), refuse a class on Never at once, then
//! hand the call to the host's window thread as a [`Pending`] and wait for
//! its answer. The host decides everything again: it is the authority.

use crate::binding::{self, Capabilities};
use crate::discovery::{self, Discovery, Kind};
use crate::policy::Level;
use crate::protocol::{ASK_TIMEOUT, CallRequest, ErrorCode, ToolError};
use crate::server::{self, CLIENT_HEADER, Pending, Served};
use crate::token::Token;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// The controller's capability: the endpoint, `host_session`, the
/// `host_*` tools.
pub const CONTROLLER: &str = "controller";

/// What a host of this build implements.
pub const CAPABILITIES: &[(&str, u32)] = &[(CONTROLLER, 1)];

/// [`CAPABILITIES`] as a map.
pub fn capabilities() -> Capabilities {
    CAPABILITIES.iter().map(|(g, v)| ((*g).to_owned(), *v)).collect()
}

/// The prefix of a `host_session`.
pub const HOST_SESSION_PREFIX: &str = "hc-";

/// How long a call waits for the host's window thread by default (its
/// ticks are 200 ms apart, 1 s when no character runs).
pub const HOST_TIMEOUT: Duration = Duration::from_secs(10);

/// What a controller tool does, for the level it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostClass {
    /// Bring a character forward.
    Activate,
    /// Warm or Economy behind.
    BackgroundMode,
    /// Open or close a monitor, the cycling monitor.
    Monitors,
    /// Start a character's viewer at its login screen, or bring that
    /// screen forward. Never logs in.
    OpenLogin,
    Restart,
    Logout,
    /// Let a viewer go: it goes on by itself.
    StopManaging,
}

impl HostClass {
    pub const ALL: [HostClass; 7] = [
        HostClass::Activate,
        HostClass::BackgroundMode,
        HostClass::Monitors,
        HostClass::OpenLogin,
        HostClass::Restart,
        HostClass::Logout,
        HostClass::StopManaging,
    ];
}

/// One level per class; Ask for each by default (Folderstorm (C++)'s
/// default for every write class).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostLevels {
    pub activate: Level,
    pub background_mode: Level,
    pub monitors: Level,
    pub open_login: Level,
    pub restart: Level,
    pub logout: Level,
    pub stop_managing: Level,
}

impl Default for HostLevels {
    fn default() -> Self {
        Self {
            activate: Level::Ask,
            background_mode: Level::Ask,
            monitors: Level::Ask,
            open_login: Level::Ask,
            restart: Level::Ask,
            logout: Level::Ask,
            stop_managing: Level::Ask,
        }
    }
}

impl HostLevels {
    pub fn level(&self, class: HostClass) -> Level {
        match class {
            HostClass::Activate => self.activate,
            HostClass::BackgroundMode => self.background_mode,
            HostClass::Monitors => self.monitors,
            HostClass::OpenLogin => self.open_login,
            HostClass::Restart => self.restart,
            HostClass::Logout => self.logout,
            HostClass::StopManaging => self.stop_managing,
        }
    }

    pub fn set(&mut self, class: HostClass, level: Level) {
        let field = match class {
            HostClass::Activate => &mut self.activate,
            HostClass::BackgroundMode => &mut self.background_mode,
            HostClass::Monitors => &mut self.monitors,
            HostClass::OpenLogin => &mut self.open_login,
            HostClass::Restart => &mut self.restart,
            HostClass::Logout => &mut self.logout,
            HostClass::StopManaging => &mut self.stop_managing,
        };
        *field = level;
    }

    /// Every class on that level.
    pub fn all(level: Level) -> Self {
        let mut levels = Self::default();
        for class in HostClass::ALL {
            levels.set(class, level);
        }
        levels
    }
}

/// One tool of the controller's catalogue.
pub struct HostTool {
    pub name: &'static str,
    pub description: &'static str,
    /// The class it needs; None for the list.
    pub class: Option<HostClass>,
    pub schema: fn() -> Value,
}

const PROFILE: &str = "The character's profile name, exactly as host_characters lists it (a character is never addressed by its slot number, which changes when cards swap places).";
const LAUNCH: &str = "Optional: the character's `launch` from host_characters. The call is refused (session_changed) when its viewer was started again since.";

fn no_arguments() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn character() -> Value {
    json!({
        "type": "object",
        "properties": {
            "profile": {"type": "string", "description": PROFILE},
            "launch": {"type": "integer", "minimum": 1, "description": LAUNCH}
        },
        "required": ["profile"],
        "additionalProperties": false
    })
}

fn background_mode() -> Value {
    let mut schema = character();
    schema["properties"]["mode"] = json!({"type": "string", "enum": ["warm", "economy"],
        "description": "warm: its window waits behind the active one; economy: minimised, the least memory and processor."});
    schema["required"] = json!(["profile", "mode"]);
    schema
}

fn monitor() -> Value {
    json!({
        "type": "object",
        "properties": {
            "profile": {"type": "string", "description": "The character whose monitor opens or closes (not with `cycling`)."},
            "launch": {"type": "integer", "minimum": 1, "description": LAUNCH},
            "cycling": {"type": "boolean", "description": "true: the cycling monitor, which shows each background character in turn (not with `profile`)."},
            "open": {"type": "boolean", "description": "true opens the monitor's window, false closes it."}
        },
        "required": ["open"],
        "additionalProperties": false
    })
}

/// The controller's tools.
pub const HOST_TOOLS: &[HostTool] = &[
    HostTool {
        name: "host_characters",
        description: "The session host's characters that the user gave controller access: slot, profile, state (stopped, starting, login, loading, in_world, closing, ended, crashed, unmanaged), active, background mode, launch number. Names, aliases, colours and monitor details only when the user allows profile and monitor metadata. Needs the host's MCP controller turned on in its settings; separate from each viewer's local assistant.",
        class: None,
        schema: no_arguments,
    },
    HostTool {
        name: "host_activate",
        description: "Bring a logged-in character forward in the session host (it becomes the active character, as its Switch button does). The user may be asked first.",
        class: Some(HostClass::Activate),
        schema: character,
    },
    HostTool {
        name: "host_background_mode",
        description: "Set how a character waits while another one is active: warm (behind) or economy (minimised). The user may be asked first.",
        class: Some(HostClass::BackgroundMode),
        schema: background_mode,
    },
    HostTool {
        name: "host_monitor",
        description: "Open or close a character's monitor (a small read-only picture of its view, in a window of the host) or the cycling monitor. The user may be asked first.",
        class: Some(HostClass::Monitors),
        schema: monitor,
    },
    HostTool {
        name: "host_open_login",
        description: "Start a character's viewer at its login screen, or bring that login screen forward. It never logs in and never fills or sends anything: the user logs in there. The user may be asked first.",
        class: Some(HostClass::OpenLogin),
        schema: character,
    },
    HostTool {
        name: "host_restart",
        description: "Restart a character's viewer: it logs out and closes, then starts again at its login screen (one that ended starts again). The user logs in again there. The user may be asked first.",
        class: Some(HostClass::Restart),
        schema: character,
    },
    HostTool {
        name: "host_logout",
        description: "Log a character out: its viewer logs out and closes, as its Close button does. The user may be asked first.",
        class: Some(HostClass::Logout),
        schema: character,
    },
    HostTool {
        name: "host_stop_managing",
        description: "The session host stops managing a character: its viewer goes on by itself, logged in, as when the host closes. The user may be asked first.",
        class: Some(HostClass::StopManaging),
        schema: character,
    },
];

pub fn host_tool(name: &str) -> Option<&'static HostTool> {
    HOST_TOOLS.iter().find(|t| t.name == name)
}

/// The controller's `/v1/health`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControllerHealth {
    pub api: u32,
    pub product: String,
    pub version: String,
    #[serde(default)]
    pub capabilities: Capabilities,
    /// This start of the controller: the sidecar pins it.
    pub host_session: String,
}

/// The host restarted, or its controller was turned off and on, since the
/// sidecar pinned `host_session`: nothing was run.
pub fn host_changed(current: &str) -> ToolError {
    ToolError {
        viewer_session: Some(current.to_owned()),
        ..ToolError::new(
            ErrorCode::SessionChanged,
            "the session host restarted, or its MCP controller was turned off and on: this call was not run. Call host_characters and look again before retrying",
        )
    }
}

/// The user's levels refuse this class.
pub fn never() -> ToolError {
    ToolError::new(
        ErrorCode::PermissionDenied,
        "the user does not allow this in the session host's settings › MCP controller",
    )
}

pub struct ControllerConfig {
    /// Where the discovery file goes; None writes none (tests).
    pub discovery_dir: Option<PathBuf>,
    pub pid: u32,
    pub version: String,
    pub levels: HostLevels,
    pub host_timeout: Duration,
}

struct Shared {
    token: Token,
    port: u16,
    version: String,
    host_session: String,
    host_timeout: Duration,
    levels: Mutex<HostLevels>,
    stopping: AtomicBool,
    connections: AtomicUsize,
    queue: SyncSender<Pending>,
}

/// The running controller. Dropping it stops the listener and deletes the
/// discovery file.
pub struct Controller {
    shared: Arc<Shared>,
    discovery: Option<PathBuf>,
    accept: Option<JoinHandle<()>>,
}

impl Controller {
    /// Bind 127.0.0.1 on a free port, make a token and a `host_session`,
    /// write the discovery file and start listening. The receiver gives
    /// the calls to answer, on the host's window thread.
    pub fn start(config: ControllerConfig) -> std::io::Result<(Controller, Receiver<Pending>)> {
        let token = Token::generate()?;
        let host_session = binding::random_id(HOST_SESSION_PREFIX)?;
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let (queue, calls) = mpsc::sync_channel(server::MAX_QUEUED);
        let shared = Arc::new(Shared {
            token,
            port,
            version: config.version,
            host_session,
            host_timeout: config.host_timeout,
            levels: Mutex::new(config.levels),
            stopping: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
            queue,
        });
        let discovery = match &config.discovery_dir {
            Some(dir) => Some(discovery::write_as(
                Kind::Controller,
                dir,
                &Discovery::new(config.pid, port, &shared.token),
            )?),
            None => None,
        };
        let accepting = shared.clone();
        let accept = std::thread::Builder::new()
            .name("mcp-controller-accept".into())
            .spawn(move || server::accept_loop(listener, accepting));
        let accept = match accept {
            Ok(handle) => handle,
            Err(e) => {
                if let Some(path) = &discovery {
                    let _ = discovery::remove(path);
                }
                return Err(e);
            }
        };
        log::info!("session host: MCP controller listening on 127.0.0.1:{port}");
        Ok((
            Controller {
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

    /// The token, for tests only.
    pub fn token(&self) -> &Token {
        &self.shared.token
    }

    /// This start's `host_session`.
    pub fn host_session(&self) -> &str {
        &self.shared.host_session
    }

    pub fn set_levels(&self, levels: HostLevels) {
        *self.shared.levels.lock().unwrap_or_else(PoisonError::into_inner) = levels;
    }

    pub fn levels(&self) -> HostLevels {
        *self.shared.levels.lock().unwrap_or_else(PoisonError::into_inner)
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
            log::warn!("session host: the MCP controller's discovery file was not deleted: {e}");
        }
        if let Some(accept) = self.accept.take() {
            let _ = TcpStream::connect_timeout(
                &SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.shared.port).into(),
                Duration::from_secs(1),
            );
            if accept.join().is_err() {
                log::warn!("session host: the MCP controller's listener panicked");
            }
            log::info!("session host: MCP controller stopped");
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Served for Shared {
    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    fn connections(&self) -> &AtomicUsize {
        &self.connections
    }

    fn serve(&self, stream: &mut TcpStream) {
        match self.handle(stream) {
            Ok(result) => server::reply(stream, 200, &json!({"ok": true, "result": result})),
            Err(error) => server::reply_error(stream, &error),
        }
    }
}

impl Shared {
    fn handle(&self, stream: &mut TcpStream) -> Result<Value, ToolError> {
        let (head, body) = server::read_request(stream)?;
        server::check_head(&head, self.port, &self.token)?;
        if self.stopping.load(Ordering::SeqCst) {
            return Err(ToolError::new(ErrorCode::Unavailable, "the MCP controller is stopping"));
        }
        let client = head.header(CLIENT_HEADER).and_then(server::clean_client);
        match (head.first[0].as_str(), head.first[1].as_str()) {
            ("GET", "/v1/health") => serde_json::to_value(ControllerHealth {
                api: crate::API_VERSION,
                product: folderstorm_brand::PRODUCT.to_owned(),
                version: self.version.clone(),
                capabilities: capabilities(),
                host_session: self.host_session.clone(),
            })
            .map_err(|_| ToolError::new(ErrorCode::Internal, "health not serialized")),
            ("POST", "/v1/call") => {
                let call: CallRequest =
                    serde_json::from_slice(&body).map_err(|_| ToolError::new(ErrorCode::BadRequest, "the body is not a tool call"))?;
                let result = self.call(&call, client);
                let outcome = match &result {
                    Ok(_) => "done".to_owned(),
                    Err(e) => e.to_string().split(':').next().unwrap_or("error").to_owned(),
                };
                // the tool and how it ended, never its arguments
                let tool: String = call.tool.chars().filter(|c| c.is_ascii_graphic()).take(48).collect();
                log::info!("session host: MCP controller call {tool}: {outcome}");
                result.map_err(|e| e.for_sidecar(call.features.as_ref()))
            }
            (_, "/v1/health" | "/v1/call") => Err(ToolError::new(ErrorCode::BadRequest, "wrong method")),
            _ => Err(ToolError::new(ErrorCode::NotFound, "no such endpoint")),
        }
    }

    fn call(&self, call: &CallRequest, client: Option<String>) -> Result<Value, ToolError> {
        let Some(tool) = host_tool(&call.tool) else {
            return Err(ToolError::new(
                ErrorCode::Unsupported,
                format!(
                    "the running {} session host does not know the tool `{}`: update it; nothing was done",
                    folderstorm_brand::PRODUCT,
                    call.tool
                ),
            ));
        };
        // a fast refusal only: the host decides again
        if let Some(pinned) = &call.session
            && *pinned != self.host_session
        {
            return Err(host_changed(&self.host_session));
        }
        let level = tool
            .class
            .map(|class| self.levels.lock().unwrap_or_else(PoisonError::into_inner).level(class));
        let ask = match level {
            Some(Level::Never) => return Err(never()),
            Some(Level::Ask) => true,
            Some(Level::Allow) | None => false,
        };
        let (mut pending, answer) = Pending::detached(&call.tool, call.arguments.clone(), ask, None);
        pending.client = client;
        pending.session = call.session.clone();
        match self.queue.try_send(pending) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(ToolError::new(ErrorCode::Busy, "too many calls waiting for the session host")),
            Err(TrySendError::Disconnected(_)) => return Err(ToolError::new(ErrorCode::Unavailable, "the MCP controller is stopping")),
        }
        let wait = self.host_timeout + if ask { ASK_TIMEOUT } else { Duration::ZERO };
        match answer.recv_timeout(wait) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(ToolError::new(ErrorCode::Timeout, "the session host did not answer in time")),
            Err(RecvTimeoutError::Disconnected) => Err(ToolError::new(ErrorCode::Unavailable, "the call was dropped")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Client, ClientError};

    fn start(dir: Option<&Path>, levels: HostLevels) -> (Controller, Receiver<Pending>) {
        Controller::start(ControllerConfig {
            discovery_dir: dir.map(Path::to_path_buf),
            pid: 4242,
            version: "test".into(),
            levels,
            host_timeout: Duration::from_secs(2),
        })
        .expect("controller started")
    }

    fn client(c: &Controller) -> Client {
        Client::new(c.port(), c.token().clone())
    }

    #[test]
    fn the_catalogue_is_whole() {
        assert_eq!(HOST_TOOLS.len(), 8);
        for class in HostClass::ALL {
            assert_eq!(HOST_TOOLS.iter().filter(|t| t.class == Some(class)).count(), 1, "{class:?}");
        }
        for tool in HOST_TOOLS {
            assert!(tool.name.starts_with("host_"), "{}", tool.name);
            let schema = (tool.schema)();
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
            // never a slot number to address a character
            assert!(schema["properties"].get("slot").is_none(), "{}", tool.name);
        }
        assert!(host_tool("host_activate").is_some() && host_tool("inventory_search").is_none());
        // levels: Ask by default, a missing field too
        let read: HostLevels = serde_json::from_value(json!({"logout": "never"})).expect("levels");
        assert_eq!(read.level(HostClass::Logout), Level::Never);
        assert_eq!(read.level(HostClass::Activate), Level::Ask);
        assert_eq!(HostLevels::all(Level::Allow).level(HostClass::StopManaging), Level::Allow);
        assert!(binding::is_random_id(
            &binding::random_id(HOST_SESSION_PREFIX).expect("random"),
            "hc-"
        ));
    }

    #[test]
    fn health_calls_and_refusals_over_the_socket() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut levels = HostLevels::all(Level::Allow);
        levels.set(HostClass::Logout, Level::Never);
        levels.set(HostClass::Restart, Level::Ask);
        let (controller, calls) = start(Some(dir.path()), levels);
        let path = controller.discovery_path().expect("a file").to_path_buf();
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("folderstorm2-controller-4242.json"));
        assert!(discovery::find(dir.path(), None).is_empty(), "never read as a viewer");
        let found = discovery::find_kind(Kind::Controller, dir.path(), None);
        assert_eq!(found.len(), 1);
        let host = std::thread::spawn(move || {
            let mut seen = Vec::new();
            while let Ok(call) = calls.recv() {
                seen.push((call.tool.clone(), call.ask, call.client.clone(), call.session.clone()));
                let tool = call.tool.clone();
                call.answer(Ok(json!({"tool": tool})));
            }
            seen
        });
        let c = client(&controller).with_client_name("Claude Code");
        let health: ControllerHealth = serde_json::from_value(c.get("/v1/health").expect("health")).expect("health");
        assert_eq!(health.host_session, controller.host_session());
        assert_eq!(health.capabilities.get(CONTROLLER), Some(&1));
        // the list and an allowed action reach the host
        let session = Some(health.host_session.as_str());
        assert_eq!(
            c.call_in("host_characters", json!({}), session).expect("list")["tool"],
            "host_characters"
        );
        c.call_in("host_activate", json!({"profile": "a"}), session).expect("allowed");
        c.call_in("host_restart", json!({"profile": "a"}), session).expect("asked");
        // Never: refused at the listener
        let refused = c.call_in("host_logout", json!({"profile": "a"}), session);
        assert!(
            matches!(refused, Err(ClientError::Tool(ref e)) if e.code == ErrorCode::PermissionDenied),
            "{refused:?}"
        );
        // another host_session: refused, with the current one
        let stale = c.call_in(
            "host_activate",
            json!({"profile": "a"}),
            Some("hc-00000000000000000000000000000000"),
        );
        match stale {
            Err(ClientError::Tool(e)) => {
                assert_eq!(e.code, ErrorCode::SessionChanged);
                assert_eq!(e.viewer_session.as_deref(), Some(controller.host_session()));
            }
            other => panic!("{other:?}"),
        }
        // a viewer's tool is not the controller's
        let unknown = c.call_in("inventory_search", json!({}), session);
        assert!(
            matches!(unknown, Err(ClientError::Tool(ref e)) if e.code == ErrorCode::Unsupported),
            "{unknown:?}"
        );
        // a wrong token
        let wrong = Client::new(controller.port(), Token::generate().expect("token")).get("/v1/health");
        assert!(matches!(wrong, Err(ClientError::Tool(ref e)) if e.code == ErrorCode::Unauthorized));
        controller.stop();
        assert!(!path.exists(), "the file goes with the controller");
        let seen = host.join().expect("host thread");
        assert_eq!(seen.len(), 3);
        assert_eq!(
            seen[0],
            (
                "host_characters".into(),
                false,
                Some("Claude Code".into()),
                session.map(str::to_owned)
            )
        );
        assert!(!seen[1].1 && seen[2].1, "Allow runs, Ask asks");
    }

    #[test]
    fn a_host_that_does_not_answer_times_out() {
        let (controller, calls) = start(None, HostLevels::all(Level::Allow));
        let c = client(&controller);
        let late = c.call_in("host_characters", json!({}), None);
        assert!(
            matches!(late, Err(ClientError::Tool(ref e)) if e.code == ErrorCode::Timeout),
            "{late:?}"
        );
        drop(calls);
    }
}
