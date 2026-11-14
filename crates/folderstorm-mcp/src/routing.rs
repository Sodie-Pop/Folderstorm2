//! The sidecar's own tools (folderstorm/plans/mcp-routing.md, section 2):
//! `viewer_list` and `viewer_select`, answered by the sidecar itself, never
//! sent to a viewer, and needing no permission class. With the viewers'
//! catalogue (`protocol`) and the host controller's (`controller`), every
//! name `tools/list` gives.

use folderstorm_mcp_bridge::{controller, protocol};
use serde_json::{Value, json};

/// A tool the sidecar answers itself.
pub struct LocalTool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
}

fn no_arguments() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn select_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "profile": {"type": "string", "description": "The viewer's profile, as viewer_list gives it (\"main\" for the main profile)."},
            "viewer_session": {"type": "string", "description": "An exact login from viewer_list: refused when that viewer is on another login now."},
            "pid": {"type": "integer", "minimum": 1, "description": "The viewer's process number, from viewer_list."}
        },
        "minProperties": 1,
        "maxProperties": 1,
        "additionalProperties": false
    })
}

pub const LOCAL_TOOLS: &[LocalTool] = &[
    LocalTool {
        name: "viewer_list",
        description: "The viewers running on this computer with local assistant access on, one per character (several when the session host runs characters): profile, process, logged in or not, its login (viewer_session), the character's name when that viewer allows reads, and which one this connector talks to. With several and none chosen, call viewer_select before any other tool.",
        schema: no_arguments,
    },
    LocalTool {
        name: "viewer_select",
        description: "Choose the viewer (character) every following call goes to, by profile, exact login (viewer_session) or process. Calls stay bound to it: if it logs out, logs in again or quits, calls are refused (session_changed or unavailable), never sent to another character. Ask the user which character when several run.",
        schema: select_schema,
    },
];

pub fn local_tool(name: &str) -> Option<&'static LocalTool> {
    LOCAL_TOOLS.iter().find(|t| t.name == name)
}

/// A name this sidecar lists: a viewer's tool, its own, or the host's.
pub fn known(name: &str) -> bool {
    protocol::tool(name).is_some() || local_tool(name).is_some() || controller::host_tool(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_name_is_in_two_catalogues() {
        let mut names: Vec<&str> = protocol::all().map(|t| t.name).collect();
        names.extend(LOCAL_TOOLS.iter().map(|t| t.name));
        names.extend(controller::HOST_TOOLS.iter().map(|t| t.name));
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
        assert!(known("viewer_select") && known("host_logout") && known("worn_items") && !known("viewer_delete"));
    }
}
