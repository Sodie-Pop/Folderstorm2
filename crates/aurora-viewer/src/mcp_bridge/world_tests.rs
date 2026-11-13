//! Folderstorm: the world observations (phase 3, part 3) against the demo
//! plaza, with each RLVa restriction the viewer applies to them.

use super::world::{Observation, answer, answers};
use crate::demo::{DEMO_AGENT, DEMO_LOUP};
use crate::scene::sounds::history::{Entry, History};
use crate::world::World;
use aurora_net::build::ObjectProps;
use aurora_net::{NetEvent, ParcelInfo, RegionHandle, parcel_flags as pf};
use folderstorm_mcp_bridge::ErrorCode;
use folderstorm_mcp_bridge::protocol::world::WORLD_TOOLS;
use glam::{DVec3, Vec3};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Where our avatar stands on the demo plaza, at Violet Wolf's height;
/// the wolf is at (136, 128.5), 4.7 m away.
const ME: (f32, f32) = (132.0, 126.0);
const STRANGER: Uuid = Uuid::from_u128(0x5747);
const NOBODY: &[&HashMap<Uuid, ObjectProps>] = &[];

/// The demo region with its prims and avatars, as `world/derender.rs`'s
/// fixture builds it, and our avatar placed.
fn plaza() -> (World, RegionHandle) {
    let mut world = World::new(Arc::new(crate::scene::avatar::AvatarLibrary::load()));
    world.agent_id = DEMO_AGENT;
    let mut handle = 0;
    for ev in crate::demo::events() {
        match &ev {
            NetEvent::MainRegionChanged { handle: h } => handle = *h,
            NetEvent::RegionHandshake(_) | NetEvent::ObjectUpdates { .. } => {}
            _ => continue,
        }
        let _ = world.apply(ev);
    }
    let wolf = world
        .objects
        .index_of_uuid(&DEMO_LOUP)
        .and_then(|i| world.objects.get(i))
        .expect("the wolf");
    world.agent.position = Vec3::new(ME.0, ME.1, wolf.position.z);
    world.parcel_name = "Aurora Plaza".into();
    (world, handle)
}

fn observe(world: &World, tool: &str, args: Value) -> Result<Value, folderstorm_mcp_bridge::ToolError> {
    observe_with(world, tool, args, NOBODY, None)
}

fn observe_with(
    world: &World,
    tool: &str,
    args: Value,
    props: &[&HashMap<Uuid, ObjectProps>],
    sounds: Option<&History>,
) -> Result<Value, folderstorm_mcp_bridge::ToolError> {
    let obs = Observation {
        world,
        demo: true,
        object_props: props,
        sounds,
    };
    answer(tool, &args, &obs)
}

fn ok(world: &World, tool: &str, args: Value) -> Value {
    observe(world, tool, args).unwrap_or_else(|e| panic!("{tool}: {e}"))
}

fn code(result: Result<Value, folderstorm_mcp_bridge::ToolError>) -> ErrorCode {
    result.expect_err("refused").code
}

fn restrict(world: &mut World, commands: &str) {
    world.rlva.configure(true);
    world.rlva_console_input(&format!("@{commands}"));
    world.refresh_rlva_privacy();
}

fn wolf_name(world: &World) -> String {
    world.person_name(&DEMO_LOUP).expect("the wolf's name tag")
}

fn row(out: &Value, id: Uuid) -> Option<&Value> {
    out["rows"].as_array().expect("rows").iter().find(|r| r["id"] == json!(id))
}

#[test]
fn every_world_tool_is_answered_here() {
    for t in WORLD_TOOLS {
        assert!(answers(t.name), "{}", t.name);
    }
    assert!(!answers("inventory_search"));
    let (world, _) = plaza();
    assert_eq!(code(observe(&world, "world_nowhere", json!({}))), ErrorCode::UnknownTool);
}

#[test]
fn arguments_are_checked_and_bounded() {
    let (world, _) = plaza();
    for bad in [
        json!({"radius": 0.5}),
        json!({"radius": 257}),
        json!({"radius": "far"}),
        json!({"limit": 0}),
        json!({"limit": 101}),
        json!({"limit": 2.5}),
        json!({"everything": true}),
        json!([]),
    ] {
        assert_eq!(
            code(observe(&world, "world_nearby_objects", bad.clone())),
            ErrorCode::InvalidArguments,
            "{bad}"
        );
    }
    // the place takes no arguments at all
    assert_eq!(
        code(observe(&world, "world_location", json!({"radius": 5}))),
        ErrorCode::InvalidArguments
    );
    let out = ok(&world, "world_nearby_objects", json!({"radius": 256, "limit": 100}));
    assert_eq!(out["limit"], 100);
    assert_eq!(out["radius"], 256.0);
}

#[test]
fn nothing_is_observed_logged_out_or_during_a_teleport() {
    let world = World::new(Arc::new(crate::scene::avatar::AvatarLibrary::load()));
    let obs = Observation {
        world: &world,
        demo: false,
        object_props: NOBODY,
        sounds: None,
    };
    for t in WORLD_TOOLS {
        assert_eq!(answer(t.name, &json!({}), &obs).expect_err(t.name).code, ErrorCode::NotReady);
    }
    let (mut world, _) = plaza();
    world.teleporting = true;
    assert_eq!(code(observe(&world, "world_nearby_avatars", json!({}))), ErrorCode::NotReady);
}

#[test]
fn location_gives_the_region_the_position_and_the_facing() {
    let (mut world, _) = plaza();
    world.agent.yaw = 0.0; // facing east
    let out = ok(&world, "world_location", json!({}));
    assert_eq!(out["region"]["name"], json!(world.region_name()));
    assert!(!world.region_name().is_empty());
    assert_eq!(out["parcel"]["name"], "Aurora Plaza");
    assert_eq!(out["position"]["x"], 132.0);
    assert_eq!(out["position"]["y"], 126.0);
    assert_eq!(out["global"]["x"], 256_000.0 + 132.0);
    assert_eq!(out["facing"], json!({"heading_degrees": 90.0, "compass": "E"}));
    assert_eq!(out["location_hidden_by_rlv"], false);
    world.agent.yaw = std::f32::consts::FRAC_PI_2; // north
    assert_eq!(ok(&world, "world_location", json!({}))["facing"]["compass"], "N");
}

#[test]
fn showloc_hides_the_location_as_the_navigation_bar_does() {
    let (mut world, _) = plaza();
    let region = world.region_name();
    restrict(&mut world, "showloc=n");
    let out = ok(&world, "world_location", json!({}));
    assert_eq!(out["location_hidden_by_rlv"], true);
    assert!(out.get("position").is_none() && out.get("global").is_none());
    assert_eq!(out["region"]["name"], json!(world.rlva_region_label()));
    let text = out.to_string();
    assert!(!text.contains(&region) && !text.contains("Aurora Plaza"), "{text}");
    // the rest of what the avatar does stays
    assert!(out["facing"]["compass"].is_string());
}

fn parcel(owner: Uuid) -> Arc<ParcelInfo> {
    Arc::new(ParcelInfo {
        local_id: 7,
        name: "Aurora Plaza".into(),
        desc: "The demo's square".into(),
        owner_id: owner,
        area: 4096,
        total_prims: 120,
        max_prims: 937,
        flags: pf::ALLOW_FLY | pf::ALLOW_OTHER_SCRIPTS | pf::FOR_SALE,
        sale_price: 5000,
        music_url: "http://stream.example/secret?token=1".into(),
        media_url: "https://media.example/private".into(),
        ..Default::default()
    })
}

#[test]
fn parcel_gives_about_land_s_summary() {
    let (mut world, _) = plaza();
    assert_eq!(code(observe(&world, "world_parcel", json!({}))), ErrorCode::NotReady);
    world.parcel = Some(parcel(DEMO_LOUP));
    let out = ok(&world, "world_parcel", json!({}));
    assert_eq!(out["name"], "Aurora Plaza");
    assert_eq!(out["description"], "The demo's square");
    assert_eq!(out["area_m2"], 4096);
    assert_eq!(out["prims"], json!({"used": 120, "max": 937}));
    assert_eq!(out["owner"]["kind"], "resident");
    assert_eq!(out["owner"]["id"], json!(DEMO_LOUP));
    assert_eq!(out["flags"]["fly"], true);
    assert_eq!(out["flags"]["build"], false);
    assert_eq!(out["sale_price"], 5000);
    // no URL leaves through the parcel
    assert!(!out.to_string().contains("example"));
}

#[test]
fn showloc_refuses_the_parcel_unless_the_user_owns_it() {
    let (mut world, _) = plaza();
    world.parcel = Some(parcel(DEMO_LOUP));
    restrict(&mut world, "showloc=n");
    assert_eq!(code(observe(&world, "world_parcel", json!({}))), ErrorCode::NotAllowed);
    world.parcel = Some(parcel(DEMO_AGENT));
    let out = ok(&world, "world_parcel", json!({}));
    // answered, but its name still goes through the viewer's @showloc filter
    assert_eq!(out["name"], json!(world.rlva_parcel_label()));
    assert_eq!(out["area_m2"], 4096);
}

#[test]
fn shownames_makes_the_parcel_owner_anonymous() {
    let (mut world, _) = plaza();
    world.parcel = Some(parcel(DEMO_LOUP));
    let name = wolf_name(&world);
    restrict(&mut world, "shownames=n");
    let out = ok(&world, "world_parcel", json!({}));
    assert_eq!(out["owner"]["anonymous"], true);
    assert!(out["owner"].get("id").is_none());
    assert!(!out.to_string().contains(&name) && !out.to_string().contains(&DEMO_LOUP.to_string()));
}

#[test]
fn nearby_avatars_list_the_wolf_with_its_distance() {
    let (world, _) = plaza();
    let out = ok(&world, "world_nearby_avatars", json!({}));
    let wolf = row(&out, DEMO_LOUP).expect("Violet Wolf is listed");
    assert_eq!(wolf["name"], json!(wolf_name(&world)));
    assert_eq!(wolf["distance"], 4.7);
    assert_eq!(wolf["same_region"], true);
    assert_eq!(wolf["loaded"], true);
    assert_eq!(wolf["friend"], false);
    assert_eq!(wolf["offset"]["east"], 4.0);
    // never ourselves
    assert!(row(&out, DEMO_AGENT).is_none());
    assert_eq!(out["positions_hidden_by_rlv"], false);
    // outside the radius
    let near = ok(&world, "world_nearby_avatars", json!({"radius": 2}));
    assert_eq!(near["total"], 0);
}

#[test]
fn nearby_avatars_are_bounded_and_nearest_first() {
    let (mut world, handle) = plaza();
    let crowd: Vec<(Uuid, Vec3)> = (0..150u128)
        .map(|n| (Uuid::from_u128(0xC0_0000 + n), Vec3::new(132.0 + (n % 10) as f32, 140.0, 30.0)))
        .collect();
    world.coarse.insert(handle, crowd);
    let out = ok(&world, "world_nearby_avatars", json!({"radius": 256, "limit": 100}));
    assert_eq!(out["total"], 151);
    assert_eq!(out["count"], 100);
    assert_eq!(out["truncated"], true);
    assert_eq!(out["rows"][0]["id"], json!(DEMO_LOUP), "the nearest first");
    let distances: Vec<f64> = out["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["distance"].as_f64().expect("distance"))
        .collect();
    assert!(distances.windows(2).all(|w| w[0] <= w[1]));
    // a coarse location without its altitude has no distance
    world.coarse.insert(handle, vec![(STRANGER, Vec3::new(133.0, 126.0, 0.0))]);
    let out = ok(&world, "world_nearby_avatars", json!({}));
    let stranger = row(&out, STRANGER).expect("listed");
    assert!(stranger["distance"].is_null() && stranger.get("offset").is_none());
}

#[test]
fn shownearby_refuses_the_nearby_avatars() {
    let (mut world, _) = plaza();
    restrict(&mut world, "shownearby=n");
    assert_eq!(code(observe(&world, "world_nearby_avatars", json!({}))), ErrorCode::NotAllowed);
}

#[test]
fn shownames_and_shownametags_make_the_avatars_anonymous() {
    for restriction in ["shownames=n", "shownametags=n"] {
        let (mut world, _) = plaza();
        let name = wolf_name(&world);
        restrict(&mut world, restriction);
        let out = ok(&world, "world_nearby_avatars", json!({}));
        let rows = out["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 1, "{restriction}");
        let wolf = &rows[0];
        assert_eq!(wolf["anonymous"], true, "{restriction}");
        assert_eq!(
            wolf["name"],
            json!(crate::world::rlva::privacy::anonymous(DEMO_LOUP).display_name),
            "{restriction}"
        );
        assert!(wolf.get("id").is_none() && wolf.get("friend").is_none(), "{restriction}");
        let text = out.to_string();
        assert!(
            !text.contains(&name) && !text.contains(&DEMO_LOUP.to_string()),
            "{restriction}: {text}"
        );
        // the distance stays, as in the People list
        assert_eq!(wolf["distance"], 4.7);
    }
    // an exception names the resident again
    let (mut world, _) = plaza();
    restrict(&mut world, &format!("shownames=n,shownames:{DEMO_LOUP}=add"));
    let out = ok(&world, "world_nearby_avatars", json!({}));
    assert!(row(&out, DEMO_LOUP).is_some());
}

#[test]
fn showminimap_hides_where_the_avatars_are() {
    let (mut world, _) = plaza();
    restrict(&mut world, "showminimap=n");
    let out = ok(&world, "world_nearby_avatars", json!({}));
    assert_eq!(out["positions_hidden_by_rlv"], true);
    let wolf = row(&out, DEMO_LOUP).expect("listed");
    assert!(wolf.get("offset").is_none());
    assert_eq!(wolf["distance"], 4.7);
}

#[test]
fn nearby_objects_are_whole_linksets_without_avatars_or_attachments() {
    let (world, _) = plaza();
    let out = ok(&world, "world_nearby_objects", json!({"radius": 256, "limit": 100}));
    let rows = out["rows"].as_array().expect("rows");
    assert!(!rows.is_empty());
    assert!(out["total"].as_u64().expect("total") >= rows.len() as u64);
    for id in [DEMO_LOUP, DEMO_AGENT, crate::demo::wolf_hat()] {
        assert!(row(&out, id).is_none(), "{id} is not a rezzed object");
    }
    for r in rows {
        assert_eq!(r["name_known"], false, "nothing is fetched for a call");
        assert!(r["distance"].as_f64().expect("distance") <= 256.0);
        assert!(r["prims"].as_u64().expect("prims") >= 1);
    }
    let default = ok(&world, "world_nearby_objects", json!({}));
    assert!(default["count"].as_u64().expect("count") <= 25);
}

#[test]
fn nearby_objects_name_what_the_viewer_already_knows() {
    let (mut world, _) = plaza();
    let near = ok(&world, "world_nearby_objects", json!({"limit": 1}));
    let id: Uuid = serde_json::from_value(near["rows"][0]["id"].clone()).expect("an object");
    let props: HashMap<Uuid, ObjectProps> = [(
        id,
        ObjectProps {
            object_id: id,
            name: format!("{}'s bench", wolf_name(&world)),
            description: "Sit here".into(),
            owner_id: DEMO_LOUP,
            ..Default::default()
        },
    )]
    .into();
    let out = observe_with(&world, "world_nearby_objects", json!({"limit": 1}), &[&props], None).expect("objects");
    let r = &out["rows"][0];
    assert_eq!(r["name"], json!(format!("{}'s bench", wolf_name(&world))));
    assert_eq!(r["description"], "Sit here");
    assert_eq!(r["owner"]["id"], json!(DEMO_LOUP));
    // @shownames: the owner and the name in the object's name are hidden
    let name = wolf_name(&world);
    restrict(&mut world, "shownames=n");
    let out = observe_with(&world, "world_nearby_objects", json!({"limit": 1}), &[&props], None).expect("objects");
    let r = &out["rows"][0];
    assert_eq!(r["owner"]["anonymous"], true);
    assert!(r["owner"].get("id").is_none());
    let text = out.to_string();
    assert!(!text.contains(&name) && !text.contains(&DEMO_LOUP.to_string()), "{text}");
}

fn sound(id: u64, owner: Uuid, source: Uuid, at: DVec3, ago: u64, playing: bool) -> Entry {
    let started = Instant::now() - Duration::from_secs(ago);
    Entry {
        id,
        asset: Uuid::from_u128(0xA55E7 + u128::from(id)),
        source,
        owner,
        position: at,
        started,
        stopped: (!playing).then_some(started),
        triggered: id == 2,
        looped: id == 3,
    }
}

#[test]
fn audio_sources_follow_the_sound_explorer_without_asset_ids() {
    let (mut world, _) = plaza();
    let here = world.agent_global().expect("placed");
    let object = Uuid::from_u128(0x0B1);
    let mut history = History::default();
    history.sync(
        [
            sound(1, DEMO_LOUP, DEMO_LOUP, here + DVec3::new(4.0, 2.5, 0.0), 1, true),
            sound(2, DEMO_LOUP, object, here + DVec3::new(10.0, 0.0, 0.0), 30, true),
            sound(3, Uuid::nil(), object, here + DVec3::new(0.0, 20.0, 0.0), 5, true),
            sound(4, DEMO_LOUP, object, here + DVec3::new(200.0, 0.0, 0.0), 2, true),
        ]
        .into_iter(),
        Instant::now(),
    );
    let out = observe_with(&world, "world_audio_sources", json!({"radius": 64}), NOBODY, Some(&history)).expect("audio");
    assert_eq!(out["total"], 3, "the far one is outside the radius");
    let kinds: Vec<_> = out["rows"].as_array().expect("rows").iter().map(|r| r["kind"].clone()).collect();
    assert!(kinds.contains(&json!("avatar")) && kinds.contains(&json!("llTriggerSound")) && kinds.contains(&json!("llLoopSound")));
    let text = out.to_string();
    for id in [1u128, 2, 3] {
        assert!(!text.contains(&Uuid::from_u128(0xA55E7 + id).to_string()), "no sound asset id");
    }
    assert!(
        !text.contains(&object.to_string()) && !text.contains(&DEMO_LOUP.to_string()),
        "no ids"
    );
    let name = wolf_name(&world);
    assert!(text.contains(&name));
    // unknown owner
    assert!(out["rows"].as_array().expect("rows").iter().any(|r| r["owner"].is_null()));
    restrict(&mut world, "shownames=n");
    let out = observe_with(&world, "world_audio_sources", json!({}), NOBODY, Some(&history)).expect("audio");
    assert!(!out.to_string().contains(&name), "the owner is anonymous, as in the Sound Explorer");
    assert_eq!(code(observe(&world, "world_audio_sources", json!({}))), ErrorCode::NotReady);
}

#[test]
fn media_sources_give_flags_never_urls() {
    let (mut world, _) = plaza();
    world.parcel = Some(parcel(DEMO_LOUP));
    // put media on a face of the nearest object
    let near = ok(&world, "world_nearby_objects", json!({"limit": 1}));
    let id: Uuid = serde_json::from_value(near["rows"][0]["id"].clone()).expect("an object");
    let index = world.objects.index_of_uuid(&id).expect("loaded");
    let object = world.objects.slots[index].as_mut().expect("object");
    let mut te = (**object.te.as_ref().expect("faces")).clone();
    te.faces[0].media_flags = 1;
    object.te = Some(Arc::new(te));
    let out = ok(&world, "world_media_sources", json!({}));
    assert_eq!(out["parcel"]["music"]["configured"], true);
    assert_eq!(out["parcel"]["media"]["configured"], true);
    let r = row(&out, id).expect("the object with media");
    assert_eq!(r["media_faces"], 1);
    assert_eq!(out["total"], 1);
    assert!(!out.to_string().contains("example"), "no URL");
}
