//! Folderstorm: the local assistant's changes (phase 2) against the demo
//! inventory, the Ask dialog's queue, the server's answers, the bake wait,
//! and a change from the sidecar to the demo inventory over a real socket.

use super::changes::{Planned, Work, plan};
use super::desk::{Bake, Desk, Outcome};
use super::tools::{Ctx, Session};
use crate::world::appearance::{self, Action};
use crate::world::inventory::{FetchState, Folder, Inventory, actions};
use aurora_net::inventory::{InvFolder, InvItem};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{Class, ErrorCode, Level, Permissions, Preset, ToolError};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use uuid::Uuid;

const AGENT: Uuid = Uuid::from_u128(2);
const DEMO_FOLDER: Uuid = Uuid::from_u128(8000);
const SUBFOLDER: Uuid = Uuid::from_u128(8001);
const MARKETPLACE: Uuid = Uuid::from_u128(8002);
const OBJECT: Uuid = Uuid::from_u128(8100);
const TEXTURE: Uuid = Uuid::from_u128(8104);
const NO_COPY: Uuid = Uuid::from_u128(8109);
const NO_TRANSFER: Uuid = Uuid::from_u128(8110);
const LINK: Uuid = Uuid::from_u128(8113);
const COF: Uuid = Uuid::from_u128(701);
const COP_OUTFIT: Uuid = Uuid::from_u128(703);
const SILHOUETTE: Uuid = Uuid::from_u128(710);
const JACKET: Uuid = Uuid::from_u128(715);
const SPARE_SHIRT: Uuid = Uuid::from_u128(722);

fn demo_inventory() -> Inventory {
    let mut inv = Inventory {
        root: Uuid::from_u128(80),
        owner: AGENT,
        ..Default::default()
    };
    inv.folders.insert(
        inv.root,
        Folder {
            info: InvFolder {
                id: inv.root,
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
    crate::world::inventory::demo::seed(&mut inv, AGENT);
    inv
}

fn ctx(inv: &Inventory) -> Ctx<'_> {
    let mut worn = HashSet::new();
    if let Some(cof) = appearance::cof(inv) {
        worn.extend(
            appearance::folder_links(inv, cof)
                .into_iter()
                .filter(|l| !l.folder)
                .map(|l| l.target),
        );
    }
    Ctx {
        inv,
        session: Session::Demo,
        worn,
        attachment_points: HashMap::new(),
        inventory_locked: false,
        protected: HashSet::new(),
        outfit_points: HashMap::new(),
        viewer_session: None,
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

/// Run a planned change as the demo's server would.
fn apply(inv: &mut Inventory, planned: Planned) {
    match planned.work {
        Work::Nothing => {}
        Work::Inventory(change) => {
            actions::demo_mutate(inv, AGENT, change).expect("applied");
        }
        Work::Outfit(action) => {
            let (change, _) = appearance::plan(inv, action, &HashMap::new()).expect("outfit planned");
            appearance::demo_mutate(inv, AGENT, &change).expect("outfit applied");
        }
    }
}

fn cof_targets(inv: &Inventory) -> HashSet<Uuid> {
    appearance::folder_links(inv, COF).into_iter().map(|l| l.target).collect()
}

#[test]
fn create_a_folder_where_the_window_offers_it() {
    let mut inv = demo_inventory();
    let p = planned(
        &inv,
        "inventory_create_folder",
        json!({"name": "  Sorted  ", "folder_id": DEMO_FOLDER.to_string()}),
    );
    let id = p.created.expect("a new folder");
    assert_eq!(p.reply["name"], "Sorted");
    assert!(p.summary.contains("Sorted"), "{}", p.summary);
    apply(&mut inv, p);
    assert_eq!(inv.folders[&id].info.parent, DEMO_FOLDER);
    assert_eq!(inv.folders[&id].info.name, "Sorted");
    // My Inventory by default, a path too
    let p = planned(&inv, "inventory_create_folder", json!({"name": "Top"}));
    assert!(matches!(&p.work, Work::Inventory(m) if m.refresh.contains(&inv.root)));
    let path = inv.folders[&DEMO_FOLDER].info.name.clone();
    planned(&inv, "inventory_create_folder", json!({"name": "Deep", "path": path}));
    // the folders that take nothing new
    let trash = actions::system(&inv, 14).expect("trash");
    let my_outfits = actions::system(&inv, 48).expect("my outfits");
    for parent in [COF, trash, my_outfits, inv.lib_root] {
        assert_eq!(
            refused(
                &inv,
                "inventory_create_folder",
                json!({"name": "x", "folder_id": parent.to_string()})
            ),
            ErrorCode::NotAllowed,
            "{parent}"
        );
    }
    assert_eq!(
        refused(
            &inv,
            "inventory_create_folder",
            json!({"name": "x", "folder_id": MARKETPLACE.to_string()})
        ),
        ErrorCode::NotAllowed
    );
    for bad in [
        json!({"name": ""}),
        json!({"name": "a|b"}),
        json!({}),
        json!({"name": "x", "extra": 1}),
    ] {
        assert_eq!(
            refused(&inv, "inventory_create_folder", bad.clone()),
            ErrorCode::InvalidArguments,
            "{bad}"
        );
    }
}

#[test]
fn rename_and_describe_follow_the_window() {
    let mut inv = demo_inventory();
    let p = planned(&inv, "inventory_rename", json!({"id": TEXTURE.to_string(), "name": "Brick"}));
    apply(&mut inv, p);
    assert_eq!(inv.items[&TEXTURE].name, "Brick");
    let p = planned(&inv, "inventory_rename", json!({"id": SUBFOLDER.to_string(), "name": "Shelf"}));
    apply(&mut inv, p);
    assert_eq!(inv.folders[&SUBFOLDER].info.name, "Shelf");
    // the same name changes nothing and asks nothing
    assert!(matches!(
        planned(&inv, "inventory_rename", json!({"id": TEXTURE.to_string(), "name": "Brick"})).work,
        Work::Nothing
    ));
    let trash = actions::system(&inv, 14).expect("trash");
    for id in [trash, COF, LINK, inv.lib_root, MARKETPLACE] {
        assert_eq!(
            refused(&inv, "inventory_rename", json!({"id": id.to_string(), "name": "x"})),
            ErrorCode::NotAllowed,
            "{id}"
        );
    }
    assert_eq!(
        refused(&inv, "inventory_rename", json!({"id": Uuid::from_u128(1).to_string(), "name": "x"})),
        ErrorCode::NotFound
    );

    let p = planned(
        &inv,
        "inventory_describe",
        json!({"item_id": OBJECT.to_string(), "description": "A crate"}),
    );
    apply(&mut inv, p);
    assert_eq!(inv.items[&OBJECT].desc, "A crate");
    let long = "x".repeat(128);
    assert_eq!(
        refused(
            &inv,
            "inventory_describe",
            json!({"item_id": OBJECT.to_string(), "description": long})
        ),
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        refused(
            &inv,
            "inventory_describe",
            json!({"item_id": DEMO_FOLDER.to_string(), "description": "x"})
        ),
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        refused(&inv, "inventory_describe", json!({"item_id": LINK.to_string(), "description": "x"})),
        ErrorCode::NotAllowed
    );
}

/// AGENTS.md § 2 "Tests": each rule for an item of the owner, a group-owned
/// item, someone else's, a Library item, no copy, no transfer, no modify and
/// full permissions. Rename and describe need Modify, a copy needs Copy (a
/// Library item copies), through `actions::held`.
#[test]
fn creators_permissions_for_every_owner() {
    let mut inv = demo_inventory();
    let other = Uuid::from_u128(77);
    let full = actions::FULL | actions::MOVE;
    let cases = [
        // (owner, group owned, owner mask, everyone mask, may modify, may copy)
        (AGENT, false, full, 0, true, true),
        (AGENT, true, full, 0, false, false),
        (other, false, full, 0, false, false),
        (inv.lib_owner, false, full, actions::COPY, false, true),
        (AGENT, false, full & !actions::COPY, 0, true, false),
        (AGENT, false, full & !actions::TRANSFER, 0, true, true),
        (AGENT, false, full & !actions::MODIFY, 0, false, true),
        (other, false, full, actions::FULL, true, true),
    ];
    for (n, (owner, group_owned, owner_mask, everyone_mask, modify, copy)) in cases.into_iter().enumerate() {
        let id = Uuid::from_u128(99_300 + n as u128);
        let library = owner == inv.lib_owner;
        inv.add_items(vec![InvItem {
            id,
            parent: if library { inv.lib_root } else { DEMO_FOLDER },
            name: format!("case {n}"),
            asset_type: 7,
            inv_type: 7,
            owner,
            group_owned,
            owner_mask,
            everyone_mask,
            ..Default::default()
        }]);
        let renamed = plan("inventory_rename", &json!({"id": id.to_string(), "name": "y"}), &ctx(&inv));
        // the Library is never renamed, whatever its rights
        assert_eq!(renamed.is_ok(), modify && !library, "rename, case {n}");
        if let Err(e) = renamed {
            assert_eq!(e.code, ErrorCode::NotAllowed, "case {n}");
        }
        let described = plan(
            "inventory_describe",
            &json!({"item_id": id.to_string(), "description": "y"}),
            &ctx(&inv),
        );
        assert_eq!(described.is_ok(), modify && !library, "describe, case {n}");
        let copied = plan(
            "inventory_copy",
            &json!({"ids": [id.to_string()], "folder_id": SUBFOLDER.to_string()}),
            &ctx(&inv),
        );
        assert_eq!(copied.is_ok(), copy, "copy, case {n}: {:?}", copied.err());
    }
}

#[test]
fn move_copy_and_link() {
    let mut inv = demo_inventory();
    let p = planned(
        &inv,
        "inventory_move",
        json!({"ids": [TEXTURE.to_string(), NO_COPY.to_string()], "folder_id": SUBFOLDER.to_string()}),
    );
    assert_eq!(p.reply["moved"], json!([TEXTURE, NO_COPY]));
    apply(&mut inv, p);
    assert_eq!(inv.items[&TEXTURE].parent, SUBFOLDER);
    assert_eq!(inv.items[&NO_COPY].parent, SUBFOLDER);

    let trash = actions::system(&inv, 14).expect("trash");
    assert_eq!(
        refused(
            &inv,
            "inventory_move",
            json!({"ids": [OBJECT.to_string()], "folder_id": trash.to_string()})
        ),
        ErrorCode::InvalidArguments,
        "the Trash is inventory_trash's"
    );
    assert_eq!(
        refused(
            &inv,
            "inventory_move",
            json!({"ids": [DEMO_FOLDER.to_string()], "folder_id": SUBFOLDER.to_string()})
        ),
        ErrorCode::NotAllowed,
        "into itself"
    );
    assert_eq!(
        refused(
            &inv,
            "inventory_move",
            json!({"ids": [trash.to_string()], "folder_id": DEMO_FOLDER.to_string()})
        ),
        ErrorCode::NotAllowed,
        "a system folder"
    );
    assert_eq!(
        refused(
            &inv,
            "inventory_move",
            json!({"ids": [OBJECT.to_string()], "folder_id": MARKETPLACE.to_string()})
        ),
        ErrorCode::NotAllowed,
        "the Marketplace"
    );
    assert_eq!(
        refused(&inv, "inventory_move", json!({"ids": [OBJECT.to_string()]})),
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        refused(&inv, "inventory_move", json!({"ids": [], "folder_id": SUBFOLDER.to_string()})),
        ErrorCode::InvalidArguments
    );
    let too_many: Vec<String> = (0..51).map(|_| OBJECT.to_string()).collect();
    assert_eq!(
        refused(&inv, "inventory_move", json!({"ids": too_many, "folder_id": SUBFOLDER.to_string()})),
        ErrorCode::InvalidArguments
    );
    // a locked folder stays where it is
    let mut cx = ctx(&inv);
    cx.protected.insert(SUBFOLDER);
    let locked = plan(
        "inventory_move",
        &json!({"ids": [SUBFOLDER.to_string()], "folder_id": inv.root.to_string()}),
        &cx,
    );
    assert_eq!(locked.expect_err("locked").code, ErrorCode::NotAllowed);

    // a copy of what may be copied; never a no-copy item
    let p = planned(
        &inv,
        "inventory_copy",
        json!({"ids": [OBJECT.to_string(), NO_TRANSFER.to_string()], "path": ""}),
    );
    assert!(matches!(&p.work, Work::Inventory(m) if m.copies.len() == 2));
    assert_eq!(
        refused(
            &inv,
            "inventory_copy",
            json!({"ids": [NO_COPY.to_string()], "folder_id": DEMO_FOLDER.to_string()})
        ),
        ErrorCode::NotAllowed
    );
    // links
    let before = inv.folders[&SUBFOLDER].items.len();
    let p = planned(
        &inv,
        "inventory_link",
        json!({"ids": [OBJECT.to_string()], "folder_id": SUBFOLDER.to_string()}),
    );
    apply(&mut inv, p);
    let folder = &inv.folders[&SUBFOLDER];
    assert_eq!(folder.items.len(), before + 1);
    assert!(
        folder
            .items
            .iter()
            .any(|id| inv.items[id].asset_type == 24 && inv.items[id].asset_id == OBJECT)
    );
}

#[test]
fn trash_and_restore_but_never_delete() {
    let mut inv = demo_inventory();
    let trash = actions::system(&inv, 14).expect("trash");
    let p = planned(&inv, "inventory_trash", json!({"ids": [TEXTURE.to_string()]}));
    apply(&mut inv, p);
    assert_eq!(inv.items[&TEXTURE].parent, trash);
    // in the Trash already: the window would purge it, never here
    assert_eq!(
        refused(&inv, "inventory_trash", json!({"ids": [TEXTURE.to_string()]})),
        ErrorCode::NotAllowed
    );
    // worn, system and Current Outfit entries stay
    for id in [SILHOUETTE, COF, trash] {
        assert_eq!(
            refused(&inv, "inventory_trash", json!({"ids": [id.to_string()]})),
            ErrorCode::NotAllowed,
            "{id}"
        );
    }
    let expected = actions::default_folder(&inv, TEXTURE).expect("a folder");
    let p = planned(&inv, "inventory_restore", json!({"ids": [TEXTURE.to_string()]}));
    assert_eq!(p.reply["restored"][0]["folder_id"], json!(expected));
    apply(&mut inv, p);
    assert_eq!(inv.items[&TEXTURE].parent, expected);
    assert_eq!(
        refused(&inv, "inventory_restore", json!({"ids": [OBJECT.to_string()]})),
        ErrorCode::InvalidArguments,
        "not in the Trash"
    );
    for never in folderstorm_mcp_bridge::protocol::NEVER {
        assert_eq!(refused(&inv, never, json!({"ids": [TEXTURE.to_string()]})), ErrorCode::UnknownTool);
    }
}

#[test]
fn wear_and_take_off_outfits_and_items() {
    let mut inv = demo_inventory();
    let cop: HashSet<Uuid> = appearance::folder_links(&inv, COP_OUTFIT).into_iter().map(|l| l.target).collect();
    let p = planned(&inv, "outfit_wear", json!({"outfit_id": COP_OUTFIT.to_string()}));
    assert!(matches!(p.work, Work::Outfit(Action::Wear(id, false)) if id == COP_OUTFIT));
    apply(&mut inv, p);
    let worn = cof_targets(&inv);
    assert!(cop.iter().all(|id| worn.contains(id)), "the outfit is worn");
    assert!(matches!(
        planned(&inv, "outfit_wear", json!({"outfit_id": COP_OUTFIT.to_string(), "mode": "add"})).work,
        Work::Outfit(Action::Wear(_, true))
    ));
    assert_eq!(
        refused(&inv, "outfit_wear", json!({"outfit_id": COP_OUTFIT.to_string(), "mode": "over"})),
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        refused(&inv, "outfit_wear", json!({"outfit_id": COF.to_string()})),
        ErrorCode::InvalidArguments
    );
    assert_eq!(refused(&inv, "outfit_wear", json!({})), ErrorCode::InvalidArguments);

    let p = planned(&inv, "outfit_take_off", json!({"outfit_id": COP_OUTFIT.to_string()}));
    apply(&mut inv, p);
    let worn = cof_targets(&inv);
    assert!(worn.contains(&SILHOUETTE), "body parts stay");
    assert!(!worn.contains(&JACKET), "clothing comes off");

    // items, through their links too
    let p = planned(&inv, "items_wear", json!({"item_ids": [SPARE_SHIRT.to_string()]}));
    apply(&mut inv, p);
    assert!(cof_targets(&inv).contains(&SPARE_SHIRT));
    assert_eq!(
        refused(&inv, "items_wear", json!({"item_ids": [TEXTURE.to_string()]})),
        ErrorCode::InvalidArguments
    );
    let shelved = Uuid::from_u128(99_400);
    inv.add_items(vec![InvItem {
        id: shelved,
        parent: inv.lib_root,
        name: "Library shirt".into(),
        asset_type: 5,
        inv_type: 18,
        flags: 4,
        owner: inv.lib_owner,
        everyone_mask: actions::COPY,
        ..Default::default()
    }]);
    assert_eq!(
        refused(&inv, "items_wear", json!({"item_ids": [shelved.to_string()]})),
        ErrorCode::NotAllowed,
        "copy it first"
    );

    let p = planned(&inv, "items_take_off", json!({"item_ids": [SPARE_SHIRT.to_string()]}));
    apply(&mut inv, p);
    assert!(!cof_targets(&inv).contains(&SPARE_SHIRT));
    assert_eq!(
        refused(&inv, "items_take_off", json!({"item_ids": [SPARE_SHIRT.to_string()]})),
        ErrorCode::InvalidArguments,
        "not worn"
    );
    assert_eq!(
        refused(&inv, "items_take_off", json!({"item_ids": [SILHOUETTE.to_string()]})),
        ErrorCode::NotAllowed,
        "body parts are only replaced"
    );
}

#[test]
fn an_rlv_inventory_lock_refuses_changes() {
    let inv = demo_inventory();
    let mut cx = ctx(&inv);
    cx.inventory_locked = true;
    let e = plan("inventory_rename", &json!({"id": TEXTURE.to_string(), "name": "x"}), &cx).expect_err("locked");
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    cx.inventory_locked = false;
    cx.session = Session::LoggedOut;
    let e = plan("inventory_rename", &json!({"id": TEXTURE.to_string(), "name": "x"}), &cx).expect_err("logged out");
    assert_eq!(e.code, ErrorCode::NotReady);
}

// --- the desk: questions, runs, the server's answers, the bake ---

fn call(tool: &str, args: Value, ask: bool) -> (Pending, Receiver<Result<Value, ToolError>>) {
    Pending::detached(tool, args, ask, Some("Claude Code"))
}

fn answered(answer: &Receiver<Result<Value, ToolError>>) -> Result<Value, ToolError> {
    answer.try_recv().expect("answered")
}

const IDLE: Bake = Bake {
    outfit_change_pending: false,
    cof_version: 3,
    appearance_cof_version: 3,
    server_bakes: true,
};

/// Every class that can on Allow, the rest as Read only leaves it.
fn allow_all() -> Permissions {
    let mut p = Permissions::default();
    for class in Class::ALL {
        p.set(class, Level::Allow);
    }
    p
}

/// A desk with these levels in force.
fn desk_with(permissions: Permissions) -> Desk {
    let mut desk = Desk::default();
    desk.set_permissions(permissions);
    desk
}

#[test]
fn ask_shows_the_client_and_waits_for_the_answer() {
    let inv = demo_inventory();
    let cx = ctx(&inv);
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let rename = json!({"id": TEXTURE.to_string(), "name": "Brick"});
    let (a, refused_answer) = call("inventory_rename", rename.clone(), true);
    let (b, allowed_answer) = call("inventory_rename", rename.clone(), true);
    desk.take(a, &cx, now);
    desk.take(b, &cx, now);
    let q = desk.question(now).expect("a question");
    assert_eq!(q.client.as_deref(), Some("Claude Code"));
    assert!(q.summary.contains("Brick"), "{}", q.summary);
    assert_eq!(q.seconds_left, 60);
    assert_eq!(q.waiting, 1);
    // nothing runs while the user reads
    assert!(desk.tick(&IDLE, now + Duration::from_secs(5)).is_empty());
    assert!(refused_answer.try_recv().is_err());
    desk.decide(q.number, false);
    assert!(desk.tick(&IDLE, now).is_empty());
    let e = answered(&refused_answer).expect_err("refused");
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    let q = desk.question(now).expect("the next one");
    assert_eq!(q.waiting, 0);
    desk.decide(q.number, true);
    let runs = desk.tick(&IDLE, now);
    assert_eq!(runs.len(), 1, "allowed: the app runs it");
    assert_eq!(runs[0].tool, "inventory_rename");
    assert!(allowed_answer.try_recv().is_err(), "answered once the change is done");
    assert!(desk.question(now).is_none());
}

#[test]
fn ask_refuses_after_sixty_seconds() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let (a, answer) = call("inventory_trash", json!({"ids": [TEXTURE.to_string()]}), true);
    desk.take(a, &ctx(&inv), now);
    assert!(desk.tick(&IDLE, now + Duration::from_secs(59)).is_empty());
    assert!(answer.try_recv().is_err());
    assert!(desk.tick(&IDLE, now + Duration::from_secs(60)).is_empty());
    let e = answered(&answer).expect_err("timed out");
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert!(e.message.contains("60"), "{}", e.message);
    assert_eq!(desk.counts(), (0, 0, 0, 0));
}

#[test]
fn allowed_changes_run_at_once_and_refusals_need_no_question() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let (a, _) = call("inventory_trash", json!({"ids": [TEXTURE.to_string()]}), false);
    desk.take(a, &ctx(&inv), now);
    assert!(desk.question(now).is_none());
    assert_eq!(desk.tick(&IDLE, now).len(), 1);
    // refused by the viewer's rules before anyone is asked
    let (b, answer) = call("inventory_trash", json!({"ids": [COF.to_string()]}), true);
    desk.take(b, &ctx(&inv), now);
    assert!(desk.question(now).is_none());
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::NotAllowed);
    // nothing to change: answered without a question
    let name = inv.items[&TEXTURE].name.clone();
    let (c, answer) = call("inventory_rename", json!({"id": TEXTURE.to_string(), "name": name}), true);
    desk.take(c, &ctx(&inv), now);
    assert!(desk.question(now).is_none());
    assert!(answered(&answer).is_ok());
    // reads answer at once
    let (d, answer) = call("worn_items", json!({}), false);
    desk.take(d, &ctx(&inv), now);
    assert!(answered(&answer).is_ok());
}

#[test]
fn a_change_answers_once_the_server_confirms_it() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = Desk::default();
    let args = json!({"name": "New"});
    let p = planned(&inv, "inventory_create_folder", args.clone());
    let temporary = p.created.expect("created");
    let server_id = Uuid::from_u128(55_555);

    // the demo answers inside the send
    let (a, answer) = call("inventory_create_folder", args.clone(), false);
    desk.begin(a, &p, false, now);
    let request = Uuid::from_u128(1);
    desk.observe((request, false, Outcome::Done(HashMap::from([(temporary, server_id)]))));
    desk.sent(Some(request), None);
    let reply = answered(&answer).expect("done");
    assert_eq!(reply["folder_id"], json!(server_id), "the server's id");
    assert_eq!(reply["done"], true);

    // the grid answers later; copies first, then the end
    let (b, answer) = call("inventory_create_folder", args.clone(), false);
    desk.begin(b, &p, false, now);
    let request = Uuid::from_u128(2);
    desk.sent(Some(request), None);
    desk.observe((Uuid::from_u128(99), false, Outcome::Done(HashMap::new())));
    desk.observe((request, true, Outcome::Done(HashMap::new())));
    desk.observe((request, false, Outcome::Copying));
    assert!(answer.try_recv().is_err(), "other answers are not this one's");
    desk.observe((request, false, Outcome::Done(HashMap::new())));
    assert_eq!(answered(&answer).expect("done")["folder_id"], json!(temporary));

    // refused by the server
    let (c, answer) = call("inventory_create_folder", args.clone(), false);
    desk.begin(c, &p, false, now);
    desk.sent(Some(Uuid::from_u128(3)), None);
    desk.observe((Uuid::from_u128(3), false, Outcome::Failed("nope".into())));
    assert_eq!(answered(&answer).expect_err("failed").code, ErrorCode::Failed);

    // never confirmed
    let (d, answer) = call("inventory_create_folder", args.clone(), false);
    desk.begin(d, &p, false, now);
    desk.sent(Some(Uuid::from_u128(4)), None);
    desk.tick(&IDLE, now + Duration::from_secs(29));
    assert!(answer.try_recv().is_err());
    desk.tick(&IDLE, now + Duration::from_secs(30));
    assert_eq!(answered(&answer).expect_err("timed out").code, ErrorCode::Timeout);

    // not started at all (an outfit change RLVa dropped)
    let (e, answer) = call("items_wear", json!({"item_ids": [SPARE_SHIRT.to_string()]}), false);
    desk.begin(e, &p, true, now);
    desk.sent(None, Some(String::new()));
    let e = answered(&answer).expect_err("not applied");
    assert_eq!(e.code, ErrorCode::NotAllowed);
    assert!(e.message.contains("RLV"));
    assert_eq!(desk.counts(), (0, 0, 0, 0));
}

#[test]
fn wait_for_bake_until_settled_or_the_timeout() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = Desk::default();
    let busy = Bake {
        outfit_change_pending: true,
        ..IDLE
    };
    let unbaked = Bake {
        appearance_cof_version: 2,
        ..IDLE
    };
    let (a, answer) = call("wait_for_bake", json!({"timeout_seconds": 5}), false);
    desk.take(a, &ctx(&inv), now);
    desk.tick(&busy, now);
    desk.tick(&unbaked, now + Duration::from_secs(1));
    assert!(answer.try_recv().is_err());
    desk.tick(&IDLE, now + Duration::from_secs(2));
    let reply = answered(&answer).expect("settled");
    assert_eq!(reply["settled"], true);
    assert_eq!(reply["cof_version"], 3);
    // a region that bakes nothing only waits for the outfit write
    assert!(
        Bake {
            server_bakes: false,
            ..unbaked
        }
        .settled()
    );

    let (b, answer) = call("wait_for_bake", json!({}), false);
    desk.take(b, &ctx(&inv), now);
    desk.tick(&busy, now + Duration::from_secs(29));
    assert!(answer.try_recv().is_err(), "30 s by default");
    desk.tick(&busy, now + Duration::from_secs(30));
    assert_eq!(answered(&answer).expect("answered")["settled"], false);

    for bad in [json!({"timeout_seconds": 0}), json!({"timeout_seconds": 121}), json!({"other": 1})] {
        let (c, answer) = call("wait_for_bake", bad.clone(), false);
        desk.take(c, &ctx(&inv), now);
        assert_eq!(answered(&answer).expect_err("bad").code, ErrorCode::InvalidArguments, "{bad}");
    }
}

/// The sidecar's MCP layer → a real 127.0.0.1 socket → the bridge (Ask
/// before changes) → the desk, its dialog answered → the change applied to
/// the demo inventory as the demo's server would.
#[test]
fn an_assistant_changes_the_demo_inventory_end_to_end() {
    use folderstorm_mcp::connect::Viewer;
    use folderstorm_mcp::jsonrpc::{Backend, Server};
    use folderstorm_mcp_bridge::server::Config;
    use folderstorm_mcp_bridge::{Bridge, Preset};

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
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        let mut ask = |line: &str| -> Value { serde_json::from_str(&server.handle_line(line).expect("answer")).expect("json") };
        ask(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code"}}}"#,
        );
        let list = ask(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let moved = ask(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"inventory_move","arguments":{{"ids":["{TEXTURE}"],"folder_id":"{SUBFOLDER}"}}}}}}"#
        ));
        let trashed = ask(&format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"inventory_trash","arguments":{{"ids":["{OBJECT}"]}}}}}}"#
        ));
        let deleted = ask(&format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"inventory_delete","arguments":{{"ids":["{OBJECT}"]}}}}}}"#
        ));
        (list, moved, trashed, deleted)
    });
    let mut inv = demo_inventory();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let mut questions = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    // the frame loop: never blocks on the assistant
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        while let Ok(pending) = calls.try_recv() {
            desk.take(pending, &ctx(&inv), now);
        }
        if let Some(q) = desk.question(now) {
            // the user allows the move and refuses the Trash
            let allow = q.summary.contains(&inv.folders[&SUBFOLDER].info.name);
            questions.push((q.client.clone(), q.summary.clone(), allow));
            desk.decide(q.number, allow);
        }
        for run in desk.tick(&IDLE, now) {
            let planned = plan(&run.tool, &run.arguments, &ctx(&inv)).expect("still allowed");
            let Work::Inventory(change) = planned.work.clone() else {
                panic!("an inventory change")
            };
            desk.begin(run, &planned, false, now);
            let request = Uuid::new_v4();
            let reply = actions::demo_mutate(&mut inv, AGENT, change);
            desk.observe((request, false, Outcome::Done(reply.expect("applied").created)));
            desk.sent(Some(request), None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let (list, moved, trashed, deleted) = assistant.join().expect("assistant");
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for name in ["inventory_move", "inventory_trash", "items_wear", "wait_for_bake"] {
        assert!(names.contains(&name), "{name} listed");
    }
    assert_eq!(moved["result"]["isError"], false, "{moved}");
    assert_eq!(inv.items[&TEXTURE].parent, SUBFOLDER);
    assert_eq!(trashed["result"]["isError"], true, "{trashed}");
    assert_eq!(inv.items[&OBJECT].parent, DEMO_FOLDER, "refused: not in the Trash");
    assert_eq!(deleted["result"]["isError"], true);
    assert_eq!(questions.len(), 2, "{questions:?}");
    assert!(questions.iter().all(|(client, _, _)| client.as_deref() == Some("Claude Code")));
    let activity = bridge.activity();
    assert_eq!(activity.len(), 2, "the delete never reached the bridge");
    bridge.stop();
}

/// A waiting call decided again with the levels and the login in force.
mod recheck;

/// Calls bound to one login (`viewer_session`) and capability versions.
mod binding;
