//! Finding the viewer: the discovery files of the settings folder (every
//! profile's viewer writes there), the viewers whose bridge answers with
//! our API version; the one chosen ([`Scope`], `viewer_select`), then the
//! login binding and the capability check of each call ([`Viewer`]).

use folderstorm_mcp_bridge::binding::{self, Capabilities};
use folderstorm_mcp_bridge::client::{Client, ClientError};
use folderstorm_mcp_bridge::discovery;
use folderstorm_mcp_bridge::protocol::{self, Health};
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::{Value, json};
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

/// What this sidecar may talk to (folderstorm/plans/mcp-routing.md,
/// section 2): any viewer of the folder, one profile's (`--profile`), or
/// one process (`--viewer-pid`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Any,
    /// A profile's name, compared without case; `main` for the main one.
    Profile(String),
    Pid(u32),
}

/// The name the main profile is listed and chosen by.
pub const MAIN_PROFILE: &str = "main";

/// A viewer's profile as listed: its `--profile`, or [`MAIN_PROFILE`]
/// (a viewer older than session routing says none: it wrote its file from
/// the main profile, the only one that wrote here).
pub fn profile_of(health: &Health) -> &str {
    health.profile.as_deref().unwrap_or(MAIN_PROFILE)
}

fn same_profile(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl Scope {
    fn admits(&self, pid: u32, health: &Health) -> bool {
        match self {
            Scope::Any => true,
            Scope::Profile(name) => same_profile(profile_of(health), name),
            Scope::Pid(wanted) => pid == *wanted,
        }
    }

    fn pid(&self) -> Option<u32> {
        match self {
            Scope::Pid(pid) => Some(*pid),
            _ => None,
        }
    }
}

/// A viewer found, with the health it answered.
pub struct Found {
    pub client: Client,
    pub pid: u32,
    pub health: Health,
}

/// Every viewer of `dir` within `scope` that answers with our API version,
/// newest first. An empty list is [`NotFound`].
pub fn candidates(dir: Option<&Path>, scope: &Scope, probe: bool) -> Result<Vec<Found>, NotFound> {
    let dir = dir.ok_or(NotFound::NoFolder)?;
    let files = discovery::find(dir, scope.pid());
    if files.is_empty() {
        return Err(NotFound::NoFile);
    }
    let mut failure = NotFound::NoAnswer;
    let mut found = Vec::new();
    for (_, file) in files {
        let mut client = Client::from_discovery(&file).with_timeout(PROBE_TIMEOUT);
        if probe {
            client = client.probe();
        }
        match client.health() {
            Ok(health) if health.api == folderstorm_mcp_bridge::API_VERSION => {
                if scope.admits(file.pid, &health) {
                    let client = Client::from_discovery(&file);
                    found.push(Found {
                        client: if probe { client.probe() } else { client },
                        pid: file.pid,
                        health,
                    });
                }
            }
            Ok(health) => failure = NotFound::OtherVersion(health.api),
            Err(_) => {}
        }
    }
    if found.is_empty() { Err(failure) } else { Ok(found) }
}

/// The viewer of `dir` within `scope`, checked with a health request: the
/// newest that answers (`--diagnose`).
pub fn find(dir: Option<&Path>, scope: &Scope, probe: bool) -> Result<Found, NotFound> {
    candidates(dir, scope, probe).map(|mut all| all.swap_remove(0))
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

/// The sidecar's link to the viewer it chose, found again when that viewer
/// restarts (new port, new token) or starts after the sidecar.
///
/// **Choosing** (folderstorm/plans/mcp-routing.md, section 2): with one
/// viewer running (within the [`Scope`]) it is chosen at the first call;
/// with several, nothing is sent until the assistant chooses one
/// ([`Viewer::select`], after [`Viewer::list`]): `target_required`. The
/// choice is a profile; a lost connection looks again for that profile
/// only. A character's viewer that is gone answers `unavailable`, **never**
/// does a call go to another character.
///
/// **Login binding** (folderstorm/plans/mcp-receipts.md, section 2): the
/// sidecar pins the chosen viewer's `viewer_session` and sends it with
/// every call; the viewer refuses a call pinned to another login with
/// `session_changed`. The sidecar then pins the new login (or keeps the old
/// pin while the viewer is logged out, so that the next login is reported
/// too) and **never sends the call again**: the assistant looks again and
/// decides. A viewer found again after a refused connection or token gets
/// the call only when it is on the pinned login; otherwise the answer is
/// `session_changed` and nothing is sent. With nothing pinned yet (logged
/// out, or a viewer older than login binding) calls are unbound, as before.
pub struct Viewer {
    dir: Option<PathBuf>,
    scope: Scope,
    /// The profile chosen (as its viewer names it).
    choice: Option<String>,
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

/// Several viewers and no choice.
fn target_required(found: &[Found]) -> ToolError {
    let mut profiles: Vec<&str> = found.iter().map(|f| profile_of(&f.health)).collect();
    profiles.sort_unstable();
    profiles.dedup();
    ToolError::new(
        ErrorCode::TargetRequired,
        format!(
            "{} viewers are running (profiles: {}): nothing was sent. Ask the user which character to use, then call viewer_select (viewer_list shows them)",
            found.len(),
            profiles.join(", ")
        ),
    )
}

/// The chosen character's viewer is not running.
fn choice_gone(choice: &str) -> ToolError {
    ToolError::new(
        ErrorCode::Unavailable,
        format!(
            "the chosen character's viewer (profile {}) is not running, or its local assistant access is off: nothing was sent, and no other character was used. Start it, or call viewer_list and viewer_select",
            choice
        ),
    )
}

impl Viewer {
    /// A sidecar limited to `pid` when given (`--viewer-pid`).
    pub fn new(dir: Option<PathBuf>, pid: Option<u32>) -> Viewer {
        Viewer::scoped(dir, pid.map_or(Scope::Any, Scope::Pid))
    }

    /// A sidecar limited to `scope` (`--profile`, `--viewer-pid`).
    pub fn scoped(dir: Option<PathBuf>, scope: Scope) -> Viewer {
        Viewer {
            dir,
            scope,
            choice: None,
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

    /// The profile chosen, if any.
    pub fn chosen(&self) -> Option<&str> {
        self.choice.as_deref()
    }

    fn link_of(&self, found: &Found) -> Link {
        let mut client = found.client.clone();
        if let Some(name) = &self.name {
            client = client.with_client_name(name);
        }
        let capabilities = found.health.capabilities();
        Link {
            binding: capabilities.contains_key(binding::LOGIN_BINDING),
            client,
            capabilities,
        }
    }

    /// Look for the viewer chosen (or the only one): the link, and the
    /// login it is on.
    fn connect(&mut self) -> Result<(Link, Option<String>), ToolError> {
        let all = match candidates(self.dir.as_deref(), &self.scope, false) {
            Ok(all) => all,
            Err(e) => {
                return Err(match &self.choice {
                    Some(choice) if e != NotFound::NoFolder => choice_gone(choice),
                    _ => ToolError::new(ErrorCode::Unavailable, e.to_string()),
                });
            }
        };
        let found = match &self.choice {
            // that character only: a restart of it is found, another never
            Some(choice) => all
                .into_iter()
                .find(|f| same_profile(profile_of(&f.health), choice))
                .ok_or_else(|| choice_gone(choice))?,
            None if all.len() == 1 => all.into_iter().next().ok_or_else(choice_gone_any)?,
            None => return Err(target_required(&all)),
        };
        self.choice = Some(profile_of(&found.health).to_owned());
        let link = self.link_of(&found);
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

    /// `viewer_list`: every viewer within the scope that answers, with
    /// which one is chosen. Names only from viewers whose Read is Allow.
    pub fn list(&self) -> Result<Value, ToolError> {
        let found = match candidates(self.dir.as_deref(), &self.scope, false) {
            Ok(found) => found,
            Err(NotFound::NoFolder) => return Err(ToolError::new(ErrorCode::Unavailable, NotFound::NoFolder.to_string())),
            Err(_) => Vec::new(),
        };
        let rows: Vec<Value> = found.iter().map(|f| self.row(f)).collect();
        Ok(json!({
            "viewers": rows,
            "count": rows.len(),
            "chosen": self.choice.clone(),
            "selection_required": self.choice.is_none() && found.len() > 1,
            "scope": match &self.scope {
                Scope::Any => Value::Null,
                Scope::Profile(name) => json!({"profile": name}),
                Scope::Pid(pid) => json!({"pid": pid}),
            },
        }))
    }

    fn row(&self, f: &Found) -> Value {
        let profile = profile_of(&f.health);
        let chosen = self.choice.as_deref().is_some_and(|c| same_profile(c, profile));
        let mut row = json!({
            "profile": profile,
            "pid": f.pid,
            "managed": f.health.managed,
            "logged_in": f.health.viewer_session.is_some(),
            "selected": chosen,
            "read_allowed": f.health.read_allowed,
        });
        if let Some(session) = &f.health.viewer_session {
            row["viewer_session"] = json!(session);
        }
        if let Some(name) = &f.health.character {
            row["character"] = json!(name);
        }
        let missing = binding::missing(&f.health.capabilities());
        if !missing.is_empty() {
            row["missing_capabilities"] = json!(missing);
        }
        row
    }

    /// `viewer_select`: choose the viewer of a profile, of an exact login
    /// (`viewer_session`) or of a process; from now on every call goes to
    /// it, pinned to its login.
    pub fn select(&mut self, arguments: &Value) -> Result<Value, ToolError> {
        let invalid = |m: &str| ToolError::new(ErrorCode::InvalidArguments, m.to_owned());
        let object = arguments.as_object().ok_or_else(|| invalid("the arguments must be an object"))?;
        if object.keys().any(|k| !matches!(k.as_str(), "profile" | "viewer_session" | "pid")) {
            return Err(invalid("viewer_select takes `profile`, `viewer_session` or `pid`"));
        }
        if object.len() != 1 {
            return Err(invalid("give exactly one of `profile`, `viewer_session` or `pid`"));
        }
        let profile = object
            .get("profile")
            .map(|v| v.as_str().ok_or_else(|| invalid("`profile` must be a string")))
            .transpose()?;
        let session = object
            .get("viewer_session")
            .map(|v| v.as_str().ok_or_else(|| invalid("`viewer_session` must be a string")))
            .transpose()?;
        let pid = object
            .get("pid")
            .map(|v| {
                v.as_u64()
                    .and_then(|p| u32::try_from(p).ok())
                    .ok_or_else(|| invalid("`pid` must be a process number"))
            })
            .transpose()?;
        let found = match candidates(self.dir.as_deref(), &self.scope, false) {
            Ok(found) => found,
            Err(NotFound::NoFolder) => return Err(ToolError::new(ErrorCode::Unavailable, NotFound::NoFolder.to_string())),
            Err(_) => Vec::new(),
        };
        let wanted = found.into_iter().find(|f| {
            profile.is_some_and(|p| same_profile(profile_of(&f.health), p))
                || session.is_some_and(|s| f.health.viewer_session.as_deref() == Some(s))
                || pid.is_some_and(|p| f.pid == p)
        });
        let Some(found) = wanted else {
            let limited = if self.scope == Scope::Any {
                String::new()
            } else {
                " within the viewers this connector is set to (--profile / --viewer-pid)".to_owned()
            };
            return Err(ToolError::new(
                ErrorCode::NotFound,
                format!("no running viewer has that profile, login or process{limited}: call viewer_list; nothing changed"),
            ));
        };
        self.choice = Some(profile_of(&found.health).to_owned());
        let link = self.link_of(&found);
        // the login it is on now (logged out: the first login seen)
        self.pinned = if link.binding { found.health.viewer_session.clone() } else { None };
        self.link = Some(link);
        Ok(self.row(&found))
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
        Err(match &self.choice {
            Some(choice) => choice_gone(choice),
            None => ToolError::new(ErrorCode::Unavailable, NotFound::NoAnswer.to_string()),
        })
    }
}

fn choice_gone_any() -> ToolError {
    ToolError::new(ErrorCode::Unavailable, NotFound::NoAnswer.to_string())
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
