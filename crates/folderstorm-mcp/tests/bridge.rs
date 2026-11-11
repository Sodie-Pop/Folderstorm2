//! The sidecar's logic against an in-process bridge over a real localhost
//! socket, with a fake viewer answering the calls.

use folderstorm_mcp::connect::{NotFound, Viewer, find};
use folderstorm_mcp::jsonrpc::{Backend, Server};
use folderstorm_mcp_bridge::server::{Config, Pending, VIEWER_TIMEOUT};
use folderstorm_mcp_bridge::{Bridge, Class, ErrorCode, Level, Permissions, Preset, ToolError, discovery};
use serde_json::{Value, json};
use std::sync::mpsc::Receiver;

struct Live(Viewer);

impl Backend for Live {
    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
        self.0.call(tool, arguments)
    }

    fn client_name(&mut self, name: &str) {
        self.0.set_client_name(name);
    }
}

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

fn fake_viewer(calls: Receiver<Pending>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while let Ok(call) = calls.recv() {
            let answer = match call.tool.as_str() {
                "viewer_status" => Ok(json!({"session": "demo", "inventory": {"usable": true}})),
                "inventory_search" => Ok(json!({"results": [{"name": "Boots", "query": call.arguments["query"]}], "truncated": false})),
                _ => Err(ToolError::new(ErrorCode::NotFound, "nothing")),
            };
            call.answer(answer);
        }
    })
}

fn rpc(server: &mut Server<Live>, line: &str) -> Value {
    serde_json::from_str(&server.handle_line(line).expect("an answer")).expect("json")
}

#[test]
fn an_assistant_reaches_the_viewer_through_the_sidecar() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = start(dir.path(), 4242);
    let viewer = fake_viewer(calls);
    let mut server = Server::new(Live(Viewer::new(Some(dir.path().to_path_buf()), None)));
    rpc(
        &mut server,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    );
    let a = rpc(
        &mut server,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"inventory_search","arguments":{"query":"boot"}}}"#,
    );
    assert_eq!(a["result"]["isError"], false, "{a}");
    assert_eq!(a["result"]["structuredContent"]["results"][0]["query"], "boot");
    let a = rpc(
        &mut server,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"inventory_item","arguments":{"item_id":"x"}}}"#,
    );
    assert_eq!(a["result"]["isError"], true);
    assert_eq!(bridge.activity().len(), 2);

    // reads turned off in Preferences: refused, the viewer never asked
    let mut permissions = Permissions::default();
    permissions.set(Class::Read, Level::Never);
    bridge.set_permissions(permissions);
    let a = rpc(
        &mut server,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"inventory_search","arguments":{"query":"boot"}}}"#,
    );
    assert_eq!(a["result"]["isError"], true);
    assert!(
        a["result"]["content"][0]["text"]
            .as_str()
            .expect("text")
            .starts_with("permission_denied")
    );

    // the viewer turns access off and on again: new port, new token
    bridge.stop();
    viewer.join().expect("fake viewer");
    let a = rpc(
        &mut server,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"viewer_status"}}"#,
    );
    assert_eq!(a["result"]["isError"], true, "no viewer now");
    assert!(a["result"]["content"][0]["text"].as_str().expect("text").contains("not running"));
    let (bridge, calls) = start(dir.path(), 4242);
    let viewer = fake_viewer(calls);
    let a = rpc(
        &mut server,
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"viewer_status"}}"#,
    );
    assert_eq!(a["result"]["structuredContent"]["session"], "demo", "{a}");
    bridge.stop();
    viewer.join().expect("fake viewer");
}

#[test]
fn diagnose_reports_without_the_token() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (report, ok) = folderstorm_mcp::diagnose::run(Some(dir.path()), None);
    assert!(!ok);
    assert_eq!(report["stage"], "discovery");

    let (bridge, calls) = start(dir.path(), 777);
    let viewer = fake_viewer(calls);
    let (report, ok) = folderstorm_mcp::diagnose::run(Some(dir.path()), Some(777));
    assert!(ok, "{report}");
    assert_eq!(report["stage"], "done");
    assert_eq!(report["port"], bridge.port());
    assert_eq!(report["read_allowed"], true);
    assert_eq!(report["inventory_usable"], true);
    assert!(!report.to_string().contains(bridge.token().secret()));
    assert!(bridge.activity().is_empty(), "a probe is not an assistant's activity");
    let (report, ok) = folderstorm_mcp::diagnose::run(Some(dir.path()), Some(778));
    assert!(!ok);
    assert_eq!(report["stage"], "discovery");
    bridge.stop();
    viewer.join().expect("fake viewer");
}

#[test]
fn a_stale_file_is_skipped() {
    let dir = tempfile::tempdir().expect("temp dir");
    // a file left by a viewer that crashed: its port no longer answers
    let stale = {
        let (bridge, _calls) = start(dir.path(), 1);
        let path = bridge.discovery_path().expect("written").to_path_buf();
        let kept = std::fs::read(&path).expect("read");
        drop(bridge);
        std::fs::write(&path, kept).expect("rewritten");
        path
    };
    assert!(matches!(find(Some(dir.path()), None, false), Err(NotFound::NoAnswer)));
    let (bridge, calls) = start(dir.path(), 2);
    let viewer = fake_viewer(calls);
    let (_, pid) = find(Some(dir.path()), None, false).expect("the live viewer");
    assert_eq!(pid, 2);
    assert_eq!(discovery::find(dir.path(), None).len(), 2);
    assert!(stale.exists());
    bridge.stop();
    viewer.join().expect("fake viewer");
}

#[test]
fn a_change_reaches_the_viewer_with_the_clients_name_and_the_ask_flag() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = start(dir.path(), 5151);
    bridge.set_permissions(Preset::AskBeforeChanges.permissions());
    let viewer = std::thread::spawn(move || {
        let call = calls.recv().expect("a call");
        let seen = (call.tool.clone(), call.ask, call.client.clone());
        // the user refuses in the dialog
        call.answer(Err(ToolError::new(ErrorCode::PermissionDenied, "the user refused")));
        seen
    });
    let mut server = Server::new(Live(Viewer::new(Some(dir.path().to_path_buf()), None)));
    rpc(
        &mut server,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code","version":"2"}}}"#,
    );
    let a = rpc(
        &mut server,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"inventory_trash","arguments":{"ids":["x"]}}}"#,
    );
    assert_eq!(a["result"]["isError"], true, "{a}");
    assert_eq!(
        viewer.join().expect("viewer"),
        ("inventory_trash".to_owned(), true, Some("Claude Code".to_owned()))
    );
    bridge.stop();
}
