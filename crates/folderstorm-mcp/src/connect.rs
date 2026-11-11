//! Finding the viewer: the discovery files of the settings folder, newest
//! first, the first whose bridge answers with our API version.

use folderstorm_mcp_bridge::client::{Client, ClientError};
use folderstorm_mcp_bridge::discovery;
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

/// The viewer of `dir` (or `pid`'s), checked with a health request.
pub fn find(dir: Option<&Path>, pid: Option<u32>, probe: bool) -> Result<(Client, u32), NotFound> {
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
                return Ok((if probe { client.probe() } else { client }, found.pid));
            }
            Ok(health) => failure = NotFound::OtherVersion(health.api),
            Err(_) => {}
        }
    }
    Err(failure)
}

/// The sidecar's link to the viewer, found again when the viewer restarts
/// (new port, new token) or starts after the sidecar.
pub struct Viewer {
    dir: Option<PathBuf>,
    pid: Option<u32>,
    client: Option<Client>,
    /// The MCP client's own name, for the viewer's Ask dialog.
    name: Option<String>,
}

impl Viewer {
    pub fn new(dir: Option<PathBuf>, pid: Option<u32>) -> Viewer {
        Viewer {
            dir,
            pid,
            client: None,
            name: None,
        }
    }

    /// Name the calls after the MCP client (`clientInfo.name`).
    pub fn set_client_name(&mut self, name: &str) {
        self.name = Some(name.to_owned());
        self.client = self.client.take().map(|c| c.with_client_name(name));
    }

    /// Call a tool; a connection that fails or a refused token makes it
    /// look for the viewer again, once.
    pub fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
        for attempt in 0..2 {
            let client = match &self.client {
                Some(client) => client.clone(),
                None => {
                    let (mut client, _) =
                        find(self.dir.as_deref(), self.pid, false).map_err(|e| ToolError::new(ErrorCode::Unavailable, e.to_string()))?;
                    if let Some(name) = &self.name {
                        client = client.with_client_name(name);
                    }
                    self.client = Some(client.clone());
                    client
                }
            };
            match client.call(tool, arguments.clone()) {
                Ok(result) => return Ok(result),
                Err(ClientError::Connect(_))
                | Err(ClientError::Tool(ToolError {
                    code: ErrorCode::Unauthorized,
                    ..
                })) if attempt == 0 => {
                    self.client = None;
                }
                Err(ClientError::Tool(e)) => return Err(e),
                Err(other) => {
                    self.client = None;
                    return Err(ToolError::new(ErrorCode::Unavailable, other.to_string()));
                }
            }
        }
        Err(ToolError::new(ErrorCode::Unavailable, NotFound::NoAnswer.to_string()))
    }
}
