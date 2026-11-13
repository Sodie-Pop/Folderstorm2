//! Folderstorm: the tools of receipts, plans and operations
//! (folderstorm/plans/mcp-receipts.md, sections 4 to 6). Kept apart from
//! the other groups' catalogues; [`super::all`] lists every group.
//!
//! - `assistant_operation_status` (`receipts`): one receipt, or the newest
//!   of this login. No class, like `viewer_status`: it only reads back what
//!   the assistant itself asked for, and gives names only when Read is
//!   Allow. Bound to the login, so a receipt of another login answers
//!   `not_found`.

use super::ToolSpec;
use crate::binding;
use crate::receipts::{CLIENT_REQUEST_ID, CLIENT_REQUEST_ID_MAX};
use serde_json::{Value, json};
use std::time::Duration;

/// Whether the tools of `group` belong to one login although they need no
/// class ([`ToolSpec::bound`]).
pub fn bound_group(group: &str) -> bool {
    group == binding::RECEIPTS
}

/// The `client_request_id` every change takes ([`ToolSpec::input_schema`]).
pub fn client_request_id_schema() -> Value {
    json!({
        "type": "string", "minLength": 1, "maxLength": CLIENT_REQUEST_ID_MAX,
        "description": "Your own id for this request (1 to 64 printable ASCII characters). Sent again with the same arguments after a lost answer, it returns the first request's receipt instead of running again."
    })
}

fn status_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "operation_id": {"type": "string", "description": "The operation_id a change answered with (also in its errors and timeouts)."},
            CLIENT_REQUEST_ID: {"type": "string", "description": "Instead of operation_id: the client_request_id you gave the change."}
        },
        "additionalProperties": false
    })
}

/// The tools of this file.
pub const ASSISTANT_TOOLS: &[ToolSpec] = &[ToolSpec {
    name: "assistant_operation_status",
    description: "What became of a change you asked for: its state (waiting, asking, queued, running, done, failed, refused, cancelled, stopped, stale, unconfirmed), the user's decision, what was sent and what the server said, by operation_id or client_request_id. Without arguments: your newest 20 operations on this login. Use it after a timeout or a lost answer instead of retrying. Names only when inventory reads are allowed.",
    class: None,
    also: None,
    schema: status_schema,
    group: binding::RECEIPTS,
    wait: Duration::ZERO,
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol;

    #[test]
    fn the_status_is_bound_and_needs_no_class() {
        let status = protocol::tool("assistant_operation_status").expect("listed");
        assert!(status.class.is_none() && !status.changes());
        assert!(status.bound(), "a receipt belongs to one login");
        assert!(!protocol::tool("viewer_status").expect("listed").bound());
        // every change takes a client_request_id; reads do not
        let rename = protocol::tool("inventory_rename").expect("listed").input_schema();
        assert_eq!(rename["properties"][CLIENT_REQUEST_ID]["maxLength"], CLIENT_REQUEST_ID_MAX);
        assert_eq!(rename["additionalProperties"], false);
        let search = protocol::tool("inventory_search").expect("listed").input_schema();
        assert!(search["properties"].get(CLIENT_REQUEST_ID).is_none());
        assert!(
            status.input_schema()["properties"].get(CLIENT_REQUEST_ID).is_some(),
            "its own lookup key"
        );
    }
}
