//! Login binding and capability versions (folderstorm/plans/mcp-receipts.md,
//! sections 2 and 3), from the sidecar's side: pinning, `session_changed`
//! never followed by a second send, a viewer restart between two calls, and
//! viewers older and newer than this sidecar. Over real 127.0.0.1 sockets,
//! with the real bridge or a hand-written older one.

use folderstorm_mcp::connect::Viewer;
use folderstorm_mcp_bridge::discovery::{self, Discovery};
use folderstorm_mcp_bridge::server::{Config, Pending, VIEWER_TIMEOUT};
use folderstorm_mcp_bridge::token::Token;
use folderstorm_mcp_bridge::{Bridge, ErrorCode, Permissions, ToolError};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

fn start(dir: &std::path::Path, pid: u32) -> (Bridge, Receiver<Pending>) {
    Bridge::start(Config {
        discovery_dir: Some(dir.to_path_buf()),
        pid,
        version: "test".into(),
        permissions: Permissions::default(),
        viewer_timeout: VIEWER_TIMEOUT,
    })
    .expect("bridge started")
}

/// Answers every call with its tool and the login it was pinned to, and
/// counts the calls that reached it.
fn counting_viewer(calls: Receiver<Pending>) -> (Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let handle = std::thread::spawn(move || {
        while let Ok(call) = calls.recv() {
            seen.fetch_add(1, Ordering::SeqCst);
            let answer = json!({"tool": call.tool, "session": call.session});
            call.answer(Ok(answer));
        }
    });
    (count, handle)
}

fn viewer(dir: &std::path::Path) -> Viewer {
    Viewer::new(Some(dir.to_path_buf()), None)
}

fn search(v: &mut Viewer) -> Result<Value, ToolError> {
    v.call("inventory_search", json!({"query": "boot"}))
}

#[test]
fn calls_are_pinned_and_never_sent_again_after_session_changed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = start(dir.path(), 101);
    let (reached, fake) = counting_viewer(calls);
    bridge.set_login(1, true);
    let first = bridge.viewer_session().expect("logged in");
    let mut v = viewer(dir.path());
    let answer = search(&mut v).expect("runs");
    assert_eq!(answer["session"], json!(first), "sent with the pinned login");
    assert_eq!(v.pinned(), Some(first.as_str()));
    // logged out and in again: refused, and not sent again
    bridge.set_login(3, true);
    let second = bridge.viewer_session().expect("logged in");
    let e = search(&mut v).expect_err("refused");
    assert_eq!(e.code, ErrorCode::SessionChanged);
    assert_eq!(e.viewer_session.as_deref(), Some(second.as_str()));
    assert!(e.message.contains("viewer_status"), "{e}");
    assert_eq!(reached.load(Ordering::SeqCst), 1, "the refused call never reached the viewer");
    // the assistant looks again: the next call runs on the new login
    assert_eq!(v.pinned(), Some(second.as_str()));
    assert_eq!(search(&mut v).expect("runs")["session"], json!(second));
    assert_eq!(reached.load(Ordering::SeqCst), 2);
    bridge.stop();
    fake.join().expect("fake viewer");
}

#[test]
fn a_logout_keeps_the_pin_so_that_the_next_login_is_reported_too() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = start(dir.path(), 102);
    let (reached, fake) = counting_viewer(calls);
    bridge.set_login(1, true);
    let first = bridge.viewer_session().expect("logged in");
    let mut v = viewer(dir.path());
    search(&mut v).expect("runs");
    bridge.set_login(2, false);
    let e = search(&mut v).expect_err("logged out");
    assert_eq!((e.code, e.viewer_session.clone()), (ErrorCode::SessionChanged, None));
    assert!(e.message.contains("logged out"), "{e}");
    assert_eq!(v.pinned(), Some(first.as_str()));
    // viewer_status answers on any login
    assert_eq!(v.call("viewer_status", json!({})).expect("status")["tool"], "viewer_status");
    // the next login is another session: reported once, then pinned
    bridge.set_login(3, true);
    let e = search(&mut v).expect_err("another login");
    assert_eq!(e.code, ErrorCode::SessionChanged);
    let third = bridge.viewer_session().expect("logged in");
    assert_eq!(e.viewer_session.as_deref(), Some(third.as_str()));
    search(&mut v).expect("runs");
    assert_eq!(
        reached.load(Ordering::SeqCst),
        3,
        "the first search, viewer_status, the last search"
    );
    bridge.stop();
    fake.join().expect("fake viewer");
}

#[test]
fn nothing_is_pinned_while_logged_out_and_the_first_login_is_pinned() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = start(dir.path(), 103);
    let (_, fake) = counting_viewer(calls);
    bridge.set_login(0, false);
    let mut v = viewer(dir.path());
    assert_eq!(search(&mut v).expect("unbound")["session"], Value::Null);
    assert_eq!(v.pinned(), None);
    bridge.set_login(1, true);
    let first = bridge.viewer_session().expect("logged in");
    // the login is learnt before the call, which goes bound
    assert_eq!(search(&mut v).expect("runs")["session"], json!(first));
    assert_eq!(v.pinned(), Some(first.as_str()));
    bridge.stop();
    fake.join().expect("fake viewer");
}

#[test]
fn a_viewer_restart_between_two_calls_is_reported_instead_of_retried() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = start(dir.path(), 104);
    let (_, fake) = counting_viewer(calls);
    bridge.set_login(1, true);
    let mut v = viewer(dir.path());
    search(&mut v).expect("runs");
    // the viewer restarts (or access is turned off and on), on the same
    // login generation: a new epoch, so a new viewer_session
    bridge.stop();
    fake.join().expect("fake viewer");
    let (bridge, calls) = start(dir.path(), 104);
    let (reached, fake) = counting_viewer(calls);
    bridge.set_login(1, true);
    let now = bridge.viewer_session().expect("logged in");
    let e = search(&mut v).expect_err("not sent to the new viewer");
    assert_eq!(e.code, ErrorCode::SessionChanged);
    assert_eq!(e.viewer_session.as_deref(), Some(now.as_str()));
    assert!(e.message.contains("lost and found again"), "{e}");
    assert_eq!(reached.load(Ordering::SeqCst), 0, "nothing reached the new viewer");
    // the assistant decides to go on: the next call runs there
    assert_eq!(search(&mut v).expect("runs")["session"], json!(now));
    assert_eq!(reached.load(Ordering::SeqCst), 1);
    bridge.stop();
    fake.join().expect("fake viewer");
}

#[test]
fn every_tool_is_listed_whatever_the_viewer() {
    use folderstorm_mcp::jsonrpc::{Backend, Server};
    struct Nothing;
    impl Backend for Nothing {
        fn call(&mut self, _: &str, _: Value) -> Result<Value, ToolError> {
            Err(ToolError::new(ErrorCode::Unavailable, "no viewer"))
        }
    }
    let mut server = Server::new(Nothing);
    let a: Value = serde_json::from_str(
        &server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .expect("answer"),
    )
    .expect("json");
    assert_eq!(
        a["result"]["tools"].as_array().expect("tools").len(),
        folderstorm_mcp_bridge::protocol::all().count()
    );
}

/// A hand-written bridge of another build: it answers health with `health`
/// and every call with `{"ok":true,"result":{}}`, and keeps the bodies of
/// the calls it got.
struct OtherViewer {
    calls: Arc<Mutex<Vec<Value>>>,
}

impl OtherViewer {
    fn start(dir: &std::path::Path, pid: u32, health: Value) -> OtherViewer {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bound");
        let port = listener.local_addr().expect("address").port();
        let token = Token::generate().expect("random");
        discovery::write(dir, &Discovery::new(pid, port, &token)).expect("discovery file");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let kept = calls.clone();
        // detached: it ends with the test process
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (head, body) = read_request(&mut stream);
                let result = if head.starts_with("GET /v1/health") {
                    health.clone()
                } else {
                    kept.lock()
                        .expect("calls")
                        .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
                    json!({})
                };
                let answer = json!({"ok": true, "result": result}).to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        OtherViewer { calls }
    }

    fn calls(&self) -> Vec<Value> {
        self.calls.lock().expect("calls").clone()
    }
}

fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let end = loop {
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        let n = stream.read(&mut buf).expect("read");
        assert!(n > 0, "the request ended early");
        data.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&data[..end]).into_owned();
    let length: usize = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse().expect("length"))
        })
        .unwrap_or(0);
    let mut body = data[end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut buf).expect("read");
        assert!(n > 0, "the body ended early");
        body.extend_from_slice(&buf[..n]);
    }
    (head, body)
}

#[test]
fn an_older_viewer_without_capabilities_is_called_unbound() {
    let dir = tempfile::tempdir().expect("temp dir");
    let old = OtherViewer::start(
        dir.path(),
        201,
        json!({"api": 1, "product": "P", "version": "0.1", "read_allowed": true}),
    );
    let mut v = viewer(dir.path());
    search(&mut v).expect("phases 1 and 2 are assumed");
    v.call("inventory_trash", json!({"ids": ["x"]})).expect("a change too");
    assert_eq!(v.pinned(), None, "an older viewer has no login to pin");
    let calls = old.calls();
    assert_eq!(calls.len(), 2);
    for call in &calls {
        assert!(call.get("session").is_none(), "{call}");
        assert_eq!(call["features"], json!(folderstorm_mcp_bridge::binding::capabilities()));
    }
}

#[test]
fn a_tool_the_viewer_lacks_is_refused_before_sending() {
    let dir = tempfile::tempdir().expect("temp dir");
    // a viewer that advertises capabilities, but not this tool's group
    let other = OtherViewer::start(
        dir.path(),
        202,
        json!({"api": 1, "product": "P", "version": "x", "read_allowed": true,
               "capabilities": {"login_binding": 1}}),
    );
    let mut v = viewer(dir.path());
    let e = search(&mut v).expect_err("unsupported");
    assert_eq!(e.code, ErrorCode::Unsupported);
    assert!(e.message.contains("Update"), "{e}");
    assert!(other.calls().is_empty(), "nothing was sent");
    // through MCP: an error result the assistant reads, not a protocol error
    use folderstorm_mcp::jsonrpc::{Backend, Server};
    struct Live(Viewer);
    impl Backend for Live {
        fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
            self.0.call(tool, arguments)
        }
    }
    let mut server = Server::new(Live(v));
    let a: Value = serde_json::from_str(
        &server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"worn_items"}}"#)
            .expect("answer"),
    )
    .expect("json");
    assert_eq!(a["result"]["isError"], true, "{a}");
    assert!(
        a["result"]["content"][0]["text"]
            .as_str()
            .expect("text")
            .starts_with("unsupported:")
    );
}

#[test]
fn a_newer_viewer_with_more_groups_and_versions_is_used() {
    let dir = tempfile::tempdir().expect("temp dir");
    let newer = OtherViewer::start(
        dir.path(),
        203,
        json!({"api": 1, "product": "P", "version": "9", "read_allowed": true,
               "capabilities": {"inventory": 2, "login_binding": 1, "plans": 1, "receipts": 1},
               "viewer_session": "vs-0123456789abcdef0123456789abcdef", "something_new": true}),
    );
    let mut v = viewer(dir.path());
    search(&mut v).expect("runs");
    assert_eq!(v.pinned(), Some("vs-0123456789abcdef0123456789abcdef"));
    assert_eq!(newer.calls()[0]["session"], "vs-0123456789abcdef0123456789abcdef");
}

/// Folderstorm: receipts (mcp-receipts.md, section 3): a viewer that
/// advertises no `receipts` would refuse a `client_request_id` as an unknown
/// argument, and dropping it would make a retry unsafe: the sidecar answers
/// `unsupported` and sends nothing. Without one the call goes as before.
#[test]
fn a_client_request_id_is_refused_before_sending_to_a_viewer_without_receipts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let older = OtherViewer::start(
        dir.path(),
        204,
        json!({"api": 1, "product": "P", "version": "x", "read_allowed": true,
               "capabilities": {"inventory": 1, "login_binding": 1},
               "viewer_session": "vs-0123456789abcdef0123456789abcdef"}),
    );
    let mut v = viewer(dir.path());
    let e = v
        .call("inventory_rename", json!({"id": "x", "name": "y", "client_request_id": "r1"}))
        .expect_err("unsupported");
    assert_eq!(e.code, ErrorCode::Unsupported);
    assert!(e.message.contains("client_request_id"), "{e}");
    assert!(older.calls().is_empty(), "nothing was sent");
    v.call("inventory_rename", json!({"id": "x", "name": "y"})).expect("sent as before");
    assert_eq!(older.calls().len(), 1);
    // the receipts' tools need a viewer that has them
    let e = v.call("assistant_operation_status", json!({})).expect_err("unsupported");
    assert_eq!(e.code, ErrorCode::Unsupported);
}
