//! Finding the viewer: the discovery files of the settings folder, newest
//! first, the first whose bridge answers with our API version; then the
//! login binding and the capability check of each call ([`Viewer`]).

use folderstorm_mcp_bridge::binding::{self, Capabilities};
use folderstorm_mcp_bridge::client::{Client, ClientError};
use folderstorm_mcp_bridge::discovery;
use folderstorm_mcp_bridge::protocol::{self, Health};
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a candidate viewer may take to answer its health check.
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

/// Why no viewer was found.
#[derive(Debug, PartialEq)]
pub enum NotFound {
    /// No settings folder to look in.
    NoFolder,
    /// No discovery file (the viewer is not running, or access is off).
    NoFile,
    /// Files, but no viewer answered with their token (stale files).
    NoAnswer,
    /// A viewer answered with another API version.
    OtherVersion(u32),
}

impl std::fmt::Display for NotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let product = folderstorm_brand::PRODUCT;
        match self {
            NotFound::NoFolder => write!(f, "{product}'s settings folder was not found; set {}", discovery::ENV),
            NotFound::NoFile => write!(
                f,
                "{product} is not running, or \"Allow local assistant access\" is off (Preferences › Privacy › Local assistant)"
            ),
            NotFound::NoAnswer => write!(
                f,
                "{product} did not answer; it may have quit. Start it, or turn local assistant access off and on"
            ),
            NotFound::OtherVersion(v) => write!(
                f,
                "{product} speaks bridge API {v}, this sidecar {}: use the sidecar installed with the viewer",
                folderstorm_mcp_bridge::API_VERSION
            ),
        }
    }
}

/// A viewer found, with the health it answered.
pub struct Found {
    pub client: Client,
    pub pid: u32,
    pub health: Health,
}

/// The viewer of `dir` (or `pid`'s), checked with a health request.
pub fn find(dir: Option<&Path>, pid: Option<u32>, probe: bool) -> Result<Found, NotFound> {
    let dir = dir.ok_or(NotFound::NoFolder)?;
    let candidates = discovery::find(dir, pid);
    if candidates.is_empty() {
        return Err(NotFound::NoFile);
    }
    let mut failure = NotFound::NoAnswer;
    for (_, found) in candidates {
        let mut client = Client::from_discovery(&found).with_timeout(PROBE_TIMEOUT);
        if probe {
            client = client.probe();
        }
        match client.health() {
            Ok(health) if health.api == folderstorm_mcp_bridge::API_VERSION => {
                let client = Client::from_discovery(&found);
                return Ok(Found {
                    client: if probe { client.probe() } else { client },
                    pid: found.pid,
                    health,
                });
            }
            Ok(health) => failure = NotFound::OtherVersion(health.api),
            Err(_) => {}
        }
    }
    Err(failure)
}

/// The viewer the sidecar talks to now.
#[derive(Clone)]
struct Link {
    client: Client,
    /// What it implements (`Health::capabilities`).
    capabilities: Capabilities,
    /// It advertises login binding: it enforces `CallRequest::session`.
    binding: bool,
}

/// The sidecar's link to the viewer, found again when the viewer restarts
/// (new port, new token) or starts after the sidecar.
///
/// **Login binding** (folderstorm/plans/mcp-receipts.md, section 2): the
/// sidecar pins the first `viewer_session` it sees and sends it with every
/// call; the viewer refuses a call pinned to another login with
/// `session_changed`. The sidecar then pins the new login (or keeps the old
/// pin while the viewer is logged out, so that the next login is reported
/// too) and **never sends the call again**: the assistant looks again and
/// decides. A viewer found again after a refused connection or token gets
/// the call only when it is on the pinned login; otherwise the answer is
/// `session_changed` and nothing is sent. With nothing pinned yet (logged
/// out, or a viewer older than login binding) calls are unbound, as before.
pub struct Viewer {
    dir: Option<PathBuf>,
    pid: Option<u32>,
    link: Option<Link>,
    /// The MCP client's own name, for the viewer's Ask dialog.
    name: Option<String>,
    /// The login the calls are bound to.
    pinned: Option<String>,
}

/// The viewer found again after a lost connection is not on the pinned
/// login (it restarted, access was turned off and on, or it logged in
/// again).
fn found_elsewhere(current: Option<&str>) -> ToolError {
    let mut e = binding::session_changed(current);
    e.message = format!("the connection to the viewer was lost and found again; {}", e.message);
    e
}

impl Viewer {
    pub fn new(dir: Option<PathBuf>, pid: Option<u32>) -> Viewer {
        Viewer {
            dir,
            pid,
            link: None,
            name: None,
            pinned: None,
        }
    }

    /// Name the calls after the MCP client (`clientInfo.name`).
    pub fn set_client_name(&mut self, name: &str) {
        self.name = Some(name.to_owned());
        if let Some(link) = &mut self.link {
            link.client = link.client.clone().with_client_name(name);
        }
    }

    /// The `viewer_session` the calls are bound to, if any.
    pub fn pinned(&self) -> Option<&str> {
        self.pinned.as_deref()
    }

    /// Look for the viewer: the link, and the login it is on.
    fn connect(&mut self) -> Result<(Link, Option<String>), ToolError> {
        let found = find(self.dir.as_deref(), self.pid, false).map_err(|e| ToolError::new(ErrorCode::Unavailable, e.to_string()))?;
        let mut client = found.client;
        if let Some(name) = &self.name {
            client = client.with_client_name(name);
        }
        let capabilities = found.health.capabilities();
        let link = Link {
            binding: capabilities.contains_key(binding::LOGIN_BINDING),
            client,
            capabilities,
        };
        self.link = Some(link.clone());
        Ok((link, found.health.viewer_session))
    }

    /// The viewer answered `session_changed` (or was found on another
    /// login): pin its login now, or keep the old pin while it is logged
    /// out, so that the next login is reported as well.
    fn repin(&mut self, current: Option<String>) {
        if current.is_some() {
            self.pinned = current;
        }
    }

    /// Call a tool. A connection that fails or a refused token makes it look
    /// for the viewer again, once; the call goes to that viewer only when it
    /// is on the pinned login, or when nothing is pinned.
    pub fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
        let spec = protocol::tool(tool).ok_or_else(|| ToolError::new(ErrorCode::UnknownTool, format!("unknown tool `{tool}`")))?;
        for attempt in 0..2 {
            let fresh = self.link.is_none();
            let (link, current) = match &self.link {
                Some(link) if link.binding && self.pinned.is_none() => {
                    // nothing pinned yet: learn the login before the call
                    match link.client.health() {
                        Ok(health) => (link.clone(), health.viewer_session),
                        Err(e) if lost(&e) && attempt == 0 => {
                            self.link = None;
                            continue;
                        }
                        Err(e) => {
                            self.link = None;
                            return Err(unavailable(e));
                        }
                    }
                }
                Some(link) => (link.clone(), None),
                None => self.connect()?,
            };
            if fresh || self.pinned.is_none() {
                match &self.pinned {
                    Some(pin) if current.as_deref() != Some(pin.as_str()) => {
                        let e = found_elsewhere(current.as_deref());
                        self.repin(current);
                        return Err(e);
                    }
                    Some(_) => {}
                    None if link.binding => self.pinned = current,
                    None => {}
                }
            }
            if !binding::supports(&link.capabilities, spec) {
                return Err(binding::unsupported(spec));
            }
            binding::check_arguments(&link.capabilities, spec, &arguments)?; // Folderstorm: receipts
            let session = if link.binding { self.pinned.as_deref() } else { None };
            match link.client.call_in(tool, arguments.clone(), session) {
                Ok(result) => return Ok(result),
                // the call never reached the viewer: look for it again
                Err(e) if lost(&e) && attempt == 0 => self.link = None,
                Err(ClientError::Tool(e)) if e.code == ErrorCode::SessionChanged => {
                    self.repin(e.viewer_session.clone());
                    return Err(e);
                }
                Err(ClientError::Tool(e)) => return Err(e),
                Err(other) => {
                    self.link = None;
                    return Err(unavailable(other));
                }
            }
        }
        Err(ToolError::new(ErrorCode::Unavailable, NotFound::NoAnswer.to_string()))
    }
}

/// A refused connection or token: the request never reached a viewer.
fn lost(e: &ClientError) -> bool {
    matches!(
        e,
        ClientError::Connect(_)
            | ClientError::Tool(ToolError {
                code: ErrorCode::Unauthorized,
                ..
            })
    )
}

fn unavailable(e: ClientError) -> ToolError {
    match e {
        ClientError::Tool(e) => e,
        other => ToolError::new(ErrorCode::Unavailable, other.to_string()),
    }
}
