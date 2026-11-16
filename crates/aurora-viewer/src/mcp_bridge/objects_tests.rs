//! Folderstorm: the object tools (folderstorm/plans/mcp-objects.md) on the
//! demo plaza, with a small simulator for the prims' contents: the listing
//! (owner first, then the contents), the permissions matrix (owned,
//! deeded, someone else's, no modify, worn; the source items), snapshots
//! and staleness, puts (a script stopped, a verified copy, a hidden one
//! reconciled), rename, remove and take (always asking), RLVa, receipts
//! and the required `client_request_id`, and `object_rez_identify`.

use super::changes::Work;
use super::desk::{Bake, Desk};
use super::objects::{Scene, Send, Want};
use super::tools::{Ctx, Session};
use crate::demo::DEMO_AGENT;
use crate::world::World;
use crate::world::inventory::{FetchState, Folder, actions};
use aurora_net::NetEvent;
use aurora_net::build::{BuildCmd, ObjectProps, TaskItem, derez};
use aurora_net::inventory::{InvFolder, InvItem};
use folderstorm_mcp_bridge::protocol::objects::{LIST, PUT, RECONCILE, REMOVE, RENAME, REZ_IDENTIFY, STATUS, TAKE, TAKE_COPY};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{Class, ErrorCode, Level, Permissions, Preset, ToolError};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// The plaza's cut box (local id 118): the agent's in the demo.
const BOX_LOCAL: u32 = 118;
const SCRIPT: Uuid = Uuid::from_u128(8102);
const NOTECARD: Uuid = Uuid::from_u128(8103);
const ANIMATION: Uuid = Uuid::from_u128(8101);
const NO_COPY: Uuid = Uuid::from_u128(8109);
const LINK: Uuid = Uuid::from_u128(8113);
const STRANGER: Uuid = Uuid::from_u128(0x5747);
const GROUP: Uuid = Uuid::from_u128(0x6409);
const IDLE: Bake = Bake {
    outfit_change_pending: false,
    cof_version: 3,
    appearance_cof_version: 3,
    server_bakes: true,
};

fn prim_id(local: u32) -> Uuid {
    Uuid::from_u128(0xA0E0_0000_0000_0000_0000_0000_0000_0000 | local as u128)
}

/// The demo region and its prims, the demo inventory, the avatar next to
/// the cut box.
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
    let at = position(&world, BOX_LOCAL);
    world.agent.position = at + glam::Vec3::new(2.0, 0.0, 0.0);
    world
}

fn index(world: &World, local: u32) -> usize {
    world.objects.index_of_uuid(&prim_id(local)).expect("the prim")
}

fn position(world: &World, local: u32) -> glam::Vec3 {
    world.objects.get(index(world, local)).map(|o| o.position).expect("placed")
}

fn set_flags(world: &mut World, local: u32, flags: u32) {
    let idx = index(world, local);
    if let Some(o) = world.objects.get_mut(idx) {
        o.update_flags = flags;
    }
}

fn flags_all() -> u32 {
    use crate::ui::context::flags::*;
    OBJECT_YOU_OWNER | OBJECT_ANY_OWNER | OBJECT_MODIFY | OBJECT_MOVE | OBJECT_COPY
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
        avatar: Default::default(),
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

/// What the simulator holds and answers.
#[derive(Default)]
struct Sim {
    contents: HashMap<Uuid, (i16, Vec<TaskItem>)>,
    owners: HashMap<Uuid, Uuid>,
    /// New items arrive with their asset hidden.
    hidden: bool,
}

fn task_item(object: Uuid, n: u128, name: &str, asset_type: i8, owner_mask: u32) -> TaskItem {
    TaskItem {
        item_id: Uuid::from_u128(0x7A5C_0000 + n),
        parent_id: object,
        owner_id: DEMO_AGENT,
        base_mask: 0x7fff_ffff,
        owner_mask,
        asset_id: Uuid::from_u128(0xA55E_0000 + n),
        asset_type,
        inv_type: asset_type,
        name: name.into(),
        ..Default::default()
    }
}

impl Sim {
    /// The cut box with a script, a notecard you may not copy, and a
    /// texture you may not modify.
    fn plaza() -> Sim {
        let object = prim_id(BOX_LOCAL);
        let mut sim = Sim::default();
        sim.owners.insert(object, DEMO_AGENT);
        sim.contents.insert(
            object,
            (
                5,
                vec![
                    task_item(object, 1, "Door", 10, 0x7fff_ffff),
                    task_item(object, 2, "Read me", 7, 0x7fff_ffff & !actions::COPY),
                    task_item(object, 3, "Wood", 0, 0x7fff_ffff & !actions::MODIFY),
                ],
            ),
        );
        sim
    }

    /// Answer what the tools asked for, then let the desk move on.
    fn serve(&mut self, desk: &mut Desk, world: &World, now: Instant) {
        for _ in 0..3 {
            let wants = desk.objects().wanted();
            if wants.is_empty() {
                break;
            }
            for want in wants {
                let event = match want {
                    Want::Properties { object, .. } => NetEvent::ObjectProperties(vec![ObjectProps {
                        object_id: object,
                        owner_id: self.owners.get(&object).copied().unwrap_or(STRANGER),
                        name: "Cut box".into(),
                        ..Default::default()
                    }]),
                    Want::Contents { object, .. } => {
                        let (serial, items) = self.contents.get(&object).cloned().unwrap_or_default();
                        NetEvent::TaskInventory {
                            object,
                            serial: Some(serial),
                            result: Ok(items),
                        }
                    }
                };
                desk.objects().observe(&event, now);
            }
            desk.tick_reads(world, now);
        }
    }

    /// The simulator receives a command.
    fn apply(&mut self, world: &mut World, command: &BuildCmd) {
        let object_of = |world: &World, local: u32| {
            world
                .objects
                .index_of(&crate::world::objects::ObjKey {
                    region: world.main_region.unwrap_or_default(),
                    local_id: local,
                })
                .and_then(|i| world.objects.get(i))
                .map(|o| o.full_id)
                .expect("the prim")
        };
        match command {
            BuildCmd::RezScript { local_id, item, .. } | BuildCmd::UpdateTaskInventory { local_id, item, .. } => {
                let object = object_of(world, *local_id);
                let hidden = self.hidden;
                let entry = self.contents.entry(object).or_default();
                let mut it = (**item).clone();
                if it.item_id.is_nil() || world.inventory.items.contains_key(&it.item_id) {
                    it.item_id = Uuid::new_v4();
                    if hidden {
                        it.asset_id = Uuid::nil();
                    }
                }
                match entry.1.iter_mut().find(|i| i.item_id == it.item_id) {
                    Some(old) => *old = it,
                    None => entry.1.push(it),
                }
                entry.0 += 1;
            }
            BuildCmd::RemoveTaskInventory { local_id, item_id, .. } => {
                let object = object_of(world, *local_id);
                let entry = self.contents.entry(object).or_default();
                entry.1.retain(|i| i.item_id != *item_id);
                entry.0 += 1;
            }
            BuildCmd::DeRez {
                handle,
                local_ids,
                destination,
                destination_id,
                ..
            } => {
                world.inventory.add_items(vec![InvItem {
                    id: Uuid::new_v4(),
                    parent: *destination_id,
                    name: "Cut box".into(),
                    asset_type: 6,
                    inv_type: 6,
                    owner: DEMO_AGENT,
                    base_mask: 0x7fff_ffff,
                    owner_mask: 0x7fff_ffff,
                    ..Default::default()
                }]);
                if *destination == derez::TAKE_INTO_AGENT_INVENTORY {
                    let _ = world.apply(NetEvent::ObjectsKilled {
                        handle: *handle,
                        local_ids: local_ids.clone(),
                    });
                }
            }
            other => panic!("not an object tool's command: {other:?}"),
        }
    }
}

/// List a prim through the desk and the simulator.
fn list(desk: &mut Desk, world: &World, sim: &mut Sim, object: Uuid, now: Instant) -> Result<Value, ToolError> {
    let (c, answer) = call(LIST, json!({"object_id": object.to_string()}));
    desk.take(c, &ctx(world), now);
    sim.serve(desk, world, now);
    answered(&answer)
}

fn snapshot(desk: &mut Desk, world: &World, sim: &mut Sim, now: Instant) -> String {
    let page = list(desk, world, sim, prim_id(BOX_LOCAL), now).expect("listed");
    page["snapshot_id"].as_str().expect("a snapshot").to_owned()
}

/// The change the desk lets run now, planned again as just before the
/// send, and sent (to the simulator, then watched).
fn run(desk: &mut Desk, world: &mut World, sim: &mut Sim, now: Instant) -> Option<Send> {
    let runs = desk.tick(&IDLE, now);
    let call = runs.into_iter().next()?;
    let login = desk.login();
    let call = desk.recheck(call, login, None, &ctx(world), now)?;
    let planned = match desk.objects().plan(&call, &ctx(world), now, true) {
        Ok(planned) => planned,
        Err(e) => {
            call.answer(Err(e));
            return None;
        }
    };
    let Work::Object(send) = planned.work else {
        panic!("not an object change");
    };
    let send = *send;
    match send.command.clone() {
        None => desk.objects().associate(call, planned.reply),
        Some(command) => {
            desk.objects().sending(call, &send, planned.reply, world, now);
            sim.apply(world, &command);
        }
    }
    Some(send)
}

/// Let the viewer list again (its polls), with the simulator answering.
fn settle(desk: &mut Desk, world: &World, sim: &mut Sim, from: Instant, seconds: u64) {
    for s in 0..=seconds {
        let now = from + Duration::from_secs(s);
        desk.tick_reads(world, now);
        sim.serve(desk, world, now);
    }
}

fn change(desk: &mut Desk, world: &World, tool: &str, args: Value, now: Instant) -> Receiver<Result<Value, ToolError>> {
    let (c, answer) = call(tool, args);
    desk.take(c, &ctx(world), now);
    answer
}

fn refused_code(answer: &Receiver<Result<Value, ToolError>>) -> ErrorCode {
    answered(answer).expect_err("refused").code
}

fn attach(world: &mut World, item: Uuid) -> bool {
    let worn = crate::demo::outfit_attachment(&aurora_net::AttachRequest {
        item_id: item,
        owner_id: DEMO_AGENT,
        point: 2,
        add: true,
        flags: 0,
        group_mask: 0,
        everyone_mask: 0,
        next_owner_mask: 0,
        name: String::new(),
        desc: String::new(),
    });
    let handle = world.main_region.expect("a region");
    let _ = world.apply(NetEvent::ObjectUpdates {
        handle,
        objects: vec![worn],
    });
    world.objects.index_of_uuid(&item).is_some()
}

#[test]
fn a_listing_asks_the_owner_then_the_contents_and_never_gives_an_asset_id() {
    let world = plaza();
    let sim = Sim::plaza();
    let now = Instant::now();
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    let object = prim_id(BOX_LOCAL);
    let (c, answer) = call(LIST, json!({"object_id": object.to_string()}));
    desk.take(c, &ctx(&world), now);
    // the owner first: the update gave none
    let wants = desk.objects().wanted();
    assert!(
        matches!(wants.as_slice(), [Want::Properties { object: o, .. }] if *o == object),
        "{wants:?}"
    );
    assert!(answer.try_recv().is_err(), "waits");
    desk.objects().observe(
        &NetEvent::ObjectProperties(vec![ObjectProps {
            object_id: object,
            owner_id: DEMO_AGENT,
            name: "Cut box".into(),
            ..Default::default()
        }]),
        now,
    );
    desk.tick_reads(&world, now);
    let wants = desk.objects().wanted();
    assert!(
        matches!(wants.as_slice(), [Want::Contents { object: o, local_id: BOX_LOCAL, .. }] if *o == object),
        "{wants:?}"
    );
    let (serial, items) = sim.contents[&object].clone();
    desk.objects().observe(
        &NetEvent::TaskInventory {
            object,
            serial: Some(serial),
            result: Ok(items),
        },
        now,
    );
    desk.tick_reads(&world, now);
    let page = answered(&answer).expect("listed");
    assert!(page["snapshot_id"].as_str().is_some_and(|s| s.starts_with("snap-")), "{page}");
    assert_eq!((page["total"].as_u64(), page["count"].as_u64()), (Some(3), Some(3)));
    assert_eq!(page["object_name"], "Cut box");
    assert_eq!(page["expires_in"], 120);
    assert_eq!(page["items"][0]["type"], "script");
    assert_eq!(page["items"][1]["rights"]["copy"], false);
    assert_eq!(page["items"][1]["can_remove"], false);
    assert_eq!(page["items"][2]["can_rename"], false);
    assert!(page["note"].as_str().is_some_and(|n| n.contains("data")));
    let text = page.to_string();
    assert!(!text.contains("a55e") && !text.contains("asset"), "no asset id: {text}");
    // a later page from the snapshot, without asking the server
    let id = page["snapshot_id"].as_str().expect("id").to_owned();
    let (c, answer) = call(
        LIST,
        json!({"object_id": object.to_string(), "snapshot_id": id, "offset": 2, "limit": 5}),
    );
    desk.take(c, &ctx(&world), now);
    let page = answered(&answer).expect("a page");
    assert_eq!((page["count"].as_u64(), page["truncated"].as_bool()), (Some(1), Some(false)));
    assert!(desk.objects().wanted().is_empty());
    // a later page without its snapshot
    let (c, answer) = call(LIST, json!({"object_id": object.to_string(), "offset": 2}));
    desk.take(c, &ctx(&world), now);
    assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments);
}

#[test]
fn only_objects_you_own_and_may_modify_and_never_worn_ones() {
    let mut world = plaza();
    let now = Instant::now();
    let refused = |world: &World, sim: &mut Sim, object: Uuid| {
        let mut desk = desk_with(Preset::ReadOnly.permissions());
        list(&mut desk, world, sim, object, now).expect_err("refused")
    };
    // someone else's, though the update says "you own" (group powers)
    let mut sim = Sim::plaza();
    sim.owners.insert(prim_id(BOX_LOCAL), STRANGER);
    assert_eq!(refused(&world, &mut sim, prim_id(BOX_LOCAL)).code, ErrorCode::NotAllowed);
    // deeded to a group
    sim.owners.insert(prim_id(BOX_LOCAL), GROUP);
    let e = refused(&world, &mut sim, prim_id(BOX_LOCAL));
    assert!(e.message.contains("group"), "{e}");
    // not "you own", or no modify
    let mut sim = Sim::plaza();
    for flags in [
        flags_all() & !crate::ui::context::flags::OBJECT_YOU_OWNER,
        flags_all() & !crate::ui::context::flags::OBJECT_MODIFY,
    ] {
        set_flags(&mut world, BOX_LOCAL, flags);
        assert_eq!(refused(&world, &mut sim, prim_id(BOX_LOCAL)).code, ErrorCode::NotAllowed);
    }
    set_flags(&mut world, BOX_LOCAL, flags_all());
    // an avatar, an unknown id
    assert_eq!(refused(&world, &mut sim, crate::demo::DEMO_LOUP).code, ErrorCode::NotAllowed);
    assert_eq!(refused(&world, &mut sim, Uuid::from_u128(1)).code, ErrorCode::NotFound);
    // an attachment of ours, owned and modifiable: never
    let item = Uuid::from_u128(8100);
    assert!(attach(&mut world, item), "the attachment is loaded");
    sim.owners.insert(item, DEMO_AGENT);
    let e = refused(&world, &mut sim, item);
    assert_eq!(e.code, ErrorCode::NotAllowed);
    assert!(e.message.contains("worn"), "{e}");
    // ours and modifiable: listed
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    assert!(list(&mut desk, &world, &mut sim, prim_id(BOX_LOCAL), now).is_ok());
}

#[test]
fn a_put_copies_your_own_copyable_item_and_a_script_goes_in_stopped() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let object = prim_id(BOX_LOCAL);
    let args = json!({"object_id": object.to_string(), "snapshot_id": snap, "item_id": SCRIPT.to_string(), "client_request_id": "put-1"});
    let answer = change(&mut desk, &world, PUT, args.clone(), now);
    let send = run(&mut desk, &mut world, &mut sim, now).expect("sent");
    assert!(
        matches!(&send.command, Some(BuildCmd::RezScript { enabled: false, .. })),
        "a script goes in stopped: {:?}",
        send.command
    );
    assert!(answer.try_recv().is_err(), "waits for the listing");
    settle(&mut desk, &world, &mut sim, now, 2);
    let reply = answered(&answer).expect("done");
    assert_eq!((reply["done"].as_bool(), reply["state"].as_str()), (Some(true), Some("done")));
    assert_eq!(reply["running"], false);
    assert_eq!(reply["asset_identity_verified"], true);
    let task = reply["task_item_id"].as_str().expect("the copy").to_owned();
    assert!(sim.contents[&object].1.iter().any(|i| i.item_id.to_string() == task));
    assert!(
        !reply.to_string().contains(&Uuid::from_u128(9000 + 8102).to_string()),
        "no asset id"
    );
    // the same request again: its receipt, nothing sent
    let before = sim.contents[&object].1.len();
    let again = change(&mut desk, &world, PUT, args, now);
    let dup = answered(&again).expect("its receipt");
    assert_eq!((dup["duplicate"].as_bool(), dup["state"].as_str()), (Some(true), Some("done")));
    assert!(run(&mut desk, &mut world, &mut sim, now).is_none());
    assert_eq!(sim.contents[&object].1.len(), before);
    // the snapshot is stale now: the assistant changed the prim
    let late = change(
        &mut desk,
        &world,
        PUT,
        json!({"object_id": object.to_string(), "snapshot_id": snap, "item_id": NOTECARD.to_string(), "client_request_id": "put-2"}),
        now,
    );
    assert_eq!(refused_code(&late), ErrorCode::Stale);
    // its status
    let (c, answer) = call(STATUS, json!({"client_request_id": "put-1"}));
    desk.take(c, &ctx(&world), now);
    let status = answered(&answer).expect("status");
    assert_eq!(status["receipt"]["state"], "done");
    assert_eq!(status["object"]["task_item_id"], json!(task));
}

#[test]
fn a_put_needs_a_request_id_and_your_own_copyable_item() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let object = prim_id(BOX_LOCAL).to_string();
    let put =
        |item: Uuid, id: &str| json!({"object_id": object, "snapshot_id": snap, "item_id": item.to_string(), "client_request_id": id});
    // no client_request_id
    let answer = change(
        &mut desk,
        &world,
        PUT,
        json!({"object_id": object, "snapshot_id": snap, "item_id": SCRIPT.to_string()}),
        now,
    );
    let e = answered(&answer).expect_err("refused");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
    assert!(e.message.contains("client_request_id"), "{e}");
    // no copy, a link, a folder, an animation
    let root = world.inventory.root;
    for (item, code) in [
        (NO_COPY, ErrorCode::NotAllowed),
        (LINK, ErrorCode::NotAllowed),
        (root, ErrorCode::InvalidArguments),
        (ANIMATION, ErrorCode::NotAllowed),
    ] {
        let answer = change(&mut desk, &world, PUT, put(item, &format!("p-{item}")), now);
        assert_eq!(refused_code(&answer), code, "{item}");
    }
    // someone else's, a group's
    for (n, group) in [(1u128, false), (2, true)] {
        let id = Uuid::from_u128(0xBEEF_0000 + n);
        let parent = world.inventory.items[&SCRIPT].parent;
        world.inventory.add_items(vec![InvItem {
            id,
            parent,
            name: "Given".into(),
            asset_type: 7,
            inv_type: 7,
            owner: if group { GROUP } else { STRANGER },
            group_owned: group,
            base_mask: 0x7fff_ffff,
            owner_mask: 0x7fff_ffff,
            everyone_mask: 0x7fff_ffff,
            ..Default::default()
        }]);
        let answer = change(&mut desk, &world, PUT, put(id, &format!("o-{n}")), now);
        assert_eq!(refused_code(&answer), ErrorCode::NotAllowed, "group {group}");
    }
    // worn
    let mut cx = ctx(&world);
    cx.worn.insert(NOTECARD);
    let (c, answer) = call(PUT, put(NOTECARD, "worn"));
    desk.take(c, &cx, now);
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    assert!(run(&mut desk, &mut world, &mut sim, now).is_none(), "nothing sent");
}

#[test]
fn a_hidden_copy_is_unconfirmed_and_reconciled_once_by_choice() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    sim.hidden = true;
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let object = prim_id(BOX_LOCAL).to_string();
    let before = snapshot(&mut desk, &world, &mut sim, now);
    let answer = change(
        &mut desk,
        &world,
        PUT,
        json!({"object_id": object, "snapshot_id": before, "item_id": NOTECARD.to_string(), "client_request_id": "put-h"}),
        now,
    );
    run(&mut desk, &mut world, &mut sim, now).expect("sent");
    settle(&mut desk, &world, &mut sim, now, 2);
    let reply = answered(&answer).expect("answered");
    assert_eq!(
        (reply["done"].as_bool(), reply["state"].as_str()),
        (Some(false), Some("unconfirmed"))
    );
    let operation = reply["operation_id"].as_str().expect("its operation").to_owned();
    let candidate = reply["candidates"][0].as_str().expect("a candidate").to_owned();
    assert!(reply["next"].as_str().is_some_and(|n| n.contains("never send it again")));
    let later = now + Duration::from_secs(3);
    let reconcile = |snapshot: &str, item: &str, id: &str| json!({"object_id": object, "operation_id": operation, "item_id": item, "snapshot_id": snapshot, "client_request_id": id});
    // the snapshot from before the put: the prim changed since
    let answer = change(&mut desk, &world, RECONCILE, reconcile(&before, &candidate, "r-0"), later);
    assert_eq!(refused_code(&answer), ErrorCode::Stale);
    let fresh = snapshot(&mut desk, &world, &mut sim, later);
    // an item that was there before
    let door = Uuid::from_u128(0x7A5C_0001).to_string();
    let answer = change(&mut desk, &world, RECONCILE, reconcile(&fresh, &door, "r-1"), later);
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    // chosen: associated, unverified
    let answer = change(&mut desk, &world, RECONCILE, reconcile(&fresh, &candidate, "r-2"), later);
    run(&mut desk, &mut world, &mut sim, later).expect("recorded");
    let reply = answered(&answer).expect("associated");
    assert_eq!(reply["association"], "associated_unverified");
    assert_eq!(reply["asset_identity_verified"], false);
    assert_eq!(reply["copy_outcome"], "unknown");
    assert_eq!(reply["original_operation_id"], json!(operation));
    // the put's status shows it; its receipt stays unconfirmed
    let (c, answer) = call(STATUS, json!({"operation_id": operation}));
    desk.take(c, &ctx(&world), later);
    let status = answered(&answer).expect("status");
    assert_eq!(status["receipt"]["state"], "unconfirmed");
    assert_eq!(status["object"]["association"]["task_item_id"], json!(candidate));
    // never changed afterwards
    let answer = change(&mut desk, &world, RECONCILE, reconcile(&fresh, &candidate, "r-3"), later);
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
}

#[test]
fn rename_needs_modify_and_remove_always_asks_and_needs_copy_and_modify() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let object = prim_id(BOX_LOCAL).to_string();
    let item = |n: u128| Uuid::from_u128(0x7A5C_0000 + n).to_string();
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    // no modify: not renamed; a `|`: invalid
    let answer = change(
        &mut desk,
        &world,
        RENAME,
        json!({"object_id": object, "snapshot_id": snap, "item_id": item(3), "name": "Oak", "client_request_id": "n-1"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    let answer = change(
        &mut desk,
        &world,
        RENAME,
        json!({"object_id": object, "snapshot_id": snap, "item_id": item(1), "name": "a|b", "client_request_id": "n-2"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments);
    let answer = change(
        &mut desk,
        &world,
        RENAME,
        json!({"object_id": object, "snapshot_id": snap, "item_id": item(1), "name": "Gate", "client_request_id": "n-3"}),
        now,
    );
    run(&mut desk, &mut world, &mut sim, now).expect("sent");
    settle(&mut desk, &world, &mut sim, now, 2);
    assert_eq!(answered(&answer).expect("renamed")["done"], true);
    assert!(sim.contents[&prim_id(BOX_LOCAL)].1.iter().any(|i| i.name == "Gate"));
    // remove: no copy, refused before any dialog
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let answer = change(
        &mut desk,
        &world,
        REMOVE,
        json!({"object_id": object, "snapshot_id": snap, "item_id": item(2), "client_request_id": "d-1"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    assert!(desk.question(now).is_none());
    // a removable item asks, even on Allow
    let answer = change(
        &mut desk,
        &world,
        REMOVE,
        json!({"object_id": object, "snapshot_id": snap, "item_id": item(1), "client_request_id": "d-2"}),
        now,
    );
    let q = desk.question(now).expect("always asks");
    assert!(q.summary.contains("for good") && q.summary.contains("Gate"), "{}", q.summary);
    assert!(desk.tick(&IDLE, now).is_empty(), "nothing runs before the answer");
    desk.decide(q.number, true);
    run(&mut desk, &mut world, &mut sim, now).expect("sent");
    settle(&mut desk, &world, &mut sim, now, 2);
    assert_eq!(answered(&answer).expect("removed")["done"], true);
    assert!(!sim.contents[&prim_id(BOX_LOCAL)].1.iter().any(|i| i.name == "Gate"));
}

#[test]
fn a_snapshot_goes_stale_when_it_expires_or_the_contents_change() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let object = prim_id(BOX_LOCAL);
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let args =
        |id: &str| json!({"object_id": object.to_string(), "snapshot_id": snap, "item_id": SCRIPT.to_string(), "client_request_id": id});
    // 121 seconds later
    let answer = change(&mut desk, &world, PUT, args("s-1"), now + Duration::from_secs(121));
    assert_eq!(refused_code(&answer), ErrorCode::Stale);
    // the server lists another serial (the user changed it)
    desk.objects().observe(
        &NetEvent::TaskInventory {
            object,
            serial: Some(99),
            result: Ok(Vec::new()),
        },
        now,
    );
    let answer = change(&mut desk, &world, PUT, args("s-2"), now);
    let e = answered(&answer).expect_err("stale");
    assert_eq!(e.code, ErrorCode::Stale);
    assert!(e.message.contains("object_contents_list"), "{e}");
    // an unknown snapshot
    let answer = change(
        &mut desk,
        &world,
        PUT,
        json!({"object_id": object.to_string(), "snapshot_id": "snap-0", "item_id": SCRIPT.to_string(), "client_request_id": "s-3"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotFound);
}

#[test]
fn a_snapshot_is_checked_again_just_before_the_send() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let object = prim_id(BOX_LOCAL);
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let answer = change(
        &mut desk,
        &world,
        PUT,
        json!({"object_id": object.to_string(), "snapshot_id": snap, "item_id": SCRIPT.to_string(), "client_request_id": "b-1"}),
        now,
    );
    let q = desk.question(now).expect("asks");
    assert!(q.summary.contains("not running"), "{}", q.summary);
    // the contents change while the dialog is open
    desk.objects().observe(
        &NetEvent::TaskInventory {
            object,
            serial: Some(42),
            result: Ok(Vec::new()),
        },
        now,
    );
    desk.decide(q.number, true);
    assert!(run(&mut desk, &mut world, &mut sim, now).is_none());
    assert_eq!(refused_code(&answer), ErrorCode::Stale);
    assert_eq!(sim.contents[&object].1.len(), 3, "nothing sent");
}

#[test]
fn take_always_asks_take_copy_follows_its_level_and_both_watch_the_outcome() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let object = prim_id(BOX_LOCAL);
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    // a copy runs on Allow and finds the item
    let answer = change(
        &mut desk,
        &world,
        TAKE_COPY,
        json!({"object_id": object.to_string(), "snapshot_id": snap, "client_request_id": "t-1"}),
        now,
    );
    assert!(desk.question(now).is_none());
    let send = run(&mut desk, &mut world, &mut sim, now).unwrap_or_else(|| panic!("not sent: {:?}", answer.try_recv()));
    assert!(matches!(
        send.command,
        Some(BuildCmd::DeRez {
            destination: derez::ACQUIRE_TO_AGENT_INVENTORY,
            ..
        })
    ));
    desk.tick_reads(&world, now);
    let reply = answered(&answer).expect("taken");
    assert_eq!(
        (reply["done"].as_bool(), reply["item_id_confirmed"].as_bool()),
        (Some(true), Some(false))
    );
    assert!(reply["item_id"].as_str().is_some());
    assert!(world.objects.index_of_uuid(&object).is_some(), "the original stays");
    // take: always asks, then leaves the world
    let answer = change(
        &mut desk,
        &world,
        TAKE,
        json!({"object_id": object.to_string(), "snapshot_id": snap, "client_request_id": "t-2"}),
        now,
    );
    let q = desk.question(now).expect("always asks");
    assert!(q.summary.contains("leaves the world"), "{}", q.summary);
    desk.decide(q.number, true);
    run(&mut desk, &mut world, &mut sim, now).expect("sent");
    desk.tick_reads(&world, now);
    let reply = answered(&answer).expect("taken");
    assert_eq!(
        (reply["done"].as_bool(), reply["removed_from_world"].as_bool()),
        (Some(true), Some(true))
    );
}

#[test]
fn take_refusals_copy_and_move_rights_rlv_rez_and_the_folder() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let object = prim_id(BOX_LOCAL);
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let take = |id: &str| json!({"object_id": object.to_string(), "snapshot_id": snap, "client_request_id": id});
    set_flags(&mut world, BOX_LOCAL, flags_all() & !crate::ui::context::flags::OBJECT_COPY);
    assert_eq!(
        refused_code(&change(&mut desk, &world, TAKE_COPY, take("x-1"), now)),
        ErrorCode::NotAllowed
    );
    set_flags(&mut world, BOX_LOCAL, flags_all() & !crate::ui::context::flags::OBJECT_MOVE);
    assert_eq!(
        refused_code(&change(&mut desk, &world, TAKE, take("x-2"), now)),
        ErrorCode::NotAllowed
    );
    set_flags(&mut world, BOX_LOCAL, flags_all());
    world.rlva.configure(true);
    world.rlva_console_input("@rez=n");
    let e = answered(&change(&mut desk, &world, TAKE_COPY, take("x-3"), now)).expect_err("refused");
    assert!(e.message.contains("@rez"), "{e}");
    world.rlva_console_input("@clear");
    // a folder that takes no object
    let trash = actions::system(&world.inventory, 14).expect("a Trash");
    let answer = change(
        &mut desk,
        &world,
        TAKE_COPY,
        json!({"object_id": object.to_string(), "snapshot_id": snap, "folder_id": trash.to_string(), "client_request_id": "x-4"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    assert!(run(&mut desk, &mut world, &mut sim, now).is_none());
}

#[test]
fn rlv_restrictions_the_viewer_applies_refuse_the_object_tools() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let object = prim_id(BOX_LOCAL);
    world.rlva.configure(true);
    let editobj = format!("@editobj:{object}=n");
    for (rule, expect) in [
        ("@edit=n", "@edit"),
        (editobj.as_str(), "@edit"),
        ("@interact=n", "@interact"),
        ("@fartouch:1=n", "@fartouch"),
    ] {
        world.rlva_console_input(rule);
        let mut desk = desk_with(allow_all());
        let e = list(&mut desk, &world, &mut sim, object, now).expect_err("refused");
        assert_eq!(e.code, ErrorCode::NotAllowed, "{rule}");
        assert!(e.message.contains(expect), "{rule}: {e}");
        world.rlva_console_input("@clear");
    }
    // @showinv: nothing given from the inventory
    let mut desk = desk_with(allow_all());
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    world.rlva_console_input("@showinv=n");
    let answer = change(
        &mut desk,
        &world,
        PUT,
        json!({"object_id": object.to_string(), "snapshot_id": snap, "item_id": SCRIPT.to_string(), "client_request_id": "v-1"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::PermissionDenied);
}

#[test]
fn levels_on_never_and_a_login_change_refuse() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut p = Preset::AskBeforeChanges.permissions();
    p.set(Class::Objects, Level::Never);
    let mut desk = desk_with(p);
    let object = prim_id(BOX_LOCAL);
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let answer = change(
        &mut desk,
        &world,
        PUT,
        json!({"object_id": object.to_string(), "snapshot_id": snap, "item_id": SCRIPT.to_string(), "client_request_id": "l-1"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::PermissionDenied);
    // a listing waiting when the login changes
    let (c, answer) = call(LIST, json!({"object_id": object.to_string()}));
    desk.take(c, &ctx(&world), now);
    desk.set_login(7, Some("vs-77777777777777777777777777777777".into()));
    assert_eq!(refused_code(&answer), ErrorCode::SessionChanged);
    // the snapshots went with the old login
    desk.set_permissions(allow_all());
    let answer = change(
        &mut desk,
        &world,
        PUT,
        json!({"object_id": object.to_string(), "snapshot_id": snap, "item_id": SCRIPT.to_string(), "client_request_id": "l-2"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotFound);
}

#[test]
fn a_rez_is_identified_by_choice_only_with_a_new_owned_root() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    // no rez receipt: this build has no rez tool
    let answer = change(
        &mut desk,
        &world,
        REZ_IDENTIFY,
        json!({"operation_id": "op-1", "object_id": prim_id(BOX_LOCAL).to_string(), "client_request_id": "z-1"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotFound);
    // the owner known (a listing), then a rez tracked without the box
    snapshot(&mut desk, &world, &mut sim, now);
    let operation = "op-00000000000000000000000000000077";
    desk.objects().track_rez(operation, [prim_id(117)].into_iter().collect(), now);
    let answer = change(
        &mut desk,
        &world,
        REZ_IDENTIFY,
        json!({"operation_id": operation, "object_id": prim_id(BOX_LOCAL).to_string(), "client_request_id": "z-2"}),
        now,
    );
    run(&mut desk, &mut world, &mut sim, now).expect("recorded");
    let reply = answered(&answer).expect("chosen");
    assert_eq!(
        (reply["association"].as_str(), reply["object_id_confirmed"].as_bool()),
        (Some("chosen"), Some(false))
    );
    let answer = change(
        &mut desk,
        &world,
        REZ_IDENTIFY,
        json!({"operation_id": operation, "object_id": prim_id(BOX_LOCAL).to_string(), "client_request_id": "z-3"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed, "never changed afterwards");
    // an object present before the rez
    let other = "op-00000000000000000000000000000078";
    desk.objects().track_rez(other, [prim_id(BOX_LOCAL)].into_iter().collect(), now);
    let answer = change(
        &mut desk,
        &world,
        REZ_IDENTIFY,
        json!({"operation_id": other, "object_id": prim_id(BOX_LOCAL).to_string(), "client_request_id": "z-4"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
}

#[test]
fn an_unconfirmed_change_answers_so_and_settles_late() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let object = prim_id(BOX_LOCAL);
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let answer = change(
        &mut desk,
        &world,
        PUT,
        json!({"object_id": object.to_string(), "snapshot_id": snap, "item_id": SCRIPT.to_string(), "client_request_id": "u-1"}),
        now,
    );
    let send = run(&mut desk, &mut world, &mut sim, now).expect("sent");
    // the server's listings never show it (lost on the way)
    let kept: Vec<TaskItem> = sim.contents[&object].1.iter().take(3).cloned().collect();
    sim.contents.insert(object, (5, kept));
    settle(&mut desk, &world, &mut sim, now, 31);
    let reply = answered(&answer).expect("answered");
    assert_eq!(
        (reply["state"].as_str(), reply["unconfirmed"].as_bool()),
        (Some("unconfirmed"), Some(true))
    );
    // it shows later: the receipt settles
    let command = send.command.expect("a command");
    sim.apply(&mut world, &command);
    let (serial, items) = sim.contents[&object].clone();
    desk.objects().observe(
        &NetEvent::TaskInventory {
            object,
            serial: Some(serial),
            result: Ok(items),
        },
        now + Duration::from_secs(40),
    );
    let (c, answer) = call(STATUS, json!({"client_request_id": "u-1"}));
    desk.take(c, &ctx(&world), now);
    assert_eq!(answered(&answer).expect("status")["receipt"]["state"], "done");
}

/// The sidecar's MCP layer → a real 127.0.0.1 socket → the bridge (Ask
/// before changes) → the desk, the dialog allowed → the simulator: a
/// listing, then a put of the demo script, confirmed by the listing after.
#[test]
fn an_assistant_lists_and_puts_into_an_owned_prim_end_to_end() {
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
    let object = prim_id(BOX_LOCAL);
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        let mut ask = |line: &str| -> Value { serde_json::from_str(&server.handle_line(line).expect("answer")).expect("json") };
        ask(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code"}}}"#,
        );
        let listed = ask(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"object_contents_list","arguments":{{"object_id":"{object}"}}}}}}"#
        ));
        let snapshot = listed["result"]["structuredContent"]["snapshot_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let put = ask(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"object_contents_put","arguments":{{"object_id":"{object}","snapshot_id":"{snapshot}","item_id":"{SCRIPT}","client_request_id":"e2e-1"}}}}}}"#
        ));
        (listed, put)
    });
    let mut world = plaza();
    let mut sim = Sim::plaza();
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
        run(&mut desk, &mut world, &mut sim, now);
        sim.serve(&mut desk, &world, now);
        desk.tick_reads(&world, now);
        std::thread::sleep(Duration::from_millis(5));
    }
    let (listed, put) = assistant.join().expect("assistant");
    assert_eq!(listed["result"]["isError"], false, "{listed}");
    assert_eq!(listed["result"]["structuredContent"]["total"], 3);
    assert_eq!(put["result"]["isError"], false, "{put}");
    let reply = &put["result"]["structuredContent"];
    assert_eq!(
        (reply["done"].as_bool(), reply["running"].as_bool()),
        (Some(true), Some(false)),
        "{reply}"
    );
    assert_eq!(questions.len(), 1, "{questions:?}");
    assert!(questions[0].contains("not running"), "{questions:?}");
    assert_eq!(sim.contents[&object].1.len(), 4);
    bridge.stop();
}
