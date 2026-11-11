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
    let client = Client::new(bridge.port(), bridge.token().clone());
    assert_eq!(
        code(client.call("inventory_teleport", json!({})).expect_err("refused")),
        ErrorCode::UnknownTool
    );
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
