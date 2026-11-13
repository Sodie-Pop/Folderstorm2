//! Folderstorm: the tools of receipts, plans and operations
//! (folderstorm/plans/mcp-receipts.md, sections 4 to 6). Kept apart from
//! the other groups' catalogues; [`super::all`] lists every group.
//!
//! - `assistant_operation_status` (`receipts`): one receipt, or the newest
//!   of this login. No class, like `viewer_status`: it only reads back what
//!   the assistant itself asked for, and gives names only when Read is
//!   Allow. Bound to the login, so a receipt of another login answers
//!   `not_found`.
//! - `inventory_preview_batch_rename` / `inventory_preview_batch_move`
//!   (`plans`): reads that plan 1 to 50 entries as the native review does
//!   and keep the plan for 5 minutes; `inventory_execute_plan` runs a plan
//!   once, with the plan's own class (Edit for a rename, Move for a move),
//!   which only the viewer knows: the listener refuses it only when both
//!   are on Never ([`any_decision`]).

use super::{CHANGE_MAX, CHANGE_WAIT, ToolSpec};
use crate::binding;
use crate::policy::{Class, Decision, Permissions};
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

pub const PREVIEW_RENAME: &str = "inventory_preview_batch_rename";
pub const PREVIEW_MOVE: &str = "inventory_preview_batch_move";
pub const EXECUTE_PLAN: &str = "inventory_execute_plan";

/// Whether `spec`'s class comes from its plan.
pub fn from_plan(spec: &ToolSpec) -> bool {
    spec.name == EXECUTE_PLAN
}

/// The listener's fast decision for a call whose class comes from its plan:
/// refused only when every class it may need is on Never.
pub fn any_decision(permissions: &Permissions, spec: &ToolSpec) -> Decision {
    let decisions: Vec<Decision> = spec.classes().map(|c| permissions.decide(c)).collect();
    if decisions.iter().all(|d| *d == Decision::Refuse) {
        Decision::Refuse
    } else if decisions.contains(&Decision::Ask) {
        Decision::Ask
    } else {
        Decision::Run
    }
}

fn ids(what: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": CHANGE_MAX, "uniqueItems": true, "description": what})
}

fn preview_rename_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "items": {"type": "array", "minItems": 1, "maxItems": CHANGE_MAX, "description": "Each entry and its new name (1 to 63 characters, no `|`).",
                "items": {"type": "object", "properties": {"id": {"type": "string"}, "name": {"type": "string"}}, "required": ["id", "name"], "additionalProperties": false}},
            "ids": ids("Instead of `items`: the entries to rename with `pattern`, in this order."),
            "pattern": {"type": "string", "description": "With `ids`: the new name, where {name} is the current name and {n} the entry's number in `ids` (from 1), e.g. \"Sorted {n} - {name}\"."}
        },
        "additionalProperties": false
    })
}

fn preview_move_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ids": ids("The items' and folders' UUIDs."),
            "folder_id": {"type": "string", "description": "The destination folder's UUID."},
            "path": {"type": "string", "description": "The destination folder's path, e.g. \"Clothes/Old\"."}
        },
        "required": ["ids"],
        "additionalProperties": false
    })
}

fn execute_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "plan_id": {"type": "string", "description": "The plan_id a preview answered with (valid 5 minutes, once)."}
        },
        "required": ["plan_id"],
        "additionalProperties": false
    })
}

/// The tools of this file.
pub const ASSISTANT_TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "assistant_operation_status",
        description: "What became of a change you asked for: its state (waiting, asking, queued, running, done, failed, refused, cancelled, stopped, stale, unconfirmed), the user's decision, what was sent and what the server said, by operation_id or client_request_id. Without arguments: your newest 20 operations on this login. Use it after a timeout or a lost answer instead of retrying. Names only when inventory reads are allowed.",
        class: None,
        also: None,
        schema: status_schema,
        group: binding::RECEIPTS,
        wait: Duration::ZERO,
    },
    ToolSpec {
        name: PREVIEW_RENAME,
        description: "Plan renaming 1 to 50 entries, without changing anything: explicit names (`items`) or one pattern ({name}, {n}). Answers every row, its name before and after or why it is left out (system folders, links, no Modify…), and a plan_id to run it with inventory_execute_plan within 5 minutes, plus whether running it is allowed, asks the user, or is refused (`execution`).",
        class: Some(Class::Read),
        also: None,
        schema: preview_rename_schema,
        group: binding::PLANS,
        wait: Duration::ZERO,
    },
    ToolSpec {
        name: PREVIEW_MOVE,
        description: "Plan moving 1 to 50 entries into one folder, without changing anything. Answers every row (where it goes, or why it stays: system, locked, worn entries…) and a plan_id to run it with inventory_execute_plan within 5 minutes. To delete, use inventory_trash.",
        class: Some(Class::Read),
        also: None,
        schema: preview_move_schema,
        group: binding::PLANS,
        wait: Duration::ZERO,
    },
    ToolSpec {
        name: EXECUTE_PLAN,
        description: "Run a plan from inventory_preview_batch_rename or inventory_preview_batch_move, once, as one change: it needs the user's permission for renaming (Edit) or moving (Move) and may ask once for the whole plan. It is planned again first: if anything changed since the preview it answers `stale`, sends nothing and the plan is gone (preview again). Executing a plan again answers its first operation.",
        class: Some(Class::Edit),
        also: Some(Class::Move),
        schema: execute_schema,
        group: binding::PLANS,
        wait: CHANGE_WAIT,
    },
];

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

    #[test]
    fn previews_are_reads_and_execute_takes_its_plans_class() {
        use crate::policy::Level;
        for name in [PREVIEW_RENAME, PREVIEW_MOVE] {
            let t = protocol::tool(name).expect("listed");
            assert_eq!((t.class, t.changes(), t.group), (Some(Class::Read), false, binding::PLANS));
        }
        let execute = protocol::tool(EXECUTE_PLAN).expect("listed");
        assert!(execute.changes() && from_plan(execute) && execute.bound());
        assert!(execute.input_schema()["properties"].get(CLIENT_REQUEST_ID).is_some());
        let mut p = Permissions::default();
        assert_eq!(any_decision(&p, execute), Decision::Refuse, "both on Never");
        p.set(Class::Move, Level::Allow);
        assert_eq!(any_decision(&p, execute), Decision::Run, "a move plan may run");
        p.set(Class::Edit, Level::Ask);
        assert_eq!(any_decision(&p, execute), Decision::Ask);
        assert_eq!(crate::ErrorCode::Stale.for_sidecar(None), crate::ErrorCode::NotAllowed);
        assert_eq!(
            crate::ErrorCode::Stale.for_sidecar(Some(&binding::capabilities())),
            crate::ErrorCode::Stale
        );
    }
}
