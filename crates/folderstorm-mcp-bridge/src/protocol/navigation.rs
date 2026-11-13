//! Folderstorm: the navigation tools of phase 3, part 4
//! (folderstorm/plans/mcp.md, "Phase 3, part 4: navigation"), after
//! Folderstorm (C++)'s `tools/fs-mcp/internal/mcptools/navigation.go` and
//! `doc/plans/mcp-navigation.md`. Kept apart from the other groups'
//! catalogues; [`super::all`] lists every group.
//!
//! - `navigation_status` (Read): where the avatar is, and how the
//!   assistant's last move or teleport ended.
//! - `navigation_move_to`, `navigation_flying_set`, `navigation_teleport`
//!   (the `navigation` class, "Move my avatar"): changes, with receipts. A
//!   teleport always asks ([`always_asks`]) and needs a
//!   `client_request_id` ([`needs_client_request_id`]); a move and a
//!   teleport answer early for a sidecar that declares `operations`
//!   ([`answers_early`]).
//! - `navigation_stop` (no class, bound to the login, like
//!   `assistant_operation_cancel`): stops the assistant's own move, or asks
//!   the viewer to cancel its own teleport.

use super::ToolSpec;
use crate::binding;
use crate::policy::Class;
use serde_json::{Value, json};
use std::time::Duration;

pub const STATUS: &str = "navigation_status";
pub const MOVE_TO: &str = "navigation_move_to";
pub const FLYING_SET: &str = "navigation_flying_set";
pub const TELEPORT: &str = "navigation_teleport";
pub const STOP: &str = "navigation_stop";

/// The farthest point a move goes to, in metres (flat for a walk, in 3D for
/// a flight).
pub const MOVE_DISTANCE_MAX: f64 = 64.0;
/// How close to the target a move ends, in metres: least, most, default.
pub const STOP_DISTANCE: (f64, f64) = (0.25, 4.0);
pub const STOP_DISTANCE_DEFAULT: f64 = 1.0;
/// How long a move may take, in seconds: most and default (least 1).
pub const MOVE_TIMEOUT_MAX: u64 = 30;
pub const MOVE_TIMEOUT_DEFAULT: u64 = 30;
/// The highest altitude a move or a teleport goes to, in metres.
pub const HEIGHT_MAX: f64 = 4096.0;
/// The least time between two teleports the assistant submits.
pub const TELEPORT_INTERVAL: Duration = Duration::from_secs(15);
/// How long a submitted teleport is followed before it answers
/// `unconfirmed` (it may still complete).
pub const TELEPORT_WAIT: Duration = Duration::from_secs(60);

/// Whether a call of `tool` asks the user even on Allow: a teleport.
pub fn always_asks(tool: &str) -> bool {
    tool == TELEPORT
}

/// Whether a call of `tool` must give a `client_request_id`: a teleport,
/// whose effect cannot be looked at afterwards (decision 3 of
/// folderstorm/plans/mcp-receipts.md).
pub fn needs_client_request_id(tool: &str) -> bool {
    tool == TELEPORT
}

/// Whether a call of `tool` answers as soon as the viewer took it.
pub fn answers_early(tool: &str) -> bool {
    tool == MOVE_TO || tool == TELEPORT
}

/// Whether `tool` is one of this group's.
pub fn handles(tool: &str) -> bool {
    NAVIGATION_TOOLS.iter().any(|t| t.name == tool)
}

fn no_arguments() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn vector(what: &str) -> Value {
    json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": what})
}

fn move_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "position": vector("Where to, in this region's coordinates (metres): [x, y, z] as world_location's `position` gives the avatar's centre."),
            "global_position": vector("Instead of `position`: where to in global coordinates (metres), [x, y, z] as world_location's `global`. It must lie in the avatar's region."),
            "mode": {"type": "string", "enum": ["walk", "fly"], "default": "walk",
                "description": "walk: on the ground (not while flying); fly: takes off if needed and hovers at the point."},
            "stop_distance": {"type": "number", "minimum": STOP_DISTANCE.0, "maximum": STOP_DISTANCE.1, "default": STOP_DISTANCE_DEFAULT,
                "description": "How close to the point the move ends, in metres."},
            "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": MOVE_TIMEOUT_MAX, "default": MOVE_TIMEOUT_DEFAULT,
                "description": "The move stops after this long."}
        },
        "additionalProperties": false
    })
}

fn flying_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"flying": {"type": "boolean", "description": "true: take off; false: land."}},
        "required": ["flying"],
        "additionalProperties": false
    })
}

fn teleport_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "global_position": vector("Where to, in global coordinates (metres): x and y of the grid, z the altitude (0 to 4096).")
        },
        "required": ["global_position"],
        "additionalProperties": false
    })
}

fn stop_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"operation_id": {"type": "string", "description": "The operation_id navigation_move_to or navigation_teleport answered with."}},
        "required": ["operation_id"],
        "additionalProperties": false
    })
}

/// A change of this group (one constructor for the fields they share).
const fn change(name: &'static str, description: &'static str, schema: fn() -> Value, wait: Duration) -> ToolSpec {
    ToolSpec {
        name,
        description,
        class: Some(Class::Navigation),
        also: None,
        schema,
        group: binding::NAVIGATION,
        wait,
    }
}

/// The navigation tools.
pub const NAVIGATION_TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: STATUS,
        description: "Where the avatar is (as world_location), whether it flies, sits or teleports, whether a walk of the viewer's own runs, the assistant's move or teleport under way, and how the last one ended (arrived, or why it stopped). Changes nothing.",
        class: Some(Class::Read),
        also: None,
        schema: no_arguments,
        group: binding::NAVIGATION,
        wait: Duration::ZERO,
    },
    change(
        MOVE_TO,
        "Walk or fly the avatar to a point of its region, at most 64 m away, through the viewer's own autopilot (it goes around what it can; arrival is not guaranteed). Needs the user's permission to move the avatar, and may ask first. Answers at once with an operation_id; poll navigation_status or assistant_operation_status. It stops on its timeout, any movement key the user presses, another walk the user starts, a teleport, a logout or another character, and with navigation_stop. Refused while sitting, while teleporting, during a walk or follow of the viewer's own, or where the viewer would refuse (no-fly land, RLV restrictions).",
        move_schema,
        Duration::from_secs(MOVE_TIMEOUT_MAX + 5),
    ),
    change(
        FLYING_SET,
        "Take off (true) or land (false), as the viewer's Fly button does. Refused where taking off is not allowed (no-fly land, RLV @fly, sitting) and during a walk. Does not move the avatar anywhere.",
        flying_schema,
        Duration::from_secs(5),
    ),
    change(
        TELEPORT,
        "Teleport the avatar to a global position, through the viewer's own teleport (its RLV restrictions and content rating checks apply). Always asks the user first. Requires a client_request_id: sent again with the same id, it returns the first request's receipt and never teleports twice. Answers at once with an operation_id; poll navigation_status. Once submitted it is never sent again, and it cannot be reliably cancelled. One at a time, 15 seconds at least between two.",
        teleport_schema,
        Duration::from_secs(TELEPORT_WAIT.as_secs() + 5),
    ),
    ToolSpec {
        name: STOP,
        description: "Stop your own move (navigation_move_to) by its operation_id, or, waiting for the user's answer or its turn, cancel your move or teleport. A teleport already submitted can only be asked to cancel, as the teleport screen's Cancel: it may still complete. Never stops a walk the user started.",
        class: None,
        also: None,
        schema: stop_schema,
        group: binding::NAVIGATION,
        wait: Duration::ZERO,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol;
    use crate::receipts::CLIENT_REQUEST_ID;

    #[test]
    fn the_catalogue_has_its_classes_and_bounds() {
        assert_eq!(NAVIGATION_TOOLS.len(), 5);
        for t in NAVIGATION_TOOLS {
            assert!(t.name.starts_with("navigation_"), "{}", t.name);
            assert_eq!(t.group, binding::NAVIGATION);
            assert!(t.bound(), "{} belongs to one login", t.name);
            assert_eq!((t.schema)()["additionalProperties"], false);
            assert!(protocol::tool(t.name).is_some(), "{} reachable", t.name);
            assert!(handles(t.name));
        }
        let status = protocol::tool(STATUS).expect("listed");
        assert_eq!((status.class, status.changes()), (Some(Class::Read), false));
        let stop = protocol::tool(STOP).expect("listed");
        assert!(stop.class.is_none() && !stop.changes() && stop.bound());
        for name in [MOVE_TO, FLYING_SET, TELEPORT] {
            let t = protocol::tool(name).expect("listed");
            assert_eq!(t.classes().collect::<Vec<_>>(), vec![Class::Navigation], "{name}");
            assert!(t.changes() && t.wait >= Duration::from_secs(5), "{name}");
        }
        let schema = move_schema();
        assert_eq!(schema["properties"]["timeout_seconds"]["maximum"], MOVE_TIMEOUT_MAX);
        assert_eq!(schema["properties"]["stop_distance"]["minimum"], STOP_DISTANCE.0);
        assert!(protocol::tool(MOVE_TO).expect("listed").wait.as_secs() > MOVE_TIMEOUT_MAX);
        assert!(protocol::tool(TELEPORT).expect("listed").wait > TELEPORT_WAIT);
    }

    #[test]
    fn a_teleport_always_asks_and_needs_its_request_id() {
        assert!(protocol::always_asks(TELEPORT, &json!({})));
        assert!(!protocol::always_asks(MOVE_TO, &json!({})));
        let teleport = protocol::tool(TELEPORT).expect("listed");
        assert!(teleport.needs_client_request_id());
        let schema = teleport.input_schema();
        assert_eq!(schema["required"], json!(["global_position", CLIENT_REQUEST_ID]));
        assert!(schema["properties"].get(CLIENT_REQUEST_ID).is_some());
        let walk = protocol::tool(MOVE_TO).expect("listed");
        assert!(!walk.needs_client_request_id());
        assert!(walk.input_schema().get("required").is_none(), "a move's id stays optional");
        assert!(walk.input_schema()["properties"].get(CLIENT_REQUEST_ID).is_some());
    }

    #[test]
    fn a_move_and_a_teleport_answer_early() {
        use protocol::assistant::answers_early as early;
        assert!(early(protocol::tool(MOVE_TO).expect("listed"), &json!({})));
        assert!(early(protocol::tool(TELEPORT).expect("listed"), &json!({})));
        assert!(!early(protocol::tool(FLYING_SET).expect("listed"), &json!({})));
        assert!(!early(protocol::tool(STATUS).expect("listed"), &json!({})));
    }
}
