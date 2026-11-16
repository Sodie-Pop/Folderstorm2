//! Folderstorm: content editing (folderstorm/plans/mcp-content.md): the
//! creators' permissions matrix of reads and saves, snapshots and the stale
//! check, the text limits, embedded items kept byte for byte, the compile
//! result reported apart from the source save, RLVa, receipts and the
//! required `client_request_id`, the dialog, open editors, and end to end
//! over a real socket.

use super::change_tests::{ctx, demo_inventory};
use super::changes::{self, Work};
use super::content::{self, Saved, demo};
use super::desk::{Bake, Desk, Outcome};
use crate::world::World;
use crate::world::inventory::{Inventory, actions};
use aurora_net::inventory::InvItem;
use aurora_net::inventory::notecard::{FIRST_EMBEDDED_CHAR, Notecard};
use folderstorm_mcp_bridge::protocol::content::{GET, PREVIEW, SAVE_SCRIPT, STATUS, TEXT_MAX, UPDATE_NOTECARD};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{Class, ErrorCode, Level, Permissions, Preset, ToolError};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use uuid::Uuid;

const AGENT: Uuid = Uuid::from_u128(2);
const S1: &str = "vs-11111111111111111111111111111111";
const IDLE: Bake = Bake {
    outfit_change_pending: false,
    cof_version: 3,
    appearance_cof_version: 3,
    server_bakes: true,
};

/// The demo inventory with the content fixtures, and what the demo serves.
struct Fixture {
    world: World,
    documents: HashMap<Uuid, Vec<u8>>,
    /// The scripts' sources as seeded, by item.
    sources: HashMap<Uuid, String>,
    /// The sources saved since, by asset: the simulator sends the source
    /// of the asset `LoadScript` names (the item's asset in the inventory).
    saved_sources: HashMap<Uuid, String>,
}

fn fixture() -> Fixture {
    let mut world = World::new(std::sync::Arc::new(crate::scene::avatar::AvatarLibrary::load()));
    world.inventory = demo_inventory();
    let (documents, sources) = demo::seed(&mut world.inventory, AGENT);
    Fixture {
        world,
        documents: documents.into_iter().collect(),
        sources: sources.into_iter().collect(),
        saved_sources: HashMap::new(),
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
    desk.set_login(1, Some(S1.to_owned()));
    desk
}

fn call(tool: &str, args: Value, ask: bool) -> (Pending, Receiver<Result<Value, ToolError>>) {
    Pending::detached(tool, args, ask, Some("Claude Code"))
}

fn answered(answer: &Receiver<Result<Value, ToolError>>) -> Result<Value, ToolError> {
    answer.try_recv().expect("answered")
}

/// Serve what the desk's reads want, as the app and the server would.
fn serve(desk: &mut Desk, f: &Fixture) {
    for fetch in desk.wanted_content() {
        let item = &f.world.inventory.items[&fetch.item];
        match fetch.kind {
            content::Kind::Notecard => {
                let bytes = f.documents.get(&fetch.item).cloned().unwrap_or_default();
                desk.content().notecard_loaded(item.id, item.asset_id, &Ok(bytes));
            }
            content::Kind::Script => {
                let source = f
                    .saved_sources
                    .get(&item.asset_id)
                    .or_else(|| f.sources.get(&fetch.item))
                    .cloned()
                    .unwrap_or_default();
                desk.content().script_loaded(fetch.request, &Ok(source));
            }
        }
    }
}

fn read(desk: &mut Desk, f: &Fixture, item: Uuid) -> Result<Value, ToolError> {
    let now = Instant::now();
    let (c, answer) = call(GET, json!({"item_id": item.to_string()}), false);
    desk.take(c, &ctx(&f.world.inventory), now);
    if let Ok(result) = answer.try_recv() {
        return result;
    }
    serve(desk, f);
    desk.tick_reads(&f.world, now);
    answered(&answer)
}

fn snapshot(desk: &mut Desk, f: &Fixture, item: Uuid) -> (String, String) {
    let r = read(desk, f, item).expect("read");
    (
        r["snapshot_hash"].as_str().expect("a snapshot").to_owned(),
        r["text"].as_str().expect("text").to_owned(),
    )
}

/// Take a save and run what the desk lets run, answered by `server`.
fn save(
    desk: &mut Desk,
    f: &mut Fixture,
    tool: &str,
    args: Value,
    server: impl Fn(&content::Save) -> Result<Saved, String>,
) -> Result<Value, ToolError> {
    let now = Instant::now();
    let (c, answer) = call(tool, args, false);
    desk.take(c, &ctx(&f.world.inventory), now);
    if let Ok(result) = answer.try_recv() {
        return result;
    }
    for run in desk.tick(&IDLE, now) {
        let planned = match changes::plan(&run.tool, &run.arguments, &ctx(&f.world.inventory)) {
            Ok(p) => p,
            Err(e) => {
                run.answer(Err(e));
                continue;
            }
        };
        let Work::Content(work) = planned.work.clone() else {
            run.answer(Ok(planned.reply));
            continue;
        };
        desk.begin(run, &planned, false, now);
        let request = Uuid::new_v4();
        desk.sent(Some(request), None);
        let result = server(&work);
        if let Ok(saved) = &result {
            match work.kind {
                // the app files the item the notecard's save read back
                content::Kind::Notecard => {
                    if let Some(it) = f.world.inventory.items.get_mut(&work.item) {
                        it.asset_id = saved.asset;
                    }
                    f.documents.insert(work.item, work.data.clone());
                }
                // a script's item gets its new asset from the bridge (below)
                content::Kind::Script => {
                    f.saved_sources
                        .insert(saved.asset, String::from_utf8(work.data.clone()).expect("utf-8"));
                }
            }
        }
        desk.observe((request, false, Outcome::Content(result)));
    }
    // the app's frame (`local_assistant_frame`), before the next calls
    desk.content().hold(&mut f.world.inventory);
    answered(&answer)
}

fn saved_ok(work: &content::Save) -> Result<Saved, String> {
    Ok(Saved {
        item: work.item,
        asset: Uuid::new_v4(),
        compiled: (work.kind == content::Kind::Script).then_some(true),
        checked: work.kind == content::Kind::Script,
        errors: Vec::new(),
    })
}

fn edit(item: Uuid, hash: &str, text: &str, request: &str) -> Value {
    json!({"item_id": item.to_string(), "snapshot_hash": hash, "text": text, "client_request_id": request})
}

// --- creators' permissions ---

fn item(id: u128, asset_type: i32, owner_mask: u32) -> InvItem {
    InvItem {
        id: Uuid::from_u128(id),
        parent: demo::FOLDER,
        name: format!("item {id}"),
        asset_type,
        inv_type: asset_type,
        asset_id: Uuid::from_u128(id + 50_000),
        owner: AGENT,
        creator: Uuid::from_u128(77),
        base_mask: 0x7fff_ffff,
        owner_mask,
        ..Default::default()
    }
}

/// Owner, group-owned, someone else's, Library, no copy, no modify, full,
/// an SLua script, Trash, Marketplace: who reads, who saves.
#[test]
fn the_permissions_matrix_follows_the_viewer_s_editors() {
    let mut f = fixture();
    let inv: &mut Inventory = &mut f.world.inventory;
    let full = 0x7fff_ffff;
    let copy_only = actions::COPY | actions::TRANSFER | actions::MOVE;
    let mut rows = vec![
        // (item, readable, savable)
        (item(9100, 7, full), true, true),
        (item(9101, 7, full & !actions::COPY), false, false),
        (item(9102, 7, full & !actions::MODIFY), true, false),
        (item(9103, 10, full), true, true),
        (item(9104, 10, full & !actions::MODIFY), false, false),
        (item(9105, 10, full & !actions::COPY), false, false),
    ];
    // group-owned: the everyone mask applies, and it is not the agent's
    let mut group = item(9106, 7, full);
    group.group_owned = true;
    group.everyone_mask = actions::COPY;
    rows.push((group, true, false));
    // someone else's, readable and modifiable by everyone
    let mut other = item(9107, 7, full);
    other.owner = Uuid::from_u128(66);
    other.everyone_mask = actions::COPY | actions::MODIFY;
    rows.push((other, true, false));
    // Library: a script with Copy alone is readable; neither is ever saved
    let mut lib_note = item(9108, 7, copy_only);
    lib_note.parent = inv.lib_root;
    lib_note.owner = inv.lib_owner;
    lib_note.everyone_mask = copy_only;
    let mut lib_script = item(9109, 10, copy_only);
    lib_script.parent = inv.lib_root;
    lib_script.owner = inv.lib_owner;
    lib_script.everyone_mask = copy_only;
    rows.push((lib_note, true, false));
    rows.push((lib_script, true, false));
    // an SLua script: readable, not saved as LSL
    let mut lua = item(9110, 10, full);
    lua.flags = 1;
    rows.push((lua, true, false));
    inv.add_items(rows.iter().map(|(it, _, _)| it.clone()).collect());
    for (it, read, save) in &rows {
        let it = &inv.items[&it.id];
        let kind = if it.asset_type == 7 {
            content::Kind::Notecard
        } else {
            content::Kind::Script
        };
        assert_eq!(content::readable(inv, it, kind).is_ok(), *read, "read {}", it.name);
        assert_eq!(content::unsavable(inv, it, kind).is_none(), *save, "save {}", it.name);
    }
    // Trash and Marketplace: stricter than the editors
    let trash = Uuid::from_u128(9190);
    inv.folders.insert(
        trash,
        crate::world::inventory::Folder {
            info: aurora_net::inventory::InvFolder {
                id: trash,
                parent: inv.root,
                name: "Trash".into(),
                type_default: 14,
                ..Default::default()
            },
            children: vec![],
            items: vec![],
            state: crate::world::inventory::FetchState::Fetched,
            library: false,
        },
    );
    let mut trashed = item(9111, 7, full);
    trashed.parent = trash;
    let mut listed = item(9112, 7, full);
    listed.parent = Uuid::from_u128(8002);
    inv.add_items(vec![trashed.clone(), listed.clone()]);
    for it in [&trashed, &listed] {
        let why = content::unsavable(inv, &inv.items[&it.id], content::Kind::Notecard);
        assert!(why.is_some(), "{}", it.name);
    }
    // the reads through the desk: refused with not_allowed, never a text
    let mut desk = desk_with(allow_all());
    for id in [9101u128, 9104, 9105] {
        let e = read(&mut desk, &f, Uuid::from_u128(id)).expect_err("refused");
        assert_eq!(e.code, ErrorCode::NotAllowed, "{id}");
    }
    // a readable item that cannot be saved gives no snapshot
    f.documents
        .insert(Uuid::from_u128(9102), Notecard::default().encode("text\n").expect("encoded"));
    let r = read(&mut desk, &f, Uuid::from_u128(9102)).expect("read");
    assert_eq!(r["editable"], false);
    assert!(r["snapshot_hash"].is_null());
    assert!(r["not_editable"].as_str().is_some_and(|w| w.contains("Modify")), "{r}");
}

#[test]
fn a_save_needs_its_classes_and_ownership() {
    let mut f = fixture();
    let mut desk = desk_with(allow_all());
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES);
    let longer = format!("{text}x\n");
    // Content on Never: refused before anything
    let mut p = allow_all();
    p.set(Class::Content, Level::Never);
    desk.set_permissions(p);
    let e = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES, &hash, &longer, "r1"),
        saved_ok,
    )
    .expect_err("refused");
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    // Read on Never refuses it too (Content and Read)
    let mut p = allow_all();
    p.set(Class::Read, Level::Never);
    desk.set_permissions(p);
    let e = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES, &hash, &longer, "r2"),
        saved_ok,
    )
    .expect_err("refused");
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    // a no-modify notecard: not_allowed
    desk.set_permissions(allow_all());
    let e = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES_NO_MODIFY, &hash, "x", "r3"),
        saved_ok,
    )
    .expect_err("refused");
    assert_eq!(e.code, ErrorCode::NotAllowed);
    // the wrong tool for the kind
    let e = save(&mut desk, &mut f, SAVE_SCRIPT, edit(demo::NOTES, &hash, "x", "r4"), saved_ok).expect_err("refused");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
}

// --- reads ---

#[test]
fn a_read_gives_the_text_a_snapshot_and_the_embedded_items_never_an_asset_id() {
    let f = fixture();
    let mut desk = desk_with(allow_all());
    let r = read(&mut desk, &f, demo::NOTES_ITEMS).expect("read");
    assert_eq!(r["kind"], "notecard");
    assert_eq!(r["editable"], true, "{r}");
    assert_eq!(r["complete"], true);
    assert!(r["note"].as_str().is_some_and(|n| n.contains("data")));
    let embedded = r["embedded"].as_array().expect("embedded");
    assert_eq!(embedded.len(), 2);
    assert_eq!(embedded[0]["marker"], "U+100000");
    assert_eq!(embedded[1]["type"], "landmark");
    let text = r["text"].as_str().expect("text");
    assert_eq!(text.chars().filter(|c| *c as u32 >= FIRST_EMBEDDED_CHAR).count(), 2);
    let hash = r["snapshot_hash"].as_str().expect("hash");
    assert_eq!(hash.len(), 64);
    // no asset id: neither the item's nor an embedded item's; no embedded item id
    let shown = r.to_string();
    let it = &f.world.inventory.items[&demo::NOTES_ITEMS];
    for id in [
        it.asset_id,
        Uuid::from_u128(18_000 + 8104),
        Uuid::from_u128(18_000 + 8120),
        Uuid::from_u128(8104),
    ] {
        assert!(!shown.contains(&id.to_string()), "{id} in {shown}");
    }
    // the same read twice gives the same hash; a script reads too
    let again = read(&mut desk, &f, demo::NOTES_ITEMS).expect("read");
    assert_eq!(again["snapshot_hash"], r["snapshot_hash"]);
    let s = read(&mut desk, &f, demo::SCRIPT).expect("read");
    assert_eq!(s["kind"], "script");
    assert_eq!(s["preprocessed"], false);
    assert!(s["text"].as_str().is_some_and(|t| t.contains("Hello, Avatar!")));
}

#[test]
fn a_link_is_followed_and_a_read_waits_then_times_out() {
    let mut f = fixture();
    let link = Uuid::from_u128(9200);
    f.world.inventory.add_items(vec![InvItem {
        id: link,
        parent: demo::FOLDER,
        name: "link".into(),
        asset_type: 24,
        inv_type: 7,
        asset_id: demo::NOTES,
        owner: AGENT,
        ..Default::default()
    }]);
    let mut desk = desk_with(allow_all());
    let r = read(&mut desk, &f, link).expect("read");
    assert_eq!(r["item_id"], json!(demo::NOTES));
    // nothing comes: timeout after 45 s
    let now = Instant::now();
    let (c, answer) = call(GET, json!({"item_id": demo::SCRIPT.to_string()}), false);
    desk.take(c, &ctx(&f.world.inventory), now);
    assert_eq!(desk.wanted_content().len(), 1);
    assert!(desk.wanted_content().is_empty(), "asked once");
    desk.tick_reads(&f.world, now + Duration::from_secs(44));
    assert!(answer.try_recv().is_err());
    desk.tick_reads(&f.world, now + Duration::from_secs(45));
    assert_eq!(answered(&answer).expect_err("late").code, ErrorCode::Timeout);
    // a fetch that could not be sent
    let (c, answer) = call(GET, json!({"item_id": demo::NOTES.to_string()}), false);
    desk.take(c, &ctx(&f.world.inventory), now);
    let fetch = desk.wanted_content()[0];
    desk.content().fetch_failed(fetch.request, "lost".into());
    desk.tick_reads(&f.world, now);
    assert_eq!(answered(&answer).expect_err("failed").code, ErrorCode::Failed);
}

#[test]
fn rights_lowered_or_read_refused_while_the_text_comes_give_nothing() {
    let mut f = fixture();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let (c, answer) = call(GET, json!({"item_id": demo::NOTES.to_string()}), false);
    desk.take(c, &ctx(&f.world.inventory), now);
    serve(&mut desk, &f);
    if let Some(it) = f.world.inventory.items.get_mut(&demo::NOTES) {
        it.owner_mask &= !actions::COPY;
    }
    desk.tick_reads(&f.world, now);
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::NotAllowed);
    // Read set to Never while it waits
    let (c, answer) = call(GET, json!({"item_id": demo::SCRIPT.to_string()}), false);
    desk.take(c, &ctx(&f.world.inventory), now);
    let mut p = allow_all();
    p.set(Class::Read, Level::Never);
    desk.set_permissions(p);
    desk.tick_reads(&f.world, now);
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::PermissionDenied);
    // a login change refuses a read still waiting
    desk.set_permissions(allow_all());
    let (c, answer) = call(GET, json!({"item_id": demo::SCRIPT.to_string()}), false);
    desk.take(c, &ctx(&f.world.inventory), now);
    desk.set_login(2, None);
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::SessionChanged);
}

#[test]
fn a_script_longer_than_the_limit_is_read_incomplete_and_not_editable() {
    let mut f = fixture();
    let long = format!("default {{}}\n// {}\n", "é".repeat(TEXT_MAX));
    f.sources.insert(demo::SCRIPT, long.clone());
    let mut desk = desk_with(allow_all());
    let r = read(&mut desk, &f, demo::SCRIPT).expect("read");
    assert_eq!(r["complete"], false);
    assert_eq!(r["editable"], false);
    assert!(r["snapshot_hash"].is_null());
    assert_eq!(r["bytes"], long.len());
    assert!(r["text"].as_str().expect("text").len() <= TEXT_MAX);
}

#[test]
fn old_or_lossy_notecards_with_embedded_items_are_not_editable() {
    let mut f = fixture();
    // version 1: embedded items as bytes from 0x80
    let v2 = String::from_utf8(f.documents[&demo::NOTES_ITEMS].clone()).expect("utf-8");
    let records = &v2[v2.find("LLEmbeddedItems").expect("records")..v2.find("Text length").expect("text")];
    let body: &[u8] = &[b'a', b' ', 0x80, b' ', b'b', b'\n'];
    let mut v1 = format!("Linden text version 1\n{{\n{records}Text length {}\n", body.len()).into_bytes();
    v1.extend_from_slice(body);
    v1.extend_from_slice(b"}\n");
    f.documents.insert(demo::NOTES_ITEMS, v1);
    let mut desk = desk_with(allow_all());
    let r = read(&mut desk, &f, demo::NOTES_ITEMS).expect("read");
    assert_eq!(r["editable"], false, "{r}");
    assert!(r["not_editable"].as_str().is_some_and(|w| w.contains("version 1")), "{r}");
    // version 2, holding a record its text does not show
    let mut card = Notecard::default();
    let mut text = String::from("one\n");
    let embedded = InvItem {
        id: Uuid::from_u128(8104),
        name: "Texture".into(),
        asset_type: 0,
        inv_type: 0,
        next_owner_mask: 0xe000,
        base_mask: 0x7fff_ffff,
        owner_mask: 0x7fff_ffff,
        ..Default::default()
    };
    card.insert_attachment(&mut text, 0, &embedded).expect("embedded");
    card.insert_attachment(&mut text, 0, &embedded).expect("embedded");
    let encoded = String::from_utf8(card.encode(&text).expect("encoded")).expect("utf-8");
    let marker = char::from_u32(FIRST_EMBEDDED_CHAR + 1).expect("marker").to_string();
    let orphan = encoded.replacen(&marker, "", 1).replace(
        &format!("Text length {}", text.len()),
        &format!("Text length {}", text.len() - marker.len()),
    );
    f.documents.insert(demo::NOTES_ITEMS, orphan.into_bytes());
    let r = read(&mut desk, &f, demo::NOTES_ITEMS).expect("read");
    assert_eq!(r["editable"], false, "{r}");
    assert!(r["not_editable"].as_str().is_some_and(|w| w.contains("does not show")), "{r}");
}

// --- preview ---

#[test]
fn a_preview_counts_the_lines_and_lists_what_a_save_would_refuse() {
    let f = fixture();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES_ITEMS);
    let now = Instant::now();
    let preview = |desk: &mut Desk, cx: &super::tools::Ctx, hash: &str, new: &str| {
        let (c, answer) = call(
            PREVIEW,
            json!({"item_id": demo::NOTES_ITEMS.to_string(), "snapshot_hash": hash, "text": new}),
            false,
        );
        desk.take(c, cx, now);
        answered(&answer)
    };
    let cx = ctx(&f.world.inventory);
    let p = preview(&mut desk, &cx, &hash, &format!("{text}Added\n")).expect("preview");
    assert_eq!((p["lines_added"].as_u64(), p["lines_removed"].as_u64()), (Some(1), Some(0)));
    assert_eq!(p["save"], "ask");
    assert_eq!(p["problems"], json!([]));
    assert!(p["diff"].as_str().is_some_and(|d| d.contains("+Added")), "{p}");
    // a marker removed: the save would refuse it
    let without: String = text.chars().filter(|c| (*c as u32) < FIRST_EMBEDDED_CHAR).collect();
    let p = preview(&mut desk, &cx, &hash, &without).expect("preview");
    assert!(p["problems"][0].as_str().is_some_and(|m| m.contains("embedded")), "{p}");
    // the notecard open in the viewer
    let mut cx = ctx(&f.world.inventory);
    cx.content.open.insert(demo::NOTES_ITEMS);
    let p = preview(&mut desk, &cx, &hash, &text).expect("preview");
    assert_eq!(p["editor_open"], true);
    assert_eq!(p["unchanged"], true);
    // an unknown snapshot: stale
    let e = preview(&mut desk, &cx, &"0".repeat(64), &text).expect_err("stale");
    assert_eq!(e.code, ErrorCode::Stale);
}

// --- saves ---

#[test]
fn a_notecard_save_keeps_its_embedded_items_byte_for_byte() {
    let mut f = fixture();
    let original = String::from_utf8(f.documents[&demo::NOTES_ITEMS].clone()).expect("utf-8");
    let mut desk = desk_with(allow_all());
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES_ITEMS);
    let new = text
        .replacen("plaza", "harbour", 1)
        .replacen("place", "port", 1)
        .replacen("広場", "港", 1);
    assert_ne!(new, text);
    let r = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES_ITEMS, &hash, &new, "n1"),
        saved_ok,
    )
    .expect("saved");
    assert_eq!(r["source_saved"], true);
    assert_eq!(r["embedded_kept"], 2);
    assert_eq!(r["state"], "done");
    assert!(r["snapshot_hash"].is_null(), "embedded records may be rewritten: read again");
    let saved = String::from_utf8(f.documents[&demo::NOTES_ITEMS].clone()).expect("utf-8");
    let records = |s: &str| s[s.find("LLEmbeddedItems").expect("records")..s.find("Text length").expect("text")].to_owned();
    assert_eq!(records(&saved), records(&original), "every record written back as it was");
    assert_eq!(Notecard::decode(saved.as_bytes()).expect("card").text, new);
    // a marker removed, or two swapped: refused, nothing saved
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES_ITEMS);
    let without: String = text.chars().filter(|c| (*c as u32) < FIRST_EMBEDDED_CHAR).collect();
    let e = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES_ITEMS, &hash, &without, "n2"),
        saved_ok,
    )
    .expect_err("refused");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
    let a = char::from_u32(FIRST_EMBEDDED_CHAR).expect("a");
    let b = char::from_u32(FIRST_EMBEDDED_CHAR + 1).expect("b");
    let swapped: String = text
        .chars()
        .map(|c| {
            if c == a {
                b
            } else if c == b {
                a
            } else {
                c
            }
        })
        .collect();
    let e = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES_ITEMS, &hash, &swapped, "n3"),
        saved_ok,
    )
    .expect_err("refused");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
    assert_eq!(f.documents[&demo::NOTES_ITEMS], saved.into_bytes(), "nothing more saved");
}

#[test]
fn a_save_is_stale_when_the_item_changed_since_its_read() {
    let mut f = fixture();
    let mut desk = desk_with(allow_all());
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES);
    // saved elsewhere before the save is taken
    if let Some(it) = f.world.inventory.items.get_mut(&demo::NOTES) {
        it.asset_id = Uuid::from_u128(424_242);
    }
    let e = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES, &hash, &format!("{text}x\n"), "s1"),
        saved_ok,
    )
    .expect_err("stale");
    assert_eq!(e.code, ErrorCode::Stale);
    // a hash nobody gave
    let e = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES, &"a".repeat(64), "x", "s2"),
        saved_ok,
    )
    .expect_err("stale");
    assert_eq!(e.code, ErrorCode::Stale);
    // saved elsewhere after the take, before the upload: the plan just
    // before the send says stale
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES);
    let now = Instant::now();
    let (c, answer) = call(UPDATE_NOTECARD, edit(demo::NOTES, &hash, &format!("{text}y\n"), "s3"), false);
    desk.take(c, &ctx(&f.world.inventory), now);
    let runs = desk.tick(&IDLE, now);
    assert_eq!(runs.len(), 1);
    if let Some(it) = f.world.inventory.items.get_mut(&demo::NOTES) {
        it.asset_id = Uuid::from_u128(434_343);
    }
    for run in runs {
        let e = changes::plan(&run.tool, &run.arguments, &ctx(&f.world.inventory)).expect_err("stale");
        assert_eq!(e.code, ErrorCode::Stale);
        run.answer(Err(e));
    }
    assert_eq!(answered(&answer).expect_err("stale").code, ErrorCode::Stale);
    // a login change forgets every snapshot
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES);
    desk.set_login(2, Some("vs-22222222222222222222222222222222".into()));
    let e = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES, &hash, &format!("{text}z\n"), "s4"),
        saved_ok,
    )
    .expect_err("stale");
    assert_eq!(e.code, ErrorCode::Stale);
}

#[test]
fn the_text_limits() {
    let mut f = fixture();
    let mut desk = desk_with(allow_all());
    let (hash, _) = snapshot(&mut desk, &f, demo::NOTES);
    let e = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES, &hash, &"a".repeat(TEXT_MAX + 1), "l1"),
        saved_ok,
    )
    .expect_err("long");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
    let e = save(&mut desk, &mut f, UPDATE_NOTECARD, edit(demo::NOTES, &hash, "a\0b", "l2"), saved_ok).expect_err("nul");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
    // exactly the limit is fine
    let r = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES, &hash, &"a".repeat(TEXT_MAX), "l3"),
        saved_ok,
    )
    .expect("saved");
    assert_eq!(r["source_saved"], true);
    // a compiler other than mono, a compiler for a notecard
    let (hash, text) = snapshot(&mut desk, &f, demo::SCRIPT);
    let mut args = edit(demo::SCRIPT, &hash, &text, "l4");
    args["compiler"] = json!("lsl2");
    let e = save(&mut desk, &mut f, SAVE_SCRIPT, args, saved_ok).expect_err("compiler");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES);
    let mut args = edit(demo::NOTES, &hash, &text, "l5");
    args["compiler"] = json!("mono");
    let e = save(&mut desk, &mut f, UPDATE_NOTECARD, args, saved_ok).expect_err("compiler");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
}

#[test]
fn a_script_s_compile_result_is_reported_apart_from_its_source_save() {
    let mut f = fixture();
    let mut desk = desk_with(allow_all());
    let (hash, text) = snapshot(&mut desk, &f, demo::SCRIPT_BROKEN);
    let broken = text.replace("llSay", "llSay(");
    let r = save(
        &mut desk,
        &mut f,
        SAVE_SCRIPT,
        edit(demo::SCRIPT_BROKEN, &hash, &broken, "c1"),
        |w| {
            Ok(Saved {
                item: w.item,
                asset: Uuid::from_u128(777),
                compiled: Some(false),
                checked: true,
                errors: vec!["(4, 13) : ERROR : Syntax error".into(), "a line without position".into()],
            })
        },
    )
    .expect("the source was saved");
    let operation = r["operation_id"].as_str().expect("operation").to_owned();
    assert_eq!(r["source_saved"], true);
    assert_eq!(r["compiled"], false);
    assert_eq!(r["compile_checked"], true);
    assert_eq!(r["compiler"], "mono");
    assert_eq!(r["state"], "done", "the source save is done");
    assert_eq!(r["errors"][0]["line"], 5, "from 1");
    assert_eq!(r["errors"][0]["column"], 14);
    assert_eq!(r["errors"][0]["message"], "Syntax error");
    assert_eq!(r["errors"][1]["line"], Value::Null);
    assert!(r["snapshot_hash"].is_string(), "a new snapshot of the saved source");
    // the inventory carries the new asset
    assert_eq!(f.world.inventory.items[&demo::SCRIPT_BROKEN].asset_id, Uuid::from_u128(777));
    // the status tool says the same, by operation or request id
    for args in [json!({"operation_id": operation}), json!({"client_request_id": "c1"})] {
        let (c, answer) = call(STATUS, args, false);
        desk.take(c, &ctx(&f.world.inventory), Instant::now());
        let s = answered(&answer).expect("status");
        assert_eq!(s["source_saved"], true);
        assert_eq!(s["compiled"], false);
        assert_eq!(s["state"], "done");
        assert_eq!(s["errors"][0]["line"], 5);
    }
    // the next edit uses the new snapshot
    let hash = r["snapshot_hash"].as_str().expect("hash").to_owned();
    let r = save(&mut desk, &mut f, SAVE_SCRIPT, edit(demo::SCRIPT_BROKEN, &hash, &text, "c2"), |w| {
        Ok(Saved {
            item: w.item,
            asset: Uuid::from_u128(778),
            compiled: Some(true),
            checked: false,
            errors: vec![],
        })
    })
    .expect("saved");
    assert_eq!(r["compiled"], true);
    assert_eq!(r["compile_checked"], false, "a grid that does not compile inventory scripts");
    // the server refused the save: nothing saved
    let hash = r["snapshot_hash"].as_str().expect("hash").to_owned();
    let e = save(
        &mut desk,
        &mut f,
        SAVE_SCRIPT,
        edit(demo::SCRIPT_BROKEN, &hash, &broken, "c3"),
        |_| Err("no".into()),
    )
    .expect_err("failed");
    assert_eq!(e.code, ErrorCode::Failed);
    let (c, answer) = call(STATUS, json!({"client_request_id": "c3"}), false);
    desk.take(c, &ctx(&f.world.inventory), Instant::now());
    let s = answered(&answer).expect("status");
    assert_eq!(s["source_saved"], false);
    assert_eq!(s["state"], "failed");
    // not confirmed: unknown, never sent again
    let e = save(
        &mut desk,
        &mut f,
        SAVE_SCRIPT,
        edit(demo::SCRIPT_BROKEN, &hash, &broken, "c4"),
        |_| Err(aurora_i18n::tr!("script-save-unconfirmed").into()),
    )
    .expect_err("unconfirmed");
    assert_eq!(e.code, ErrorCode::Timeout);
    let (c, answer) = call(STATUS, json!({"client_request_id": "c4"}), false);
    desk.take(c, &ctx(&f.world.inventory), Instant::now());
    let s = answered(&answer).expect("status");
    assert_eq!(s["source_saved"], Value::Null);
    assert_eq!(s["state"], "unconfirmed");
}

/// A script saved twice in a row reads back each saved source with the
/// snapshot its save gave, even when an inventory record that still
/// carries an asset a save replaced comes after it (a stale item read):
/// the source is read by the item's asset.
#[test]
fn a_saved_script_reads_back_its_new_source_after_each_save() {
    let mut f = fixture();
    let mut desk = desk_with(allow_all());
    let mut assets = vec![f.world.inventory.items[&demo::SCRIPT].asset_id];
    let (mut hash, original) = snapshot(&mut desk, &f, demo::SCRIPT);
    let mut hashes = vec![hash.clone()];
    for (n, greeting) in ["Hello from the first save!", "Hello from the second save!"].iter().enumerate() {
        let text = original.replace("Hello, Avatar!", greeting);
        let r = save(
            &mut desk,
            &mut f,
            SAVE_SCRIPT,
            edit(demo::SCRIPT, &hash, &text, &format!("readback-{n}")),
            saved_ok,
        )
        .expect("saved");
        let saved = r["snapshot_hash"].as_str().expect("a new snapshot").to_owned();
        assert!(!hashes.contains(&saved), "save {n}: a new snapshot_hash");
        // the item read after the save comes back with an older asset
        for old in &assets {
            let mut stale = f.world.inventory.items[&demo::SCRIPT].clone();
            stale.asset_id = *old;
            f.world.inventory.add_items(vec![stale]);
            desk.content().hold(&mut f.world.inventory);
        }
        let back = read(&mut desk, &f, demo::SCRIPT).expect("read back");
        assert_eq!(back["text"], text.as_str(), "save {n}: the saved source, not the original");
        assert_eq!(back["snapshot_hash"], saved.as_str(), "save {n}: the snapshot the save gave");
        assets.push(f.world.inventory.items[&demo::SCRIPT].asset_id);
        hashes.push(saved.clone());
        hash = saved;
    }
    // saved elsewhere since (the viewer's editor): the hold ends
    let elsewhere = Uuid::from_u128(4242);
    if let Some(it) = f.world.inventory.items.get_mut(&demo::SCRIPT) {
        it.asset_id = elsewhere;
    }
    desk.content().hold(&mut f.world.inventory);
    if let Some(it) = f.world.inventory.items.get_mut(&demo::SCRIPT) {
        it.asset_id = assets[0];
    }
    desk.content().hold(&mut f.world.inventory);
    assert_eq!(f.world.inventory.items[&demo::SCRIPT].asset_id, assets[0], "no longer held");
}

/// The control: a notecard's save reads back its new text too.
#[test]
fn a_saved_notecard_reads_back_its_new_text() {
    let mut f = fixture();
    let mut desk = desk_with(allow_all());
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES);
    let text = format!("{text}one more line\n");
    let r = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES, &hash, &text, "note-readback"),
        saved_ok,
    )
    .expect("saved");
    let saved = r["snapshot_hash"].as_str().expect("a new snapshot");
    assert_ne!(saved, hash);
    let back = read(&mut desk, &f, demo::NOTES).expect("read back");
    assert_eq!(back["text"], text.as_str());
    assert_eq!(back["snapshot_hash"], saved);
}

#[test]
fn compile_errors_are_bounded() {
    let many: Vec<String> = (0..100).map(|n| format!("({n}, 0) : ERROR : e{n}")).collect();
    let errors = content::compile_errors(&many);
    let list = errors.as_array().expect("list");
    assert_eq!(list.len(), 65, "64 and the count left out");
    assert_eq!(list[64]["truncated"], 36);
    let huge = vec!["x".repeat(9000)];
    assert_eq!(content::compile_errors(&huge).as_array().expect("list")[0]["truncated"], 1);
}

#[test]
fn rlva_restrictions_refuse_reads_and_saves() {
    let f = fixture();
    let mut desk = desk_with(allow_all());
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES);
    let now = Instant::now();
    for (limit, item, tool) in [
        ("viewnote", demo::NOTES, GET),
        ("viewscript", demo::SCRIPT, GET),
        ("edit", demo::NOTES, UPDATE_NOTECARD),
        ("viewnote", demo::NOTES, UPDATE_NOTECARD),
    ] {
        let mut cx = ctx(&f.world.inventory);
        match limit {
            "viewnote" => cx.content.viewnote = true,
            "viewscript" => cx.content.viewscript = true,
            _ => cx.content.edit = true,
        }
        let args = if tool == GET {
            json!({"item_id": item.to_string()})
        } else {
            edit(item, &hash, &format!("{text}r\n"), &format!("rlv-{limit}"))
        };
        let (c, answer) = call(tool, args, false);
        desk.take(c, &cx, now);
        assert_eq!(
            answered(&answer).expect_err("refused").code,
            ErrorCode::NotAllowed,
            "{limit} {tool}"
        );
    }
    // @showinv: nothing at all
    let mut cx = ctx(&f.world.inventory);
    cx.inventory_locked = true;
    let (c, answer) = call(GET, json!({"item_id": demo::NOTES.to_string()}), false);
    desk.take(c, &cx, now);
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::PermissionDenied);
}

#[test]
fn a_save_needs_a_client_request_id_and_runs_once_per_id() {
    let mut f = fixture();
    let mut desk = desk_with(allow_all());
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES);
    let new = format!("{text}once\n");
    let mut args = edit(demo::NOTES, &hash, &new, "x");
    args.as_object_mut().expect("object").remove("client_request_id");
    let e = save(&mut desk, &mut f, UPDATE_NOTECARD, args, saved_ok).expect_err("required");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
    assert!(e.message.contains("client_request_id"));
    let sends = std::cell::Cell::new(0);
    let counted = |w: &content::Save| {
        sends.set(sends.get() + 1);
        saved_ok(w)
    };
    let first = save(&mut desk, &mut f, UPDATE_NOTECARD, edit(demo::NOTES, &hash, &new, "once"), counted).expect("saved");
    // sent again after a "lost" answer: the first receipt, nothing sent
    let again = save(&mut desk, &mut f, UPDATE_NOTECARD, edit(demo::NOTES, &hash, &new, "once"), counted).expect("receipt");
    assert_eq!(again["duplicate"], true);
    assert_eq!(again["operation_id"], first["operation_id"]);
    assert_eq!(sends.get(), 1);
    // the receipt holds no text, only its target
    let receipt = again["receipt"].to_string();
    assert!(!receipt.contains("once\\n") && !receipt.contains(text.trim()), "{receipt}");
    assert!(receipt.contains(&demo::NOTES.to_string()), "its target: {receipt}");
    // the same id with another text: conflict
    let e = save(
        &mut desk,
        &mut f,
        UPDATE_NOTECARD,
        edit(demo::NOTES, &hash, "other", "once"),
        counted,
    )
    .expect_err("conflict");
    assert_eq!(e.code, ErrorCode::Conflict);
    // an unchanged notecard saves nothing
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES);
    let r = save(&mut desk, &mut f, UPDATE_NOTECARD, edit(demo::NOTES, &hash, &text, "same"), counted).expect("nothing");
    assert_eq!(r["unchanged"], true);
    assert_eq!(r["source_saved"], false);
    assert_eq!(sends.get(), 1);
}

#[test]
fn the_dialog_names_the_item_and_the_changed_lines() {
    let f = fixture();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let (hash, text) = snapshot(&mut desk, &f, demo::SCRIPT);
    let new = text.replace("Hello, Avatar!", "Bonjour !") + &"// more\n".repeat(7);
    let now = Instant::now();
    let (c, answer) = call(SAVE_SCRIPT, edit(demo::SCRIPT, &hash, &new, "d1"), true);
    desk.take(c, &ctx(&f.world.inventory), now);
    let q = desk.question(now).expect("asked");
    let name = &f.world.inventory.items[&demo::SCRIPT].name;
    assert!(q.summary.contains(name.as_str()), "{}", q.summary);
    assert!(q.summary.contains('8'), "8 added: {}", q.summary);
    assert_eq!(q.detail.len(), 6, "5 lines and how many more: {:?}", q.detail);
    assert!(q.detail[0].starts_with("− ") && q.detail[0].contains("Hello"), "{:?}", q.detail);
    assert!(q.detail[1].starts_with("+ ") && q.detail[1].contains("Bonjour"), "{:?}", q.detail);
    desk.decide(q.number, false);
    desk.tick(&IDLE, now);
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::PermissionDenied);
}

#[test]
fn an_item_open_in_the_viewer_is_left_to_the_user() {
    let f = fixture();
    let mut desk = desk_with(allow_all());
    let (hash, text) = snapshot(&mut desk, &f, demo::NOTES);
    let now = Instant::now();
    let mut cx = ctx(&f.world.inventory);
    cx.content.open.insert(demo::NOTES);
    let (c, answer) = call(UPDATE_NOTECARD, edit(demo::NOTES, &hash, &format!("{text}b\n"), "o1"), false);
    desk.take(c, &cx, now);
    assert_eq!(answered(&answer).expect_err("busy").code, ErrorCode::Busy);
}

#[test]
fn the_demo_reads_then_saves_with_a_compile_error() {
    let mut f = fixture();
    let mut desk = desk_with(allow_all());
    for (tool, args) in demo::reads() {
        assert_eq!(tool, GET);
        let item = Uuid::parse_str(args["item_id"].as_str().expect("id")).expect("uuid");
        let r = read(&mut desk, &f, item).expect("read");
        let (tool, args) = demo::save_after(&r).expect("a save");
        let compiled = |w: &content::Save| {
            let source = String::from_utf8(w.data.clone()).expect("utf-8");
            let broken = source.contains(demo::SYNTAX_ERROR);
            Ok(Saved {
                item: w.item,
                asset: Uuid::new_v4(),
                compiled: (w.kind == content::Kind::Script).then_some(!broken),
                checked: true,
                errors: if broken {
                    vec!["(5, 9) : ERROR : Syntax error".into()]
                } else {
                    vec![]
                },
            })
        };
        let r = save(&mut desk, &mut f, tool, args, compiled).expect("saved");
        assert_eq!(r["source_saved"], true);
        if tool == SAVE_SCRIPT {
            assert_eq!(r["compiled"], false);
        }
    }
}

/// End to end: the sidecar's logic → a real socket → the bridge → the desk,
/// served as the app would: a read, a preview and a save, then the same
/// save again (a lost answer) answering its receipt.
#[test]
fn read_preview_and_save_end_to_end() {
    use folderstorm_mcp::connect::Viewer;
    use folderstorm_mcp_bridge::Bridge;
    use folderstorm_mcp_bridge::server::Config;

    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = Bridge::start(Config {
        discovery_dir: Some(dir.path().to_path_buf()),
        pid: std::process::id(),
        version: "test".into(),
        permissions: allow_all(),
        viewer_timeout: Duration::from_secs(10),
    })
    .expect("bridge started");
    bridge.set_login(1, true);
    let folder = dir.path().to_path_buf();
    let assistant = std::thread::spawn(move || {
        let mut viewer = Viewer::new(Some(folder), None);
        let read = viewer.call(GET, json!({"item_id": demo::NOTES.to_string()})).expect("read");
        let hash = read["snapshot_hash"].as_str().expect("hash").to_owned();
        let text = format!("{}Saved over the socket. Émoji 🦊.\n", read["text"].as_str().expect("text"));
        let preview = viewer
            .call(
                PREVIEW,
                json!({"item_id": demo::NOTES.to_string(), "snapshot_hash": hash, "text": text}),
            )
            .expect("preview");
        let args = json!({"item_id": demo::NOTES.to_string(), "snapshot_hash": hash, "text": text, "client_request_id": "socket-1"});
        let saved = viewer.call(UPDATE_NOTECARD, args.clone());
        let again = viewer.call(UPDATE_NOTECARD, args);
        (preview, saved, again, text)
    });
    let mut f = fixture();
    let mut desk = desk_with(allow_all());
    let mut sends = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        let session = bridge.viewer_session();
        desk.set_login(1, session.clone());
        let mut cx = ctx(&f.world.inventory);
        cx.viewer_session = session;
        while let Ok(pending) = calls.try_recv() {
            desk.take(pending, &cx, now);
        }
        serve(&mut desk, &f);
        desk.tick_reads(&f.world, now);
        for run in desk.tick(&IDLE, now) {
            let planned = changes::plan(&run.tool, &run.arguments, &ctx(&f.world.inventory)).expect("planned");
            let Work::Content(work) = planned.work.clone() else {
                panic!("a content save")
            };
            sends += 1;
            desk.begin(run, &planned, false, now);
            let request = Uuid::new_v4();
            if let Some(it) = f.world.inventory.items.get_mut(&work.item) {
                it.asset_id = Uuid::new_v4();
            }
            f.documents.insert(work.item, work.data.clone());
            let saved = Saved::notecard(&f.world.inventory.items[&work.item]);
            desk.observe((request, false, Outcome::Content(Ok(saved))));
            desk.sent(Some(request), None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let (preview, saved, again, text) = assistant.join().expect("assistant");
    assert_eq!(preview["lines_added"], 1, "{preview}");
    let saved = saved.expect("saved");
    assert_eq!(saved["source_saved"], true, "{saved}");
    assert_eq!(saved["state"], "done");
    assert!(saved["snapshot_hash"].is_string());
    let again = again.expect("receipt");
    assert_eq!(again["duplicate"], true);
    assert_eq!(again["operation_id"], saved["operation_id"]);
    assert_eq!(sends, 1, "sent once");
    assert_eq!(Notecard::decode(&f.documents[&demo::NOTES]).expect("card").text, text);
    bridge.stop();
}
