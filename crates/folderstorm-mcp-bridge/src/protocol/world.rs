//! Folderstorm: the world observation tools of phase 3, part 3
//! (folderstorm/plans/mcp.md, "Phase 3, part 3: world observation"), after
//! Folderstorm (C++)'s `tools/fs-mcp/internal/mcptools/world.go` and
//! `doc/plans/mcp-world-observation.md`. Read-only: they change nothing in
//! the viewer or the world, ask the server for nothing, and only report what
//! the viewer has already loaded and shows. Kept apart from the phase 1 and
//! 2 catalogue so that the other phase 3 groups add theirs without touching
//! these; [`super::all`] lists every group.

use super::ToolSpec;
use crate::policy::Class;
use serde_json::{Value, json};
use std::time::Duration;

/// The observation radius in metres: default and largest (the C++ bounds).
pub const RADIUS_DEFAULT: f64 = 64.0;
pub const RADIUS_MIN: f64 = 1.0;
pub const RADIUS_MAX: f64 = 256.0;
/// Rows one observation gives: default and most.
pub const LIMIT_DEFAULT: usize = 25;
pub const LIMIT_MAX: usize = 100;

/// A read of this group (one constructor, so that a field the catalogue
/// gains is given once for all of them).
const fn read(name: &'static str, description: &'static str, schema: fn() -> Value) -> ToolSpec {
    ToolSpec {
        name,
        description,
        class: Some(Class::Read),
        also: None,
        schema,
        group: crate::binding::WORLD,
        wait: Duration::ZERO,
    }
}

fn no_arguments() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn nearby_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "radius": {"type": "number", "minimum": RADIUS_MIN, "maximum": RADIUS_MAX, "default": RADIUS_DEFAULT,
                "description": "Observation radius around the avatar, in metres."},
            "limit": {"type": "integer", "minimum": 1, "maximum": LIMIT_MAX, "default": LIMIT_DEFAULT,
                "description": "Most rows to return, nearest first (see `truncated`)."}
        },
        "additionalProperties": false
    })
}

/// The world observation tools.
pub const WORLD_TOOLS: &[ToolSpec] = &[
    read(
        "world_location",
        "Where the avatar is: region, maturity, position in the region, facing, flying or sitting, as the navigation bar shows it. An RLV @showloc restriction hides the region, the parcel and the coordinates, as it does in the viewer.",
        no_arguments,
    ),
    read(
        "world_parcel",
        "The parcel the avatar stands on, as About Land shows it: name, description, owner, area, prims, a few read-only flags (fly, build, scripts, damage, push, voice, sounds). Does not select or edit land. Refused (not_allowed) under RLV @showloc, unless the user owns or manages the parcel.",
        no_arguments,
    ),
    read(
        "world_nearby_avatars",
        "Avatars the viewer sees around the avatar, nearest first, as the People › Nearby list shows them: name, distance, same region, sitting, friend. Not a region census; a name may still be unknown. Refused (not_allowed) under RLV @shownearby; names hidden by @shownames or @shownametags come anonymous and without an id.",
        nearby_schema,
    ),
    read(
        "world_nearby_objects",
        "Rezzed objects the viewer has loaded around the avatar (whole linksets, not attachments or avatars), nearest first: id, distance, prim count, and the name and owner once the viewer knows them (Area Search loads them). No contents, scripts or asset ids; never fetches anything. Empty does not prove absence.",
        nearby_schema,
    ),
    read(
        "world_audio_sources",
        "Sounds the viewer heard lately, newest first, as the Sound Explorer lists them: kind (llPlaySound, llLoopSound, llTriggerSound, avatar), owner, whether it still plays, how long ago, distance where it played. The owner is not proof of who made the sound. No sound asset ids; does not play, stop or fetch sounds.",
        nearby_schema,
    ),
    read(
        "world_media_sources",
        "Media configured around the avatar: whether the parcel has music or media set (with its type and size), and the loaded objects nearby with media on a face. URLs are never given. Does not start, stop, navigate or download media; whether something plays is not reported.",
        nearby_schema,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_tools_are_reads_with_bounded_schemas() {
        for t in WORLD_TOOLS {
            assert!(t.name.starts_with("world_"), "{}", t.name);
            assert_eq!(t.class, Some(Class::Read), "{} never asks", t.name);
            assert!(!t.changes());
            let schema = (t.schema)();
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
            if let Some(limit) = schema["properties"].get("limit") {
                assert_eq!(limit["maximum"], LIMIT_MAX);
                assert_eq!(schema["properties"]["radius"]["maximum"], RADIUS_MAX);
            }
            assert!(super::super::tool(t.name).is_some(), "{} reachable", t.name);
            assert!(super::super::TOOLS.iter().all(|other| other.name != t.name), "{} unique", t.name);
        }
        assert_eq!(WORLD_TOOLS.len(), 6);
    }
}
