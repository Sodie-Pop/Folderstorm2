//! Folderstorm: the building tools (folderstorm/plans/mcp-building.md) on
//! the demo plaza: which objects (owned, deeded, someone else's, no move,
//! physical, a child), revisions and staleness, vectors and limits, the
//! parcel, textures and materials with full rights only, rezzes (never an
//! id guessed, candidates, `object_rez_identify`), next owner rights with
//! the Properties rules (always asking), outcomes, RLVa, no asset id in any
//! answer, and end to end over a real socket.

use super::building::{self, Send};
use super::changes::Work;
use super::desk::{Bake, Desk};
use super::objects::{Scene, Want};
use super::tools::{Ctx, Session};
use crate::demo::DEMO_AGENT;
use crate::world::World;
use crate::world::inventory::{FetchState, Folder, actions};
use aurora_net::build::{BuildCmd, ObjectProps};
use aurora_net::inventory::{InvFolder, InvItem};
use aurora_net::{NetEvent, ParcelInfo};
use folderstorm_mcp_bridge::protocol::building::{
    MATERIAL, NEXT_OWNER_SET, OBJECT_GET, PERMISSIONS_GET, REZ_INVENTORY, REZ_PRIMITIVE, SURFACE, TRANSFORM,
};
use folderstorm_mcp_bridge::protocol::objects::{REZ_IDENTIFY, STATUS};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{Class, ErrorCode, Level, Permissions, Preset, ToolError};
use glam::Vec3;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// The plaza's cut box (local id 118): the agent's in the demo.
const BOX_LOCAL: u32 = 118;
const OBJECT: Uuid = Uuid::from_u128(8100);
const NOTECARD: Uuid = Uuid::from_u128(8103);
const TEXTURE: Uuid = Uuid::from_u128(8104);
const SKY: Uuid = Uuid::from_u128(8107);
const MATERIAL_ITEM: Uuid = Uuid::from_u128(8108);
const NO_COPY: Uuid = Uuid::from_u128(8109);
const NO_TRANSFER: Uuid = Uuid::from_u128(8110);
const STRANGER: Uuid = Uuid::from_u128(0x5747);
const IDLE: Bake = Bake {
    outfit_change_pending: false,
    cof_version: 3,
    appearance_cof_version: 3,
    server_bakes: true,
};

fn prim_id(local: u32) -> Uuid {
    Uuid::from_u128(0xA0E0_0000_0000_0000_0000_0000_0000_0000 | local as u128)
}

fn the_box() -> Uuid {
    prim_id(BOX_LOCAL)
}

/// The demo region and its prims, the demo inventory, the avatar next to
/// the cut box, on a parcel of the agent's covering the region.
fn plaza() -> World {
    let mut world = World::new(Arc::new(crate::scene::avatar::AvatarLibrary::load()));
    world.agent_id = DEMO_AGENT;
    for ev in crate::demo::events() {
        if matches!(
            ev,
            NetEvent::MainRegionChanged { .. } | NetEvent::RegionHandshake(_) | NetEvent::ObjectUpdates { .. }
        ) {
            let _ = world.apply(ev);
        }
    }
    let root = Uuid::from_u128(80);
    world.inventory.root = root;
    world.inventory.owner = DEMO_AGENT;
    world.inventory.folders.insert(
        root,
        Folder {
            info: InvFolder {
                id: root,
                name: "My Inventory".into(),
                type_default: 8,
                ..Default::default()
            },
            children: vec![],
            items: vec![],
            state: FetchState::Fetched,
            library: false,
        },
    );
    crate::world::inventory::demo::seed(&mut world.inventory, DEMO_AGENT);
    world.agent.position = position(&world) + Vec3::new(2.0, 0.0, 0.0);
    world.parcel = Some(Arc::new(ParcelInfo {
        local_id: 1,
        owner_id: DEMO_AGENT,
        bitmap: vec![0xFF; 512],
        ..Default::default()
    }));
    world
}

fn index(world: &World) -> usize {
    world.objects.index_of_uuid(&the_box()).expect("the prim")
}

fn position(world: &World) -> Vec3 {
    world.objects.get(index(world)).map(|o| o.position).expect("placed")
}

fn object_mut(world: &mut World) -> &mut crate::world::objects::Object {
    let idx = index(world);
    world.objects.get_mut(idx).expect("the prim")
}

fn ctx(world: &World) -> Ctx<'_> {
    Ctx {
        inv: &world.inventory,
        session: Session::Demo,
        worn: HashSet::new(),
        attachment_points: HashMap::new(),
        inventory_locked: world.rlva.has("showinv"),
        protected: HashSet::new(),
        outfit_points: HashMap::new(),
        stage: Default::default(),
        viewer_session: None,
        identity: Default::default(),
        nav: Default::default(),
        content: Default::default(),
        objects: Scene {
            world: Some(world),
            props: &[],
        },
    }
}

fn allow_all() -> Permissions {
    let mut p = Permissions::default();
    for class in Class::ALL {
        p.set(class, Level::Allow);
    }
    p
}

fn desk_with(permissions: Permissions) -> Desk {
    let mut desk = Desk::default();
    desk.set_permissions(permissions);
    desk
}

fn call(tool: &str, args: Value) -> (Pending, Receiver<Result<Value, ToolError>>) {
    Pending::detached(tool, args, false, Some("Claude Code"))
}

fn answered(answer: &Receiver<Result<Value, ToolError>>) -> Result<Value, ToolError> {
    answer.try_recv().expect("answered")
}

fn code(result: Result<Value, ToolError>) -> ErrorCode {
    result.expect_err("refused").code
}

/// The server answers the owner the tools asked for (`owner` for every
/// object), then the desk moves on.
fn serve(desk: &mut Desk, world: &World, owner: Uuid, now: Instant) {
    for want in desk.objects().wanted() {
        if let Want::Properties { object, .. } = want {
            let event = NetEvent::ObjectProperties(vec![ObjectProps {
                object_id: object,
                owner_id: owner,
                name: "Cut box".into(),
                ..Default::default()
            }]);
            desk.objects().observe(&event, now);
        }
    }
    desk.tick_reads(world, now);
}

/// Read the box through the desk, the server saying who owns it.
fn read(desk: &mut Desk, world: &World, now: Instant) -> Result<Value, ToolError> {
    let (c, answer) = call(OBJECT_GET, json!({"object_id": the_box().to_string()}));
    desk.take(c, &ctx(world), now);
    serve(desk, world, DEMO_AGENT, now);
    answered(&answer)
}

fn revision(desk: &mut Desk, world: &World, now: Instant) -> String {
    read(desk, world, now).expect("read")["revision"]
        .as_str()
        .expect("revision")
        .to_owned()
}

/// What a change answers when planned now (refusals and plans, nothing
/// run).
fn plan(desk: &mut Desk, world: &World, tool: &str, args: Value, now: Instant) -> Result<Value, ToolError> {
    let (c, answer) = call(tool, args);
    desk.take(c, &ctx(world), now);
    match answer.try_recv() {
        Ok(result) => result,
        Err(_) => Ok(json!({"waiting": true})),
    }
}

/// The change the desk lets run now, planned again as just before the
/// send, and sent; the planned work comes back (a send or an inventory
/// change).
fn run(desk: &mut Desk, world: &World, now: Instant) -> Option<Work> {
    let call = desk.tick(&IDLE, now).into_iter().next()?;
    let login = desk.login();
    let call = desk.recheck(call, login, None, &ctx(world), now)?;
    let planned = match desk.building_plan(&call, &ctx(world), now) {
        Ok(planned) => planned,
        Err(e) => {
            call.answer(Err(e));
            return None;
        }
    };
    match &planned.work {
        Work::Build(send) => desk.build_sending(call, send, planned.reply.clone(), world, now),
        Work::Inventory(_) => call.answer(Ok(planned.reply.clone())),
        other => panic!("not a building change: {other:?}"),
    }
    Some(planned.work)
}

fn sent(work: Option<Work>) -> Send {
    match work {
        Some(Work::Build(send)) => *send,
        other => panic!("not sent: {other:?}"),
    }
}

/// The simulator's echo of a transform (an object update).
fn echo(world: &mut World, command: &BuildCmd) {
    if let BuildCmd::Transform { updates, .. } = command {
        let o = object_mut(world);
        for u in updates {
            if let Some(p) = u.position {
                o.position = p;
            }
            if let Some(r) = u.rotation {
                o.rotation = r;
            }
            if let Some(s) = u.scale {
                o.scale = s;
            }
        }
    }
}

fn no_asset_ids(value: &Value, world: &World) {
    let text = value.to_string();
    for item in world.inventory.items.values() {
        if !item.asset_id.is_nil() {
            assert!(!text.contains(&item.asset_id.to_string()), "an asset id leaked: {text}");
        }
    }
    if let Some(te) = world.objects.get(index(world)).and_then(|o| o.te.clone()) {
        for f in &te.faces {
            assert!(!text.contains(&f.texture.to_string()), "a texture id leaked: {text}");
        }
    }
}

#[test]
fn a_read_asks_the_owner_then_answers_the_object_with_a_revision() {
    let world = plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let (c, answer) = call(OBJECT_GET, json!({"object_id": the_box().to_string()}));
    desk.take(c, &ctx(&world), now);
    // the owner is not known yet: the properties are asked for
    assert!(answer.try_recv().is_err());
    serve(&mut desk, &world, DEMO_AGENT, now);
    let read = answered(&answer).expect("read");
    assert_eq!(read["object_id"], json!(the_box()));
    assert!(read["revision"].as_str().is_some_and(|r| r.starts_with("rev-")), "{read}");
    assert_eq!(read["revision_seconds"], 120);
    assert!(read["position"].as_array().is_some_and(|p| p.len() == 3));
    assert!(read["rotation"].as_array().is_some_and(|r| r.len() == 4));
    let faces = read["faces"].as_array().expect("faces");
    assert!(!faces.is_empty() && faces.len() <= 64);
    assert!(faces.iter().all(|f| f["texture"].is_string() && f["pbr_material"].is_boolean()));
    no_asset_ids(&read, &world);
    // a second read needs no question to the server
    let (c, answer) = call(OBJECT_GET, json!({"object_id": the_box().to_string()}));
    desk.take(c, &ctx(&world), now);
    assert!(answered(&answer).is_ok());
}

#[test]
fn only_owned_modifiable_roots_are_read() {
    let now = Instant::now();
    // someone else's (or a group's, as the server's owner says)
    let world = plaza();
    let mut desk = desk_with(allow_all());
    let (c, answer) = call(OBJECT_GET, json!({"object_id": the_box().to_string()}));
    desk.take(c, &ctx(&world), now);
    serve(&mut desk, &world, STRANGER, now);
    assert_eq!(code(answered(&answer)), ErrorCode::NotAllowed);
    // no modify flag
    let mut world = plaza();
    object_mut(&mut world).update_flags &= !crate::ui::context::flags::OBJECT_MODIFY;
    let mut desk = desk_with(allow_all());
    assert_eq!(code(read(&mut desk, &world, now)), ErrorCode::NotAllowed);
    // an unknown object
    let world = plaza();
    let mut desk = desk_with(allow_all());
    let (c, answer) = call(OBJECT_GET, json!({"object_id": Uuid::from_u128(0xDEAD).to_string()}));
    desk.take(c, &ctx(&world), now);
    assert_eq!(code(answered(&answer)), ErrorCode::NotFound);
    // RLVa @edit
    let mut world = plaza();
    world.rlva.configure(true);
    world.rlva_console_input("@edit=n");
    let mut desk = desk_with(allow_all());
    assert_eq!(code(read(&mut desk, &world, now)), ErrorCode::NotAllowed);
    // the server never says: a timeout
    let world = plaza();
    let mut desk = desk_with(allow_all());
    let (c, answer) = call(OBJECT_GET, json!({"object_id": the_box().to_string()}));
    desk.take(c, &ctx(&world), now);
    desk.objects().wanted();
    desk.tick_reads(&world, now + Duration::from_secs(21));
    assert_eq!(code(answered(&answer)), ErrorCode::Timeout);
}

#[test]
fn a_transform_is_confirmed_by_the_object_update() {
    let mut world = plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let rev = revision(&mut desk, &world, now);
    let to = position(&world) + Vec3::new(0.0, 0.0, 0.5);
    let (c, answer) = call(
        TRANSFORM,
        json!({"object_id": the_box().to_string(), "revision": rev, "position": [to.x, to.y, to.z], "client_request_id": "t-1"}),
    );
    desk.take(c, &ctx(&world), now);
    let send = sent(run(&mut desk, &world, now));
    let BuildCmd::Transform { updates, .. } = &send.command else {
        panic!("a MultipleObjectUpdate")
    };
    assert_eq!(updates.len(), 1);
    assert!(updates[0].linked && updates[0].position == Some(to) && updates[0].scale.is_none());
    desk.tick_reads(&world, now);
    assert!(answer.try_recv().is_err(), "waits for the update");
    echo(&mut world, &send.command);
    desk.tick_reads(&world, now + Duration::from_secs(1));
    let reply = answered(&answer).expect("done");
    assert_eq!(reply["done"], true, "{reply}");
    assert_eq!(reply["state"], "done", "{reply}");
    // the old revision is stale now (the assistant changed the object)
    let (c, answer) = call(
        TRANSFORM,
        json!({"object_id": the_box().to_string(), "revision": rev, "position": [to.x, to.y, to.z + 0.5], "client_request_id": "t-2"}),
    );
    desk.take(c, &ctx(&world), now);
    assert_eq!(code(answered(&answer)), ErrorCode::Stale);
}

#[test]
fn a_transform_without_an_update_ends_unconfirmed_then_settles_late() {
    let mut world = plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let rev = revision(&mut desk, &world, now);
    let (c, answer) = call(
        TRANSFORM,
        json!({"object_id": the_box().to_string(), "revision": rev, "rotation": [0.0, 0.0, 0.6, 0.8], "client_request_id": "t-3"}),
    );
    desk.take(c, &ctx(&world), now);
    let send = sent(run(&mut desk, &world, now));
    desk.tick_reads(&world, now + Duration::from_secs(31));
    let reply = answered(&answer).expect("answered");
    assert_eq!(
        (reply["done"].as_bool(), reply["unconfirmed"].as_bool()),
        (Some(false), Some(true)),
        "{reply}"
    );
    assert_eq!(reply["state"], "unconfirmed");
    let operation = reply["operation_id"].as_str().expect("operation").to_owned();
    echo(&mut world, &send.command);
    desk.tick_reads(&world, now + Duration::from_secs(40));
    let state = desk.ledger().with(|s| s.find(None, &operation).map(|r| r.state));
    assert_eq!(state, Some(folderstorm_mcp_bridge::receipts::State::Done));
}

#[test]
fn transforms_are_checked_against_the_object_and_the_limits() {
    let now = Instant::now();
    let world = plaza();
    let mut desk = desk_with(allow_all());
    let rev = revision(&mut desk, &world, now);
    let p = position(&world);
    let at = |args: Value| {
        let mut a = json!({"object_id": the_box().to_string(), "revision": rev, "client_request_id": Uuid::new_v4().to_string()});
        for (k, v) in args.as_object().expect("map") {
            a[k] = v.clone();
        }
        a
    };
    for (args, expected) in [
        (json!({}), ErrorCode::InvalidArguments),
        (json!({"rotation": [0.0, 0.0, 0.5, 0.5]}), ErrorCode::InvalidArguments),
        (json!({"position": [p.x, p.y]}), ErrorCode::InvalidArguments),
        (json!({"position": [p.x + 80.0, p.y, p.z]}), ErrorCode::NotAllowed),
        (json!({"position": [-1.0, p.y, p.z]}), ErrorCode::NotAllowed),
        (json!({"position": [p.x, p.y, 5000.0]}), ErrorCode::NotAllowed),
        (json!({"scale": [0.0001, 1.0, 1.0]}), ErrorCode::NotAllowed),
        (json!({"scale": [65.0, 1.0, 1.0]}), ErrorCode::NotAllowed),
        (json!({"position": [p.x, p.y, p.z], "extra": 1}), ErrorCode::InvalidArguments),
    ] {
        assert_eq!(code(plan(&mut desk, &world, TRANSFORM, at(args.clone()), now)), expected, "{args}");
    }
    // a revision of another login or unknown, and an expired one
    let unknown =
        json!({"object_id": the_box().to_string(), "revision": "rev-nope", "position": [p.x, p.y, p.z + 0.1], "client_request_id": "x-1"});
    assert_eq!(code(plan(&mut desk, &world, TRANSFORM, unknown, now)), ErrorCode::NotFound);
    let late = at(json!({"position": [p.x, p.y, p.z + 0.1]}));
    assert_eq!(
        code(plan(&mut desk, &world, TRANSFORM, late, now + Duration::from_secs(121))),
        ErrorCode::Stale
    );
    // the object changed since the read
    let mut moved = plaza();
    let mut desk = desk_with(allow_all());
    let rev = revision(&mut desk, &moved, now);
    object_mut(&mut moved).position.x += 1.0;
    let args = json!({"object_id": the_box().to_string(), "revision": rev, "position": [p.x, p.y, p.z + 0.1], "client_request_id": "x-2"});
    assert_eq!(code(plan(&mut desk, &moved, TRANSFORM, args, now)), ErrorCode::Stale);
    // physical, or not movable: refused
    for change in [building_flags::PHYSICAL, building_flags::NO_MOVE] {
        let mut world = plaza();
        let mut desk = desk_with(allow_all());
        let rev = revision(&mut desk, &world, now);
        match change {
            building_flags::PHYSICAL => object_mut(&mut world).update_flags |= crate::build::edits::FLAGS_USE_PHYSICS,
            _ => object_mut(&mut world).update_flags &= !crate::ui::context::flags::OBJECT_MOVE,
        }
        let args =
            json!({"object_id": the_box().to_string(), "revision": rev, "position": [p.x, p.y, p.z + 0.1], "client_request_id": "x-3"});
        assert_eq!(code(plan(&mut desk, &world, TRANSFORM, args, now)), ErrorCode::NotAllowed);
    }
}

mod building_flags {
    pub const PHYSICAL: u8 = 0;
    pub const NO_MOVE: u8 = 1;
}

#[test]
fn a_surface_change_sends_the_whole_entry_and_needs_a_full_permission_texture() {
    let mut world = plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let rev = revision(&mut desk, &world, now);
    let args = |item: Uuid, id: &str| {
        json!({
            "object_id": the_box().to_string(),
            "revision": rev,
            "face": 0,
            "color": [1.0, 0.0, 0.0, 1.0],
            "glow": 0.2,
            "texture_item": item.to_string(),
            "client_request_id": id,
        })
    };
    // a texture without full rights, a non-texture, a link to nowhere
    if let Some(it) = world.inventory.items.get_mut(&TEXTURE) {
        it.owner_mask &= !actions::TRANSFER;
    }
    assert_eq!(
        code(plan(&mut desk, &world, SURFACE, args(TEXTURE, "s-0"), now)),
        ErrorCode::NotAllowed
    );
    if let Some(it) = world.inventory.items.get_mut(&TEXTURE) {
        it.owner_mask |= actions::TRANSFER;
    }
    assert_eq!(
        code(plan(&mut desk, &world, SURFACE, args(NOTECARD, "s-1"), now)),
        ErrorCode::NotAllowed
    );
    assert_eq!(
        code(plan(&mut desk, &world, SURFACE, args(Uuid::from_u128(1), "s-2"), now)),
        ErrorCode::NotFound
    );
    let (c, answer) = call(SURFACE, args(TEXTURE, "s-3"));
    desk.take(c, &ctx(&world), now);
    let send = sent(run(&mut desk, &world, now));
    let BuildCmd::SetTextures { texture_entry, .. } = &send.command else {
        panic!("an ObjectImage")
    };
    let faces = read(&mut desk, &world, now).expect("read")["face_count"].as_u64().expect("count") as usize;
    assert!((1..=9).contains(&faces), "a box's faces, not the entry's 45: {faces}");
    let mut te = aurora_prim::te::parse_texture_entry(texture_entry).expect("an entry");
    te.faces.truncate(faces);
    let asset = world.inventory.items[&TEXTURE].asset_id;
    assert_eq!(te.faces[0].texture, asset);
    assert!((te.faces[0].color[0] - 1.0).abs() < 0.01 && te.faces[0].color[1] < 0.01);
    // the echo: the object's entry shows it
    object_mut(&mut world).te = Some(Arc::new(te));
    desk.tick_reads(&world, now + Duration::from_secs(1));
    let reply = answered(&answer).expect("done");
    assert_eq!(reply["done"], true, "{reply}");
    no_asset_ids(&reply, &world);
}

#[test]
fn a_pbr_face_is_left_to_the_material_tool_which_needs_a_full_permission_material() {
    let mut world = plaza();
    object_mut(&mut world).extra.render_materials = vec![(0, Uuid::from_u128(0x3A7))];
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let rev = revision(&mut desk, &world, now);
    let surface = json!({"object_id": the_box().to_string(), "revision": rev, "face": 0, "glow": 0.5, "client_request_id": "m-0"});
    assert_eq!(code(plan(&mut desk, &world, SURFACE, surface, now)), ErrorCode::NotAllowed);
    let material = |item: Uuid, id: &str| json!({"object_id": the_box().to_string(), "revision": rev, "face": 0, "material_item": item.to_string(), "client_request_id": id});
    assert_eq!(
        code(plan(&mut desk, &world, MATERIAL, material(TEXTURE, "m-1"), now)),
        ErrorCode::NotAllowed
    );
    let faces = json!({"object_id": the_box().to_string(), "revision": rev, "face": 63, "material_item": MATERIAL_ITEM.to_string(), "client_request_id": "m-2"});
    assert_eq!(code(plan(&mut desk, &world, MATERIAL, faces, now)), ErrorCode::InvalidArguments);
    let (c, answer) = call(MATERIAL, material(MATERIAL_ITEM, "m-3"));
    desk.take(c, &ctx(&world), now);
    let send = sent(run(&mut desk, &world, now));
    let BuildCmd::Cap { cap, body, tag, .. } = &send.command else {
        panic!("a ModifyMaterialParams post")
    };
    assert_eq!(cap, "ModifyMaterialParams");
    assert!(*tag >= 1 << 62, "far from the build tool's tags");
    let asset = world.inventory.items[&MATERIAL_ITEM].asset_id;
    assert_eq!(body.as_array()[0]["asset_id"].as_uuid(), asset);
    assert_eq!(body.as_array()[0]["gltf_json"].to_string_value(), "");
    object_mut(&mut world).extra.render_materials = vec![(0, asset)];
    desk.tick_reads(&world, now + Duration::from_secs(1));
    let reply = answered(&answer).expect("done");
    assert_eq!(reply["done"], true, "{reply}");
    no_asset_ids(&reply, &world);
}

#[test]
fn a_rez_never_guesses_the_object_and_its_receipt_can_be_identified() {
    let mut world = plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let at = world.agent.position + Vec3::new(2.0, 0.0, 0.0);
    let (c, answer) = call(
        REZ_PRIMITIVE,
        json!({"shape": "cube", "position": [at.x, at.y, at.z], "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [0.5, 0.5, 0.5], "client_request_id": "r-1"}),
    );
    desk.take(c, &ctx(&world), now);
    let send = sent(run(&mut desk, &world, now));
    let BuildCmd::Add(prim) = &send.command else {
        panic!("an ObjectAdd")
    };
    assert!(prim.bypass_raycast && prim.ray_end == at && prim.add_flags == 0);
    assert!(answer.try_recv().is_err(), "waits for candidates");
    // the demo simulator makes the object
    let mut sim = crate::build::demo_sim::DemoSim::default();
    for ev in sim.reply(&world, &send.command) {
        let _ = world.apply(ev);
    }
    desk.tick_reads(&world, now + Duration::from_secs(1));
    let reply = answered(&answer).expect("answered");
    assert_eq!(reply["object_id_confirmed"], false, "{reply}");
    assert_eq!(reply["state"], "unconfirmed", "{reply}");
    let candidates = reply["candidates"].as_array().expect("candidates").clone();
    assert_eq!(candidates.len(), 1, "{reply}");
    let operation = reply["operation_id"].as_str().expect("operation").to_owned();
    // object_operation_status reads the rez receipt
    let (c, status) = call(STATUS, json!({"operation_id": operation}));
    desk.take(c, &ctx(&world), now);
    assert!(answered(&status).is_ok());
    // object_rez_identify records the choice, once the viewer knows the
    // candidate's owner (a read of it asks the server, as for any object)
    let new = candidates[0].as_str().and_then(|s| Uuid::parse_str(s).ok()).expect("id");
    let (c, ident) = call(
        REZ_IDENTIFY,
        json!({"operation_id": operation, "object_id": new.to_string(), "client_request_id": "r-2"}),
    );
    desk.take(c, &ctx(&world), now);
    assert_eq!(code(answered(&ident)), ErrorCode::NotReady);
    let (c, answer) = call(OBJECT_GET, json!({"object_id": new.to_string()}));
    desk.take(c, &ctx(&world), now);
    serve(&mut desk, &world, DEMO_AGENT, now);
    assert!(answered(&answer).is_ok());
    let (c, ident) = call(
        REZ_IDENTIFY,
        json!({"operation_id": operation, "object_id": new.to_string(), "client_request_id": "r-3"}),
    );
    desk.take(c, &ctx(&world), now);
    let allowed = desk.tick(&IDLE, now).into_iter().next().expect("allowed");
    let planned = desk.objects().plan(&allowed, &ctx(&world), now, true).expect("planned");
    desk.objects().associate(allowed, planned.reply);
    let identified = answered(&ident).expect("identified");
    assert_eq!(identified["association"], "chosen", "{identified}");
    assert_eq!(identified["object_id_confirmed"], false);
    // an object that was there before the rez is never chosen
    let (c, ident) = call(
        REZ_IDENTIFY,
        json!({"operation_id": operation, "object_id": the_box().to_string(), "client_request_id": "r-4"}),
    );
    desk.take(c, &ctx(&world), now);
    let refused = match answer_or_run(&mut desk, &world, &ident, now) {
        Err(e) => e.code,
        Ok(v) => panic!("chosen: {v}"),
    };
    assert_eq!(refused, ErrorCode::NotAllowed);
}

/// The call's answer now, or once the desk lets it run and plans it.
fn answer_or_run(desk: &mut Desk, world: &World, answer: &Receiver<Result<Value, ToolError>>, now: Instant) -> Result<Value, ToolError> {
    if let Ok(result) = answer.try_recv() {
        return result;
    }
    let allowed = desk.tick(&IDLE, now).into_iter().next().expect("allowed");
    match desk.objects().plan(&allowed, &ctx(world), now, true) {
        Ok(planned) => desk.objects().associate(allowed, planned.reply),
        Err(e) => allowed.answer(Err(e)),
    }
    answered(answer)
}

#[test]
fn rezzing_needs_buildable_land_of_the_parcel_and_no_rlv_rez() {
    let now = Instant::now();
    let rez = |world: &World, offset: Vec3| {
        let at = world.agent.position + offset;
        json!({"shape": "sphere", "position": [at.x, at.y, at.z], "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0], "client_request_id": Uuid::new_v4().to_string()})
    };
    // someone else's land, closed to building
    let mut world = plaza();
    world.parcel = Some(Arc::new(ParcelInfo {
        owner_id: STRANGER,
        bitmap: vec![0xFF; 512],
        ..Default::default()
    }));
    let mut desk = desk_with(allow_all());
    assert_eq!(
        code(plan(&mut desk, &world, REZ_PRIMITIVE, rez(&world, Vec3::X), now)),
        ErrorCode::NotAllowed
    );
    // another parcel than the one the agent stands on
    let mut world = plaza();
    let p = world.agent.position;
    let mut bitmap = vec![0u8; 512];
    let cell = (p.y as usize / 4) * 64 + p.x as usize / 4;
    bitmap[cell / 8] |= 1 << (cell % 8);
    world.parcel = Some(Arc::new(ParcelInfo {
        owner_id: DEMO_AGENT,
        bitmap,
        ..Default::default()
    }));
    let mut desk = desk_with(allow_all());
    assert_eq!(
        code(plan(&mut desk, &world, REZ_PRIMITIVE, rez(&world, Vec3::new(20.0, 0.0, 0.0)), now)),
        ErrorCode::NotAllowed
    );
    // the parcel unknown
    let mut world = plaza();
    world.parcel = None;
    let mut desk = desk_with(allow_all());
    assert_eq!(
        code(plan(&mut desk, &world, REZ_PRIMITIVE, rez(&world, Vec3::X), now)),
        ErrorCode::NotReady
    );
    // RLVa @rez
    let mut world = plaza();
    world.rlva.configure(true);
    world.rlva_console_input("@rez=n");
    let mut desk = desk_with(allow_all());
    assert_eq!(
        code(plan(&mut desk, &world, REZ_PRIMITIVE, rez(&world, Vec3::X), now)),
        ErrorCode::NotAllowed
    );
    // an unknown shape
    let world = plaza();
    let mut desk = desk_with(allow_all());
    let mut args = rez(&world, Vec3::X);
    args["shape"] = json!("torus");
    assert_eq!(code(plan(&mut desk, &world, REZ_PRIMITIVE, args, now)), ErrorCode::InvalidArguments);
    // the client_request_id is required
    let mut args = rez(&world, Vec3::X);
    args.as_object_mut().map(|a| a.remove("client_request_id"));
    assert_eq!(code(plan(&mut desk, &world, REZ_PRIMITIVE, args, now)), ErrorCode::InvalidArguments);
}

#[test]
fn a_rez_from_the_inventory_needs_a_copyable_object_and_keeps_the_item() {
    let world = plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let at = world.agent.position + Vec3::new(1.0, 1.0, 0.0);
    let rez =
        |item: Uuid| json!({"item_id": item.to_string(), "position": [at.x, at.y, at.z], "client_request_id": Uuid::new_v4().to_string()});
    assert_eq!(
        code(plan(&mut desk, &world, REZ_INVENTORY, rez(NO_COPY), now)),
        ErrorCode::NotAllowed
    );
    assert_eq!(
        code(plan(&mut desk, &world, REZ_INVENTORY, rez(NOTECARD), now)),
        ErrorCode::NotAllowed
    );
    let (c, answer) = call(REZ_INVENTORY, rez(OBJECT));
    desk.take(c, &ctx(&world), now);
    let send = sent(run(&mut desk, &world, now));
    let BuildCmd::RezInventory(r) = &send.command else {
        panic!("a RezObject")
    };
    assert!(!r.remove && !r.selected && r.target.is_nil() && r.from_task.is_nil());
    assert_eq!(r.item.id, OBJECT);
    assert_eq!(r.ray_end, at);
    desk.tick_reads(&world, now + building::REZ_WAIT + Duration::from_secs(1));
    let reply = answered(&answer).expect("answered");
    assert_eq!(reply["candidates"], json!([]), "{reply}");
    assert_eq!(reply["object_id_confirmed"], false);
}

#[test]
fn permissions_are_read_with_a_revision_and_no_asset_id() {
    let world = plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let (c, answer) = call(PERMISSIONS_GET, json!({"item_id": NOTECARD.to_string()}));
    desk.take(c, &ctx(&world), now);
    let read = answered(&answer).expect("read");
    assert_eq!(read["next_owner"], json!({"copy": true, "modify": true, "transfer": true}));
    assert_eq!(read["next_owner_editable"], true);
    assert!(read["revision"].as_str().is_some_and(|r| r.starts_with("perm-")));
    no_asset_ids(&read, &world);
    // an object: readable, but its next owner rights are left to Properties
    let (c, answer) = call(PERMISSIONS_GET, json!({"item_id": OBJECT.to_string()}));
    desk.take(c, &ctx(&world), now);
    let read = answered(&answer).expect("read");
    assert_eq!(read["next_owner_editable"], false);
    assert!(read["why_not"].as_str().is_some_and(|w| w.contains("Properties")));
    // not transferable by you: the window does not change it
    let (c, answer) = call(PERMISSIONS_GET, json!({"item_id": NO_TRANSFER.to_string()}));
    desk.take(c, &ctx(&world), now);
    assert_eq!(answered(&answer).expect("read")["next_owner_editable"], false);
}

/// The next owner change of `item` to (copy, modify, transfer).
fn next_owner(
    desk: &mut Desk,
    world: &World,
    item: Uuid,
    rights: (bool, bool, bool),
    id: &str,
    now: Instant,
) -> Receiver<Result<Value, ToolError>> {
    let (c, answer) = call(PERMISSIONS_GET, json!({"item_id": item.to_string()}));
    desk.take(c, &ctx(world), now);
    let rev = answered(&answer).map(|r| r["revision"].clone()).unwrap_or(json!("perm-none"));
    let (c, answer) = call(
        NEXT_OWNER_SET,
        json!({"item_id": item.to_string(), "revision": rev, "copy": rights.0, "modify": rights.1, "transfer": rights.2, "client_request_id": id}),
    );
    desk.take(c, &ctx(world), now);
    answer
}

#[test]
fn next_owner_rights_always_ask_and_follow_the_properties_rules() {
    let world = plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    // asks even on Allow
    let answer = next_owner(&mut desk, &world, NOTECARD, (true, true, false), "n-1", now);
    let q = desk.question(now).expect("asks even on Allow");
    assert!(q.summary.contains('✗'), "{}", q.summary);
    desk.decide(q.number, true);
    let Some(Work::Inventory(change)) = run(&mut desk, &world, now) else {
        panic!("the Properties window's inventory patch")
    };
    let body = format!("{:?}", change.operations);
    let item = &world.inventory.items[&NOTECARD];
    let expected = (item.next_owner_mask & !actions::TRANSFER) as i32;
    assert!(body.contains(&expected.to_string()), "{body}");
    assert!(!body.contains("sale_info") && !body.contains("\"name\""), "{body}");
    let reply = answered(&answer).expect("answered");
    assert_eq!(reply["next_owner"], json!({"copy": true, "modify": true, "transfer": false}));
    // the rules: no copy and no transfer, objects, settings' copy, rights
    // you do not have, a stale revision
    for (item, rights, expected) in [
        (NOTECARD, (false, true, false), ErrorCode::NotAllowed),
        (OBJECT, (true, true, false), ErrorCode::NotAllowed),
        (SKY, (false, true, true), ErrorCode::NotAllowed),
        (NO_TRANSFER, (true, true, true), ErrorCode::NotAllowed),
        (NO_COPY, (true, true, true), ErrorCode::NotAllowed),
    ] {
        let answer = next_owner(&mut desk, &world, item, rights, &Uuid::new_v4().to_string(), now);
        assert_eq!(code(answered(&answer)), expected, "{item} {rights:?}");
    }
    let (c, answer) = call(
        NEXT_OWNER_SET,
        json!({"item_id": NOTECARD.to_string(), "revision": "perm-0000", "copy": true, "modify": false, "transfer": true, "client_request_id": "n-9"}),
    );
    desk.take(c, &ctx(&world), now);
    assert_eq!(code(answered(&answer)), ErrorCode::Stale);
    // Never on Edit refuses it
    let mut permissions = allow_all();
    permissions.set(Class::Edit, Level::Never);
    let mut desk = desk_with(permissions);
    let answer = next_owner(&mut desk, &world, NOTECARD, (true, false, true), "n-10", now);
    assert_eq!(code(answered(&answer)), ErrorCode::PermissionDenied);
}

#[test]
fn building_asks_in_ask_before_changes_and_never_refuses() {
    let world = plaza();
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let rev = revision(&mut desk, &world, now);
    let p = position(&world);
    let (c, _answer) = call(
        TRANSFORM,
        json!({"object_id": the_box().to_string(), "revision": rev, "position": [p.x, p.y, p.z + 0.2], "client_request_id": "a-1"}),
    );
    desk.take(c, &ctx(&world), now);
    let q = desk.question(now).expect("asks");
    assert!(q.summary.contains("Cut box"), "{}", q.summary);
    // Never on Build refuses
    let mut permissions = allow_all();
    permissions.set(Class::Build, Level::Never);
    let mut desk = desk_with(permissions);
    let at = world.agent.position + Vec3::X;
    let (c, answer) = call(
        REZ_PRIMITIVE,
        json!({"shape": "cube", "position": [at.x, at.y, at.z], "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [0.5, 0.5, 0.5], "client_request_id": "a-2"}),
    );
    desk.take(c, &ctx(&world), now);
    assert_eq!(code(answered(&answer)), ErrorCode::PermissionDenied);
    // a duplicate client_request_id answers the first receipt
    let mut desk = desk_with(allow_all());
    let args = json!({"shape": "cube", "position": [at.x, at.y, at.z], "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [0.5, 0.5, 0.5], "client_request_id": "dup-1"});
    let (c, _first) = call(REZ_PRIMITIVE, args.clone());
    desk.take(c, &ctx(&world), now);
    let (c, second) = call(REZ_PRIMITIVE, args);
    desk.take(c, &ctx(&world), now);
    assert_eq!(answered(&second).expect("duplicate")["duplicate"], true);
}

/// The sidecar's MCP layer → a real 127.0.0.1 socket → the bridge (Ask
/// before changes) → the desk, the dialog allowed: a read, then a move,
/// confirmed by the object update.
#[test]
fn an_assistant_reads_and_moves_an_owned_object_end_to_end() {
    use folderstorm_mcp::connect::Viewer;
    use folderstorm_mcp::jsonrpc::{Backend, Server};
    use folderstorm_mcp_bridge::Bridge;
    use folderstorm_mcp_bridge::server::Config;

    struct Live(Viewer);
    impl Backend for Live {
        fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
            self.0.call(tool, arguments)
        }
        fn client_name(&mut self, name: &str) {
            self.0.set_client_name(name);
        }
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = Bridge::start(Config {
        discovery_dir: Some(dir.path().to_path_buf()),
        pid: std::process::id(),
        version: "test".into(),
        permissions: Preset::AskBeforeChanges.permissions(),
        viewer_timeout: Duration::from_secs(10),
    })
    .expect("bridge started");
    let folder = dir.path().to_path_buf();
    let object = the_box();
    let mut world = plaza();
    let to = position(&world) + Vec3::new(0.0, 0.0, 0.5);
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        let mut ask = |line: &str| -> Value { serde_json::from_str(&server.handle_line(line).expect("answer")).expect("json") };
        ask(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code"}}}"#,
        );
        let read = ask(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"build_object_get","arguments":{{"object_id":"{object}"}}}}}}"#
        ));
        let revision = read["result"]["structuredContent"]["revision"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let moved = ask(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"build_object_transform","arguments":{{"object_id":"{object}","revision":"{revision}","position":[{},{},{}],"client_request_id":"e2e-build-1"}}}}}}"#,
            to.x, to.y, to.z
        ));
        (read, moved)
    });
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let mut questions = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        while let Ok(pending) = calls.try_recv() {
            desk.take(pending, &ctx(&world), now);
        }
        if let Some(q) = desk.question(now) {
            questions.push(q.summary.clone());
            desk.decide(q.number, true);
        }
        if let Some(Work::Build(send)) = run(&mut desk, &world, now) {
            echo(&mut world, &send.command);
        }
        serve(&mut desk, &world, DEMO_AGENT, now);
        std::thread::sleep(Duration::from_millis(5));
    }
    let (read, moved) = assistant.join().expect("assistant");
    assert_eq!(read["result"]["isError"], false, "{read}");
    assert_eq!(moved["result"]["isError"], false, "{moved}");
    assert_eq!(moved["result"]["structuredContent"]["done"], true, "{moved}");
    assert_eq!(questions.len(), 1, "{questions:?}");
    assert!((position(&world) - to).length() < 0.001);
    bridge.stop();
}

#[test]
fn the_demo_recipe_reads_moves_rezzes_and_asks_for_the_next_owner() {
    let world = plaza();
    let (tool, args) = building::demo::get();
    assert_eq!(tool, OBJECT_GET);
    assert_eq!(args["object_id"], json!(the_box().to_string()));
    let read = json!({"revision": "rev-1", "position": [10.0, 20.0, 30.0]});
    let (tool, args) = building::demo::move_after(&read).expect("move");
    assert_eq!(tool, TRANSFORM);
    assert_eq!(args["position"], json!([10.0, 20.0, 30.5]));
    let (tool, args) = building::demo::rez(world.agent.position);
    assert_eq!(tool, REZ_PRIMITIVE);
    assert_eq!(args["shape"], "cube");
    let (tool, _) = building::demo::permissions();
    assert_eq!(tool, PERMISSIONS_GET);
    let (tool, args) = building::demo::next_owner_after(&json!({"revision": "perm-1", "next_owner_editable": true})).expect("set");
    assert_eq!(tool, NEXT_OWNER_SET);
    assert_eq!((args["copy"].as_bool(), args["transfer"].as_bool()), (Some(true), Some(false)));
    assert!(building::demo::next_owner_after(&json!({"revision": "perm-1", "next_owner_editable": false})).is_none());
    // the demo's box, notecard and parcel are there
    assert!(building::demo::ready(&world));
    let _: Option<InvItem> = None;
}
