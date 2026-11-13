//! Folderstorm2's local assistant bridge (FEATURES.md 4.1): the part of the
//! viewer that a local MCP sidecar talks to. Port of Folderstorm (C++)'s
//! loopback Event API bridge (`fseventapibridge.cpp`, `doc/fs-mcp.md`),
//! without any viewer dependency so that the sidecar and the tests can use
//! it too. The plan is `folderstorm/plans/mcp.md`.
//!
//! - [`policy`]: permission classes, levels, presets and the saved settings;
//! - [`token`]: the bearer token, compared in constant time;
//! - [`discovery`]: the `folderstorm2-mcp-<pid>.json` file the sidecar reads;
//! - [`protocol`]: the tool catalogue and the wire types;
//! - [`binding`]: the login binding (`viewer_session`) and the capability
//!   versions both sides advertise;
//! - [`receipts`]: the record of each change an assistant asked for;
//! - [`server`]: the 127.0.0.1 listener;
//! - [`client`]: the sidecar's side of the wire.

pub mod binding;
pub mod client;
pub mod discovery;
mod http;
pub mod policy;
pub mod protocol;
pub mod receipts;
pub mod server;
pub mod token;

pub use policy::{Class, Decision, Level, LocalAssistant, Permissions, Preset};
pub use protocol::{ErrorCode, ToolError};
pub use server::{Bridge, Pending};

/// Version of the wire API between the bridge and the sidecar; the sidecar
/// refuses a viewer whose `/v1/health` reports another major version.
pub const API_VERSION: u32 = 1;
