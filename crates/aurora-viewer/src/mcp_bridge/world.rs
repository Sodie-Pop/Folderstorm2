//! Folderstorm: the local assistant's world observations
//! (folderstorm/plans/mcp.md, "Phase 3, part 3: world observation"), after
//! Folderstorm (C++)'s `world_*` tools (`tools/fs-mcp/internal/mcptools/
//! world.go`, `doc/plans/mcp-world-observation.md`). Pure functions of
//! [`Observation`], computed on the frame thread, so the tests run them on
//! the demo world.
//!
//! Only what the viewer has loaded and already shows the user: nothing is
//! requested from the server for a call, nothing is kept between calls (no
//! history of other avatars), every list is bounded. RLVa applies as in the
//! viewer's own windows, or stricter:
//! - @showloc: region, parcel and coordinates hidden as on the navigation
//!   bar; About Land refused unless the user owns or manages the parcel
//!   (`World::rlva_can_view_parcel`);
//! - @shownearby: the nearby avatars refused, as People › Nearby is empty;
//! - @shownames: avatar names anonymous (`rlva::privacy::anonymous`), as
//!   the People list and the Sound Explorer show them, without their ids;
//!   object names and descriptions go through the viewer's text filter;
//! - @shownametags: an avatar whose tag is hidden comes anonymous too
//!   (stricter than the People list);
//! - @showminimap: no avatar positions, which only the minimap shows.
//!
//! Creators' permissions: no asset id at all (sound assets included), no
//! object contents, no hover text, no media URL.

use crate::scene::sounds::history::History;
use crate::world::World;
use crate::world::rlva::privacy;
use aurora_net::build::ObjectProps;
use aurora_net::parcel_flags as pf;
use folderstorm_mcp_bridge::protocol::world::{LIMIT_DEFAULT, LIMIT_MAX, RADIUS_DEFAULT, RADIUS_MAX, RADIUS_MIN, WORLD_TOOLS};
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use glam::Vec3;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

/// GP_LAND_* powers that let a group member manage the group's land under
/// @showloc, as `World::rlva_can_view_parcel` checks them (RlvUIEnabler).
const LAND_POWERS: u64 = (1 << 33) | (1 << 34) | (1 << 48);
/// Longest name or description given (an object's are 63 and 127 bytes).
const TEXT_MAX: usize = 127;

/// What the observations read, gathered by the frame thread.
pub struct Observation<'a> {
    pub world: &'a World,
    pub demo: bool,
    /// Object properties the viewer already holds, first match wins.
    pub object_props: &'a [&'a HashMap<Uuid, ObjectProps>],
    /// The Sound Explorer's history.
    pub sounds: Option<&'a History>,
}

fn err(code: ErrorCode, message: impl Into<String>) -> ToolError {
    ToolError::new(code, message)
}

fn rlv_refusal(behaviour: &str, what: &str) -> ToolError {
    err(
        ErrorCode::NotAllowed,
        format!("{what} is hidden by an RLV restriction (@{behaviour})"),
    )
}

/// Whether `tool` is one of these observations.
pub fn answers(tool: &str) -> bool {
    WORLD_TOOLS.iter().any(|t| t.name == tool)
}

/// Answer one observation.
pub fn answer(tool: &str, args: &Value, obs: &Observation) -> Result<Value, ToolError> {
    let Some(args) = args.as_object() else {
        return Err(err(ErrorCode::InvalidArguments, "the arguments must be an object"));
    };
    let nearby = !matches!(tool, "world_location" | "world_parcel");
    if args.keys().any(|k| !nearby || !matches!(k.as_str(), "radius" | "limit")) {
        return Err(err(ErrorCode::InvalidArguments, "unknown argument"));
    }
    let bounds = Bounds::parse(args)?;
    ready(obs)?;
    match tool {
        "world_location" => Ok(location(obs.world)),
        "world_parcel" => parcel(obs.world),
        "world_nearby_avatars" => avatars(obs.world, bounds),
        "world_nearby_objects" => Ok(objects(obs, bounds)),
        "world_audio_sources" => audio(obs, bounds),
        "world_media_sources" => Ok(media(obs.world, bounds)),
        _ => Err(err(ErrorCode::UnknownTool, "unknown tool")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub radius: f32,
    pub limit: usize,
}

impl Bounds {
    fn parse(args: &serde_json::Map<String, Value>) -> Result<Bounds, ToolError> {
        let radius = match args.get("radius") {
            None => RADIUS_DEFAULT,
            Some(v) => v
                .as_f64()
                .filter(|r| r.is_finite() && (RADIUS_MIN..=RADIUS_MAX).contains(r))
                .ok_or_else(|| {
                    err(
                        ErrorCode::InvalidArguments,
                        format!("`radius` must be {RADIUS_MIN} to {RADIUS_MAX} metres"),
                    )
                })?,
        };
        let limit = match args.get("limit") {
            None => LIMIT_DEFAULT,
            Some(v) => {
                v.as_u64()
                    .filter(|l| (1..=LIMIT_MAX as u64).contains(l))
                    .ok_or_else(|| err(ErrorCode::InvalidArguments, format!("`limit` must be 1 to {LIMIT_MAX}")))? as usize
            }
        };
        Ok(Bounds {
            radius: radius as f32,
            limit,
        })
    }
}

fn ready(obs: &Observation) -> Result<(), ToolError> {
    let world = obs.world;
    if !obs.demo && world.login.is_none() {
        return Err(err(ErrorCode::NotReady, "the viewer is not logged in"));
    }
    if world.teleporting {
        return Err(err(ErrorCode::NotReady, "a teleport is under way: ask again once it is over"));
    }
    if world.main().is_none() {
        return Err(err(ErrorCode::NotReady, "the avatar is not in a region yet"));
    }
    Ok(())
}

/// Metres to one decimal, as the lists show them.
fn metres(v: f32) -> f64 {
    (f64::from(v) * 10.0).round() / 10.0
}

/// An offset from the avatar, in metres east, north and up.
fn offset(v: Vec3) -> Value {
    json!({"east": metres(v.x), "north": metres(v.y), "up": metres(v.z)})
}

/// The compass point of a heading in degrees (0 north, clockwise).
fn compass(degrees: f64) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    POINTS[((degrees / 45.0).round() as usize) % 8]
}

/// Text from the world, through RLVa's display filter (@shownames,
/// @showloc) and cut to the longest an object may hold.
fn shown_text(world: &World, text: &str) -> String {
    let text = world.rlva_privacy.apply(text);
    match text.char_indices().nth(TEXT_MAX) {
        Some((cut, _)) => text[..cut].to_owned(),
        None => text.into_owned(),
    }
}

/// How the viewer names a resident: anonymous when RLVa hides the name
/// (`anonymous` true, and no id is given), None while the name is unknown.
fn resident(world: &World, id: Uuid, tag_hidden: bool) -> (Option<String>, bool) {
    if !world.rlva_can_name(id) || tag_hidden {
        return (Some(privacy::anonymous(id).display_name.clone()), true);
    }
    (world.person_name(&id).filter(|n| !n.is_empty()), false)
}

/// A group's name as the viewer knows it (never asked for here).
fn group_name(world: &World, id: Uuid) -> Option<String> {
    world
        .groups
        .groups
        .iter()
        .find(|g| g.id == id)
        .map(|g| g.name.clone())
        .or_else(|| world.land.group_names.get(&id).cloned())
}

/// An owner: a group, or a resident as [`resident`] names them.
fn owner(world: &World, owner: Uuid, group: Uuid, group_owned: bool) -> Value {
    if group_owned || (owner.is_nil() && !group.is_nil()) || (!group.is_nil() && owner == group) {
        return json!({"kind": "group", "id": group, "name": group_name(world, group)});
    }
    if owner.is_nil() {
        return Value::Null;
    }
    match resident(world, owner, false) {
        (name, true) => json!({"kind": "resident", "name": name, "anonymous": true}),
        (name, false) => json!({"kind": "resident", "id": owner, "name": name}),
    }
}

fn location(world: &World) -> Value {
    let agent = &world.agent;
    let yaw = f64::from(agent.yaw).to_degrees();
    let heading = (90.0 - yaw).rem_euclid(360.0).round() % 360.0;
    let mut out = json!({
        "facing": {"heading_degrees": heading, "compass": compass(heading)},
        "flying": agent.flying,
        "sitting": agent.is_sitting(),
        "location_hidden_by_rlv": false,
    });
    if world.rlva.has("showloc") {
        // the navigation bar: "(Hidden parcel), (Hidden region)", no
        // coordinates, no SLURL
        out["location_hidden_by_rlv"] = json!(true);
        out["region"] = json!({"name": world.rlva_region_label()});
        out["parcel"] = json!({"name": world.rlva_parcel_label()});
        return out;
    }
    let region = world.main();
    let info = region.and_then(|r| r.info.as_ref());
    out["region"] = json!({
        "name": world.region_name(),
        "maturity": info.map(|i| maturity(i.sim_access)),
        "size": info.map(|i| [i.size_x, i.size_y]),
    });
    out["parcel"] = json!({"name": (!world.parcel_name.is_empty()).then(|| world.parcel_name.clone())});
    let p = agent.position;
    out["position"] = json!({"x": metres(p.x), "y": metres(p.y), "z": metres(p.z)});
    if let Some(global) = world.agent_global() {
        out["global"] = json!({"x": global.x.round(), "y": global.y.round(), "z": global.z.round()});
    }
    out
}

/// LLViewerRegion::accessToString's ratings (SIM_ACCESS_*).
fn maturity(sim_access: u8) -> &'static str {
    match sim_access {
        13 => "general",
        21 => "moderate",
        42 => "adult",
        _ => "unknown",
    }
}

fn parcel(world: &World) -> Result<Value, ToolError> {
    let Some(p) = world.parcel.as_deref() else {
        return Err(err(ErrorCode::NotReady, "the parcel is not known yet"));
    };
    // World::rlva_can_view_parcel, for the agent's parcel itself
    let manages = p.owner_id == world.agent_id
        || world
            .groups
            .groups
            .iter()
            .any(|g| g.id == p.owner_id && g.powers & LAND_POWERS != 0);
    if world.rlva.has("showloc") && !manages {
        return Err(rlv_refusal("showloc", "the parcel"));
    }
    let flag = |bit: u32| p.flags & bit != 0;
    let mut out = json!({
        "local_id": p.local_id,
        "name": shown_text(world, &p.name),
        "description": shown_text(world, &p.desc),
        "owner": owner(world, p.owner_id, p.group_id, p.is_group_owned),
        "area_m2": p.area,
        "prims": {"used": p.total_prims, "max": p.max_prims},
        "for_sale": flag(pf::FOR_SALE),
        "flags": {
            "fly": flag(pf::ALLOW_FLY),
            "build": flag(pf::CREATE_OBJECTS),
            "build_group": flag(pf::CREATE_GROUP_OBJECTS),
            "scripts": flag(pf::ALLOW_OTHER_SCRIPTS),
            "scripts_group": flag(pf::ALLOW_GROUP_SCRIPTS),
            "damage": flag(pf::ALLOW_DAMAGE),
            "push_restricted": flag(pf::RESTRICT_PUSHOBJECT),
            "landmarks": flag(pf::ALLOW_LANDMARK),
            "terraform": flag(pf::ALLOW_TERRAFORM),
            "voice": flag(pf::ALLOW_VOICE_CHAT),
            "sounds_local": flag(pf::SOUND_LOCAL),
            "see_avatars": p.see_avatars,
            "avatar_sounds": p.any_av_sounds,
        },
    });
    if flag(pf::FOR_SALE) {
        out["sale_price"] = json!(p.sale_price);
    }
    Ok(out)
}

/// Rows past the bound are counted, not given.
fn bounded(mut rows: Vec<(f32, Value)>, bounds: Bounds) -> Value {
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total = rows.len();
    let rows: Vec<Value> = rows.into_iter().take(bounds.limit).map(|(_, v)| v).collect();
    json!({
        "radius": f64::from(bounds.radius),
        "limit": bounds.limit,
        "count": rows.len(),
        "total": total,
        "truncated": total > rows.len(),
        "rows": rows,
    })
}

fn avatars(world: &World, bounds: Bounds) -> Result<Value, ToolError> {
    // People › Nearby (`ui/people/radar.rs`) and the name tags are empty
    if world.rlva.has("shownearby") {
        return Err(rlv_refusal("shownearby", "the nearby avatars list"));
    }
    let me = world.agent.position;
    let my_region = world.main_region;
    let positions = !world.rlva.has("showminimap");
    let mut rows = Vec::new();
    for s in crate::world::radar::sightings(world) {
        let flat = (s.position - me).truncate().length();
        let distance = s.altitude_known.then(|| s.position.distance(me));
        if distance.unwrap_or(flat) > bounds.radius {
            continue;
        }
        let tag_hidden = !world.rlva.nametag_visible(s.id, world.agent_id, distance.unwrap_or(f32::MAX));
        let (name, anonymous) = resident(world, s.id, tag_hidden);
        let mut row = json!({
            "name": name,
            "distance": distance.map(metres),
            "same_region": Some(s.region) == my_region,
            "loaded": s.loaded,
            "sitting": s.sitting,
        });
        if anonymous {
            row["anonymous"] = json!(true);
        } else {
            row["id"] = json!(s.id);
            row["friend"] = json!(world.social.is_friend(&s.id));
        }
        if positions && s.altitude_known {
            row["offset"] = offset(s.position - me);
        }
        rows.push((distance.unwrap_or(flat), row));
    }
    let mut out = bounded(rows, bounds);
    out["positions_hidden_by_rlv"] = json!(!positions);
    Ok(out)
}

/// Where a loaded object is in the agent's frame, following its parents;
/// None when it is worn (an avatar among its parents) or not placed yet.
fn placed<'a>(world: &'a World, mut object: &'a crate::world::objects::Object) -> Option<Vec3> {
    let mut position = object.position;
    for _ in 0..64 {
        if !position.is_finite() || object.is_avatar() {
            return None;
        }
        if object.parent_id == 0 {
            return Some(world.region_offset(object.key.region)? + position);
        }
        object = world.objects.parent_of(object).and_then(|i| world.objects.get(i))?;
        position = object.position + object.rotation * position;
    }
    None
}

fn objects(obs: &Observation, bounds: Bounds) -> Value {
    let world = obs.world;
    let me = world.agent.position;
    let store = &world.objects;
    let mut rows = Vec::new();
    for (index, o) in store.slots.iter().enumerate() {
        let Some(o) = o else { continue };
        if o.parent_id != 0 || o.is_avatar() || o.is_client_preview() || !world.regions.contains_key(&o.key.region) {
            continue;
        }
        let Some(position) = placed(world, o) else { continue };
        let distance = position.distance(me);
        if distance > bounds.radius || !world.parcel_object_visible(index) {
            continue;
        }
        let prims = 1 + store
            .children_of(&o.key)
            .iter()
            .filter(|i| store.get(**i).is_some_and(|c| !c.is_avatar()))
            .count();
        let mut row = json!({
            "id": o.full_id,
            "distance": metres(distance),
            "offset": offset(position - me),
            "prims": prims,
        });
        if o.is_tree() {
            row["tree"] = json!(true);
        }
        match obs.object_props.iter().find_map(|props| props.get(&o.full_id)) {
            Some(p) => {
                row["name"] = json!(shown_text(world, &p.name));
                row["description"] = json!(shown_text(world, &p.description));
                row["owner"] = owner(world, p.owner_id, p.group_id, false);
            }
            None => row["name_known"] = json!(false),
        }
        rows.push((distance, row));
    }
    bounded(rows, bounds)
}

fn audio(obs: &Observation, bounds: Bounds) -> Result<Value, ToolError> {
    let world = obs.world;
    let Some(history) = obs.sounds else {
        return Err(err(ErrorCode::NotReady, "the sound history is not available"));
    };
    let Some(me) = world.agent_global() else {
        return Err(err(ErrorCode::NotReady, "the avatar is not placed yet"));
    };
    let now = Instant::now();
    let mut rows = Vec::new();
    for e in history.entries() {
        let distance = (e.position - me).length() as f32;
        if !distance.is_finite() || distance > bounds.radius {
            continue;
        }
        // the Sound Explorer's Type column (`ui/sound_explorer.rs`)
        let kind = if e.source == e.owner && !e.owner.is_nil() {
            "avatar"
        } else if crate::scene::sounds::is_collision(&e.asset) {
            "collision"
        } else if e.triggered {
            "llTriggerSound"
        } else if e.looped {
            "llLoopSound"
        } else {
            "llPlaySound"
        };
        // `sound_owner_name` (app/sound_explorer.rs): anonymous under
        // @shownames, and no id given for any owner
        let owner = (!e.owner.is_nil()).then(|| resident(world, e.owner, false).0).flatten();
        let ago = |at: Instant| now.saturating_duration_since(at).as_secs();
        let mut row = json!({
            "kind": kind,
            "owner": owner,
            "playing": e.stopped.is_none(),
            "started_seconds_ago": ago(e.started),
            "distance": metres(distance),
        });
        if let Some(stopped) = e.stopped {
            row["stopped_seconds_ago"] = json!(ago(stopped));
        }
        // newest first, as the Explorer lists them
        let newest = e.stopped.unwrap_or(now).max(e.started);
        rows.push((now.saturating_duration_since(newest).as_secs_f32(), row));
    }
    Ok(bounded(rows, bounds))
}

fn media(world: &World, bounds: Bounds) -> Value {
    let me = world.agent.position;
    let mut rows = Vec::new();
    for o in world.objects.slots.iter().flatten() {
        let Some(te) = o.te.as_ref() else { continue };
        let faces = te.faces.iter().filter(|f| f.media_flags & 1 != 0).count();
        if faces == 0 || o.is_client_preview() {
            continue;
        }
        // worn objects and HUDs are left out
        let Some(position) = placed(world, o) else { continue };
        let distance = position.distance(me);
        if distance > bounds.radius {
            continue;
        }
        rows.push((
            distance,
            json!({"id": o.full_id, "distance": metres(distance), "media_faces": faces}),
        ));
    }
    let mut out = bounded(rows, bounds);
    out["parcel"] = match world.parcel.as_deref() {
        None => Value::Null,
        Some(p) => json!({
            "music": {"configured": !p.music_url.trim().is_empty()},
            "media": {
                "configured": !p.media_url.trim().is_empty(),
                "type": (!p.media.mime.is_empty()).then(|| p.media.mime.clone()),
                "description": shown_text(world, &p.media.desc),
                "width": p.media.width,
                "height": p.media.height,
                "looping": p.media.looping,
            },
        }),
    };
    out
}
