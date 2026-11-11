//! Folderstorm: a waiting call is decided again with the permissions and
//! the login in force (folderstorm/plans/mcp-receipts.md, section 1): when
//! the desk takes it, on every frame, when Allow is clicked and just before
//! the send. Nothing more is sent once a class it needs is on Never, the
//! login changed or access was turned off.

use super::*;
use crate::mcp_bridge::desk::{ACCESS_OFF, LOGIN_CHANGED, PERMISSIONS_CHANGED};

fn with_level(mut permissions: Permissions, class: Class, level: Level) -> Permissions {
    permissions.set(class, level);
    permissions
}

fn ask() -> Permissions {
    Preset::AskBeforeChanges.permissions()
}

fn rename() -> Value {
    json!({"id": TEXTURE.to_string(), "name": "Brick"})
}

fn move_args() -> Value {
    json!({"ids": [TEXTURE.to_string()], "folder_id": SUBFOLDER.to_string()})
}

const S1: &str = "vs-11111111111111111111111111111111";
const S2: &str = "vs-22222222222222222222222222222222";

fn s(session: &str) -> Option<String> {
    Some(session.to_owned())
}

/// Refused because the login changed while it waited, naming the login now.
fn login_changed(answer: &Receiver<Result<Value, ToolError>>, now: Option<&str>) {
    let e = answered(answer).expect_err("refused");
    assert_eq!(e.code, ErrorCode::SessionChanged, "{e}");
    assert_eq!(e.message, LOGIN_CHANGED);
    assert_eq!(e.viewer_session.as_deref(), now);
}

/// Refused, with this message.
fn refused_with(answer: &Receiver<Result<Value, ToolError>>, message: &str) {
    let e = answered(answer).expect_err("refused");
    assert_eq!(e.code, ErrorCode::PermissionDenied, "{e}");
    assert_eq!(e.message, message);
}

#[test]
fn ask_then_never_with_the_dialog_open_refuses_even_after_allow() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(ask());
    let (a, shown) = call("inventory_rename", rename(), true);
    let (b, queued) = call("inventory_create_folder", json!({"name": "Queued"}), true);
    desk.take(a, &ctx(&inv), now);
    desk.take(b, &ctx(&inv), now);
    let q = desk.question(now).expect("the dialog");
    assert_eq!(q.waiting, 1);
    // Edit set to Never in Preferences, then Allow clicked in the dialog
    desk.set_permissions(with_level(ask(), Class::Edit, Level::Never));
    desk.decide(q.number, true);
    refused_with(&shown, PERMISSIONS_CHANGED);
    assert!(desk.tick(&IDLE, now).is_empty(), "nothing is sent");
    refused_with(&queued, PERMISSIONS_CHANGED);
    assert!(desk.question(now).is_none(), "the dialog closed");
    assert_eq!(desk.counts(), (0, 0, 0, 0));
}

#[test]
fn never_closes_an_open_dialog_on_the_next_frame() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(ask());
    let (a, answer) = call("inventory_trash", json!({"ids": [TEXTURE.to_string()]}), true);
    desk.take(a, &ctx(&inv), now);
    assert!(desk.question(now).is_some());
    // a class it does not need changes nothing
    desk.set_permissions(with_level(ask(), Class::Edit, Level::Never));
    assert!(desk.tick(&IDLE, now).is_empty());
    assert!(answer.try_recv().is_err());
    assert!(desk.question(now).is_some(), "still asking");
    // its own class does
    desk.set_permissions(with_level(ask(), Class::Trash, Level::Never));
    assert!(desk.tick(&IDLE, now).is_empty());
    refused_with(&answer, PERMISSIONS_CHANGED);
    assert!(desk.question(now).is_none());
    // a click on the closed dialog does nothing
    desk.decide(1, true);
    assert!(desk.tick(&IDLE, now).is_empty());
}

#[test]
fn a_call_queued_under_allow_is_refused_after_never() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    // admitted by the listener under Allow, still in the channel
    let (a, answer) = call("inventory_move", move_args(), false);
    desk.set_permissions(with_level(allow_all(), Class::Move, Level::Never));
    desk.take(a, &ctx(&inv), now);
    refused_with(&answer, PERMISSIONS_CHANGED);
    assert!(desk.tick(&IDLE, now).is_empty());
    // taken, then Never before the frame runs it
    let mut desk = desk_with(allow_all());
    let (b, answer) = call("inventory_move", move_args(), false);
    desk.take(b, &ctx(&inv), now);
    desk.set_permissions(with_level(allow_all(), Class::Move, Level::Never));
    assert!(desk.tick(&IDLE, now).is_empty());
    refused_with(&answer, PERMISSIONS_CHANGED);
    // reads too: Read set to Never while a read or a bake wait waits
    let (c, answer) = call("worn_items", json!({}), false);
    let (d, bake) = call("wait_for_bake", json!({}), false);
    desk.set_permissions(with_level(allow_all(), Class::Read, Level::Never));
    desk.take(c, &ctx(&inv), now);
    refused_with(&answer, PERMISSIONS_CHANGED);
    desk.set_permissions(allow_all());
    desk.take(d, &ctx(&inv), now);
    desk.set_permissions(with_level(allow_all(), Class::Read, Level::Never));
    desk.tick(
        &Bake {
            outfit_change_pending: true,
            ..IDLE
        },
        now,
    );
    refused_with(&bake, PERMISSIONS_CHANGED);
    assert_eq!(desk.counts(), (0, 0, 0, 0));
}

#[test]
fn allow_then_ask_asks_a_call_not_asked_yet() {
    let inv = demo_inventory();
    let now = Instant::now();
    // still in the channel when the class goes to Ask
    let mut desk = desk_with(with_level(allow_all(), Class::Move, Level::Ask));
    let (a, answer) = call("inventory_move", move_args(), false);
    desk.take(a, &ctx(&inv), now);
    assert!(desk.tick(&IDLE, now).is_empty(), "not run");
    let q = desk.question(now).expect("asks");
    assert!(answer.try_recv().is_err());
    desk.decide(q.number, true);
    assert_eq!(desk.tick(&IDLE, now).len(), 1, "allowed: runs");
    // taken under Allow, Ask before the frame runs it
    let mut desk = desk_with(allow_all());
    let (b, _answer) = call("inventory_move", move_args(), false);
    desk.take(b, &ctx(&inv), now);
    desk.set_permissions(with_level(allow_all(), Class::Move, Level::Ask));
    assert!(desk.tick(&IDLE, now).is_empty());
    let q = desk.question(now).expect("asks");
    assert!(q.summary.contains(&inv.folders[&SUBFOLDER].info.name), "{}", q.summary);
}

#[test]
fn ask_then_allow_with_the_dialog_open_still_asks() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(ask());
    let (a, answer) = call("inventory_rename", rename(), true);
    desk.take(a, &ctx(&inv), now);
    desk.set_permissions(allow_all());
    assert!(desk.tick(&IDLE, now).is_empty(), "an Allow never answers a question");
    assert!(answer.try_recv().is_err());
    let q = desk.question(now).expect("still asking");
    // the same for a call admitted under Ask, taken after Allow
    let (b, _) = call("inventory_rename", rename(), true);
    desk.take(b, &ctx(&inv), now);
    assert_eq!(desk.question(now).expect("asking").waiting, 1);
    desk.decide(q.number, false);
    assert!(desk.tick(&IDLE, now).is_empty());
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::PermissionDenied);
}

#[test]
fn a_login_change_refuses_every_waiting_call() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(ask());
    desk.set_login(1, s(S1));
    // a change already sent before the relog
    let args = json!({"name": "New"});
    let p = planned(&inv, "inventory_create_folder", args.clone());
    let (sent, sent_answer) = call("inventory_create_folder", args.clone(), false);
    desk.begin(sent, &p, false, now);
    desk.sent(Some(Uuid::from_u128(1)), None);
    // a dialog for a folder at the root of My Inventory, and a bake wait
    let (a, dialog) = call("inventory_create_folder", args, true);
    let (b, bake) = call("wait_for_bake", json!({}), false);
    desk.take(a, &ctx(&inv), now);
    desk.take(b, &ctx(&inv), now);
    let q = desk.question(now).expect("the dialog");
    // logged out (or in as someone else): the next frame gives a new generation
    desk.set_login(2, None);
    login_changed(&dialog, None);
    login_changed(&bake, None);
    assert!(desk.question(now).is_none(), "the dialog closed");
    // a late click on the old dialog sends nothing
    desk.decide(q.number, true);
    assert!(desk.tick(&IDLE, now).is_empty());
    // the change already sent is not undone: it still waits for its answer
    assert_eq!(desk.counts(), (0, 0, 1, 0));
    assert!(sent_answer.try_recv().is_err());
}

#[test]
fn access_turned_off_refuses_and_reports_what_was_sent() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(ask());
    let args = json!({"name": "New"});
    let p = planned(&inv, "inventory_create_folder", args.clone());
    let (sent, sent_answer) = call("inventory_create_folder", args.clone(), false);
    desk.begin(sent, &p, false, now);
    desk.sent(Some(Uuid::from_u128(1)), None);
    let (a, dialog) = call("inventory_create_folder", args, true);
    desk.take(a, &ctx(&inv), now);
    desk.close();
    refused_with(&dialog, ACCESS_OFF);
    let e = answered(&sent_answer).expect_err("may still land");
    assert_eq!(e.code, ErrorCode::Timeout);
    assert_eq!(desk.counts(), (0, 0, 0, 0));
    assert!(desk.question(now).is_none());
}

#[test]
fn the_last_check_before_the_send() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    desk.set_login(1, s(S1));
    let run = |desk: &mut Desk, ask: bool| {
        let (a, answer) = call("inventory_move", move_args(), ask);
        desk.take(a, &ctx(&inv), now);
        let mut runs = desk.tick(&IDLE, now);
        assert_eq!(runs.len(), 1);
        (runs.remove(0), answer)
    };
    // nothing changed: sent
    let (a, _) = run(&mut desk, false);
    assert!(desk.recheck(a, 1, s(S1), &ctx(&inv), now).is_some());
    // Never by now
    let (b, answer) = run(&mut desk, false);
    desk.set_permissions(with_level(allow_all(), Class::Move, Level::Never));
    assert!(desk.recheck(b, 1, s(S1), &ctx(&inv), now).is_none());
    refused_with(&answer, PERMISSIONS_CHANGED);
    // Ask by now, not asked yet: back to the dialog
    desk.set_permissions(allow_all());
    let (c, answer) = run(&mut desk, false);
    desk.set_permissions(with_level(allow_all(), Class::Move, Level::Ask));
    assert!(desk.recheck(c, 1, s(S1), &ctx(&inv), now).is_none());
    assert!(answer.try_recv().is_err());
    assert!(desk.question(now).is_some(), "asks");
    // allowed in the dialog: Ask does not ask twice
    let q = desk.question(now).expect("asks");
    desk.decide(q.number, true);
    let mut runs = desk.tick(&IDLE, now);
    assert!(desk.recheck(runs.remove(0), 1, s(S1), &ctx(&inv), now).is_some());
    // another login by now
    desk.set_permissions(allow_all());
    let (d, answer) = run(&mut desk, false);
    assert!(desk.recheck(d, 2, s(S2), &ctx(&inv), now).is_none());
    login_changed(&answer, Some(S2));
}

/// The sidecar → a real 127.0.0.1 socket → the bridge (Ask before
/// changes) → the desk: Move set to Never while the dialog is open, then
/// Allow clicked. The assistant is refused and the inventory is untouched.
#[test]
fn never_while_the_dialog_is_open_refuses_end_to_end() {
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
        permissions: ask(),
        viewer_timeout: Duration::from_secs(10),
    })
    .expect("bridge started");
    let folder = dir.path().to_path_buf();
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        let mut send = |line: &str| -> Value { serde_json::from_str(&server.handle_line(line).expect("answer")).expect("json") };
        send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code"}}}"#,
        );
        let moved = send(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"inventory_move","arguments":{{"ids":["{TEXTURE}"],"folder_id":"{SUBFOLDER}"}}}}}}"#
        ));
        // asked again now that Move is on Never: refused by the listener
        let again = send(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"inventory_move","arguments":{{"ids":["{TEXTURE}"],"folder_id":"{SUBFOLDER}"}}}}}}"#
        ));
        (moved, again)
    });
    let inv = demo_inventory();
    let before = inv.items[&TEXTURE].parent;
    assert_ne!(before, SUBFOLDER);
    let mut desk = desk_with(ask());
    let mut questions = 0;
    let mut runs = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    // the frame loop, as `mcp_bridge::frame` runs it
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        desk.set_permissions(bridge.permissions());
        while let Ok(pending) = calls.try_recv() {
            desk.take(pending, &ctx(&inv), now);
        }
        if let Some(q) = desk.question(now) {
            questions += 1;
            // the user sets Move to Never in Preferences, then clicks Allow
            let never = with_level(ask(), Class::Move, Level::Never);
            bridge.set_permissions(never);
            desk.set_permissions(never);
            desk.decide(q.number, true);
        }
        runs += desk.tick(&IDLE, now).len();
        std::thread::sleep(Duration::from_millis(5));
    }
    let (moved, again) = assistant.join().expect("assistant");
    assert_eq!(moved["result"]["isError"], true, "{moved}");
    let text = moved["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("changed the permissions"), "{text}");
    assert_eq!(again["result"]["isError"], true, "{again}");
    assert_eq!(questions, 1);
    assert_eq!(runs, 0, "nothing was sent");
    assert_eq!(inv.items[&TEXTURE].parent, before, "not moved");
    bridge.stop();
}
