//! Folderstorm: phase 3, part 2 (inventory gaps) against the demo
//! inventory: the new reads, the journal of `inventory_changes`, landmarks,
//! the new changes and their refusals (creators' permissions included), the
//! desk's waits for a created item and a picture, and the bridge's classes.

use super::change_tests::{ctx, demo_inventory};
use super::changes::{Planned, Work, plan};
use super::desk::{Bake, Desk, Outcome};
use super::journal::{IDLE, Journal};
use super::tools::{Ctx, Session, answer};
use crate::world::inventory::{FetchState, Folder, Inventory, actions};
use aurora_net::inventory::operations::NewItem;
use aurora_net::inventory::{InvFolder, InvItem};
use folderstorm_mcp_bridge::protocol::inventory::CHANGES_MAX;
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use uuid::Uuid;

const AGENT: Uuid = Uuid::from_u128(2);
const DEMO_FOLDER: Uuid = Uuid::from_u128(8000);
const SUBFOLDER: Uuid = Uuid::from_u128(8001);
const MARKETPLACE: Uuid = Uuid::from_u128(8002);
const OBJECT: Uuid = Uuid::from_u128(8100);
const SCRIPT: Uuid = Uuid::from_u128(8102);
const TEXTURE: Uuid = Uuid::from_u128(8104);
const NO_COPY: Uuid = Uuid::from_u128(8109);
const NO_TRANSFER: Uuid = Uuid::from_u128(8110);
const LINK: Uuid = Uuid::from_u128(8113);
const COF: Uuid = Uuid::from_u128(701);
const NEUROLAB: Uuid = Uuid::from_u128(704);
const SILHOUETTE: Uuid = Uuid::from_u128(710);
const SPARE_ATTACHMENT: Uuid = Uuid::from_u128(723);
const OTHER: Uuid = Uuid::from_u128(77);

fn read(cx: &Ctx, tool: &str, args: Value) -> Value {
    answer(tool, &args, cx).unwrap_or_else(|e| panic!("{tool} {args}: {e}"))
}

fn read_error(cx: &Ctx, tool: &str, args: Value) -> ErrorCode {
    match answer(tool, &args, cx) {
        Ok(v) => panic!("{tool} {args} answered {v}"),
        Err(e) => e.code,
    }
}

fn planned(inv: &Inventory, tool: &str, args: Value) -> Planned {
    plan(tool, &args, &ctx(inv)).unwrap_or_else(|e| panic!("{tool} {args}: {e}"))
}

fn refused(inv: &Inventory, tool: &str, args: Value) -> ErrorCode {
    match plan(tool, &args, &ctx(inv)) {
        Ok(p) => panic!("{tool} {args} was planned: {:?}", p.work),
        Err(e) => e.code,
    }
}

/// Run an inventory change as the demo's server would.
fn apply(inv: &mut Inventory, planned: Planned) {
    match planned.work {
        Work::Nothing => {}
        Work::Inventory(change) => {
            actions::demo_mutate(inv, AGENT, change).expect("applied");
        }
        other => panic!("not an inventory change: {other:?}"),
    }
}

fn add_folder(inv: &mut Inventory, id: Uuid, parent: Uuid, name: &str, kind: i32, library: bool) {
    inv.folders.insert(
        id,
        Folder {
            info: InvFolder {
                id,
                parent,
                name: name.into(),
                type_default: kind,
                version: 1,
                ..Default::default()
            },
            children: vec![],
            items: vec![],
            state: FetchState::Fetched,
            library,
        },
    );
    if let Some(f) = inv.folders.get_mut(&parent) {
        f.children.push(id);
    }
}

fn texture(id: Uuid, parent: Uuid, owner: Uuid, owner_mask: u32, everyone_mask: u32, group_owned: bool) -> InvItem {
    InvItem {
        id,
        parent,
        name: format!("texture {}", id.as_u128()),
        asset_type: 0,
        inv_type: 0,
        asset_id: Uuid::from_u128(id.as_u128() + 1_000_000),
        owner,
        group_owned,
        owner_mask,
        everyone_mask,
        ..Default::default()
    }
}

// --- reads ---

#[test]
fn get_many_gives_items_and_folders_with_the_asset_id_rule() {
    let inv = demo_inventory();
    let cx = ctx(&inv);
    let missing = Uuid::from_u128(1);
    let out = read(
        &cx,
        "inventory_get_many",
        json!({"ids": [TEXTURE.to_string(), NO_TRANSFER.to_string(), DEMO_FOLDER.to_string(), missing.to_string(), TEXTURE.to_string()]}),
    );
    let entries = out["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 3, "a repeated id once");
    assert_eq!(out["missing"], json!([missing]));
    // full permissions: the asset id, as the viewer shows it
    assert_eq!(entries[0]["id"], json!(TEXTURE));
    assert!(entries[0]["asset_id"].is_string());
    // no transfer: hidden
    assert_eq!(entries[1]["asset_id_hidden"], true);
    assert!(entries[1].get("asset_id").is_none());
    assert_eq!(entries[2]["type"], "folder");
    assert!(entries[2]["items"].as_u64().is_some_and(|n| n > 0));
    assert!(entries.iter().all(|e| e.get("thumbnail").is_none()), "never a thumbnail id");
    for bad in [
        json!({}),
        json!({"ids": []}),
        json!({"ids": ["nope"]}),
        json!({"ids": [TEXTURE.to_string()], "x": 1}),
    ] {
        assert_eq!(
            read_error(&cx, "inventory_get_many", bad.clone()),
            ErrorCode::InvalidArguments,
            "{bad}"
        );
    }
    let too_many: Vec<String> = (0..51).map(|n| Uuid::from_u128(n).to_string()).collect();
    assert_eq!(
        read_error(&cx, "inventory_get_many", json!({"ids": too_many})),
        ErrorCode::InvalidArguments
    );
}

#[test]
fn system_folders_types_and_protected_folders() {
    let mut inv = demo_inventory();
    let lib_textures = Uuid::from_u128(99_001);
    let lib_root = inv.lib_root;
    add_folder(&mut inv, lib_textures, lib_root, "Textures", 0, true);
    let pinned = Uuid::from_u128(99_002);
    let root = inv.root;
    add_folder(&mut inv, pinned, root, "#RLV", -1, false);
    let mut cx = ctx(&inv);
    cx.protected.insert(SUBFOLDER);
    let trash = actions::system(&inv, 14).expect("trash");
    for (name, id) in [
        ("trash", trash),
        ("Trash", trash),
        ("current outfit", COF),
        ("cof", COF),
        ("My Outfits", Uuid::from_u128(700)),
    ] {
        assert_eq!(
            read(&cx, "inventory_system_folder", json!({"type": name}))["id"],
            json!(id),
            "{name}"
        );
    }
    // the Library's Textures is not My Inventory's
    assert_eq!(
        read_error(&cx, "inventory_system_folder", json!({"type": "textures"})),
        ErrorCode::NotFound
    );
    assert_eq!(
        read_error(&cx, "inventory_system_folder", json!({"type": "outfit"})),
        ErrorCode::InvalidArguments
    );
    assert_eq!(read_error(&cx, "inventory_system_folder", json!({})), ErrorCode::InvalidArguments);

    let types = read(&cx, "inventory_types", json!({}));
    assert!(types["kinds"].as_array().is_some_and(|k| k.contains(&json!("landmark"))));
    assert!(types["system_folders"].as_array().is_some_and(|k| k.contains(&json!("trash"))));
    assert!(types["wearables"].as_array().is_some_and(|k| k.contains(&json!("shirt"))));
    assert!(types["create_item_types"].as_array().is_some_and(|k| !k.contains(&json!("shirt"))));

    let out = read(&cx, "inventory_protected_folders", json!({}));
    let reasons = |id: Uuid| -> Vec<Value> {
        out["folders"]
            .as_array()
            .expect("folders")
            .iter()
            .find(|f| f["id"] == json!(id))
            .map(|f| f["protected"].as_array().cloned().unwrap_or_default())
            .unwrap_or_default()
    };
    assert_eq!(reasons(inv.root), vec![json!("system")]);
    assert_eq!(reasons(trash), vec![json!("system")]);
    assert_eq!(reasons(COF), vec![json!("system")]);
    assert!(reasons(MARKETPLACE).contains(&json!("marketplace")));
    assert_eq!(reasons(SUBFOLDER), vec![json!("locked")]);
    assert_eq!(reasons(pinned), vec![json!("pinned")]);
    assert!(reasons(DEMO_FOLDER).is_empty(), "an ordinary folder");
    assert!(reasons(NEUROLAB).is_empty(), "an outfit folder");
    assert!(reasons(lib_textures).is_empty(), "the Library is named once");
    assert_eq!(out["library_id"], json!(inv.lib_root));
}

#[test]
fn an_outfits_items_follow_their_links_with_the_worn_flag() {
    let inv = demo_inventory();
    let cx = ctx(&inv);
    let out = read(&cx, "appearance_outfit_items", json!({"outfit_id": NEUROLAB.to_string()}));
    let items = out["items"].as_array().expect("items");
    assert_eq!(items.len(), 10);
    let by_target = |id: Uuid| items.iter().find(|i| i["target_id"] == json!(id)).expect("listed").clone();
    assert_eq!(by_target(SILHOUETTE)["worn"], true);
    assert_eq!(by_target(SILHOUETTE)["type"], "body_part");
    assert_eq!(by_target(SILHOUETTE)["wearable"], "shape");
    assert_eq!(by_target(SPARE_ATTACHMENT)["worn"], false);
    assert!(items.iter().all(|i| i.get("asset_id").is_none()));
    let path = format!("{}/Neurolab", inv.folders[&Uuid::from_u128(700)].info.name);
    assert_eq!(
        read(&cx, "appearance_outfit_items", json!({"path": path}))["outfit_id"],
        json!(NEUROLAB)
    );
    assert_eq!(read_error(&cx, "appearance_outfit_items", json!({})), ErrorCode::InvalidArguments);
    assert_eq!(
        read_error(&cx, "appearance_outfit_items", json!({"outfit_id": Uuid::from_u128(1).to_string()})),
        ErrorCode::NotFound
    );
}

#[test]
fn search_filters_by_creator_rights_worn_favorite_date_and_links() {
    let mut inv = demo_inventory();
    if let Some(it) = inv.items.get_mut(&SCRIPT) {
        it.creator = OTHER;
        it.desc = "Opens the door".into();
        it.created_at = 1_000;
    }
    if let Some(it) = inv.items.get_mut(&TEXTURE) {
        it.favorite = true;
        it.created_at = 5_000;
    }
    let cx = ctx(&inv);
    let ids = |args: Value| -> Vec<Value> {
        read(&cx, "inventory_search", args)["results"]
            .as_array()
            .expect("results")
            .iter()
            .map(|r| r["id"].clone())
            .collect()
    };
    assert_eq!(ids(json!({"creator_id": OTHER.to_string()})), vec![json!(SCRIPT)]);
    assert_eq!(ids(json!({"description": "door"})), vec![json!(SCRIPT)]);
    let no_copy = ids(json!({"copyable": false, "folder_id": DEMO_FOLDER.to_string()}));
    assert!(
        no_copy.contains(&json!(NO_COPY)) && !no_copy.contains(&json!(TEXTURE)),
        "{no_copy:?}"
    );
    assert!(ids(json!({"modifiable": true, "folder_id": DEMO_FOLDER.to_string()})).contains(&json!(TEXTURE)));
    let worn = ids(json!({"worn": true, "limit": 200}));
    assert!(worn.contains(&json!(SILHOUETTE)));
    assert!(!worn.contains(&json!(TEXTURE)));
    assert_eq!(ids(json!({"favorite": true})), vec![json!(TEXTURE)]);
    assert_eq!(
        ids(json!({"created_before": 2_000, "folder_id": DEMO_FOLDER.to_string(), "type": "script"})),
        vec![json!(SCRIPT)]
    );
    assert!(!ids(json!({"created_after": 2_000, "folder_id": DEMO_FOLDER.to_string()})).contains(&json!(SCRIPT)));
    assert_eq!(
        ids(json!({"filter_links": "only", "folder_id": DEMO_FOLDER.to_string()})),
        vec![json!(LINK)]
    );
    assert!(!ids(json!({"filter_links": "exclude", "folder_id": DEMO_FOLDER.to_string()})).contains(&json!(LINK)));
    assert_eq!(ids(json!({"linked_id": OBJECT.to_string()})), vec![json!(LINK)]);
    // a link is judged by its original: the object is not worn, has Copy
    assert!(ids(json!({"filter_links": "only", "copyable": true, "limit": 200})).contains(&json!(LINK)));
    // an item filter leaves the folders out; the favorite filter keeps them
    assert!(
        ids(json!({"query": "", "creator_id": AGENT.to_string()}))
            .iter()
            .all(|id| *id != json!(DEMO_FOLDER))
    );
    for bad in [
        json!({"worn": "yes"}),
        json!({"filter_links": "some"}),
        json!({"creator_id": "nope"}),
        json!({"created_after": "today"}),
    ] {
        assert_eq!(
            read_error(&cx, "inventory_search", bad.clone()),
            ErrorCode::InvalidArguments,
            "{bad}"
        );
    }
    // the entries say what is a favorite
    assert_eq!(
        read(&cx, "inventory_item", json!({"item_id": TEXTURE.to_string()}))["favorite"],
        true
    );
}

// --- inventory_changes ---

#[test]
fn the_journal_reports_what_changed_since_a_generation() {
    let mut inv = demo_inventory();
    let mut journal = Journal::default();
    let now = Instant::now();
    let start = journal.answer(&json!({}), &inv, now).expect("started");
    assert!(journal.following());
    let g0 = start["generation"].as_u64().expect("generation");
    assert_eq!(start["changes"], json!([]));
    // a rename, a new folder, a removal (the server's answers bump the
    // inventory's generation; the demo's mutation alone does not)
    let p = planned(&inv, "inventory_rename", json!({"id": TEXTURE.to_string(), "name": "Brick"}));
    apply(&mut inv, p);
    let p = planned(&inv, "inventory_create_folder", json!({"name": "New"}));
    apply(&mut inv, p);
    inv.generation += 1;
    inv.remove(&[OBJECT]);
    let out = journal.answer(&json!({"since": g0}), &inv, now).expect("changes");
    let changes = out["changes"].as_array().expect("changes");
    let change = |id: Uuid| changes.iter().find(|c| c["id"] == json!(id)).map(|c| c["change"].clone());
    assert_eq!(change(TEXTURE), Some(json!("changed")));
    assert_eq!(changes.iter().find(|c| c["id"] == json!(TEXTURE)).expect("rename")["name"], "Brick");
    assert_eq!(change(OBJECT), Some(json!("removed")));
    assert!(
        changes
            .iter()
            .any(|c| c["change"] == "added" && c["kind"] == "folder" && c["name"] == "New")
    );
    assert!(changes.iter().all(|c| c.get("asset_id").is_none()));
    let g1 = out["generation"].as_u64().expect("generation");
    assert!(g1 > g0);
    assert_eq!(out["more"], false);
    // nothing since
    assert_eq!(
        journal.answer(&json!({"since": g1}), &inv, now).expect("none")["changes"],
        json!([])
    );
    // added then removed: nothing to report
    let added = Uuid::from_u128(99_500);
    inv.add_items(vec![texture(added, DEMO_FOLDER, AGENT, actions::FULL, 0, false)]);
    let later = now + Duration::from_secs(2);
    journal.follow(&inv, later);
    inv.remove(&[added]);
    let out = journal
        .answer(&json!({"since": g1}), &inv, later + Duration::from_secs(2))
        .expect("changes");
    assert_eq!(out["changes"], json!([]), "{out}");
    // ahead of the viewer, or bad arguments
    let ahead = out["generation"].as_u64().expect("g") + 10;
    assert_eq!(
        journal.answer(&json!({"since": ahead}), &inv, later).expect_err("ahead").code,
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        journal.answer(&json!({"since": -1}), &inv, later).expect_err("bad").code,
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        journal.answer(&json!({"x": 1}), &inv, later).expect_err("bad").code,
        ErrorCode::InvalidArguments
    );
}

#[test]
fn the_journal_stops_when_idle_and_pages_long_answers() {
    let mut inv = demo_inventory();
    let mut journal = Journal::default();
    let now = Instant::now();
    let g0 = journal.answer(&json!({}), &inv, now).expect("started")["generation"]
        .as_u64()
        .expect("g");
    // the Library does not count; many new items come in pages
    let lib_item = Uuid::from_u128(99_600);
    inv.add_items(vec![texture(
        lib_item,
        inv.lib_root,
        inv.lib_owner,
        actions::FULL,
        actions::COPY,
        false,
    )]);
    let many: Vec<InvItem> = (0..CHANGES_MAX as u128 + 50)
        .map(|n| texture(Uuid::from_u128(100_000 + n), DEMO_FOLDER, AGENT, actions::FULL, 0, false))
        .collect();
    inv.add_items(many);
    let first = journal.answer(&json!({"since": g0}), &inv, now).expect("page 1");
    assert_eq!(first["changes"].as_array().map(Vec::len), Some(CHANGES_MAX));
    assert_eq!(first["more"], true);
    assert!(first["changes"].as_array().expect("c").iter().all(|c| c["id"] != json!(lib_item)));
    let g1 = first["generation"].as_u64().expect("g");
    let second = journal.answer(&json!({"since": g1}), &inv, now).expect("page 2");
    assert_eq!(second["changes"].as_array().map(Vec::len), Some(50));
    assert_eq!(second["more"], false);
    let g2 = second["generation"].as_u64().expect("g");
    // ten minutes without a call: it stops, and an old cursor is reset
    journal.follow(&inv, now + IDLE);
    assert!(!journal.following());
    let reset = journal.answer(&json!({"since": g2}), &inv, now + IDLE).expect("reset");
    assert_eq!(reset["reset"], true);
    assert!(reset["generation"].as_u64().expect("g") >= g2);
    // another login stops it too
    inv.root = Uuid::from_u128(81);
    journal.follow(&inv, now + IDLE);
    assert!(!journal.following());
}

#[test]
fn inventory_changes_through_the_desk_follows_the_read_rules() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = Desk::default();
    let (a, answer) = call("inventory_changes", json!({}), false);
    desk.take(a, &ctx(&inv), now);
    assert!(answered(&answer).expect("generation")["generation"].is_u64());
    let mut cx = ctx(&inv);
    cx.inventory_locked = true;
    let (b, answer) = call("inventory_changes", json!({}), false);
    desk.take(b, &cx, now);
    assert_eq!(answered(&answer).expect_err("locked").code, ErrorCode::PermissionDenied);
    cx.inventory_locked = false;
    cx.session = Session::LoggedOut;
    let (c, answer) = call("inventory_changes", json!({}), false);
    desk.take(c, &cx, now);
    assert_eq!(answered(&answer).expect_err("logged out").code, ErrorCode::NotReady);
}

// --- inventory_landmark ---

fn landmark_world() -> crate::world::World {
    let mut world = crate::world::World::new(std::sync::Arc::new(crate::scene::avatar::AvatarLibrary::load()));
    world.inventory = demo_inventory();
    world
}

#[test]
fn a_landmark_answers_its_location_once_known() {
    let mut world = landmark_world();
    let landmark = Uuid::from_u128(99_700);
    let asset = Uuid::from_u128(99_701);
    let link = Uuid::from_u128(99_702);
    world.inventory.add_items(vec![
        InvItem {
            id: landmark,
            parent: DEMO_FOLDER,
            name: "Plaza".into(),
            asset_type: 3,
            inv_type: 3,
            asset_id: asset,
            owner: AGENT,
            owner_mask: actions::MOVE | actions::TRANSFER,
            ..Default::default()
        },
        InvItem {
            id: link,
            parent: SUBFOLDER,
            name: "Plaza".into(),
            asset_type: 24,
            inv_type: 3,
            asset_id: landmark,
            owner: AGENT,
            ..Default::default()
        },
    ]);
    let now = Instant::now();
    let mut desk = Desk::default();
    // not loaded yet: the call waits, and asks for the asset
    let (a, answer) = call("inventory_landmark", json!({"item_id": link.to_string()}), false);
    desk.take(a, &ctx(&world.inventory), now);
    desk.tick_reads(&world, now);
    assert!(answer.try_recv().is_err());
    assert_eq!(desk.wanted_landmarks(), vec![asset]);
    world
        .landmarks
        .on_data(asset, Some(b"Landmark version 1\nposition 256130.0 256200.5 23.0\n"));
    desk.tick_reads(&world, now);
    let out = answered(&answer).expect("located");
    assert_eq!(out["item_id"], json!(landmark), "the link's original");
    assert_eq!(out["global_position"], json!([256130.0, 256200.5, 23.0]));
    assert_eq!(out["region_position"], json!([130.0, 200.5, 23.0]));
    assert!(out.get("asset_id").is_none(), "{out}");
    assert!(desk.wanted_landmarks().is_empty());
    // never arrives: timeout
    let other_asset = Uuid::from_u128(99_703);
    if let Some(it) = world.inventory.items.get_mut(&landmark) {
        it.asset_id = other_asset;
    }
    let (b, answer) = call("inventory_landmark", json!({"item_id": landmark.to_string()}), false);
    desk.take(b, &ctx(&world.inventory), now);
    desk.tick_reads(&world, now + Duration::from_secs(9));
    assert!(answer.try_recv().is_err());
    desk.tick_reads(&world, now + Duration::from_secs(10));
    assert_eq!(answered(&answer).expect_err("timeout").code, ErrorCode::Timeout);
    // unreadable
    world.landmarks.on_data(other_asset, Some(b"garbage"));
    let (c, answer) = call("inventory_landmark", json!({"item_id": landmark.to_string()}), false);
    desk.take(c, &ctx(&world.inventory), now);
    desk.tick_reads(&world, now);
    assert_eq!(answered(&answer).expect_err("failed").code, ErrorCode::Failed);
    // not a landmark, unknown
    let (d, answer) = call("inventory_landmark", json!({"item_id": TEXTURE.to_string()}), false);
    desk.take(d, &ctx(&world.inventory), now);
    assert_eq!(answered(&answer).expect_err("texture").code, ErrorCode::InvalidArguments);
    let (e, answer) = call("inventory_landmark", json!({"item_id": Uuid::from_u128(1).to_string()}), false);
    desk.take(e, &ctx(&world.inventory), now);
    assert_eq!(answered(&answer).expect_err("unknown").code, ErrorCode::NotFound);
}

/// Folderstorm: a landmark read waits in the desk like a bake, so a login
/// change refuses it too (`Desk::refuse_waiting`, #32).
#[test]
fn a_login_change_refuses_a_waiting_landmark() {
    let mut world = landmark_world();
    let landmark = Uuid::from_u128(99_720);
    world.inventory.add_items(vec![InvItem {
        id: landmark,
        parent: DEMO_FOLDER,
        name: "Plaza".into(),
        asset_type: 3,
        inv_type: 3,
        asset_id: Uuid::from_u128(99_721),
        owner: AGENT,
        owner_mask: actions::MOVE | actions::TRANSFER,
        ..Default::default()
    }]);
    let now = Instant::now();
    let mut desk = Desk::default();
    desk.set_login(1, Some("vs-11111111111111111111111111111111".to_owned()));
    let (a, answer) = call("inventory_landmark", json!({"item_id": landmark.to_string()}), false);
    desk.take(a, &ctx(&world.inventory), now);
    desk.tick_reads(&world, now);
    assert!(answer.try_recv().is_err(), "waits for the asset");
    desk.set_login(2, None);
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::SessionChanged);
    assert!(desk.wanted_landmarks().is_empty());
}

#[test]
fn rlva_showloc_hides_a_landmarks_location() {
    let mut world = landmark_world();
    let landmark = Uuid::from_u128(99_710);
    let asset = Uuid::from_u128(99_711);
    world.inventory.add_items(vec![InvItem {
        id: landmark,
        parent: DEMO_FOLDER,
        name: "Home".into(),
        asset_type: 3,
        inv_type: 3,
        asset_id: asset,
        owner: AGENT,
        ..Default::default()
    }]);
    world
        .landmarks
        .on_data(asset, Some(b"Landmark version 1\nposition 10.0 20.0 30.0\n"));
    world.rlva.configure(true);
    let source = Uuid::from_u128(4242);
    let owner = world.agent_id;
    assert!(world.rlva.receive(&aurora_net::ChatMessage {
        from_name: "RLVa test object".to_owned(),
        source_id: source,
        owner_id: owner,
        source_type: aurora_net::ChatSourceType::Object,
        chat_type: aurora_net::ChatType::OwnerSay,
        position: glam::Vec3::ZERO,
        message: "@showloc=n".to_owned(),
    }));
    assert!(world.rlva.has("showloc"));
    let now = Instant::now();
    let mut desk = Desk::default();
    let (a, answer) = call("inventory_landmark", json!({"item_id": landmark.to_string()}), false);
    desk.take(a, &ctx(&world.inventory), now);
    desk.tick_reads(&world, now);
    assert_eq!(answered(&answer).expect_err("hidden").code, ErrorCode::NotAllowed);
}

// --- changes ---

#[test]
fn create_item_where_the_new_menu_offers_it() {
    let mut inv = demo_inventory();
    let notecards = Uuid::from_u128(99_800);
    let lost = Uuid::from_u128(99_801);
    let root = inv.root;
    add_folder(&mut inv, notecards, root, "Notecards", 7, false);
    add_folder(&mut inv, lost, root, "Lost And Found", 16, false);
    // the folder of its type by default, the viewer's name
    let p = planned(&inv, "inventory_create_item", json!({"type": "notecard"}));
    let Work::Create(c) = &p.work else { panic!("a creation") };
    assert_eq!(c.parent, notecards);
    assert_eq!(c.kind, NewItem::Note);
    assert_eq!(c.asset_type(), 7);
    assert!(c.name.as_deref().is_some_and(|n| !n.is_empty()));
    assert!(p.summary.contains("Notecards"), "{}", p.summary);
    // My Inventory when the type has no folder; a name and a folder given
    let p = planned(
        &inv,
        "inventory_create_item",
        json!({"type": "script", "name": "Door", "folder_id": SUBFOLDER.to_string()}),
    );
    let Work::Create(c) = &p.work else { panic!("a creation") };
    assert_eq!((c.parent, c.kind, c.name.as_deref()), (SUBFOLDER, NewItem::Script, Some("Door")));
    assert!(p.summary.contains("Door"));
    let p = planned(&inv, "inventory_create_item", json!({"type": "day_cycle"}));
    assert!(matches!(&p.work, Work::Create(c) if c.parent == inv.root && c.kind == NewItem::Settings(2)));
    // a landmark's name comes from the parcel when it is made
    let p = planned(
        &inv,
        "inventory_create_item",
        json!({"type": "landmark", "folder_id": DEMO_FOLDER.to_string()}),
    );
    assert!(matches!(&p.work, Work::Create(c) if c.kind == NewItem::Landmark && c.name.is_none()));
    // the folders that take nothing new
    let trash = actions::system(&inv, 14).expect("trash");
    let my_outfits = actions::system(&inv, 48).expect("my outfits");
    for parent in [COF, trash, my_outfits, inv.lib_root, lost, MARKETPLACE] {
        assert_eq!(
            refused(
                &inv,
                "inventory_create_item",
                json!({"type": "notecard", "folder_id": parent.to_string()})
            ),
            ErrorCode::NotAllowed,
            "{parent}"
        );
    }
    for bad in [
        json!({"type": "shirt"}),
        json!({"type": "notecard", "name": "a|b"}),
        json!({"type": "notecard", "name": ""}),
        json!({}),
        json!({"type": "notecard", "extra": 1}),
    ] {
        assert_eq!(
            refused(&inv, "inventory_create_item", bad.clone()),
            ErrorCode::InvalidArguments,
            "{bad}"
        );
    }
    let _ = &mut inv;
}

#[test]
fn favorites_follow_the_menu() {
    let mut inv = demo_inventory();
    let p = planned(
        &inv,
        "inventory_set_favorite",
        json!({"ids": [TEXTURE.to_string()], "favorite": true}),
    );
    assert!(p.summary.contains(&inv.items[&TEXTURE].name), "{}", p.summary);
    apply(&mut inv, p);
    assert!(actions::is_favorite(&inv, TEXTURE));
    // already one: nothing to change, nothing asked
    assert!(matches!(
        planned(
            &inv,
            "inventory_set_favorite",
            json!({"ids": [TEXTURE.to_string()], "favorite": true})
        )
        .work,
        Work::Nothing
    ));
    // a link marks its original
    let p = planned(
        &inv,
        "inventory_set_favorite",
        json!({"ids": [LINK.to_string(), DEMO_FOLDER.to_string()], "favorite": true}),
    );
    apply(&mut inv, p);
    assert!(actions::is_favorite(&inv, OBJECT));
    assert!(inv.folders[&DEMO_FOLDER].info.favorite);
    let p = planned(
        &inv,
        "inventory_set_favorite",
        json!({"ids": [TEXTURE.to_string()], "favorite": false}),
    );
    apply(&mut inv, p);
    assert!(!actions::is_favorite(&inv, TEXTURE));
    // the Trash, the Library, My Inventory itself, the Marketplace
    let lib_item = Uuid::from_u128(99_900);
    inv.add_items(vec![texture(
        lib_item,
        inv.lib_root,
        inv.lib_owner,
        actions::FULL,
        actions::COPY,
        false,
    )]);
    let p = planned(&inv, "inventory_trash", json!({"ids": [SCRIPT.to_string()]}));
    apply(&mut inv, p);
    for id in [SCRIPT, lib_item, inv.root, MARKETPLACE] {
        assert_eq!(
            refused(&inv, "inventory_set_favorite", json!({"ids": [id.to_string()], "favorite": true})),
            ErrorCode::NotAllowed,
            "{id}"
        );
    }
    assert_eq!(
        refused(&inv, "inventory_set_favorite", json!({"ids": [TEXTURE.to_string()]})),
        ErrorCode::InvalidArguments
    );
}

#[test]
fn batch_rename_is_all_or_nothing() {
    let mut inv = demo_inventory();
    let p = planned(
        &inv,
        "inventory_batch_rename",
        json!({"items": [{"id": TEXTURE.to_string(), "name": "Brick"}, {"id": SUBFOLDER.to_string(), "name": "Shelf"}, {"id": SCRIPT.to_string(), "name": inv.items[&SCRIPT].name.clone()}]}),
    );
    assert_eq!(p.reply["unchanged"], json!([SCRIPT]));
    assert!(p.summary.contains("Brick"), "{}", p.summary);
    apply(&mut inv, p);
    assert_eq!(inv.items[&TEXTURE].name, "Brick");
    assert_eq!(inv.folders[&SUBFOLDER].info.name, "Shelf");
    // one that cannot be renamed refuses them all
    let before = inv.items[&OBJECT].name.clone();
    let trash = actions::system(&inv, 14).expect("trash");
    for blocked in [LINK, COF, trash, inv.lib_root, MARKETPLACE] {
        assert_eq!(
            refused(
                &inv,
                "inventory_batch_rename",
                json!({"items": [{"id": OBJECT.to_string(), "name": "Crate"}, {"id": blocked.to_string(), "name": "x"}]})
            ),
            ErrorCode::NotAllowed,
            "{blocked}"
        );
    }
    assert_eq!(inv.items[&OBJECT].name, before);
    // the same names: nothing
    assert!(matches!(
        planned(
            &inv,
            "inventory_batch_rename",
            json!({"items": [{"id": TEXTURE.to_string(), "name": "Brick"}]})
        )
        .work,
        Work::Nothing
    ));
    let twice = json!({"items": [{"id": TEXTURE.to_string(), "name": "a"}, {"id": TEXTURE.to_string(), "name": "b"}]});
    let too_many: Vec<Value> = (0..51).map(|_| json!({"id": TEXTURE.to_string(), "name": "a"})).collect();
    for bad in [
        twice,
        json!({"items": []}),
        json!({"items": too_many}),
        json!({"items": [{"id": TEXTURE.to_string()}]}),
        json!({"items": [{"id": TEXTURE.to_string(), "name": "a|b"}]}),
        json!({"items": [{"id": TEXTURE.to_string(), "name": "a", "x": 1}]}),
    ] {
        assert_eq!(
            refused(&inv, "inventory_batch_rename", bad.clone()),
            ErrorCode::InvalidArguments,
            "{bad}"
        );
    }
}

/// AGENTS.md § 2 "Tests": every owner case for the new changes. A batch
/// rename needs Modify (`actions::renameable`); a picture's texture needs
/// Copy and Transfer, or the Library (`thumbnail::usable`); favorites mark
/// any entry of My Inventory (the menu's rule), never the Library's.
#[test]
fn creators_permissions_for_the_new_changes() {
    let mut inv = demo_inventory();
    let full = actions::FULL | actions::MOVE;
    let cases = [
        // (owner, group owned, owner mask, everyone mask, may rename, may be a picture)
        (AGENT, false, full, 0, true, true),
        (AGENT, true, full, 0, false, false),
        (OTHER, false, full, 0, false, false),
        (inv.lib_owner, false, full, actions::COPY, false, true),
        (AGENT, false, full & !actions::COPY, 0, true, false),
        (AGENT, false, full & !actions::TRANSFER, 0, true, false),
        (AGENT, false, full & !actions::MODIFY, 0, false, true),
        (OTHER, false, full, actions::FULL, true, true),
    ];
    for (n, (owner, group_owned, owner_mask, everyone_mask, rename, picture)) in cases.into_iter().enumerate() {
        let id = Uuid::from_u128(99_300 + n as u128);
        let library = owner == inv.lib_owner;
        inv.add_items(vec![texture(
            id,
            if library { inv.lib_root } else { DEMO_FOLDER },
            owner,
            owner_mask,
            everyone_mask,
            group_owned,
        )]);
        let renamed = plan(
            "inventory_batch_rename",
            &json!({"items": [{"id": id.to_string(), "name": "y"}]}),
            &ctx(&inv),
        );
        assert_eq!(renamed.is_ok(), rename && !library, "rename, case {n}: {:?}", renamed.err());
        let mut logged_in = ctx(&inv);
        logged_in.session = Session::LoggedIn;
        let pictured = plan(
            "inventory_set_thumbnail",
            &json!({"id": OBJECT.to_string(), "texture_id": id.to_string()}),
            &logged_in,
        );
        assert_eq!(pictured.is_ok(), picture, "picture, case {n}: {:?}", pictured.err());
        if let Err(e) = pictured {
            assert_eq!(e.code, ErrorCode::NotAllowed, "case {n}");
        }
        let favorite = plan(
            "inventory_set_favorite",
            &json!({"ids": [id.to_string()], "favorite": true}),
            &ctx(&inv),
        );
        assert_eq!(favorite.is_ok(), !library, "favorite, case {n}");
    }
}

#[test]
fn set_thumbnail_through_the_image_windows_rules() {
    let mut inv = demo_inventory();
    // a texture of full permissions on an object: the Image window's path
    let p = planned(
        &inv,
        "inventory_set_thumbnail",
        json!({"id": OBJECT.to_string(), "texture_id": TEXTURE.to_string()}),
    );
    assert!(matches!(p.work, Work::Image { target, texture } if target == OBJECT && texture == TEXTURE));
    assert!(p.summary.contains(&inv.items[&TEXTURE].name), "{}", p.summary);
    // a link to it is followed; a folder takes a picture too
    let texture_link = Uuid::from_u128(99_950);
    inv.add_items(vec![InvItem {
        id: texture_link,
        parent: SUBFOLDER,
        name: "link".into(),
        asset_type: 24,
        asset_id: TEXTURE,
        owner: AGENT,
        ..Default::default()
    }]);
    let p = planned(
        &inv,
        "inventory_set_thumbnail",
        json!({"id": DEMO_FOLDER.to_string(), "texture_id": texture_link.to_string()}),
    );
    assert!(matches!(p.work, Work::Image { texture, .. } if texture == TEXTURE));
    // a texture in the Trash, an object that is no texture
    let binned = Uuid::from_u128(99_951);
    let trash = actions::system(&inv, 14).expect("trash");
    inv.add_items(vec![texture(binned, trash, AGENT, actions::FULL, 0, false)]);
    for bad in [binned, OBJECT] {
        assert_eq!(
            refused(
                &inv,
                "inventory_set_thumbnail",
                json!({"id": SCRIPT.to_string(), "texture_id": bad.to_string()})
            ),
            ErrorCode::NotAllowed,
            "{bad}"
        );
    }
    // an asset id: only one of a texture the viewer allows (any in the demo)
    let mut cx = ctx(&inv);
    cx.session = Session::LoggedIn;
    let stranger = Uuid::from_u128(424_242);
    let asset = inv.items[&TEXTURE].asset_id;
    let by_asset = |cx: &Ctx, id: Uuid| {
        plan(
            "inventory_set_thumbnail",
            &json!({"id": OBJECT.to_string(), "texture_id": id.to_string()}),
            cx,
        )
    };
    assert_eq!(by_asset(&cx, stranger).expect_err("unknown").code, ErrorCode::NotAllowed);
    assert!(by_asset(&cx, asset).is_ok());
    assert!(
        by_asset(&ctx(&inv), stranger).is_ok(),
        "the demo accepts any id, as its window does"
    );
    // what keeps its picture: links, the Current Outfit, the Library, the
    // Marketplace, an item without Modify
    let no_modify = Uuid::from_u128(99_952);
    inv.add_items(vec![texture(
        no_modify,
        DEMO_FOLDER,
        AGENT,
        actions::FULL & !actions::MODIFY,
        0,
        false,
    )]);
    for target in [LINK, COF, inv.lib_root, MARKETPLACE, no_modify] {
        assert_eq!(
            refused(
                &inv,
                "inventory_set_thumbnail",
                json!({"id": target.to_string(), "texture_id": TEXTURE.to_string()})
            ),
            ErrorCode::NotAllowed,
            "{target}"
        );
    }
    // Remove image
    assert!(matches!(
        planned(&inv, "inventory_set_thumbnail", json!({"id": OBJECT.to_string(), "clear": true})).work,
        Work::Nothing
    ));
    if let Some(it) = inv.items.get_mut(&OBJECT) {
        it.thumbnail = Uuid::from_u128(5);
    }
    let p = planned(&inv, "inventory_set_thumbnail", json!({"id": OBJECT.to_string(), "clear": true}));
    assert!(matches!(&p.work, Work::Inventory(_)));
    apply(&mut inv, p);
    assert!(inv.items[&OBJECT].thumbnail.is_nil());
    for bad in [
        json!({"id": OBJECT.to_string()}),
        json!({"id": OBJECT.to_string(), "texture_id": TEXTURE.to_string(), "clear": true}),
        json!({"texture_id": TEXTURE.to_string()}),
        json!({"id": OBJECT.to_string(), "clear": "yes"}),
    ] {
        assert_eq!(
            refused(&inv, "inventory_set_thumbnail", bad.clone()),
            ErrorCode::InvalidArguments,
            "{bad}"
        );
    }
}

// --- the desk ---

fn call(tool: &str, args: Value, ask: bool) -> (Pending, Receiver<Result<Value, ToolError>>) {
    Pending::detached(tool, args, ask, Some("Claude Code"))
}

fn answered(answer: &Receiver<Result<Value, ToolError>>) -> Result<Value, ToolError> {
    answer.try_recv().expect("answered")
}

const IDLE_BAKE: Bake = Bake {
    outfit_change_pending: false,
    cof_version: 3,
    appearance_cof_version: 3,
    server_bakes: true,
};

#[test]
fn the_new_changes_ask_first_and_reads_never_do() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = Desk::default();
    let (a, answer) = call(
        "inventory_set_favorite",
        json!({"ids": [TEXTURE.to_string()], "favorite": true}),
        true,
    );
    desk.take(a, &ctx(&inv), now);
    let q = desk.question(now).expect("asked");
    assert!(q.summary.contains(&inv.items[&TEXTURE].name), "{}", q.summary);
    desk.decide(q.number, true);
    let runs = desk.tick(&IDLE_BAKE, now);
    assert_eq!(runs.len(), 1);
    assert!(answer.try_recv().is_err(), "answered once done");
    // refused by the rules before any question
    let (b, answer) = call(
        "inventory_batch_rename",
        json!({"items": [{"id": LINK.to_string(), "name": "x"}]}),
        true,
    );
    desk.take(b, &ctx(&inv), now);
    assert!(desk.question(now).is_none());
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::NotAllowed);
    // reads answer at once
    for (tool, args) in [
        ("inventory_types", json!({})),
        ("inventory_protected_folders", json!({})),
        ("inventory_get_many", json!({"ids": [TEXTURE.to_string()]})),
        ("appearance_outfit_items", json!({"outfit_id": NEUROLAB.to_string()})),
    ] {
        let (c, answer) = call(tool, args, false);
        desk.take(c, &ctx(&inv), now);
        assert!(answered(&answer).is_ok(), "{tool}");
    }
}

#[test]
fn a_creation_answers_when_its_item_arrives() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = Desk::default();
    let args = json!({"type": "script", "name": "Door", "folder_id": SUBFOLDER.to_string()});
    let p = planned(&inv, "inventory_create_item", args.clone());
    let arrived = |id: u128, parent: Uuid, asset_type: i32, name: &str| InvItem {
        id: Uuid::from_u128(id),
        parent,
        name: name.into(),
        asset_type,
        ..Default::default()
    };
    // the grid: sent, then the item arrives; other items are not this one
    let (a, answer) = call("inventory_create_item", args.clone(), false);
    desk.begin(a, &p, false, now);
    desk.sent(None, None);
    assert!(!desk.created(&arrived(1, DEMO_FOLDER, 10, "Door")), "another folder");
    assert!(!desk.created(&arrived(2, SUBFOLDER, 7, "Door")), "another type");
    assert!(!desk.created(&arrived(3, SUBFOLDER, 10, "Window")), "another name");
    // an inventory answer is not the creation's
    desk.observe((Uuid::from_u128(9), false, Outcome::Done(HashMap::new())));
    assert!(answer.try_recv().is_err());
    assert!(desk.created(&arrived(4, SUBFOLDER, 10, "Door")), "the assistant's: no rename box");
    let reply = answered(&answer).expect("created");
    assert_eq!(reply["item_id"], json!(Uuid::from_u128(4)));
    assert_eq!(reply["done"], true);
    // the demo: the item arrives inside the send
    let (b, answer) = call("inventory_create_item", args.clone(), false);
    desk.begin(b, &p, false, now);
    assert!(desk.created(&arrived(5, SUBFOLDER, 10, "Door")));
    desk.sent(None, None);
    assert_eq!(answered(&answer).expect("created")["item_id"], json!(Uuid::from_u128(5)));
    // never arrives
    let (c, answer) = call("inventory_create_item", args, false);
    desk.begin(c, &p, false, now);
    desk.sent(None, None);
    desk.tick(&IDLE_BAKE, now + Duration::from_secs(30));
    assert_eq!(answered(&answer).expect_err("timeout").code, ErrorCode::Timeout);
}

#[test]
fn a_picture_ends_with_the_server_or_the_image_paths_error() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = Desk::default();
    let args = json!({"id": OBJECT.to_string(), "texture_id": TEXTURE.to_string()});
    let p = planned(&inv, "inventory_set_thumbnail", args.clone());
    let request = Uuid::from_u128(31);
    let (a, answer) = call("inventory_set_thumbnail", args.clone(), false);
    desk.begin(a, &p, false, now);
    desk.sent(Some(request), None);
    assert!(desk.image_job(request));
    assert!(!desk.image_job(Uuid::from_u128(32)));
    desk.observe((request, false, Outcome::Done(HashMap::new())));
    assert_eq!(answered(&answer).expect("set")["done"], true);
    assert!(!desk.image_job(request));
    // refused by the image path (a texture too small, a lost fetch…)
    let (b, answer) = call("inventory_set_thumbnail", args, false);
    desk.begin(b, &p, false, now);
    desk.sent(Some(request), None);
    desk.image_failed(request, "the image must be square");
    let e = answered(&answer).expect_err("failed");
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(e.message.contains("square"), "{}", e.message);
}

/// The bridge's classes: Read only refuses the new changes and lets the new
/// reads through; Ask before changes asks for them.
#[test]
fn the_bridge_classes_of_the_new_tools() {
    use folderstorm_mcp_bridge::server::Config;
    use folderstorm_mcp_bridge::{Bridge, Preset};
    for (preset, change_asks) in [(Preset::ReadOnly, None), (Preset::AskBeforeChanges, Some(true))] {
        let dir = tempfile::tempdir().expect("temp dir");
        let (bridge, calls) = Bridge::start(Config {
            discovery_dir: Some(dir.path().to_path_buf()),
            pid: std::process::id(),
            version: "test".into(),
            permissions: preset.permissions(),
            viewer_timeout: Duration::from_secs(10),
        })
        .expect("bridge started");
        let folder = dir.path().to_path_buf();
        let assistant = std::thread::spawn(move || {
            let mut viewer = folderstorm_mcp::connect::Viewer::new(Some(folder), None);
            let read = viewer.call("inventory_types", json!({}));
            let change = viewer.call("inventory_set_favorite", json!({"ids": [TEXTURE.to_string()], "favorite": true}));
            (read, change)
        });
        let inv = demo_inventory();
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !assistant.is_finished() && Instant::now() < deadline {
            while let Ok(pending) = calls.try_recv() {
                seen.push((pending.tool.clone(), pending.ask));
                let result = if pending.tool == "inventory_types" {
                    answer(&pending.tool, &pending.arguments, &ctx(&inv))
                } else {
                    Ok(json!({"done": true}))
                };
                pending.answer(result);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let (read, change) = assistant.join().expect("assistant");
        assert!(read.is_ok(), "{preset:?}: {read:?}");
        match change_asks {
            None => {
                assert_eq!(change.expect_err("refused").code, ErrorCode::PermissionDenied);
                assert_eq!(
                    seen,
                    vec![("inventory_types".to_owned(), false)],
                    "the refusal never reached the viewer"
                );
            }
            Some(ask) => {
                assert!(change.is_ok());
                assert_eq!(
                    seen,
                    vec![("inventory_types".to_owned(), false), ("inventory_set_favorite".to_owned(), ask)]
                );
            }
        }
        bridge.stop();
    }
}
