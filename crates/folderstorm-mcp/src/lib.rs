//! Folderstorm2's MCP sidecar (FEATURES.md 4.2), the Rust counterpart of
//! Folderstorm (C++)'s Go `tools/fs-mcp`: an MCP server over stdio that an
//! assistant (Claude Code, Cursor, Codex…) starts, which finds the running
//! viewer through its discovery file and calls its loopback bridge.
//!
//! The MCP layer is hand-written (JSON-RPC 2.0, newline-delimited, three
//! methods); `folderstorm/plans/mcp.md` says why no SDK is used.

pub mod connect;
pub mod diagnose;
pub mod host;
pub mod jsonrpc;
pub mod routing;
