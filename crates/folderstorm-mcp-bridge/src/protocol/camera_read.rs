//! Folderstorm: the camera read (folderstorm/plans/mcp-uploads.md), after
//! Folderstorm (C++)'s `camera_get`: where the camera is now, which the
//! camera moves already give in their answers. Kept apart from the phase 3
//! camera tools so that a viewer without it answers `unsupported` (its own
//! capability, `camera_read`).

use super::ToolSpec;
use crate::policy::Class;
use serde_json::{Value, json};
use std::time::Duration;

pub const GET: &str = "camera_get";

fn get_schema() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

/// The camera read.
pub const CAMERA_READ_TOOLS: &[ToolSpec] = &[ToolSpec {
    name: GET,
    description: "Where the camera is now: its position and the point it looks at (focus), in region coordinates (metres), the distance between them, the vertical field of view, and your avatar's position. Changes nothing; the camera tools give the same after a move.",
    class: Some(Class::Read),
    also: None,
    schema: get_schema,
    group: crate::binding::CAMERA_READ,
    wait: Duration::ZERO,
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_get_is_a_bound_read_of_its_own_group() {
        let t = super::super::tool(GET).expect("listed");
        assert_eq!(t.class, Some(Class::Read));
        assert!(!t.changes());
        assert!(t.bound());
        assert_eq!(t.group, crate::binding::CAMERA_READ);
        assert_eq!(get_schema()["additionalProperties"], false);
    }
}
