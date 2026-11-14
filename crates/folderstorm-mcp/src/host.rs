//! The session host's MCP controller, from the sidecar
//! (folderstorm/plans/mcp-routing.md, section 3): its discovery file in the
//! same folder as the viewers' (`folderstorm2-controller-<pid>.json`), its
//! `host_session` pinned at the first call, and the `host_*` tools sent to
//! it. A separate authority: nothing here touches a viewer's link, pin or
//! token, and a viewer's tools never go to the host.

use folderstorm_mcp_bridge::client::{Client, ClientError};
use folderstorm_mcp_bridge::controller::{self, ControllerHealth};
use folderstorm_mcp_bridge::discovery::{self, Kind};
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

/// The host's controller is off, or no host runs.
fn not_found() -> ToolError {
    ToolError::new(
        ErrorCode::Unavailable,
        format!(
            "no {} session host with its MCP controller turned on was found: the user turns it on in the host's settings (MCP controller). Nothing was done",
            folderstorm_brand::PRODUCT
        ),
    )
}

/// The controller found again after a lost connection is another start.
fn found_elsewhere(current: &str) -> ToolError {
    let mut e = controller::host_changed(current);
    e.message = format!("the connection to the session host was lost and found again; {}", e.message);
    e
}

/// The sidecar's link to the host's controller.
pub struct Host {
    dir: Option<PathBuf>,
    link: Option<Client>,
    name: Option<String>,
    /// The controller's start the calls are bound to.
    pinned: Option<String>,
}

impl Host {
    pub fn new(dir: Option<PathBuf>) -> Host {
        Host {
            dir,
            link: None,
            name: None,
            pinned: None,
        }
    }

    pub fn set_client_name(&mut self, name: &str) {
        self.name = Some(name.to_owned());
        self.link = self.link.take().map(|c| c.with_client_name(name));
    }

    /// The `host_session` the calls are bound to, if any.
    pub fn pinned(&self) -> Option<&str> {
        self.pinned.as_deref()
    }

    /// The controller of the folder: the newest file that answers with our
    /// API, and its health.
    fn connect(&mut self) -> Result<(Client, ControllerHealth), ToolError> {
        let dir = self.dir.as_deref().ok_or_else(not_found)?;
        for (_, file) in discovery::find_kind(Kind::Controller, dir, None) {
            let client = Client::from_discovery(&file).with_timeout(PROBE_TIMEOUT);
            let Ok(value) = client.get("/v1/health") else { continue };
            let Ok(health) = serde_json::from_value::<ControllerHealth>(value) else {
                continue;
            };
            if health.api != folderstorm_mcp_bridge::API_VERSION {
                continue;
            }
            if health.capabilities.get(controller::CONTROLLER).is_none_or(|v| *v < 1) {
                return Err(ToolError::new(
                    ErrorCode::Unsupported,
                    format!(
                        "the running {} session host is older than this connector: update it; nothing was done",
                        folderstorm_brand::PRODUCT
                    ),
                ));
            }
            let mut client = Client::from_discovery(&file);
            if let Some(name) = &self.name {
                client = client.with_client_name(name);
            }
            self.link = Some(client.clone());
            return Ok((client, health));
        }
        Err(not_found())
    }

    /// Call a `host_*` tool, bound to the controller's start it first saw.
    /// A host that restarted answers `session_changed` once and the call is
    /// never sent again.
    pub fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
        if controller::host_tool(tool).is_none() {
            return Err(ToolError::new(ErrorCode::UnknownTool, format!("unknown tool `{tool}`")));
        }
        for attempt in 0..2 {
            let client = match self.link.clone() {
                Some(client) => client,
                None => {
                    let (client, health) = self.connect()?;
                    match &self.pinned {
                        Some(pin) if *pin != health.host_session => {
                            self.pinned = Some(health.host_session.clone());
                            return Err(found_elsewhere(&health.host_session));
                        }
                        Some(_) => {}
                        None => self.pinned = Some(health.host_session),
                    }
                    client
                }
            };
            match client.call_in(tool, arguments.clone(), self.pinned.as_deref()) {
                Ok(result) => return Ok(result),
                Err(e) if lost(&e) && attempt == 0 => self.link = None,
                Err(ClientError::Tool(e)) if e.code == ErrorCode::SessionChanged => {
                    if e.viewer_session.is_some() {
                        self.pinned = e.viewer_session.clone();
                    }
                    return Err(e);
                }
                Err(ClientError::Tool(e)) => return Err(e),
                Err(other) => {
                    self.link = None;
                    return Err(ToolError::new(ErrorCode::Unavailable, other.to_string()));
                }
            }
        }
        Err(not_found())
    }
}

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
