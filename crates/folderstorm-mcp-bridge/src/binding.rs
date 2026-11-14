//! Login binding and capability versions (folderstorm/plans/mcp-receipts.md,
//! sections 2 and 3): the `viewer_session` that names one login of one
//! running bridge, and the map of tool groups each side implements.
//!
//! - **`viewer_session`**: `vs-` and 32 hex digits from the OS random
//!   source, new for each login generation the viewer reports and for each
//!   start of the bridge (its epoch: a new [`crate::Bridge`] remembers no
//!   session of an older one). Not the agent id, not the simulator's session
//!   id, and not a secret: the token is still required, and the value is
//!   useless elsewhere.
//! - **Capabilities**: `{group: version}`. The viewer advertises its own in
//!   `/v1/health` and `viewer_status`; the sidecar sends its own with every
//!   call (`CallRequest::features`). An addition keeps `API_VERSION` 1; a
//!   field whose meaning changes raises its group's version (the viewer then
//!   keeps the older meaning for a sidecar declaring the older version); a
//!   broken wire raises `API_VERSION`.

use crate::protocol::{ErrorCode, ToolError, ToolSpec};
use std::collections::BTreeMap;

/// `{group: version}`.
pub type Capabilities = BTreeMap<String, u32>;

/// Phases 1 and 2: the inventory and outfit reads and changes,
/// `viewer_status`, `wait_for_bake`.
pub const INVENTORY: &str = "inventory";
/// `viewer_session`, `CallRequest::session` and `session_changed`, with the
/// capability negotiation itself (`CallRequest::features`, `unsupported`).
pub const LOGIN_BINDING: &str = "login_binding";
/// Phase 3, part 1: the camera, pictures and thumbnails (#21).
pub const CAMERA: &str = "camera";
/// Phase 3, part 2: the inventory gaps (#25).
pub const INVENTORY_GAPS: &str = "inventory_gaps";
/// Phase 3, part 3: the world observations (#27).
pub const WORLD: &str = "world";
/// Session routing (folderstorm/plans/mcp-routing.md): the shared
/// discovery folder, and `profile`, `managed` and `character` in health and
/// `viewer_status`.
pub const SESSIONS: &str = "sessions";

/// Receipts (mcp-receipts.md, section 4): `operation_id`,
/// `client_request_id`, `assistant_operation_status`, the `conflict` code.
pub const RECEIPTS: &str = "receipts";
/// Batch plans (mcp-receipts.md, section 6): the previews,
/// `inventory_execute_plan`, the `stale` code.
pub const PLANS: &str = "plans";
/// Guarded operations (mcp-receipts.md, section 5): early answers,
/// `assistant_operation_cancel`, the `cancelled` code.
pub const OPERATIONS: &str = "operations";
/// Folderstorm: the navigation tools (phase 3, part 4): `navigation_*`, the
/// `navigation` permission class.
pub const NAVIGATION: &str = "navigation";
/// Folderstorm: content editing (mcp-content.md): notecard and script
/// reads, previews, saves and compiles, and the `Content` class.
pub const CONTENT: &str = "content";
/// Folderstorm: object inspection and lifecycle (mcp-objects.md): the
/// `object_*` tools and the `objects` and `take` classes.
pub const OBJECTS: &str = "objects";
/// Folderstorm: scripts inside objects (mcp-task-scripts.md): the
/// `object_script_*` tools, `script_debug_poll` and the `scripts` class.
pub const TASK_SCRIPTS: &str = "task_scripts";
/// Folderstorm: script testing (mcp-script-testing.md): touch, script
/// dialogs, channel lines, test recipes and the `interact` class.
pub const SCRIPT_TESTING: &str = "script_testing";
/// Folderstorm: building (mcp-building.md): the `build_*` tools, the
/// inventory permission tools and the `build` class.
pub const BUILDING: &str = "building";
/// Folderstorm: gestures (mcp-gestures.md): the `gesture_*` tools and the
/// `gestures` class.
pub const GESTURES: &str = "gestures";
/// Folderstorm: own posing (mcp-gestures.md): the `pose_*` tools and the
/// `pose` class.
pub const POSING: &str = "posing";

/// What this build implements. Later groups add theirs.
pub const CAPABILITIES: &[(&str, u32)] = &[
    (INVENTORY, 1),
    (CAMERA, 1),
    (INVENTORY_GAPS, 1),
    (WORLD, 1),
    (LOGIN_BINDING, 1),
    (SESSIONS, 1),
    (RECEIPTS, 1),       // Folderstorm: receipts (mcp-receipts.md, checkpoint 3)
    (PLANS, 1),          // Folderstorm: batch plans (checkpoint 4)
    (OPERATIONS, 1),     // Folderstorm: guarded operations (checkpoint 5)
    (NAVIGATION, 1),     // Folderstorm: navigation (phase 3, part 4)
    (CONTENT, 1),        // Folderstorm: content editing (mcp-content.md)
    (OBJECTS, 1),        // Folderstorm: object inspection and lifecycle (mcp-objects.md)
    (TASK_SCRIPTS, 1),   // Folderstorm: scripts inside objects (mcp-task-scripts.md)
    (SCRIPT_TESTING, 1), // Folderstorm: script testing (mcp-script-testing.md)
    (BUILDING, 1),       // Folderstorm: building (mcp-building.md)
    (GESTURES, 1),       // Folderstorm: gestures (mcp-gestures.md)
    (POSING, 1),         // Folderstorm: own posing (mcp-gestures.md)
];

/// [`CAPABILITIES`] as a map.
pub fn capabilities() -> Capabilities {
    CAPABILITIES
        .iter()
        .map(|(group, version)| ((*group).to_owned(), *version))
        .collect()
}

/// What a viewer that advertises no capabilities implements: phases 1 and
/// 2, without login binding.
pub fn assumed() -> Capabilities {
    [(INVENTORY.to_owned(), 1)].into_iter().collect()
}

/// Whether a viewer implementing `viewer` has `spec` (version 1 of its
/// group, or later).
pub fn supports(viewer: &Capabilities, spec: &ToolSpec) -> bool {
    viewer.get(spec.group).is_some_and(|version| *version >= 1)
}

/// The groups this build implements that `other` lacks, or knows only in
/// an older version.
pub fn missing(other: &Capabilities) -> Vec<&'static str> {
    CAPABILITIES
        .iter()
        .filter(|(group, version)| other.get(*group).is_none_or(|v| v < version))
        .map(|(group, _)| *group)
        .collect()
}

/// A fresh `viewer_session`.
pub fn new_viewer_session() -> std::io::Result<String> {
    random_id("vs-")
}

/// `prefix` and 32 hex digits from the OS random source: a
/// `viewer_session` (`vs-`), the host controller's `host_session` (`hc-`).
pub fn random_id(prefix: &str) -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(format!("no random source: {e}")))?;
    Ok(format!("{prefix}{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

/// Whether `value` has the form of a `viewer_session`.
pub fn is_viewer_session(value: &str) -> bool {
    is_random_id(value, "vs-")
}

/// Whether `value` has the form of a [`random_id`] with that prefix.
pub fn is_random_id(value: &str, prefix: &str) -> bool {
    value.len() == prefix.len() + 32 && value.starts_with(prefix) && value[prefix.len()..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// A bound call reached another login than the one it was pinned to.
pub fn session_changed(current: Option<&str>) -> ToolError {
    let now = if current.is_some() {
        "the viewer is on another login now (logged out and in again, another character, or local assistant access turned off and on)"
    } else {
        "the viewer is logged out now"
    };
    ToolError {
        viewer_session: current.map(str::to_owned),
        ..ToolError::new(
            ErrorCode::SessionChanged,
            format!(
                "{now}: this call was not run. Ids from earlier answers may belong to another inventory: call viewer_status and look again before retrying"
            ),
        )
    }
}

/// The viewer does not implement `spec`: it is older than the sidecar.
pub fn unsupported(spec: &ToolSpec) -> ToolError {
    let product = folderstorm_brand::PRODUCT;
    ToolError::new(
        ErrorCode::Unsupported,
        format!(
            "the running {product} is older than this assistant connector: it does not have `{}` (tool group `{}`). Update {product}; nothing was sent",
            spec.name, spec.group
        ),
    )
}

/// Folderstorm: whether a viewer implementing `viewer` can take these
/// arguments of a tool it has: a `client_request_id` needs `receipts` (an
/// older viewer would refuse it as an unknown argument, and dropping it
/// would make a retry unsafe without a word).
pub fn check_arguments(viewer: &Capabilities, spec: &ToolSpec, arguments: &serde_json::Value) -> Result<(), ToolError> {
    if arguments.get(crate::receipts::CLIENT_REQUEST_ID).is_some() && !viewer.contains_key(RECEIPTS) {
        let product = folderstorm_brand::PRODUCT;
        return Err(ToolError::new(
            ErrorCode::Unsupported,
            format!(
                "the running {product} is older than this assistant connector: `{}` cannot take a `client_request_id` (tool group `{RECEIPTS}`). Update {product}, or call it without one; nothing was sent",
                spec.name
            ),
        ));
    }
    Ok(())
}

/// The `viewer_session` of each login generation, for one bridge.
#[derive(Debug, Default)]
pub(crate) struct Binding {
    generation: Option<u64>,
    session: Option<String>,
    /// The random source failed for this generation (logged once).
    failed: bool,
}

impl Binding {
    /// The viewer's login now: generation `generation`, logged in or not.
    pub(crate) fn update(&mut self, generation: u64, logged_in: bool) {
        if self.generation != Some(generation) {
            self.generation = Some(generation);
            self.session = None;
            self.failed = false;
        }
        if !logged_in {
            self.session = None;
        } else if self.session.is_none() && !self.failed {
            match new_viewer_session() {
                Ok(session) => self.session = Some(session),
                Err(e) => {
                    // no binding until the next login: bound calls are refused
                    log::warn!("local assistant bridge: no viewer session: {e}");
                    self.failed = true;
                }
            }
        }
    }

    pub(crate) fn session(&self) -> Option<&str> {
        self.session.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{self, CallRequest, Health};
    use serde_json::json;

    #[test]
    fn a_viewer_session_is_fresh_for_each_generation_and_absent_logged_out() {
        let mut b = Binding::default();
        b.update(0, false);
        assert_eq!(b.session(), None);
        b.update(1, true);
        let first = b.session().expect("logged in").to_owned();
        assert!(is_viewer_session(&first), "{first}");
        b.update(1, true);
        assert_eq!(b.session(), Some(first.as_str()), "the same login on every frame");
        b.update(2, false);
        assert_eq!(b.session(), None);
        b.update(3, true);
        let second = b.session().expect("logged in").to_owned();
        assert_ne!(first, second);
        // a new bridge (a new epoch) never gives the same value again for
        // the same generation
        let mut other = Binding::default();
        other.update(3, true);
        assert_ne!(other.session(), Some(second.as_str()));
        assert!(!is_viewer_session("vs-12"));
        assert!(!is_viewer_session(&format!("xx-{}", &first[3..])));
    }

    #[test]
    fn this_build_supports_its_own_catalogue() {
        let mine = capabilities();
        assert!(protocol::all().all(|t| supports(&mine, t)), "every tool has a group of this build");
        assert!(
            protocol::all().filter(|t| t.group == INVENTORY).all(|t| supports(&assumed(), t)),
            "phases 1 and 2 for a viewer that advertises nothing"
        );
        assert!(!protocol::all().all(|t| supports(&assumed(), t)), "not the later groups");
        assert!(missing(&mine).is_empty());
        assert_eq!(
            missing(&assumed()),
            vec![
                CAMERA,
                INVENTORY_GAPS,
                WORLD,
                LOGIN_BINDING,
                SESSIONS,
                RECEIPTS,
                PLANS,
                OPERATIONS,
                NAVIGATION,
                CONTENT,
                OBJECTS,
                TASK_SCRIPTS,
                SCRIPT_TESTING,
                BUILDING,
                GESTURES,
                POSING
            ]
        );
        // a newer viewer: a later version and a group this build lacks
        let mut newer = capabilities();
        newer.insert("inventory".to_owned(), 2);
        newer.insert("plans".to_owned(), 1);
        assert!(protocol::all().all(|t| supports(&newer, t)));
        assert!(missing(&newer).is_empty());
        // a tool of a group the viewer lacks
        let later = ToolSpec {
            name: "chat_send",
            group: "chat",
            ..*protocol::tool("worn_items").expect("a tool")
        };
        assert!(!supports(&mine, &later));
        let e = unsupported(&later);
        assert_eq!(e.code, ErrorCode::Unsupported);
        assert!(e.message.contains("Update"), "{e}");
    }

    #[test]
    fn new_codes_reach_only_a_sidecar_that_declares_them() {
        let old = None;
        let new = Some(capabilities());
        let changed = session_changed(Some("vs-0123456789abcdef0123456789abcdef"));
        assert_eq!(changed.clone().for_sidecar(new.as_ref()), changed);
        let mapped = changed.clone().for_sidecar(old);
        assert_eq!(mapped.code, ErrorCode::NotReady);
        assert_eq!(mapped.viewer_session, None, "today's fields only");
        assert_eq!(mapped.message, changed.message);
        let spec = protocol::tool("worn_items").expect("a tool");
        assert_eq!(unsupported(spec).for_sidecar(old).code, ErrorCode::UnknownTool);
        assert_eq!(unsupported(spec).for_sidecar(new.as_ref()).code, ErrorCode::Unsupported);
        // a sidecar declaring capabilities without login binding
        let partial: Capabilities = [("inventory".to_owned(), 1)].into_iter().collect();
        assert_eq!(ErrorCode::SessionChanged.for_sidecar(Some(&partial)), ErrorCode::NotReady);
        // the old codes go through unchanged
        for code in [ErrorCode::PermissionDenied, ErrorCode::NotReady, ErrorCode::Timeout] {
            assert_eq!(code.for_sidecar(old), code);
        }
        assert_eq!(serde_json::to_value(ErrorCode::SessionChanged).expect("json"), "session_changed");
        assert_eq!(serde_json::to_value(ErrorCode::Unsupported).expect("json"), "unsupported");
    }

    #[test]
    fn older_and_newer_wires_read_each_other() {
        // an older viewer's health: no capabilities, no session
        let old: Health =
            serde_json::from_value(json!({"api": 1, "product": "P", "version": "0.1", "read_allowed": true})).expect("an older health");
        assert_eq!(old.capabilities(), assumed());
        assert_eq!(old.viewer_session, None);
        // a newer viewer's health, with fields this build does not know
        let newer: Health = serde_json::from_value(json!({
            "api": 1, "product": "P", "version": "9", "read_allowed": false,
            "capabilities": {"inventory": 1, "login_binding": 1, "plans": 1},
            "viewer_session": "vs-0123456789abcdef0123456789abcdef",
            "operations_live": 3
        }))
        .expect("a newer health");
        assert_eq!(newer.capabilities().get("plans"), Some(&1));
        // this build's health as an older sidecar reads it: the fields it
        // knows are there
        let mine = Health {
            api: 1,
            product: "P".into(),
            version: "1".into(),
            read_allowed: true,
            capabilities: Some(capabilities()),
            viewer_session: None,
            profile: None,
            managed: false,
            character: None,
        };
        let wire = serde_json::to_value(&mine).expect("json");
        assert_eq!(wire["api"], 1);
        assert!(wire.get("viewer_session").is_none(), "absent while logged out");
        // an older sidecar's call, and a newer sidecar's with a field this
        // build does not know
        let call: CallRequest = serde_json::from_value(json!({"tool": "worn_items", "arguments": {}})).expect("an older call");
        assert_eq!((call.session, call.features), (None, None));
        let call: CallRequest = serde_json::from_value(json!({
            "tool": "worn_items", "session": "vs-0123456789abcdef0123456789abcdef",
            "features": {"inventory": 1, "receipts": 1}, "client_request_id": "x"
        }))
        .expect("a newer call");
        assert_eq!(call.features.expect("features").get("receipts"), Some(&1));
        // an unbound call is today's body exactly
        let wire = serde_json::to_value(CallRequest {
            tool: "worn_items".into(),
            arguments: json!({}),
            session: None,
            features: None,
        })
        .expect("json");
        assert_eq!(wire, json!({"tool": "worn_items", "arguments": {}}));
        // a newer viewer's error with a code this build does not know
        // fails to parse: the client keeps its message (client.rs)
        assert!(serde_json::from_value::<ToolError>(json!({"code": "some_later_code", "message": "m"})).is_err());
    }
}
