//! Folderstorm: the local assistant's tools against the demo inventory,
//! and the whole path from the sidecar to them over a real socket.

use super::tools::{Ctx, Session, answer};
use crate::world::appearance;
use crate::world::inventory::{FetchState, Folder, Inventory, actions};
use aurora_net::inventory::{InvFolder, InvItem};
use folderstorm_mcp_bridge::ErrorCode;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const AGENT: Uuid = Uuid::from_u128(2);
const DEMO_FOLDER: Uuid = Uuid::from_u128(8000);
const SUBFOLDER: Uuid = Uuid::from_u128(8001);
const OBJECT: Uuid = Uuid::from_u128(8100);
const TEXTURE: Uuid = Uuid::from_u128(8104);
const NO_COPY: Uuid = Uuid::from_u128(8109);
const NO_TRANSFER: Uuid = Uuid::from_u128(8110);
const LINK: Uuid = Uuid::from_u128(8113);

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
                .filter(|l| !l.folder && inv.items.get(&l.target).is_some_and(|it| it.asset_type != 6))
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
        stage: Default::default(),
        viewer_session: None,
        identity: Default::default(),
    }
}

fn call(inv: &Inventory, tool: &str, args: Value) -> Value {
    answer(tool, &args, &ctx(inv)).unwrap_or_else(|e| panic!("{tool} {args}: {e}"))
}

fn fails(inv: &Inventory, tool: &str, args: Value) -> ErrorCode {
    answer(tool, &args, &ctx(inv)).expect_err("an error").code
}

fn ids(list: &Value) -> Vec<Uuid> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|e| Uuid::parse_str(e["id"].as_str().expect("id")).expect("uuid"))
        .collect()
}

#[test]
fn viewer_status_reports_the_inventory() {
    let inv = demo_inventory();
    let status = call(&inv, "viewer_status", json!({}));
    assert_eq!(status["session"], "demo");
    assert_eq!(status["inventory"]["usable"], true);
    assert_eq!(status["inventory"]["folders_total"], inv.folders.len());
    assert_eq!(status["inventory"]["items_known"], inv.items.len());
    // logged out: status still answers, reads do not
    let empty = Inventory::default();
    let mut cx = ctx(&empty);
    cx.session = Session::LoggedOut;
    assert_eq!(
        answer("viewer_status", &json!({}), &cx).expect("status")["inventory"]["usable"],
        false
    );
    assert_eq!(
        answer("worn_items", &json!({}), &cx).expect_err("not ready").code,
        ErrorCode::NotReady
    );
}

#[test]
fn search_by_name_type_and_folder() {
    let inv = demo_inventory();
    let texture_name = inv.items[&TEXTURE].name.clone();
    let found = call(&inv, "inventory_search", json!({"query": texture_name.to_uppercase()}));
    assert!(ids(&found["results"]).contains(&TEXTURE), "{found}");
    let entry = found["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|e| e["id"] == json!(TEXTURE))
        .cloned()
        .expect("entry");
    assert_eq!(entry["type"], "texture");
    assert_eq!(entry["folder_id"], json!(DEMO_FOLDER));
    assert_eq!(entry["path"], inv.folders[&DEMO_FOLDER].info.name);
    assert!(entry.get("asset_id").is_none(), "search never carries asset ids");

    let objects = call(
        &inv,
        "inventory_search",
        json!({"type": "object", "folder_id": DEMO_FOLDER.to_string(), "limit": 200}),
    );
    let found = ids(&objects["results"]);
    assert!(found.contains(&OBJECT) && found.contains(&NO_COPY));
    assert!(
        objects["results"]
            .as_array()
            .expect("results")
            .iter()
            .all(|e| e["type"] == "object")
    );
    // the worn demo attachments live in My Inventory, outside this folder
    assert!(found.iter().all(|id| actions::under(&inv, *id, DEMO_FOLDER)));

    let folders = call(
        &inv,
        "inventory_search",
        json!({"type": "folder", "query": inv.folders[&SUBFOLDER].info.name}),
    );
    assert!(ids(&folders["results"]).contains(&SUBFOLDER));

    let by_path = call(
        &inv,
        "inventory_search",
        json!({"path": inv.folders[&DEMO_FOLDER].info.name, "type": "link"}),
    );
    assert_eq!(ids(&by_path["results"]), vec![LINK]);
}

#[test]
fn search_results_are_capped() {
    let inv = demo_inventory();
    let some = call(&inv, "inventory_search", json!({"limit": 3}));
    assert_eq!(some["results"].as_array().expect("results").len(), 3);
    assert_eq!(some["truncated"], true);
    assert!(some["total"].as_u64().expect("total") > 3);
    assert_eq!(fails(&inv, "inventory_search", json!({"limit": 0})), ErrorCode::InvalidArguments);
    assert_eq!(fails(&inv, "inventory_search", json!({"limit": 201})), ErrorCode::InvalidArguments);
    assert_eq!(
        fails(&inv, "inventory_search", json!({"type": "weapon"})),
        ErrorCode::InvalidArguments
    );
    assert_eq!(fails(&inv, "inventory_search", json!({"bogus": 1})), ErrorCode::InvalidArguments);
}

#[test]
fn trash_and_library_are_left_out_unless_asked() {
    let mut inv = demo_inventory();
    let trash = appearance::system_folder(&inv, 14).expect("trash");
    let thrown = Uuid::from_u128(99_001);
    let shelved = Uuid::from_u128(99_002);
    inv.add_items(vec![
        InvItem {
            id: thrown,
            parent: trash,
            name: "Zebra thrown".into(),
            asset_type: 7,
            owner: AGENT,
            ..Default::default()
        },
        InvItem {
            id: shelved,
            parent: inv.lib_root,
            name: "Zebra shelved".into(),
            asset_type: 7,
            owner: inv.lib_owner,
            ..Default::default()
        },
    ]);
    assert!(ids(&call(&inv, "inventory_search", json!({"query": "zebra"}))["results"]).is_empty());
    let all = call(
        &inv,
        "inventory_search",
        json!({"query": "zebra", "include_trash": true, "include_library": true}),
    );
    let mut found = ids(&all["results"]);
    found.sort();
    assert_eq!(found, vec![thrown, shelved]);
    let shelf = call(&inv, "inventory_search", json!({"query": "zebra", "path": "Library"}));
    assert_eq!(ids(&shelf["results"]), vec![shelved]);
    assert_eq!(shelf["results"][0]["path"], "Library");
}

#[test]
fn list_a_folder_by_id_path_or_root() {
    let inv = demo_inventory();
    let root = call(&inv, "inventory_list_folder", json!({}));
    assert_eq!(root["folder"]["id"], json!(inv.root));
    assert_eq!(root["folder"]["system"], "root");
    assert!(ids(&root["folders"]).contains(&DEMO_FOLDER));

    let demo = call(&inv, "inventory_list_folder", json!({"folder_id": DEMO_FOLDER.to_string()}));
    assert_eq!(ids(&demo["folders"]), vec![SUBFOLDER]);
    assert!(ids(&demo["items"]).contains(&TEXTURE));
    assert_eq!(demo["truncated"], false);

    let name = &inv.folders[&DEMO_FOLDER].info.name;
    let sub = &inv.folders[&SUBFOLDER].info.name;
    for path in [
        format!("{name}/{sub}"),
        format!("My Inventory/{name}/{sub}"),
        format!("/{}/{}/", name.to_lowercase(), sub),
    ] {
        let listed = call(&inv, "inventory_list_folder", json!({"path": path}));
        assert_eq!(listed["folder"]["id"], json!(SUBFOLDER), "{path}");
    }
    assert_eq!(
        fails(&inv, "inventory_list_folder", json!({"path": "Nowhere"})),
        ErrorCode::NotFound
    );
    assert_eq!(
        fails(&inv, "inventory_list_folder", json!({"folder_id": Uuid::from_u128(1).to_string()})),
        ErrorCode::NotFound
    );
    assert_eq!(
        fails(&inv, "inventory_list_folder", json!({"folder_id": "nope"})),
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        fails(
            &inv,
            "inventory_list_folder",
            json!({"folder_id": DEMO_FOLDER.to_string(), "path": "x"})
        ),
        ErrorCode::InvalidArguments
    );
}

#[test]
fn an_ambiguous_path_lists_its_candidates() {
    let mut inv = demo_inventory();
    let twin = Uuid::from_u128(99_010);
    let name = inv.folders[&DEMO_FOLDER].info.name.clone();
    inv.created_folders(vec![InvFolder {
        id: twin,
        parent: inv.root,
        name: name.clone(),
        type_default: -1,
        version: 1,
        ..Default::default()
    }]);
    let error = answer("inventory_list_folder", &json!({"path": name}), &ctx(&inv)).expect_err("ambiguous");
    assert_eq!(error.code, ErrorCode::Ambiguous);
    assert!(error.message.contains(&twin.to_string()) && error.message.contains(&DEMO_FOLDER.to_string()));
}

#[test]
fn item_details_hide_asset_ids_without_full_permissions() {
    let inv = demo_inventory();
    // the agent's own full-permission texture: shown, as in the viewer
    let texture = call(&inv, "inventory_item", json!({"item_id": TEXTURE.to_string()}));
    assert_eq!(texture["asset_id"], json!(inv.items[&TEXTURE].asset_id));
    assert_eq!(texture["asset_id_hidden"], false);
    assert_eq!(
        texture["permissions"]["you"],
        json!({"copy": true, "modify": true, "transfer": true})
    );
    for id in [NO_COPY, NO_TRANSFER] {
        let item = call(&inv, "inventory_item", json!({"item_id": id.to_string()}));
        assert!(item.get("asset_id").is_none(), "{item}");
        assert_eq!(item["asset_id_hidden"], true);
    }
    // a link gives its target, never an asset
    let link = call(&inv, "inventory_item", json!({"item_id": LINK.to_string()}));
    assert_eq!(link["link"]["target_id"], json!(OBJECT));
    assert_eq!(link["link"]["target_name"], inv.items[&OBJECT].name);
    assert!(link.get("asset_id").is_none());
    assert_eq!(
        fails(&inv, "inventory_item", json!({"item_id": DEMO_FOLDER.to_string()})),
        ErrorCode::InvalidArguments
    );
    assert_eq!(fails(&inv, "inventory_item", json!({})), ErrorCode::InvalidArguments);
    assert_eq!(
        fails(&inv, "inventory_item", json!({"item_id": Uuid::from_u128(5).to_string()})),
        ErrorCode::NotFound
    );
}

/// The rule for every kind of owner and mask (AGENTS.md § 2 "Tests").
#[test]
fn asset_ids_follow_the_viewer_rule_for_every_owner() {
    let mut inv = demo_inventory();
    let other = Uuid::from_u128(77);
    let full = actions::FULL | actions::MOVE;
    let cases = [
        // (owner, group owned, owner mask, everyone mask, shown)
        (AGENT, false, full, 0, true),
        (AGENT, false, full & !actions::COPY, 0, false),
        (AGENT, false, full & !actions::TRANSFER, 0, false),
        (AGENT, false, full & !actions::MODIFY, 0, false),
        (AGENT, true, full, 0, false),
        (other, false, full, 0, false),
        (other, false, full, actions::FULL, true),
        (inv.lib_owner, false, full, actions::COPY, false),
    ];
    for (n, (owner, group_owned, owner_mask, everyone_mask, shown)) in cases.into_iter().enumerate() {
        let id = Uuid::from_u128(99_100 + n as u128);
        let asset = Uuid::from_u128(99_200 + n as u128);
        inv.add_items(vec![InvItem {
            id,
            parent: if owner == inv.lib_owner { inv.lib_root } else { DEMO_FOLDER },
            name: format!("case {n}"),
            asset_type: 7,
            asset_id: asset,
            owner,
            group_owned,
            owner_mask,
            everyone_mask,
            ..Default::default()
        }]);
        let item = call(&inv, "inventory_item", json!({"item_id": id.to_string()}));
        assert_eq!(item.get("asset_id") == Some(&json!(asset)), shown, "case {n}: {item}");
        assert_eq!(item["asset_id_hidden"], !shown, "case {n}");
        assert_eq!(shown, actions::held(&inv, &inv.items[&id]) & actions::FULL == actions::FULL);
    }
}

#[test]
fn outfits_and_worn_items() {
    let inv = demo_inventory();
    let outfits = call(&inv, "outfits_list", json!({}));
    let list = outfits["outfits"].as_array().expect("outfits");
    assert_eq!(list.len(), 4, "{outfits}");
    let base: Vec<_> = list.iter().filter(|o| o["current_base"] == true).collect();
    assert_eq!(base.len(), 1);
    assert_eq!(base[0]["id"], json!(Uuid::from_u128(705)));

    let worn = call(&inv, "worn_items", json!({}));
    let entries = worn["worn"].as_array().expect("worn");
    assert!(!entries.is_empty());
    assert!(entries.iter().all(|e| e["worn"] == true));
    // body parts first, then clothing
    assert_eq!(entries[0]["type"], "body_part");
    assert!(entries.iter().any(|e| e["wearable"] == "shape"));
    assert!(entries.iter().any(|e| e["type"] == "clothing"));
}

#[test]
fn an_rlv_inventory_lock_refuses_reads_but_not_status() {
    let inv = demo_inventory();
    let mut cx = ctx(&inv);
    cx.inventory_locked = true;
    for tool in ["inventory_search", "inventory_list_folder", "outfits_list", "worn_items"] {
        assert_eq!(
            answer(tool, &json!({}), &cx).expect_err("locked").code,
            ErrorCode::PermissionDenied,
            "{tool}"
        );
    }
    let status = answer("viewer_status", &json!({}), &cx).expect("status");
    assert_eq!(status["inventory"]["locked_by_rlv"], true);
}

/// The sidecar's MCP layer → a real 127.0.0.1 socket → the bridge → these
/// tools on the demo inventory, as in the running viewer.
#[test]
fn an_assistant_reads_the_demo_inventory_end_to_end() {
    use folderstorm_mcp::connect::Viewer;
    use folderstorm_mcp::jsonrpc::{Backend, Server};
    use folderstorm_mcp_bridge::server::Config;
    use folderstorm_mcp_bridge::{Bridge, Permissions, ToolError};

    struct Live(Viewer);
    impl Backend for Live {
        fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
            self.0.call(tool, arguments)
        }
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = Bridge::start(Config {
        discovery_dir: Some(dir.path().to_path_buf()),
        pid: std::process::id(),
        version: "test".into(),
        permissions: Permissions::default(),
        viewer_timeout: std::time::Duration::from_secs(10),
    })
    .expect("bridge started");
    let folder = dir.path().to_path_buf();
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        let mut ask = |line: &str| -> Value { serde_json::from_str(&server.handle_line(line).expect("answer")).expect("json") };
        ask(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#);
        let status = ask(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"viewer_status"}}"#);
        let item = ask(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"inventory_item","arguments":{{"item_id":"{NO_COPY}"}}}}}}"#
        ));
        (status, item)
    });
    let inv = demo_inventory();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    // the frame loop: answer what waits, never block on it
    while !assistant.is_finished() && std::time::Instant::now() < deadline {
        while let Ok(pending) = calls.try_recv() {
            let result = answer(&pending.tool, &pending.arguments, &ctx(&inv));
            pending.answer(result);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let (status, item) = assistant.join().expect("assistant");
    assert_eq!(status["result"]["structuredContent"]["session"], "demo", "{status}");
    let item = &item["result"]["structuredContent"];
    assert_eq!(item["id"], json!(NO_COPY), "{item}");
    assert_eq!(item["asset_id_hidden"], true);
    assert_eq!(bridge.activity().len(), 2);
    let file = bridge.discovery_path().expect("discovery").to_path_buf();
    bridge.stop();
    assert!(!file.exists());
}
