//! Folderstorm: the studio's waiting calls follow the desk's rules
//! (folderstorm/plans/mcp-receipts.md, sections 1 and 2): a login change
//! refuses a camera move waiting for the camera to stop, a picture queued
//! or drawn, and a thumbnail waiting for its picture with `session_changed`;
//! a class set to Never refuses them; a picture or thumbnail not asked yet
//! goes back to the dialog when its class now asks. Nothing is captured,
//! no image or position is given, and the gallery's snapshot is stopped
//! before it is sent. End to end over a real socket: a relog between the
//! frame and the photo refuses the picture and the sidecar never resends it.

use super::*;
use crate::mcp_bridge::desk::{ACCESS_OFF, PERMISSIONS_CHANGED};
use folderstorm_mcp_bridge::{Class, Level, Permissions};

const S1: &str = "vs-11111111111111111111111111111111";
const S2: &str = "vs-22222222222222222222222222222222";

fn levels(camera: Level) -> Permissions {
    let mut p = Permissions::default();
    for class in Class::ALL {
        p.set(class, Level::Allow);
    }
    p.set(Class::Camera, camera);
    p
}

/// A desk with every class on Allow (Camera on `camera`), logged in as S1.
fn desk(camera: Level) -> Desk {
    let mut desk = Desk::default();
    desk.set_permissions(levels(camera));
    desk.set_login(1, Some(S1.to_owned()));
    desk
}

fn answered(answer: &Receiver<Result<Value, ToolError>>) -> Result<Value, ToolError> {
    answer.try_recv().expect("answered")
}

/// A picture queued under the desk's login, the camera still since `t0`.
fn queued(studio: &mut Studio, desk: &Desk, t0: Instant, ask: bool) -> (Request, Receiver<Result<Value, ToolError>>) {
    studio.observe(pose(0.0), t0);
    let r = request(json!({"width": 64, "height": 64}));
    let (c, answer) = call("camera_snapshot", json!({}), ask);
    studio.queue_photo(c, r, desk.login(), t0);
    (r, answer)
}

#[test]
fn a_queued_picture_is_refused_after_a_relog_and_nothing_is_captured() {
    let t0 = Instant::now();
    let still = t0 + STILL;
    let mut desk = desk(Level::Allow);
    let mut studio = Studio::default();
    let (_, answer) = queued(&mut studio, &desk, t0, false);
    // logged out and in again (another character, or the same one)
    desk.set_login(3, Some(S2.to_owned()));
    assert!(studio.review(&desk).is_empty(), "nothing goes back to the dialog");
    assert_eq!(studio.photo_step(false, still), PhotoStep::Idle, "no photo starts");
    let e = answered(&answer).expect_err("refused");
    assert_eq!(e.code, ErrorCode::SessionChanged);
    assert_eq!(e.viewer_session.as_deref(), Some(S2), "the login now");
    assert_eq!(studio.counts(), (0, 0, 0, false));

    // its photo already being drawn: refused, its pixels never become an image
    let (r, answer) = queued(&mut studio, &desk, t0, false);
    assert_eq!(studio.photo_step(false, still), PhotoStep::Start(r));
    let plan = picture::plan(&r, (400, 200), None);
    studio.photo_started(plan);
    desk.set_login(4, None);
    assert!(studio.review(&desk).is_empty());
    let e = answered(&answer).expect_err("refused");
    assert_eq!((e.code, e.viewer_session.clone()), (ErrorCode::SessionChanged, None));
    assert!(e.message.contains("no image is given"), "{}", e.message);
    studio.photo_drawn(plan.render.size, gradient(64, 64), std::env::temp_dir(), "unused".into());
    assert_eq!(studio.counts(), (0, 0, 0, false), "nothing encoded");
}

#[test]
fn a_queued_picture_is_refused_when_camera_is_set_to_never() {
    let t0 = Instant::now();
    let still = t0 + STILL;
    let mut desk = desk(Level::Allow);
    let mut studio = Studio::default();
    let (_, answer) = queued(&mut studio, &desk, t0, false);
    desk.set_permissions(levels(Level::Never));
    assert!(studio.review(&desk).is_empty());
    assert_eq!(studio.photo_step(false, still), PhotoStep::Idle, "no photo starts");
    let e = answered(&answer).expect_err("refused");
    assert_eq!((e.code, e.message.as_str()), (ErrorCode::PermissionDenied, PERMISSIONS_CHANGED));
    assert_eq!(studio.counts(), (0, 0, 0, false));
}

#[test]
fn a_queued_picture_not_asked_yet_asks_when_camera_is_set_to_ask() {
    let t0 = Instant::now();
    let still = t0 + STILL;
    let inv = demo_inventory();
    let mut cx = ctx(&inv);
    cx.stage = stage();
    let mut desk = desk(Level::Allow);
    let mut studio = Studio::default();
    let (_, answer) = queued(&mut studio, &desk, t0, false);
    // one the user already allowed in the dialog keeps its place
    let (asked, asked_answer) = queued(&mut studio, &desk, t0, true);
    desk.set_permissions(levels(Level::Ask));
    let back = studio.review(&desk);
    assert_eq!(back.len(), 1, "the one not asked yet goes back to the dialog");
    for c in back {
        desk.take(c, &cx, still);
    }
    assert!(desk.question(still).is_some(), "the dialog asks");
    assert!(answer.try_recv().is_err(), "not answered: it waits for the user");
    assert_eq!(studio.photo_step(false, still), PhotoStep::Start(asked));
    assert!(asked_answer.try_recv().is_err());
}

#[test]
fn a_camera_move_waiting_for_the_camera_to_stop_is_refused_after_a_logout() {
    let t0 = Instant::now();
    let mut desk = desk(Level::Allow);
    let mut studio = Studio::default();
    studio.observe(pose(0.0), t0);
    let (c, answer) = call("camera_preset", json!({"preset": "portrait"}), false);
    studio.wait_move(c, json!({"preset": "portrait", "done": true}), desk.login(), t0);
    // Allow → Ask: the camera already moved, the move goes on
    desk.set_permissions(levels(Level::Ask));
    assert!(studio.review(&desk).is_empty());
    assert!(answer.try_recv().is_err());
    // logged out before the camera stopped
    desk.set_login(2, None);
    assert!(studio.review(&desk).is_empty());
    let e = answered(&answer).expect_err("refused");
    assert_eq!((e.code, e.viewer_session.clone()), (ErrorCode::SessionChanged, None));
    assert!(e.message.contains("the camera was moved"), "{}", e.message);
    assert!(e.message.contains("no position is given"), "{}", e.message);
    studio.tick(&pose(0.0), t0 + SETTLE_MAX);
    assert_eq!(studio.counts(), (0, 0, 0, false), "nothing answers later");
    // Camera set to Never before the camera stopped
    let mut desk = self::desk(Level::Allow);
    let (c, answer) = call("camera_reset", json!({}), false);
    studio.wait_move(c, json!({"done": true}), desk.login(), t0);
    desk.set_permissions(levels(Level::Never));
    assert!(studio.review(&desk).is_empty());
    let e = answered(&answer).expect_err("refused");
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert!(e.message.contains("no position is given"), "{}", e.message);
}

#[test]
fn a_thumbnail_waiting_for_its_picture_is_refused_after_a_relog() {
    let t0 = Instant::now();
    let mut desk = desk(Level::Allow);
    let mut studio = Studio::default();
    let (c, answer) = call("inventory_snapshot_thumbnail", json!({"id": COP_OUTFIT}), false);
    studio.thumbnail_started(c, json!({"id": COP_OUTFIT}), desk.login(), t0);
    desk.set_login(3, Some(S2.to_owned()));
    assert!(studio.review(&desk).is_empty());
    let e = answered(&answer).expect_err("refused");
    assert_eq!(e.code, ErrorCode::SessionChanged);
    assert_eq!(e.viewer_session.as_deref(), Some(S2));
    assert!(!studio.thumbnail_busy());
    // the gallery's snapshot under way is stopped before it is sent
    assert!(!studio.thumbnail_send(), "not sent");
    assert!(!studio.thumbnail_send(), "still not sent on a later frame");
    studio.thumbnail_done(Some("stopped"));
    assert!(studio.thumbnail_send(), "the user's own snapshot afterwards is theirs");
    assert!(studio.thumbnail_send());

    // Camera set to Never before the picture is sent
    let (c, answer) = call("inventory_snapshot_thumbnail", json!({"id": COP_OUTFIT}), false);
    studio.thumbnail_started(c, json!({"id": COP_OUTFIT}), desk.login(), t0);
    desk.set_permissions(levels(Level::Never));
    assert!(studio.review(&desk).is_empty());
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::PermissionDenied);
    assert!(!studio.thumbnail_send());
    studio.thumbnail_done(Some("stopped"));

    // once sent it waits for the server, as the desk's sent changes do
    desk.set_permissions(levels(Level::Allow));
    let (c, answer) = call("inventory_snapshot_thumbnail", json!({"id": COP_OUTFIT}), false);
    studio.thumbnail_started(c, json!({"id": COP_OUTFIT}), desk.login(), t0);
    assert!(studio.thumbnail_send());
    desk.set_login(5, Some(S1.to_owned()));
    assert!(studio.review(&desk).is_empty());
    assert!(answer.try_recv().is_err(), "not refused");
    studio.thumbnail_done(None);
    assert_eq!(answered(&answer).expect("set"), json!({"id": COP_OUTFIT, "done": true}));
}

#[test]
fn a_thumbnail_not_sent_asks_again_when_camera_is_set_to_ask() {
    let t0 = Instant::now();
    let inv = demo_inventory();
    let mut cx = ctx(&inv);
    cx.stage = stage();
    let mut desk = desk(Level::Allow);
    let mut studio = Studio::default();
    let (c, answer) = call("inventory_snapshot_thumbnail", json!({"id": COP_OUTFIT}), false);
    studio.thumbnail_started(c, json!({"id": COP_OUTFIT}), desk.login(), t0);
    desk.set_permissions(levels(Level::Ask));
    let back = studio.review(&desk);
    assert_eq!(back.len(), 1, "back to the dialog");
    assert!(!studio.thumbnail_send(), "the snapshot under way stops");
    for c in back {
        desk.take(c, &cx, t0);
    }
    assert!(desk.question(t0).is_some(), "the dialog asks");
    assert!(answer.try_recv().is_err(), "it waits for the user");
}

#[test]
fn turning_access_off_refuses_the_studio_calls_and_stops_the_thumbnail() {
    let t0 = Instant::now();
    let desk = desk(Level::Allow);
    let mut studio = Studio::default();
    studio.fov_before = Some(0.9);
    let (_, picture) = queued(&mut studio, &desk, t0, false);
    let (c, moving) = call("camera_reset", json!({}), false);
    studio.wait_move(c, json!({"done": true}), desk.login(), t0);
    let (c, thumbnail) = call("inventory_snapshot_thumbnail", json!({}), false);
    studio.thumbnail_started(c, json!({}), desk.login(), t0);
    studio.close();
    let e = answered(&picture).expect_err("refused");
    assert_eq!((e.code, e.message.as_str()), (ErrorCode::PermissionDenied, ACCESS_OFF));
    let e = answered(&moving).expect_err("refused");
    assert!(e.message.contains("access was turned off"), "{}", e.message);
    assert_eq!(answered(&thumbnail).expect_err("refused").message, ACCESS_OFF);
    assert!(!studio.thumbnail_send(), "the snapshot under way stops");
    studio.close();
    assert!(!studio.thumbnail_send(), "closed again on the next frame: still stopped");
    assert_eq!(studio.fov_before, Some(0.9), "the user's field of view is kept");
    assert_eq!(studio.counts(), (0, 0, 0, false));
}

/// The sidecar → a real 127.0.0.1 socket → the bridge → the desk → the
/// studio: a picture allowed and queued, then the viewer logs out and in
/// again between the frame and the drawing. The check just before the
/// photo refuses it with `session_changed`, nothing is captured, the
/// sidecar never sends it again, and the next call runs on the new login.
#[test]
fn a_relog_before_a_queued_picture_refuses_end_to_end_and_nothing_is_captured() {
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
        permissions: levels(Level::Allow),
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
        let picture = send(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"camera_snapshot","arguments":{"source":"world","width":64,"height":64}}}"#,
        );
        let after = send(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"worn_items"}}"#);
        (status, picture, after)
    });
    let inv = demo_inventory();
    let mut desk = Desk::default();
    let mut studio = Studio::default();
    let mut generation = 1;
    let mut taken = Vec::new();
    let mut queued = 0;
    let mut starts = 0;
    let t0 = Instant::now();
    let deadline = t0 + Duration::from_secs(30);
    // the frame loop, as `mcp_bridge::frame` and the drawing run it
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        bridge.set_login(generation, true);
        let session = bridge.viewer_session();
        desk.set_login(generation, session.clone());
        desk.set_permissions(bridge.permissions());
        let mut cx = ctx(&inv);
        cx.stage = stage();
        cx.viewer_session = session.clone();
        // the camera has been still for long: a queued picture starts at once
        studio.observe(pose(0.0), t0);
        for c in studio.review(&desk) {
            desk.take(c, &cx, now);
        }
        while let Ok(pending) = calls.try_recv() {
            taken.push(pending.tool.clone());
            desk.take(pending, &cx, now);
        }
        // the app runs what the desk lets through (`local_assistant_run`)
        for c in desk.tick(&bake(), now) {
            let Some(c) = desk.recheck(c, generation, session.clone(), &cx, now) else {
                continue;
            };
            match plan(&c.tool, &c.arguments, &cx).map(|p| p.work) {
                Ok(Work::Snapshot(r)) => {
                    studio.queue_photo(c, r, desk.login(), now);
                    queued += 1;
                    // logged out and in again before the frame is drawn
                    generation += 2;
                }
                other => panic!("not a picture: {other:?}"),
            }
        }
        // the drawing: decided again just before the photo (`photo_step`)
        bridge.set_login(generation, true);
        desk.set_login(generation, bridge.viewer_session());
        for c in studio.review(&desk) {
            desk.take(c, &cx, now);
        }
        if let PhotoStep::Start(_) = studio.photo_step(false, now.max(t0 + STILL)) {
            starts += 1;
        }
        studio.tick(&pose(0.0), now);
        std::thread::sleep(Duration::from_millis(5));
    }
    let (status, picture, after) = assistant.join().expect("assistant");
    assert_eq!(status["result"]["structuredContent"]["viewer_session"], json!(first), "{status}");
    assert_eq!(picture["result"]["isError"], true, "{picture}");
    let text = picture["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.starts_with("session_changed:"), "{text}");
    assert_eq!(after["result"]["isError"], false, "the next call runs on the new login: {after}");
    assert_eq!(queued, 1);
    assert_eq!(starts, 0, "nothing was captured");
    assert_eq!(
        taken,
        vec!["viewer_status", "camera_snapshot", "worn_items"],
        "the refused picture reached the viewer once, never again"
    );
    assert_eq!(studio.counts(), (0, 0, 0, false));
    bridge.stop();
}
