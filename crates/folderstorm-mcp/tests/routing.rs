//! Session routing (folderstorm/plans/mcp-routing.md), from the sidecar's
//! side, over real 127.0.0.1 sockets with real bridges in one discovery
//! folder (one per character): choosing a viewer, staying bound to it,
//! never falling back to another character; and the host's controller.

use folderstorm_mcp::connect::{Scope, Viewer};
use folderstorm_mcp::host::Host;
use folderstorm_mcp_bridge::controller::{Controller, ControllerConfig, HostLevels};
use folderstorm_mcp_bridge::server::{Config, Identity, Pending, VIEWER_TIMEOUT};
use folderstorm_mcp_bridge::{Bridge, Class, ErrorCode, Level, Permissions};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// A character's viewer: its bridge, logged in, and how many calls reached
/// it.
struct Character {
    bridge: Bridge,
    reached: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Character {
    fn start(dir: &std::path::Path, pid: u32, profile: Option<&str>, name: &str) -> Character {
        let (bridge, calls) = Bridge::start(Config {
            discovery_dir: Some(dir.to_path_buf()),
            pid,
            version: "test".into(),
            permissions: Permissions::default(),
            viewer_timeout: VIEWER_TIMEOUT,
        })
        .expect("bridge started");
        bridge.set_identity(Identity {
            profile: profile.map(str::to_owned),
            managed: profile.is_some(),
            character: Some(name.to_owned()),
        });
        bridge.set_login(1, true);
        let reached = Arc::new(AtomicUsize::new(0));
        let thread = Some(answer(calls, reached.clone(), name.to_owned()));
        Character { bridge, reached, thread }
    }

    fn reached(&self) -> usize {
        self.reached.load(Ordering::SeqCst)
    }

    fn session(&self) -> String {
        self.bridge.viewer_session().expect("logged in")
    }

    fn stop(mut self) {
        let thread = self.thread.take();
        let Character { bridge, .. } = self;
        bridge.stop();
        if let Some(thread) = thread {
            thread.join().expect("viewer thread");
        }
    }
}

fn answer(calls: Receiver<Pending>, reached: Arc<AtomicUsize>, name: String) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while let Ok(call) = calls.recv() {
            reached.fetch_add(1, Ordering::SeqCst);
            let reply = json!({"tool": call.tool, "character": name, "session": call.session});
            call.answer(Ok(reply));
        }
    })
}

fn search(v: &mut Viewer) -> Result<Value, folderstorm_mcp_bridge::ToolError> {
    v.call("inventory_search", json!({"query": "boot"}))
}

#[test]
fn one_viewer_is_chosen_at_the_first_call_and_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    let main = Character::start(dir.path(), 301, None, "Main Resident");
    let mut v = Viewer::new(Some(dir.path().to_path_buf()), None);
    assert_eq!(search(&mut v).expect("runs")["character"], "Main Resident");
    assert_eq!(v.chosen(), Some("main"));
    // a second character started later changes nothing
    let alt = Character::start(dir.path(), 302, Some("Alt1"), "Alt Resident");
    assert_eq!(search(&mut v).expect("runs")["character"], "Main Resident");
    assert_eq!(alt.reached(), 0);
    main.stop();
    alt.stop();
}

#[test]
fn several_viewers_need_a_choice_and_the_choice_is_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    let main = Character::start(dir.path(), 311, None, "Main Resident");
    let alt = Character::start(dir.path(), 312, Some("Alt1"), "Alt Resident");
    // Read on Never for the main one: no name in its row
    let mut never = Permissions::default();
    never.set(Class::Read, Level::Never);
    main.bridge.set_permissions(never);
    let mut v = Viewer::new(Some(dir.path().to_path_buf()), None);
    let e = search(&mut v).expect_err("which one?");
    assert_eq!(e.code, ErrorCode::TargetRequired);
    assert!(
        e.message.contains("Alt1") && e.message.contains("main") && e.message.contains("viewer_select"),
        "{e}"
    );
    let e = v.call("viewer_status", json!({})).expect_err("viewer_status too");
    assert_eq!(e.code, ErrorCode::TargetRequired);
    assert_eq!(main.reached() + alt.reached(), 0, "nothing was sent to anyone");
    // the list
    let list = v.list().expect("listed");
    assert_eq!(list["count"], 2);
    assert_eq!(list["selection_required"], true);
    let rows = list["viewers"].as_array().expect("rows");
    let row = |p: &str| rows.iter().find(|r| r["profile"] == p).expect("a row").clone();
    assert_eq!(row("Alt1")["character"], "Alt Resident");
    assert_eq!(row("Alt1")["managed"], true);
    assert_eq!(row("Alt1")["viewer_session"], json!(alt.session()));
    assert!(row("main").get("character").is_none(), "Read on Never: no name");
    assert_eq!(row("main")["logged_in"], true);
    // choose by profile, any case: every call goes there, bound to its login
    let chosen = v.select(&json!({"profile": "alt1"})).expect("chosen");
    assert_eq!(
        (chosen["profile"].clone(), chosen["selected"].clone()),
        (json!("Alt1"), json!(true))
    );
    assert_eq!(v.pinned(), Some(alt.session().as_str()));
    for _ in 0..3 {
        assert_eq!(search(&mut v).expect("runs")["character"], "Alt Resident");
    }
    assert_eq!((main.reached(), alt.reached()), (0, 3));
    assert_eq!(v.list().expect("listed")["selection_required"], false);
    // arguments
    for bad in [
        json!({}),
        json!({"profile": "Alt1", "pid": 312}),
        json!({"name": "x"}),
        json!({"pid": "x"}),
    ] {
        assert_eq!(v.select(&bad).expect_err("refused").code, ErrorCode::InvalidArguments, "{bad}");
    }
    assert_eq!(v.select(&json!({"profile": "Alt9"})).expect_err("none").code, ErrorCode::NotFound);
    assert_eq!(v.chosen(), Some("Alt1"), "a refused choice changes nothing");
    main.stop();
    alt.stop();
}

#[test]
fn a_chosen_viewer_that_quits_never_hands_its_calls_to_another() {
    let dir = tempfile::tempdir().expect("temp dir");
    let main = Character::start(dir.path(), 321, None, "Main Resident");
    let alt = Character::start(dir.path(), 322, Some("Alt1"), "Alt Resident");
    let mut v = Viewer::new(Some(dir.path().to_path_buf()), None);
    v.select(&json!({"profile": "Alt1"})).expect("chosen");
    search(&mut v).expect("runs");
    // a relog of the chosen character: reported once, never sent again
    alt.bridge.set_login(2, true);
    let e = search(&mut v).expect_err("another login");
    assert_eq!(e.code, ErrorCode::SessionChanged);
    assert_eq!(alt.reached(), 1);
    search(&mut v).expect("the new login");
    // it quits: unavailable while the main viewer still runs
    alt.stop();
    for _ in 0..2 {
        let e = search(&mut v).expect_err("gone");
        assert_eq!(e.code, ErrorCode::Unavailable);
        assert!(e.message.contains("Alt1") && e.message.contains("no other character"), "{e}");
    }
    assert_eq!(main.reached(), 0, "never the other character");
    assert_eq!(v.chosen(), Some("Alt1"), "the choice stays");
    // the same character started again: found, reported, then used
    let again = Character::start(dir.path(), 323, Some("Alt1"), "Alt Resident");
    let e = search(&mut v).expect_err("its new login");
    assert_eq!(e.code, ErrorCode::SessionChanged);
    assert_eq!(again.reached(), 0, "nothing sent on the report");
    assert_eq!(search(&mut v).expect("runs")["session"], json!(again.session()));
    assert_eq!(main.reached(), 0);
    main.stop();
    again.stop();
}

#[test]
fn a_login_or_a_process_can_be_chosen_exactly() {
    let dir = tempfile::tempdir().expect("temp dir");
    let main = Character::start(dir.path(), 331, None, "Main Resident");
    let alt = Character::start(dir.path(), 332, Some("Alt1"), "Alt Resident");
    let mut v = Viewer::new(Some(dir.path().to_path_buf()), None);
    let session = main.session();
    v.select(&json!({"viewer_session": session})).expect("chosen");
    assert_eq!(search(&mut v).expect("runs")["character"], "Main Resident");
    // a login that is gone: refused, nothing changes
    main.bridge.set_login(5, true);
    assert_eq!(
        v.select(&json!({"viewer_session": session})).expect_err("stale").code,
        ErrorCode::NotFound
    );
    v.select(&json!({"pid": 332})).expect("chosen");
    assert_eq!(search(&mut v).expect("runs")["character"], "Alt Resident");
    main.stop();
    alt.stop();
}

#[test]
fn a_profile_or_a_process_given_at_start_limits_the_choice() {
    let dir = tempfile::tempdir().expect("temp dir");
    let main = Character::start(dir.path(), 341, None, "Main Resident");
    let alt = Character::start(dir.path(), 342, Some("Alt1"), "Alt Resident");
    let mut v = Viewer::scoped(Some(dir.path().to_path_buf()), Scope::Profile("ALT1".into()));
    // several run, but only one is within reach: used at once
    assert_eq!(search(&mut v).expect("runs")["character"], "Alt Resident");
    let list = v.list().expect("listed");
    assert_eq!(list["count"], 1);
    assert_eq!(
        v.select(&json!({"profile": "main"})).expect_err("out of reach").code,
        ErrorCode::NotFound
    );
    let mut by_pid = Viewer::scoped(Some(dir.path().to_path_buf()), Scope::Pid(341));
    assert_eq!(search(&mut by_pid).expect("runs")["character"], "Main Resident");
    let mut main_only = Viewer::scoped(Some(dir.path().to_path_buf()), Scope::Profile("main".into()));
    assert_eq!(search(&mut main_only).expect("runs")["character"], "Main Resident");
    assert_eq!((main.reached(), alt.reached()), (2, 1));
    main.stop();
    alt.stop();
}

/// The host's controller, answered by a hand-made host thread.
fn controller(dir: &std::path::Path) -> (Controller, std::thread::JoinHandle<Vec<(String, bool)>>) {
    let (controller, calls) = Controller::start(ControllerConfig {
        discovery_dir: Some(dir.to_path_buf()),
        pid: 350,
        version: "test".into(),
        levels: HostLevels::all(Level::Allow),
        host_timeout: Duration::from_secs(5),
    })
    .expect("controller");
    let host = std::thread::spawn(move || {
        let mut seen = Vec::new();
        while let Ok(call) = calls.recv() {
            seen.push((call.tool.clone(), call.ask));
            let tool = call.tool.clone();
            call.answer(Ok(json!({"tool": tool})));
        }
        seen
    });
    (controller, host)
}

#[test]
fn the_host_s_controller_is_found_pinned_and_never_confused_with_a_viewer() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut host = Host::new(Some(dir.path().to_path_buf()));
    // no controller: a clear refusal
    let e = host.call("host_characters", json!({})).expect_err("off");
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert!(e.message.contains("MCP controller"), "{e}");
    // a viewer's file is not a controller
    let main = Character::start(dir.path(), 351, None, "Main Resident");
    assert_eq!(
        host.call("host_characters", json!({})).expect_err("off").code,
        ErrorCode::Unavailable
    );
    let (first, thread) = controller(dir.path());
    assert_eq!(host.call("host_characters", json!({})).expect("listed")["tool"], "host_characters");
    assert_eq!(host.pinned(), Some(first.host_session()));
    host.call("host_activate", json!({"profile": "Alt1"})).expect("done");
    // a viewer's tool never goes to the host
    assert_eq!(
        host.call("inventory_search", json!({})).expect_err("not a host tool").code,
        ErrorCode::UnknownTool
    );
    // the host restarts: reported once, never sent again, then pinned
    first.stop();
    let seen = thread.join().expect("host");
    assert_eq!(seen.len(), 2);
    let (second, thread) = controller(dir.path());
    let e = host.call("host_logout", json!({"profile": "Alt1"})).expect_err("restarted");
    assert_eq!(e.code, ErrorCode::SessionChanged);
    assert_eq!(host.pinned(), Some(second.host_session()));
    host.call("host_characters", json!({})).expect("listed");
    second.stop();
    let seen = thread.join().expect("host");
    assert_eq!(
        seen,
        vec![("host_characters".to_owned(), false)],
        "the logout never reached the new host"
    );
    assert_eq!(main.reached(), 0, "nothing reached the viewer");
    main.stop();
}
