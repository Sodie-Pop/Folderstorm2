use super::*;
use crate::client::{Client, ClientError};
use crate::policy::{Level, Preset};
use std::io::{Read, Write};

fn start(timeout: Duration, dir: Option<PathBuf>) -> (Bridge, Receiver<Pending>) {
    Bridge::start(Config {
        discovery_dir: dir,
        pid: std::process::id(),
        version: "test".into(),
        permissions: Permissions::default(),
        viewer_timeout: timeout,
    })
    .expect("bridge started")
}

/// Answer every call with its tool name until the bridge stops.
fn responder(calls: Receiver<Pending>) -> std::thread::JoinHandle<usize> {
    std::thread::spawn(move || {
        let mut answered = 0;
        while let Ok(call) = calls.recv() {
            let tool = call.tool.clone();
            call.answer(Ok(json!({"tool": tool})));
            answered += 1;
        }
        answered
    })
}

fn raw(port: u16, request: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connected");
    stream.write_all(request.as_bytes()).expect("sent");
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).expect("read");
    let text = String::from_utf8(answer).expect("utf-8");
    let status = text[9..12].parse().expect("status");
    let body = text.split_once("\r\n\r\n").expect("body").1;
    (status, serde_json::from_str(body).expect("json"))
}

fn code(e: ClientError) -> ErrorCode {
    match e {
        ClientError::Tool(e) => e.code,
        other => panic!("not a tool error: {other}"),
    }
}

#[test]
fn listens_on_loopback_and_answers_a_call() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let viewer = responder(calls);
    let client = Client::new(bridge.port(), bridge.token().clone());
    let health = client.health().expect("health");
    assert_eq!(health.api, crate::API_VERSION);
    assert!(health.read_allowed);
    assert_eq!(
        client.call("worn_items", json!({})).expect("answered"),
        json!({"tool": "worn_items"})
    );
    let activity = bridge.activity();
    assert_eq!(activity.len(), 1, "health is not activity");
    assert_eq!(activity[0].tool, "worn_items");
    assert_eq!(activity[0].outcome, Outcome::Done);
    bridge.stop();
    assert_eq!(viewer.join().expect("responder"), 1);
}

#[test]
fn a_wrong_or_missing_token_is_refused() {
    let (bridge, _calls) = start(VIEWER_TIMEOUT, None);
    let wrong = Client::new(bridge.port(), Token::generate().expect("random"));
    assert_eq!(code(wrong.health().expect_err("refused")), ErrorCode::Unauthorized);
    let port = bridge.port();
    let (status, body) = raw(port, &format!("GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"));
    assert_eq!(status, 401);
    assert_eq!(body["error"]["code"], "unauthorized");
    assert!(bridge.activity().is_empty());
}

#[test]
fn browsers_and_foreign_hosts_are_refused() {
    let (bridge, _calls) = start(VIEWER_TIMEOUT, None);
    let port = bridge.port();
    let token = bridge.token().secret().to_owned();
    let (status, _) = raw(
        port,
        &format!(
            "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: https://example.com\r\nAuthorization: Bearer {token}\r\n\r\n"
        ),
    );
    assert_eq!(status, 403);
    let (status, _) = raw(
        port,
        &format!("GET /v1/health HTTP/1.1\r\nHost: evil.example:{port}\r\nAuthorization: Bearer {token}\r\n\r\n"),
    );
    assert_eq!(status, 403);
    let (status, _) = raw(
        port,
        &format!("GET /v1/health HTTP/1.1\r\nHost: localhost:{port}\r\nAuthorization: Bearer {token}\r\n\r\n"),
    );
    assert_eq!(status, 200);
    let (status, _) = raw(
        port,
        &format!("GET /other HTTP/1.1\r\nHost: localhost:{port}\r\nAuthorization: Bearer {token}\r\n\r\n"),
    );
    assert_eq!(status, 404);
}

#[test]
fn oversized_requests_are_refused() {
    let (bridge, _calls) = start(VIEWER_TIMEOUT, None);
    let port = bridge.port();
    let (status, body) = raw(
        port,
        &format!(
            "POST /v1/call HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\n\r\n",
            http::MAX_REQUEST_BODY + 1
        ),
    );
    assert_eq!(status, 413);
    assert_eq!(body["error"]["code"], "too_large");
}

#[test]
fn reads_set_to_never_are_refused_before_reaching_the_viewer() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let mut permissions = Preset::ReadOnly.permissions();
    permissions.set(Class::Read, Level::Never);
    bridge.set_permissions(permissions);
    let client = Client::new(bridge.port(), bridge.token().clone());
    assert_eq!(
        code(client.call("inventory_search", json!({"query": "x"})).expect_err("refused")),
        ErrorCode::PermissionDenied
    );
    assert!(!client.health().expect("health still works").read_allowed);
    assert!(calls.try_recv().is_err(), "nothing reached the viewer");
    assert_eq!(bridge.activity()[0].outcome, Outcome::Refused);
    // the health tool needs no class
    let viewer = responder(calls);
    assert!(client.call("viewer_status", json!({})).is_ok());
    bridge.stop();
    viewer.join().expect("responder");
}

#[test]
fn unknown_tools_are_refused() {
    let (bridge, _calls) = start(VIEWER_TIMEOUT, None);
    // a sidecar that declares capabilities: the viewer is the older side
    let client = Client::new(bridge.port(), bridge.token().clone());
    let e = client.call("inventory_teleport", json!({})).expect_err("refused");
    let ClientError::Tool(e) = e else { panic!("not a tool error: {e}") };
    assert_eq!(e.code, ErrorCode::Unsupported);
    assert!(e.message.contains("update"), "{e}");
    // an older sidecar keeps today's code
    let (status, body) = post(&bridge, &json!({"tool": "inventory_teleport", "arguments": {}}));
    assert_eq!(status, 400);
    assert_eq!(body["error"]["code"], "unknown_tool");
}

/// A call as an older sidecar sends it (no `features`), or any body.
fn post(bridge: &Bridge, body: &Value) -> (u16, Value) {
    let port = bridge.port();
    let body = body.to_string();
    raw(
        port,
        &format!(
            "POST /v1/call HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}\r\nContent-Length: {}\r\n\r\n{body}",
            bridge.token().secret(),
            body.len()
        ),
    )
}

#[test]
fn health_gives_the_capabilities_and_the_login() {
    let (bridge, _calls) = start(VIEWER_TIMEOUT, None);
    let client = Client::new(bridge.port(), bridge.token().clone());
    let health = client.health().expect("health");
    assert_eq!(health.capabilities, Some(crate::binding::capabilities()));
    assert_eq!(health.viewer_session, None, "no login told yet");
    bridge.set_login(1, true);
    let first = client.health().expect("health").viewer_session.expect("logged in");
    assert!(crate::binding::is_viewer_session(&first));
    assert_eq!(bridge.viewer_session().as_deref(), Some(first.as_str()));
    bridge.set_login(1, true);
    assert_eq!(client.health().expect("health").viewer_session.as_deref(), Some(first.as_str()));
    bridge.set_login(2, false);
    assert_eq!(client.health().expect("health").viewer_session, None, "logged out");
    bridge.set_login(3, true);
    let second = client.health().expect("health").viewer_session.expect("logged in");
    assert_ne!(first, second);
    // the same login on another start of the bridge: another value
    let (other, _) = start(VIEWER_TIMEOUT, None);
    other.set_login(3, true);
    assert_ne!(other.viewer_session().as_deref(), Some(second.as_str()));
}

#[test]
fn a_call_pinned_to_another_login_is_refused_before_the_viewer() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    bridge.set_login(1, true);
    let old = bridge.viewer_session().expect("logged in");
    bridge.set_login(2, true);
    let now = bridge.viewer_session().expect("logged in");
    let client = Client::new(bridge.port(), bridge.token().clone());
    let e = client.call_in("worn_items", json!({}), Some(&old)).expect_err("refused");
    let ClientError::Tool(e) = e else { panic!("not a tool error: {e}") };
    assert_eq!(e.code, ErrorCode::SessionChanged);
    assert_eq!(e.viewer_session.as_deref(), Some(now.as_str()), "the login now");
    assert!(calls.try_recv().is_err(), "nothing reached the viewer");
    // the health tool answers on any login; the right pin goes through
    let viewer = std::thread::spawn(move || {
        let status = calls.recv().expect("viewer_status");
        status.answer(Ok(json!({})));
        let read = calls.recv().expect("a read");
        let session = read.session.clone();
        read.answer(Ok(json!({})));
        session
    });
    client.call_in("viewer_status", json!({}), Some(&old)).expect("not bound");
    client.call_in("worn_items", json!({}), Some(&now)).expect("bound to this login");
    assert_eq!(
        viewer.join().expect("viewer").as_deref(),
        Some(now.as_str()),
        "the frame thread checks again"
    );
    // logged out: every bound call pinned to a login is refused
    bridge.set_login(3, false);
    let e = client.call_in("worn_items", json!({}), Some(&now)).expect_err("refused");
    let ClientError::Tool(e) = e else { panic!("not a tool error: {e}") };
    assert_eq!((e.code, e.viewer_session), (ErrorCode::SessionChanged, None));
}

#[test]
fn an_older_sidecar_gets_only_the_codes_it_knows() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    bridge.set_login(1, true);
    let now = bridge.viewer_session().expect("logged in");
    // the frame thread refuses a waiting call: the login changed under it
    let viewer = std::thread::spawn(move || {
        for _ in 0..2 {
            let call = calls.recv().expect("a call");
            call.answer(Err(crate::binding::session_changed(None)));
        }
    });
    let (status, body) = post(&bridge, &json!({"tool": "worn_items", "arguments": {}}));
    assert_eq!(status, 409);
    assert_eq!(body["error"]["code"], "not_ready", "{body}");
    assert!(body["error"].get("viewer_session").is_none());
    let client = Client::new(bridge.port(), bridge.token().clone());
    let e = client.call_in("worn_items", json!({}), Some(&now)).expect_err("refused");
    assert_eq!(code(e), ErrorCode::SessionChanged);
    viewer.join().expect("viewer");
    // an older sidecar's call is never bound: it reaches the viewer
    assert_eq!(bridge.activity().len(), 2);
}

#[test]
fn a_viewer_that_does_not_answer_times_out() {
    let (bridge, _calls) = start(Duration::from_millis(100), None);
    let client = Client::new(bridge.port(), bridge.token().clone());
    assert_eq!(
        code(client.call("worn_items", json!({})).expect_err("timed out")),
        ErrorCode::Timeout
    );
}

#[test]
fn probes_are_not_activity() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let viewer = responder(calls);
    let client = Client::new(bridge.port(), bridge.token().clone()).probe();
    client.call("viewer_status", json!({})).expect("answered");
    assert!(bridge.activity().is_empty());
    bridge.stop();
    viewer.join().expect("responder");
}

#[test]
fn connections_beyond_the_limit_are_turned_away() {
    let (bridge, _calls) = start(VIEWER_TIMEOUT, None);
    let port = bridge.port();
    // idle connections that hold their slots
    let held: Vec<_> = (0..MAX_CONNECTIONS)
        .map(|_| TcpStream::connect(("127.0.0.1", port)).expect("connected"))
        .collect();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while bridge.shared.connections.load(Ordering::SeqCst) < MAX_CONNECTIONS && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let (status, body) = raw(port, "GET /v1/health HTTP/1.1\r\n\r\n");
    assert_eq!(status, 503);
    assert_eq!(body["error"]["code"], "busy");
    drop(held);
}

#[test]
fn the_discovery_file_lives_while_the_bridge_runs() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, _calls) = start(VIEWER_TIMEOUT, Some(dir.path().to_path_buf()));
    let path = bridge.discovery_path().expect("written").to_path_buf();
    let found = discovery::read(&path).expect("readable");
    assert_eq!(found.port, bridge.port());
    assert!(bridge.token().matches(&found.token));
    assert_eq!(found.pid, std::process::id());
    let port = bridge.port();
    drop(bridge);
    assert!(!path.exists(), "deleted when the bridge stops");
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err(), "nothing listens any more");
}

#[test]
fn a_new_start_gets_a_new_token() {
    let (a, _ca) = start(VIEWER_TIMEOUT, None);
    let (b, _cb) = start(VIEWER_TIMEOUT, None);
    assert_ne!(a.token().secret(), b.token().secret());
}

#[test]
fn changes_set_to_never_are_refused_and_ask_reaches_the_viewer_flagged() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let client = Client::new(bridge.port(), bridge.token().clone()).with_client_name("Claude\r\nCode\u{7}");
    // read only: every change refused before the viewer
    assert_eq!(
        code(client.call("inventory_trash", json!({"ids": []})).expect_err("refused")),
        ErrorCode::PermissionDenied
    );
    assert!(calls.try_recv().is_err());
    bridge.set_permissions(Preset::AskBeforeChanges.permissions());
    let viewer = std::thread::spawn(move || {
        let call = calls.recv().expect("a call");
        let seen = (call.tool.clone(), call.ask, call.client.clone());
        call.answer(Ok(json!({"done": true})));
        let read = calls.recv().expect("a read");
        let read_ask = read.ask;
        read.answer(Ok(json!({})));
        (seen, read_ask)
    });
    assert_eq!(
        client.call("inventory_rename", json!({"id": "x", "name": "y"})).expect("allowed"),
        json!({"done": true})
    );
    client.call("worn_items", json!({})).expect("read");
    let ((tool, ask, name), read_ask) = viewer.join().expect("viewer");
    assert_eq!(tool, "inventory_rename");
    assert!(ask, "Ask level: the viewer asks first");
    assert!(!read_ask, "reads never ask");
    assert_eq!(name.as_deref(), Some("ClaudeCode"), "the name is cleaned");
}

#[test]
fn permanent_deletes_are_never_allowed() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let mut all = Permissions::default();
    for class in Class::ALL {
        all.set(class, Level::Allow);
    }
    bridge.set_permissions(all);
    let client = Client::new(bridge.port(), bridge.token().clone());
    for never in protocol::NEVER {
        let error = client.call(never, json!({})).expect_err("never");
        assert_eq!(code(error), ErrorCode::PermissionDenied, "{never}");
    }
    assert!(calls.try_recv().is_err(), "nothing reached the viewer");
    assert!(bridge.activity().iter().all(|a| a.outcome == Outcome::Refused));
}

#[test]
fn an_asking_call_may_wait_longer_than_a_read() {
    let (bridge, calls) = start(Duration::from_millis(100), None);
    bridge.set_permissions(Preset::AskBeforeChanges.permissions());
    let client = Client::new(bridge.port(), bridge.token().clone());
    let viewer = std::thread::spawn(move || {
        let call = calls.recv().expect("a call");
        // the user takes a moment in the dialog
        std::thread::sleep(Duration::from_millis(400));
        call.answer(Err(ToolError::new(ErrorCode::PermissionDenied, "refused by the user")));
        calls
    });
    assert_eq!(
        code(client.call("inventory_move", json!({"ids": ["x"]})).expect_err("refused")),
        ErrorCode::PermissionDenied
    );
    let calls = viewer.join().expect("viewer");
    assert_eq!(bridge.activity()[0].outcome, Outcome::Refused);
    // a read keeps the short wait
    assert_eq!(code(client.call("worn_items", json!({})).expect_err("late")), ErrorCode::Timeout);
    drop(calls);
}

#[test]
fn a_thumbnail_from_a_picture_needs_edit_and_camera() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let client = Client::new(bridge.port(), bridge.token().clone());
    let mut p = Preset::ReadOnly.permissions();
    p.set(Class::Edit, Level::Allow);
    bridge.set_permissions(p);
    // Edit alone is not enough: Camera is on Never
    assert_eq!(
        code(
            client
                .call("inventory_snapshot_thumbnail", json!({"id": "x"}))
                .expect_err("refused")
        ),
        ErrorCode::PermissionDenied
    );
    assert!(calls.try_recv().is_err(), "refused before the viewer");
    p.set(Class::Camera, Level::Ask);
    bridge.set_permissions(p);
    let viewer = std::thread::spawn(move || {
        let call = calls.recv().expect("a call");
        let ask = call.ask;
        call.answer(Ok(json!({"done": true})));
        ask
    });
    client.call("inventory_snapshot_thumbnail", json!({"id": "x"})).expect("allowed");
    assert!(viewer.join().expect("viewer"), "Camera on Ask: the viewer asks once");
}

#[test]
fn a_picture_with_the_interface_asks_even_on_allow() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let client = Client::new(bridge.port(), bridge.token().clone());
    let mut p = Preset::ReadOnly.permissions();
    p.set(Class::Camera, Level::Allow);
    bridge.set_permissions(p);
    let viewer = std::thread::spawn(move || {
        let mut asks = Vec::new();
        for _ in 0..2 {
            let call = calls.recv().expect("a call");
            asks.push(call.ask);
            call.answer(Ok(json!({})));
        }
        (asks, calls)
    });
    client.call("camera_snapshot", json!({})).expect("taken");
    client.call("camera_snapshot", json!({"interface": true})).expect("taken");
    let (asks, calls) = viewer.join().expect("viewer");
    assert_eq!(asks, vec![false, true]);
    // Never still refuses it, without a dialog
    p.set(Class::Camera, Level::Never);
    bridge.set_permissions(p);
    assert_eq!(
        code(client.call("camera_snapshot", json!({"interface": true})).expect_err("refused")),
        ErrorCode::PermissionDenied
    );
    assert!(calls.try_recv().is_err());
}

/// Folderstorm: receipts (mcp-receipts.md, section 4): a change gets its
/// `operation_id` when it arrives, its listener's timeout names it, and the
/// activity line keeps it; a read gets none.
#[test]
fn a_change_gets_its_operation_id_when_it_arrives() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let mut permissions = Permissions::default();
    permissions.set(Class::Edit, Level::Allow);
    bridge.set_permissions(permissions);
    let viewer = std::thread::spawn(move || {
        let mut seen = Vec::new();
        while let Ok(call) = calls.recv() {
            seen.push(call.operation_id.clone());
            let answer = json!({"operation_id": call.operation_id});
            call.answer(Ok(answer));
        }
        seen
    });
    let client = Client::new(bridge.port(), bridge.token().clone());
    let renamed = client.call("inventory_rename", json!({"id": "x", "name": "y"})).expect("answered");
    let operation = renamed["operation_id"].as_str().expect("an operation").to_owned();
    assert!(receipts::is_operation_id(&operation), "{operation}");
    client.call("worn_items", json!({})).expect("answered");
    let activity = bridge.activity();
    assert_eq!(activity[0].operation_id.as_deref(), Some(operation.as_str()));
    assert_eq!(activity[1].operation_id, None);
    bridge.stop();
    let seen = viewer.join().expect("viewer");
    assert_eq!(seen, vec![Some(operation), None], "a read has no operation");
    // the listener's own timeout names the operation
    let e = timed_out(Some("op-0123456789abcdef0123456789abcdef".into()));
    assert_eq!(e.code, ErrorCode::Timeout);
    assert!(e.to_string().contains("assistant_operation_status"), "{e}");
    assert!(e.to_string().ends_with("(operation_id op-0123456789abcdef0123456789abcdef)"), "{e}");
    assert_eq!(timed_out(None).operation_id, None);
    // an older sidecar ignores the field; a newer one reads it
    let wire = serde_json::to_value(&e).expect("json");
    assert_eq!(wire["operation_id"], "op-0123456789abcdef0123456789abcdef");
    let back: ToolError = serde_json::from_value(wire).expect("read back");
    assert_eq!(back, e);
}

#[test]
fn a_conflict_reaches_only_a_sidecar_that_declares_receipts() {
    let conflict = ToolError::new(ErrorCode::Conflict, "used");
    assert_eq!(conflict.clone().for_sidecar(None).code, ErrorCode::InvalidArguments);
    let mut login_only = crate::binding::assumed();
    login_only.insert(crate::binding::LOGIN_BINDING.into(), 1);
    assert_eq!(conflict.clone().for_sidecar(Some(&login_only)).code, ErrorCode::InvalidArguments);
    assert_eq!(
        conflict.for_sidecar(Some(&crate::binding::capabilities())).code,
        ErrorCode::Conflict
    );
}

/// Folderstorm: plans (mcp-receipts.md, section 6): the listener cannot
/// know a plan's class; it refuses an execute only when Edit and Move are
/// both on Never, and the frame thread decides from the plan.
#[test]
fn a_plans_execute_is_refused_here_only_when_both_classes_are_never() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let viewer = responder(calls);
    let client = Client::new(bridge.port(), bridge.token().clone());
    let execute = || client.call("inventory_execute_plan", json!({"plan_id": "pl-x"}));
    assert_eq!(code(execute().expect_err("both Never")), ErrorCode::PermissionDenied);
    let mut permissions = Permissions::default();
    permissions.set(Class::Move, Level::Allow);
    bridge.set_permissions(permissions);
    assert_eq!(execute().expect("reaches the viewer")["tool"], "inventory_execute_plan");
    bridge.stop();
    assert_eq!(viewer.join().expect("responder"), 1);
}

/// Folderstorm: operations (mcp-receipts.md, section 5): an early answer
/// only for a sidecar that declares `operations`.
#[test]
fn an_early_answer_only_for_a_sidecar_that_declares_operations() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let mut permissions = Permissions::default();
    permissions.set(Class::Move, Level::Allow);
    bridge.set_permissions(permissions);
    let viewer = std::thread::spawn(move || {
        let mut early = Vec::new();
        while let Ok(call) = calls.recv() {
            early.push(call.early);
            call.answer(Ok(json!({})));
        }
        early
    });
    let body = |features: Option<crate::binding::Capabilities>| json!({"tool": "inventory_execute_plan", "arguments": {"plan_id": "p", "wait": false}, "features": features});
    post(&bridge, &body(Some(crate::binding::capabilities())));
    post(&bridge, &body(None));
    let mut older = crate::binding::capabilities();
    older.remove(crate::binding::OPERATIONS);
    post(&bridge, &body(Some(older)));
    bridge.stop();
    assert_eq!(viewer.join().expect("viewer"), vec![true, false, false]);
}

/// Folderstorm: navigation (phase 3, part 4): a teleport asks even on
/// Allow and answers early for a sidecar that declares `operations`; Never
/// refuses it before the viewer; `navigation_stop` needs no class.
#[test]
fn a_teleport_asks_even_on_allow_and_stop_needs_no_class() {
    let (bridge, calls) = start(VIEWER_TIMEOUT, None);
    let client = Client::new(bridge.port(), bridge.token().clone());
    let mut p = Preset::ReadOnly.permissions();
    p.set(Class::Navigation, Level::Allow);
    bridge.set_permissions(p);
    let viewer = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for _ in 0..3 {
            let call = calls.recv().expect("a call");
            seen.push((call.tool.clone(), call.ask, call.early, call.operation_id.is_some()));
            call.answer(Ok(json!({})));
        }
        (seen, calls)
    });
    let at = json!({"global_position": [256100.0, 256100.0, 30.0], "client_request_id": "tp-1"});
    client
        .call("navigation_move_to", json!({"position": [1.0, 2.0, 3.0]}))
        .expect("taken");
    client.call("navigation_teleport", at.clone()).expect("taken");
    client.call("navigation_stop", json!({"operation_id": "op-x"})).expect("taken");
    let (seen, calls) = viewer.join().expect("viewer");
    let features = seen
        .iter()
        .map(|(tool, ask, early, op)| (tool.as_str(), *ask, *early, *op))
        .collect::<Vec<_>>();
    assert_eq!(
        features,
        vec![
            // the client declares this build's capabilities, `operations` included
            ("navigation_move_to", false, true, true),
            ("navigation_teleport", true, true, true),
            ("navigation_stop", false, false, false),
        ]
    );
    // Never refuses the change, never the stop
    p.set(Class::Navigation, Level::Never);
    p.set(Class::Read, Level::Never);
    bridge.set_permissions(p);
    assert_eq!(
        code(client.call("navigation_teleport", at).expect_err("refused")),
        ErrorCode::PermissionDenied
    );
    assert!(calls.try_recv().is_err(), "refused before the viewer");
    let viewer = std::thread::spawn(move || {
        let call = calls.recv().expect("a call");
        call.answer(Ok(json!({"stopped": true})));
    });
    client.call("navigation_stop", json!({"operation_id": "op-x"})).expect("passes");
    viewer.join().expect("viewer");
}
