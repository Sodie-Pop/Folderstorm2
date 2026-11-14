//! Folderstorm: calls bound to one login (folderstorm/plans/mcp-receipts.md,
//! section 2). The desk refuses a bound call its sidecar pinned to another
//! `viewer_session`; `viewer_status` answers on any login and gives the
//! session and the capabilities; and, end to end over a real socket, a
//! relog while the dialog is open refuses the question with
//! `session_changed` and the sidecar never sends it again.

use super::*;
use folderstorm_mcp_bridge::binding;

const S1: &str = "vs-11111111111111111111111111111111";
const S2: &str = "vs-22222222222222222222222222222222";

fn pinned(tool: &str, args: Value, session: Option<&str>) -> (Pending, Receiver<Result<Value, ToolError>>) {
    let (mut call, answer) = call(tool, args, false);
    call.session = session.map(str::to_owned);
    (call, answer)
}

#[test]
fn a_call_pinned_to_another_login_is_refused_when_taken() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    desk.set_login(1, Some(S1.to_owned()));
    let mut cx = ctx(&inv);
    cx.viewer_session = Some(S1.to_owned());
    // pinned to an older login
    let (a, answer) = pinned("worn_items", json!({}), Some(S2));
    desk.take(a, &cx, now);
    let e = answered(&answer).expect_err("refused");
    assert_eq!(e.code, ErrorCode::SessionChanged);
    assert_eq!(e.viewer_session.as_deref(), Some(S1), "the login now");
    let (b, answer) = pinned("inventory_create_folder", json!({"name": "New"}), Some(S2));
    desk.take(b, &cx, now);
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::SessionChanged);
    assert_eq!(desk.counts(), (0, 0, 0, 0), "nothing waits, nothing runs");
    // pinned to this login, or not pinned at all (an older sidecar)
    let (c, answer) = pinned("worn_items", json!({}), Some(S1));
    desk.take(c, &cx, now);
    assert!(answered(&answer).is_ok());
    let (d, answer) = pinned("worn_items", json!({}), None);
    desk.take(d, &cx, now);
    assert!(answered(&answer).is_ok());
    // viewer_status answers on any login, with the session and capabilities
    let (e, answer) = pinned("viewer_status", json!({}), Some(S2));
    desk.take(e, &cx, now);
    let status = answered(&answer).expect("status");
    assert_eq!(status["viewer_session"], S1);
    assert_eq!(status["capabilities"], json!(binding::capabilities()));
    // session routing: which character (the main profile, no name given)
    assert_eq!(
        (status["profile"].clone(), status["managed"].clone()),
        (json!("main"), json!(false))
    );
    assert_eq!(status["character"], Value::Null);
    cx.identity = folderstorm_mcp_bridge::server::Identity {
        profile: Some("Alt1".into()),
        managed: true,
        character: Some("Alt Resident".into()),
    };
    let (e2, answer) = pinned("viewer_status", json!({}), None);
    desk.take(e2, &cx, now);
    let status = answered(&answer).expect("status");
    assert_eq!(
        (status["profile"].clone(), status["managed"].clone(), status["character"].clone()),
        (json!("Alt1"), json!(true), json!("Alt Resident"))
    );
    // logged out: no session, and every pinned bound call is refused
    desk.set_login(2, None);
    cx.viewer_session = None;
    let (f, answer) = pinned("worn_items", json!({}), Some(S1));
    desk.take(f, &cx, now);
    let e = answered(&answer).expect_err("refused");
    assert_eq!((e.code, e.viewer_session), (ErrorCode::SessionChanged, None));
    let (g, answer) = pinned("viewer_status", json!({}), Some(S1));
    desk.take(g, &cx, now);
    assert_eq!(answered(&answer).expect("status")["viewer_session"], Value::Null);
}

/// The sidecar → a real 127.0.0.1 socket → the bridge (Ask before changes)
/// → the desk: the assistant's change waits in the dialog when the viewer
/// logs out and in again. The question is refused with `session_changed`,
/// the sidecar pins the new login and does not send the change again, and
/// the assistant's next call runs on the new login.
#[test]
fn a_relog_while_the_dialog_is_open_refuses_end_to_end_and_nothing_is_resent() {
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
    bridge.set_login(1, true);
    let first = bridge.viewer_session().expect("logged in");
    let folder = dir.path().to_path_buf();
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        let mut send = |line: &str| -> Value { serde_json::from_str(&server.handle_line(line).expect("answer")).expect("json") };
        send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code"}}}"#,
        );
        let status = send(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"viewer_status"}}"#);
        let created = send(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"inventory_create_folder","arguments":{"name":"Sorted"}}}"#,
        );
        let after = send(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"worn_items"}}"#);
        (status, created, after)
    });
    let inv = demo_inventory();
    let folders = inv.folders.len();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let mut generation = 1;
    let mut taken = Vec::new();
    let mut questions = 0;
    let mut runs = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    // the frame loop, as `mcp_bridge::frame` runs it
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        bridge.set_login(generation, true);
        let session = bridge.viewer_session();
        desk.set_login(generation, session.clone());
        let mut cx = ctx(&inv);
        cx.viewer_session = session;
        while let Ok(pending) = calls.try_recv() {
            taken.push(pending.tool.clone());
            desk.take(pending, &cx, now);
        }
        if desk.question(now).is_some() {
            questions += 1;
            // the user logs out and in again before answering
            generation += 2;
        }
        runs += desk.tick(&IDLE, now).len();
        std::thread::sleep(Duration::from_millis(5));
    }
    let (status, created, after) = assistant.join().expect("assistant");
    assert_eq!(status["result"]["structuredContent"]["viewer_session"], json!(first), "{status}");
    assert_eq!(created["result"]["isError"], true, "{created}");
    let text = created["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.starts_with("session_changed:"), "{text}");
    assert_eq!(after["result"]["isError"], false, "the next call runs on the new login: {after}");
    assert_eq!(questions, 1);
    assert_eq!(runs, 0, "nothing was sent");
    assert_eq!(
        taken,
        vec!["viewer_status", "inventory_create_folder", "worn_items"],
        "the refused change reached the desk once, never again"
    );
    assert_eq!(inv.folders.len(), folders, "no folder made");
    assert_ne!(bridge.viewer_session().as_deref(), Some(first.as_str()));
    bridge.stop();
}

/// Folderstorm: #27's world reads are answered before the desk takes any
/// call (`mcp_bridge::frame`); `Desk::admit` checks them as `take` would.
#[test]
fn a_world_read_is_checked_before_it_is_answered() {
    let mut desk = desk_with(allow_all());
    desk.set_login(1, Some(S1.to_owned()));
    let (a, answer) = pinned("world_location", json!({}), Some(S2));
    assert!(desk.admit(a).is_none());
    let e = answered(&answer).expect_err("refused");
    assert_eq!(e.code, ErrorCode::SessionChanged);
    assert_eq!(e.viewer_session.as_deref(), Some(S1), "the login now");
    let (b, _answer) = pinned("world_location", json!({}), Some(S1));
    assert!(desk.admit(b).is_some(), "this login");
    let (c, _answer) = pinned("world_location", json!({}), None);
    assert!(desk.admit(c).is_some(), "not pinned (an older sidecar)");
    // Read set to Never while the call was queued
    let mut never = allow_all();
    never.set(Class::Read, Level::Never);
    desk.set_permissions(never);
    let (d, answer) = pinned("world_location", json!({}), Some(S1));
    assert!(desk.admit(d).is_none());
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::PermissionDenied);
}
