//! Folderstorm: receipts (folderstorm/plans/mcp-receipts.md, section 4).
//! Every change the desk takes gets one, whatever its end; a
//! `client_request_id` runs once; `assistant_operation_status` reads them
//! back on this login only, with names only when Read is Allow; a change
//! the server confirms late updates its `unconfirmed` receipt; and, end to
//! end over a real socket, a retry with the same id runs nothing.

use super::*;
use folderstorm_mcp_bridge::receipts::{self, State};

const S1: &str = "vs-11111111111111111111111111111111";
const S2: &str = "vs-22222222222222222222222222222222";

fn status(desk: &mut Desk, inv: &Inventory, args: Value) -> Result<Value, ToolError> {
    let (call, answer) = call("assistant_operation_status", args, false);
    desk.take(call, &ctx(inv), Instant::now());
    answered(&answer)
}

fn receipt(desk: &mut Desk, inv: &Inventory, operation_id: &str) -> Value {
    status(desk, inv, json!({"operation_id": operation_id})).expect("found")["receipt"].clone()
}

/// The operation a call got, read from its receipt list (newest first).
fn newest(desk: &mut Desk, inv: &Inventory) -> Value {
    status(desk, inv, json!({})).expect("listed")["receipts"][0].clone()
}

#[test]
fn every_change_gets_a_receipt_whatever_its_end() {
    let inv = demo_inventory();
    let cx = ctx(&inv);
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let rename = json!({"id": TEXTURE.to_string(), "name": "Brick"});
    // asked, then refused in the dialog
    let (a, answer) = call("inventory_rename", rename.clone(), true);
    desk.take(a, &cx, now);
    let r = newest(&mut desk, &inv);
    assert_eq!(r["state"], "asking");
    assert!(r["summary"].as_str().is_some_and(|s| s.contains("Brick")), "{r}");
    assert_eq!(r["targets"], json!([TEXTURE.to_string()]));
    assert_eq!(r["client"], "Claude Code");
    let q = desk.question(now).expect("asked");
    desk.decide(q.number, false);
    desk.tick(&IDLE, now);
    let e = answered(&answer).expect_err("refused");
    let operation = e.operation_id.clone().expect("the error names its operation");
    assert!(receipts::is_operation_id(&operation));
    let r = receipt(&mut desk, &inv, &operation);
    assert_eq!(
        (r["state"].as_str(), r["decision"].as_str()),
        (Some("refused"), Some("refused_by_user"))
    );
    // asked, allowed, sent, confirmed
    let (b, answer) = call("inventory_rename", rename.clone(), true);
    desk.take(b, &cx, now);
    let q = desk.question(now).expect("asked");
    desk.decide(q.number, true);
    let runs = desk.tick(&IDLE, now);
    assert_eq!(runs.len(), 1);
    let run = runs.into_iter().next().expect("run");
    let operation = run.operation_id.clone().expect("an operation");
    assert_eq!(receipt(&mut desk, &inv, &operation)["state"], "queued");
    let p = planned(&inv, "inventory_rename", rename.clone());
    desk.begin(run, &p, false, now);
    let request = Uuid::from_u128(7);
    desk.sent(Some(request), None);
    let r = receipt(&mut desk, &inv, &operation);
    assert_eq!(r["state"], "running");
    assert!(r["times_ms"]["sent"].as_u64().is_some());
    desk.observe((request, false, Outcome::Done(HashMap::new())));
    let reply = answered(&answer).expect("done");
    assert_eq!(reply["operation_id"], json!(operation));
    assert_eq!(reply["state"], "done");
    let r = receipt(&mut desk, &inv, &operation);
    assert_eq!(
        (r["state"].as_str(), r["decision"].as_str(), r["server"].as_str()),
        (Some("done"), Some("approved"), Some("confirmed"))
    );
    assert_eq!(r["result"], json!([TEXTURE.to_string()]));
    // refused by the viewer's rules, before anyone is asked
    let (c, answer) = call("inventory_trash", json!({"ids": [COF.to_string()]}), true);
    desk.take(c, &cx, now);
    let e = answered(&answer).expect_err("not allowed");
    assert_eq!(e.code, ErrorCode::NotAllowed);
    let r = receipt(&mut desk, &inv, e.operation_id.as_deref().expect("named"));
    assert_eq!(
        (r["state"].as_str(), r["decision"].as_str()),
        (Some("refused"), Some("refused_by_viewer"))
    );
    // the dialog's 60 seconds
    let (d, answer) = call("inventory_rename", rename, true);
    desk.take(d, &cx, now);
    desk.tick(&IDLE, now + Duration::from_secs(60));
    let e = answered(&answer).expect_err("timed out");
    let r = receipt(&mut desk, &inv, e.operation_id.as_deref().expect("named"));
    assert_eq!(r["decision"], "ask_timed_out");
    // reads get none
    let (e, answer) = call("worn_items", json!({}), false);
    desk.take(e, &cx, now);
    assert!(answered(&answer).expect("read").get("operation_id").is_none());
    assert_eq!(
        status(&mut desk, &inv, json!({})).expect("listed")["receipts"]
            .as_array()
            .map(Vec::len),
        Some(4)
    );
}

#[test]
fn a_client_request_id_runs_once() {
    let inv = demo_inventory();
    let cx = ctx(&inv);
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let args = json!({"id": TEXTURE.to_string(), "name": "Brick", "client_request_id": "rename-1"});
    let (a, first) = call("inventory_rename", args.clone(), false);
    desk.take(a, &cx, now);
    // the same again while the first waits for its turn: its receipt
    let (b, again) = call("inventory_rename", args.clone(), false);
    desk.take(b, &cx, now);
    let reply = answered(&again).expect("the first receipt");
    assert_eq!(reply["duplicate"], true);
    assert_eq!(reply["state"], "queued");
    let runs = desk.tick(&IDLE, now);
    assert_eq!(runs.len(), 1, "one run for two calls");
    let run = runs.into_iter().next().expect("run");
    assert_eq!(run.arguments.get("client_request_id"), None, "the tool never sees it");
    let operation = run.operation_id.clone().expect("an operation");
    assert_eq!(reply["operation_id"], json!(operation));
    let p = planned(&inv, "inventory_rename", run.arguments.clone());
    desk.begin(run, &p, false, now);
    desk.observe((Uuid::from_u128(1), false, Outcome::Done(HashMap::new())));
    desk.sent(Some(Uuid::from_u128(1)), None);
    assert_eq!(answered(&first).expect("done")["state"], "done");
    // after the end: the receipt, done, and nothing runs
    let (c, again) = call("inventory_rename", args, false);
    desk.take(c, &cx, now);
    let reply = answered(&again).expect("the receipt");
    assert_eq!((reply["state"].as_str(), reply["duplicate"].as_bool()), (Some("done"), Some(true)));
    assert_eq!(reply["receipt"]["client_request_id"], "rename-1");
    assert!(desk.tick(&IDLE, now).is_empty());
    // other arguments with the same id: conflict, nothing made
    let (d, other) = call(
        "inventory_rename",
        json!({"id": TEXTURE.to_string(), "name": "Stone", "client_request_id": "rename-1"}),
        false,
    );
    desk.take(d, &cx, now);
    assert_eq!(answered(&other).expect_err("conflict").code, ErrorCode::Conflict);
    // a malformed id
    let (e, bad) = call(
        "inventory_rename",
        json!({"id": TEXTURE.to_string(), "name": "Stone", "client_request_id": ""}),
        false,
    );
    desk.take(e, &cx, now);
    assert_eq!(answered(&bad).expect_err("invalid").code, ErrorCode::InvalidArguments);
    assert!(desk.tick(&IDLE, now).is_empty());
    // the status finds it by the assistant's own id
    let found = status(&mut desk, &inv, json!({"client_request_id": "rename-1"})).expect("found");
    assert_eq!(found["receipt"]["operation_id"], json!(operation));
}

#[test]
fn the_status_reads_this_login_only_and_names_only_with_read() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    desk.set_login(1, Some(S1.to_owned()));
    let (a, _answer) = call("inventory_rename", json!({"id": TEXTURE.to_string(), "name": "Brick"}), false);
    desk.take(a, &ctx(&inv), now);
    let r = newest(&mut desk, &inv);
    let operation = r["operation_id"].as_str().expect("an operation").to_owned();
    assert_eq!(r["viewer_session"], S1);
    assert!(r["summary"].as_str().is_some_and(|s| s.contains("Brick")));
    // Read on Never: the status still works, without names
    let mut no_read = allow_all();
    no_read.set(Class::Read, Level::Never);
    desk.set_permissions(no_read);
    let r = receipt(&mut desk, &inv, &operation);
    assert!(r.get("summary").is_none(), "{r}");
    assert!(!r.to_string().contains("Brick"));
    desk.set_permissions(allow_all());
    // unknown and doubled arguments
    for bad in [json!({"other": 1}), json!({"operation_id": operation, "client_request_id": "x"})] {
        assert_eq!(
            status(&mut desk, &inv, bad.clone()).expect_err("refused").code,
            ErrorCode::InvalidArguments,
            "{bad}"
        );
    }
    assert_eq!(
        status(&mut desk, &inv, json!({"operation_id": "op-0"})).expect_err("unknown").code,
        ErrorCode::NotFound
    );
    // pinned to another login: refused as every bound call
    let (b, answer) = call("assistant_operation_status", json!({"operation_id": operation}), false);
    let mut b = b;
    b.session = Some(S2.to_owned());
    desk.take(b, &ctx(&inv), now);
    assert_eq!(answered(&answer).expect_err("other login").code, ErrorCode::SessionChanged);
    // another login: the old receipts are gone, nothing tells they existed
    desk.set_login(2, Some(S2.to_owned()));
    assert_eq!(
        status(&mut desk, &inv, json!({"operation_id": operation})).expect_err("gone").code,
        ErrorCode::NotFound
    );
    assert_eq!(status(&mut desk, &inv, json!({})).expect("listed")["receipts"], json!([]));
}

#[test]
fn no_asset_id_in_any_receipt() {
    let inv = demo_inventory();
    let cx = ctx(&inv);
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    // full-permission items among them: their asset ids are shown by
    // inventory_item, never kept by a receipt
    let calls = [
        ("inventory_rename", json!({"id": TEXTURE.to_string(), "name": "Brick"})),
        (
            "inventory_describe",
            json!({"item_id": OBJECT.to_string(), "description": "A crate"}),
        ),
        (
            "inventory_move",
            json!({"ids": [TEXTURE.to_string(), OBJECT.to_string()], "folder_id": SUBFOLDER.to_string()}),
        ),
        (
            "inventory_set_thumbnail",
            json!({"id": DEMO_FOLDER.to_string(), "texture_id": TEXTURE.to_string()}),
        ),
        ("inventory_trash", json!({"ids": [NO_COPY.to_string(), NO_TRANSFER.to_string()]})),
    ];
    for (tool, args) in calls {
        let (c, _answer) = call(tool, args, false);
        desk.take(c, &cx, now);
    }
    for run in desk.tick(&IDLE, now) {
        run.answer(Ok(json!({"done": true, "asset_id": inv.items[&TEXTURE].asset_id.to_string()})));
    }
    let all = status(&mut desk, &inv, json!({})).expect("listed");
    assert_eq!(all["receipts"].as_array().map(Vec::len), Some(5), "{all}");
    let text = all.to_string();
    // (a link's asset id is its original's inventory id: not an asset)
    let assets = inv
        .items
        .values()
        .filter(|it| !it.asset_id.is_nil() && !inv.items.contains_key(&it.asset_id) && !inv.folders.contains_key(&it.asset_id));
    for item in assets {
        assert!(!text.contains(&item.asset_id.to_string()), "{}'s asset id in {text}", item.name);
    }
}

#[test]
fn a_late_answer_updates_an_unconfirmed_receipt() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let args = json!({"name": "New"});
    let p = planned(&inv, "inventory_create_folder", args.clone());
    let (a, answer) = call("inventory_create_folder", args.clone(), false);
    desk.take(a, &ctx(&inv), now);
    let run = desk.tick(&IDLE, now).into_iter().next().expect("run");
    desk.begin(run, &p, false, now);
    let request = Uuid::from_u128(40);
    desk.sent(Some(request), None);
    desk.tick(&IDLE, now + Duration::from_secs(30));
    let e = answered(&answer).expect_err("not confirmed in time");
    assert_eq!(e.code, ErrorCode::Timeout);
    let operation = e.operation_id.clone().expect("named");
    let r = receipt(&mut desk, &inv, &operation);
    assert_eq!(
        (r["state"].as_str(), r["server"].as_str()),
        (Some("unconfirmed"), Some("no_answer"))
    );
    assert_eq!(desk.late_count(), 1, "still listening");
    // the server answers two minutes later
    desk.tick(&IDLE, now + Duration::from_secs(150));
    desk.observe((request, false, Outcome::Done(HashMap::new())));
    let r = receipt(&mut desk, &inv, &operation);
    assert_eq!((r["state"].as_str(), r["server"].as_str()), (Some("done"), Some("confirmed")));
    assert_eq!(desk.late_count(), 0);
    // one that never answers stops listening after 10 minutes
    let (b, _answer) = call("inventory_create_folder", args, false);
    desk.take(b, &ctx(&inv), now);
    let run = desk.tick(&IDLE, now).into_iter().next().expect("run");
    desk.begin(run, &p, false, now);
    desk.sent(Some(Uuid::from_u128(41)), None);
    desk.tick(&IDLE, now + Duration::from_secs(30));
    assert_eq!(desk.late_count(), 1);
    desk.tick(&IDLE, now + Duration::from_secs(30) + receipts::LATE_WAIT);
    assert_eq!(desk.late_count(), 0);
    assert_eq!(newest(&mut desk, &inv)["state"], "unconfirmed");
}

#[test]
fn moves_the_grid_queues_are_never_done() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let args = json!({"ids": [TEXTURE.to_string()], "folder_id": SUBFOLDER.to_string()});
    let p = planned(&inv, "inventory_move", args.clone());
    let (a, answer) = call("inventory_move", args, false);
    desk.take(a, &ctx(&inv), now);
    let run = desk.tick(&IDLE, now).into_iter().next().expect("run");
    desk.begin(run, &p, false, now);
    desk.sent(Some(Uuid::from_u128(50)), None);
    desk.observe((Uuid::from_u128(50), false, Outcome::Queued));
    let reply = answered(&answer).expect("accepted for later");
    assert_eq!((reply["done"].as_bool(), reply["queued"].as_bool()), (Some(false), Some(true)));
    assert_eq!(reply["state"], "unconfirmed");
    let r = receipt(&mut desk, &inv, reply["operation_id"].as_str().expect("named"));
    assert_eq!(r["server"], "queued");
}

#[test]
fn access_off_keeps_the_receipts_and_a_login_change_clears_them() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    desk.set_login(1, Some(S1.to_owned()));
    let (a, answer) = call("inventory_rename", json!({"id": TEXTURE.to_string(), "name": "Brick"}), true);
    desk.take(a, &ctx(&inv), now);
    desk.close();
    let e = answered(&answer).expect_err("access off");
    let operation = e.operation_id.expect("named");
    desk.set_permissions(Preset::AskBeforeChanges.permissions());
    desk.set_login(1, Some(S1.to_owned()));
    let kept = desk.ledger().with(|store| store.find(Some(S1), &operation).map(|r| r.state));
    assert_eq!(kept, Some(State::Refused), "Preferences still shows it");
    desk.set_login(2, None);
    assert_eq!(desk.ledger().with(|store| store.all().count()), 0);
}

/// The sidecar → a real 127.0.0.1 socket → the bridge → the desk → the
/// demo inventory: a change with a `client_request_id` runs; the same call
/// again (an assistant retrying after a lost answer) answers its receipt
/// and runs nothing; `assistant_operation_status` gives it.
#[test]
fn a_retry_with_the_same_client_request_id_runs_once_end_to_end() {
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
        permissions: allow_all(),
        viewer_timeout: Duration::from_secs(10),
    })
    .expect("bridge started");
    bridge.set_login(1, true);
    let folder = dir.path().to_path_buf();
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        let mut send = |line: &str| -> Value { serde_json::from_str(&server.handle_line(line).expect("answer")).expect("json") };
        send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code"}}}"#,
        );
        let create = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"inventory_create_folder","arguments":{"name":"Sorted","client_request_id":"make-sorted"}}}"#;
        let first = send(create);
        let again = send(create);
        let status = send(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"assistant_operation_status","arguments":{"client_request_id":"make-sorted"}}}"#,
        );
        (first, again, status)
    });
    let mut inv = demo_inventory();
    let folders = inv.folders.len();
    let mut desk = desk_with(allow_all());
    let mut runs = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        let session = bridge.viewer_session();
        desk.set_login(1, session.clone());
        let mut cx = ctx(&inv);
        cx.viewer_session = session;
        while let Ok(pending) = calls.try_recv() {
            desk.take(pending, &cx, now);
        }
        for run in desk.tick(&IDLE, now) {
            runs += 1;
            let planned = plan(&run.tool, &run.arguments, &ctx(&inv)).expect("planned");
            let Work::Inventory(change) = planned.work.clone() else {
                panic!("an inventory change")
            };
            desk.begin(run, &planned, false, now);
            let request = Uuid::new_v4();
            let reply = actions::demo_mutate(&mut inv, AGENT, change).expect("applied");
            desk.observe((request, false, Outcome::Done(reply.created)));
            desk.sent(Some(request), None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let (first, again, status) = assistant.join().expect("assistant");
    let first = &first["result"]["structuredContent"];
    let operation = first["operation_id"].as_str().expect("an operation").to_owned();
    assert_eq!(first["state"], "done", "{first}");
    let again = &again["result"]["structuredContent"];
    assert_eq!(again["duplicate"], true, "{again}");
    assert_eq!(again["operation_id"], json!(operation));
    assert_eq!(runs, 1, "sent once");
    assert_eq!(inv.folders.len(), folders + 1, "one folder made");
    let receipt = &status["result"]["structuredContent"]["receipt"];
    assert_eq!(receipt["operation_id"], json!(operation), "{status}");
    assert_eq!(receipt["state"], "done");
    assert_eq!(receipt["viewer_session"], json!(bridge.viewer_session()));
    // Preferences: the activity line names the operation
    assert!(
        bridge
            .activity()
            .iter()
            .any(|a| a.operation_id.as_deref() == Some(operation.as_str()))
    );
    bridge.stop();
}
