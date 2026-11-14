//! Folderstorm: the gesture and own-posing tools
//! (folderstorm/plans/mcp-gestures.md): which gestures (own, Copy, not
//! Library or Trash, links followed), loading without activating, the
//! revision, playing once under the assistant's own key, lines that always
//! ask, viewer commands and RLVa refusing, the stop, the 120 s and the
//! revocation; the pose session (start, joints, rotations, revision, undo,
//! reset, stop, the viewer's poser winning, teleport, login change), the
//! presets (hand presets, rotations-only files, no path), the save (always
//! asks, never overwrites), no asset id in any answer, the classes in "Ask
//! before changes", and end to end over a real socket.

use super::changes::Work;
use super::desk::{Bake, Desk};
use super::gestures::{self, Avatar, Work as GestureWork};
use super::objects::Scene;
use super::posing;
use super::tools::{Ctx, Session};
use crate::demo::DEMO_AGENT;
use crate::gestures::{Entry, Manager};
use crate::world::World;
use crate::world::inventory::{FetchState, Folder, actions};
use aurora_assets::gesture::{Gesture, Step, WAIT_TIME};
use aurora_net::NetEvent;
use aurora_net::inventory::{InvFolder, InvItem};
use folderstorm_mcp_bridge::protocol::gestures::{LIST, PLAY, STATUS, STOP};
use folderstorm_mcp_bridge::protocol::posing as pose;
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{Class, ErrorCode, Level, Permissions, Preset, ToolError};
use glam::{Quat, Vec3};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use uuid::Uuid;

const GESTURE: Uuid = Uuid::from_u128(8106);
const SOUND: Uuid = Uuid::from_u128(8105);
const ANIM: Uuid = Uuid::from_u128(0xA417);
const IDLE: Bake = Bake {
    outfit_change_pending: false,
    cof_version: 3,
    appearance_cof_version: 3,
    server_bakes: true,
};

/// The demo avatar at rest, in region 1, with the demo inventory.
fn world() -> World {
    let lib = Arc::new(crate::scene::avatar::AvatarLibrary::load());
    let mut world = World::new(lib.clone());
    world.agent_id = DEMO_AGENT;
    world.main_region = Some(1);
    world.apply(NetEvent::ObjectUpdates {
        handle: 1,
        objects: vec![crate::demo::action_avatar(false)],
    });
    let rig = &lib.rig;
    world.avatar_poses.insert(
        world.agent_id,
        rig.world_with(&rig.local_rot, Vec3::ZERO, &rig.local_pos, &rig.scale)
            .iter()
            .zip(&rig.default_world_inv)
            .map(|(p, i)| *p * *i)
            .collect(),
    );
    let root = Uuid::from_u128(80);
    world.inventory.root = root;
    world.inventory.owner = DEMO_AGENT;
    world.inventory.folders.insert(
        root,
        Folder {
            info: InvFolder {
                id: root,
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
    crate::world::inventory::demo::seed(&mut world.inventory, DEMO_AGENT);
    world
}

/// An animation, a wait, and the lines asked for.
fn definition(lines: &[&str]) -> Gesture {
    let mut steps = vec![
        Step::Animation {
            name: "Wave".into(),
            asset: ANIM,
            flags: 0,
        },
        Step::Wait {
            seconds: 1.0,
            flags: WAIT_TIME,
        },
    ];
    steps.extend(lines.iter().map(|l| Step::Chat {
        text: (*l).into(),
        flags: 0,
    }));
    Gesture {
        trigger: "/wave".into(),
        steps,
        ..Default::default()
    }
}

/// What the app gives the tools: the playback manager (the demo gesture
/// loaded and active), the failed loads, the chat bar's commands and a
/// pose library folder.
struct Fixture {
    manager: Manager,
    failures: BTreeSet<(Uuid, Uuid)>,
    commands: crate::cmdline::ChatCommandSettings,
    poses: tempfile::TempDir,
}

impl Fixture {
    fn new(world: &World, gesture: Gesture) -> Self {
        let mut manager = Manager::default();
        let asset = world.inventory.items[&GESTURE].asset_id;
        manager.entries.insert(
            GESTURE,
            Entry {
                asset,
                definition: gesture,
            },
        );
        manager.active.insert(GESTURE);
        Self {
            manager,
            failures: BTreeSet::new(),
            commands: Default::default(),
            poses: tempfile::tempdir().expect("temp dir"),
        }
    }

    fn dir(&self) -> PathBuf {
        self.poses.path().join("poses")
    }
}

fn ctx<'a>(world: &'a World, f: &'a Fixture) -> Ctx<'a> {
    Ctx {
        inv: &world.inventory,
        session: Session::Demo,
        worn: HashSet::new(),
        attachment_points: HashMap::new(),
        inventory_locked: world.rlva.has("showinv"),
        protected: HashSet::new(),
        outfit_points: HashMap::new(),
        stage: super::camera::Stage {
            in_world: true,
            ..Default::default()
        },
        viewer_session: None,
        identity: Default::default(),
        nav: Default::default(),
        content: Default::default(),
        objects: Scene {
            world: Some(world),
            props: &[],
        },
        avatar: Avatar {
            gestures: Some(&f.manager),
            failures: Some(&f.failures),
            chat_commands: Some(&f.commands),
            poses: Some(f.dir()),
        },
    }
}

fn allow_all() -> Permissions {
    let mut p = Permissions::default();
    for class in Class::ALL {
        p.set(class, Level::Allow);
    }
    p
}

fn desk_with(permissions: Permissions) -> Desk {
    let mut desk = Desk::default();
    desk.set_permissions(permissions);
    desk
}

fn call(tool: &str, args: Value) -> (Pending, Receiver<Result<Value, ToolError>>) {
    Pending::detached(tool, args, false, Some("Claude Code"))
}

fn answered(answer: &Receiver<Result<Value, ToolError>>) -> Result<Value, ToolError> {
    answer.try_recv().expect("answered")
}

fn code(result: Result<Value, ToolError>) -> ErrorCode {
    result.expect_err("refused").code
}

/// A read or a change's planning answer now (nothing run).
fn take(desk: &mut Desk, world: &World, f: &Fixture, tool: &str, args: Value) -> Result<Value, ToolError> {
    let (c, answer) = call(tool, args);
    desk.take(c, &ctx(world, f), Instant::now());
    answer.try_recv().unwrap_or_else(|_| Ok(json!({"waiting": true})))
}

/// The change the desk lets run now, planned again and run as the app
/// does; the planned work comes back.
fn run(desk: &mut Desk, world: &World, f: &mut Fixture, now: Instant) -> Option<Work> {
    let call = desk.tick(&IDLE, now).into_iter().next()?;
    let login = desk.login();
    let call = desk.recheck(call, login, None, &ctx(world, f), now)?;
    let planned = match desk.avatar_plan(&call, &ctx(world, f)) {
        Ok(planned) => planned,
        Err(e) => {
            call.answer(Err(e));
            return None;
        }
    };
    match planned.work.clone() {
        Work::Gesture(work) => match *work {
            GestureWork::Play { key, item, definition } => {
                // the app's Preview path
                f.manager.play(key, definition, None);
                desk.gestures().started(call, key, item, planned.reply.clone(), now);
            }
            GestureWork::Stop { key } => {
                let _ = f.manager.stop_owned(key);
                desk.gestures().stopped(call, key, planned.reply.clone());
            }
        },
        Work::Pose(work) => desk.posing().run(call, *work, planned.reply.clone(), world, now),
        Work::Nothing => call.answer(Ok(planned.reply.clone())),
        other => panic!("not a gesture or pose change: {other:?}"),
    }
    Some(planned.work)
}

/// A change taken then run at once (Allow).
fn change(desk: &mut Desk, world: &World, f: &mut Fixture, tool: &str, args: Value) -> Result<Value, ToolError> {
    let (c, answer) = call(tool, args);
    let now = Instant::now();
    desk.take(c, &ctx(world, f), now);
    if let Ok(result) = answer.try_recv() {
        return result;
    }
    run(desk, world, f, now);
    answer.try_recv().unwrap_or_else(|_| Ok(json!({"waiting": true})))
}

/// Wait for `answer`, ticking the gesture and pose tools (their workers).
fn waited(desk: &mut Desk, world: &World, f: &Fixture, answer: &Receiver<Result<Value, ToolError>>) -> Result<Value, ToolError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        desk.tick_avatar(&ctx(world, f), Instant::now());
        if let Ok(result) = answer.try_recv() {
            return result;
        }
        assert!(Instant::now() < deadline, "answered in time");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn status(desk: &mut Desk, world: &World, f: &Fixture) -> Value {
    take(desk, world, f, STATUS, json!({"item_id": GESTURE.to_string()})).expect("status")
}

fn revision(desk: &mut Desk, world: &World, f: &Fixture) -> String {
    status(desk, world, f)["revision"].as_str().expect("revision").to_owned()
}

fn play_args(revision: &str, request: &str) -> Value {
    json!({"item_id": GESTURE.to_string(), "revision": revision, "client_request_id": request})
}

fn playback(played: &Value) -> Uuid {
    played["playback_id"].as_str().and_then(|k| k.parse().ok()).expect("playback id")
}

fn no_asset_ids(value: &Value, world: &World) {
    let text = value.to_string();
    for item in world.inventory.items.values() {
        if !item.asset_id.is_nil() {
            assert!(!text.contains(&item.asset_id.to_string()), "an asset id leaked: {text}");
        }
    }
    assert!(!text.contains(&ANIM.to_string()), "an animation id leaked: {text}");
}

fn item(world: &World, id: Uuid) -> InvItem {
    world.inventory.items[&id].clone()
}

#[test]
fn a_gesture_is_read_with_its_steps_a_revision_and_no_asset_id() {
    let world = world();
    let f = Fixture::new(&world, definition(&["Hello there", "/28574 T4"]));
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    let read = status(&mut desk, &world, &f);
    assert_eq!(read["item_id"], json!(GESTURE));
    assert_eq!(read["active"], true);
    assert_eq!(read["step_count"], 4);
    assert_eq!(read["steps"][0]["kind"], "animation");
    assert_eq!(read["steps"][0]["builtin"], false);
    assert_eq!(read["steps"][1]["seconds"], 1.0);
    assert_eq!(read["steps"][3], json!({"kind": "chat", "channel": 28574, "text": "T4"}));
    assert_eq!(read["says_something"], true);
    assert_eq!(read["playable"], true, "{read}");
    assert!(read["revision"].as_str().is_some_and(|r| r.starts_with("gr-")));
    assert!(read["note"].is_string());
    no_asset_ids(&read, &world);
    // the same revision while nothing changed; another once it is edited
    let first = read["revision"].clone();
    assert_eq!(status(&mut desk, &world, &f)["revision"], first);
    let edited = Fixture::new(&world, definition(&["Hello"]));
    assert_ne!(status(&mut desk, &world, &edited)["revision"], first);
    // the active list
    let list = take(&mut desk, &world, &f, LIST, json!({})).expect("list");
    assert_eq!(list["total"], 1);
    assert_eq!(list["gestures"][0]["trigger"], "/wave");
    assert_eq!(list["gestures"][0]["loaded"], true);
    no_asset_ids(&list, &world);
    assert_eq!(
        code(take(&mut desk, &world, &f, LIST, json!({"limit": 0}))),
        ErrorCode::InvalidArguments
    );
}

#[test]
fn a_status_loads_the_gesture_without_activating_it_then_answers_or_fails() {
    let world = world();
    let mut f = Fixture::new(&world, definition(&[]));
    f.manager.entries.clear();
    f.manager.active.clear();
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    let (c, answer) = call(STATUS, json!({"item_id": GESTURE.to_string()}));
    let now = Instant::now();
    desk.take(c, &ctx(&world, &f), now);
    assert!(answer.try_recv().is_err(), "waits for the asset");
    assert_eq!(desk.gestures().take_loads(), [GESTURE]);
    desk.tick_avatar(&ctx(&world, &f), now);
    assert!(answer.try_recv().is_err());
    // loaded (not activated): answered
    let asset = world.inventory.items[&GESTURE].asset_id;
    f.manager.entries.insert(
        GESTURE,
        Entry {
            asset,
            definition: definition(&[]),
        },
    );
    desk.tick_avatar(&ctx(&world, &f), now);
    let read = answered(&answer).expect("read");
    assert_eq!(read["active"], false);
    // a load that fails
    f.manager.entries.clear();
    let (c, answer) = call(STATUS, json!({"item_id": GESTURE.to_string()}));
    desk.take(c, &ctx(&world, &f), now);
    let _ = desk.gestures().take_loads();
    f.failures.insert((GESTURE, asset));
    desk.tick_avatar(&ctx(&world, &f), now);
    assert_eq!(code(answered(&answer)), ErrorCode::Failed);
    // and one that never comes
    f.failures.clear();
    let (c, answer) = call(STATUS, json!({"item_id": GESTURE.to_string()}));
    desk.take(c, &ctx(&world, &f), now);
    desk.tick_avatar(&ctx(&world, &f), now + Duration::from_secs(21));
    assert_eq!(code(answered(&answer)), ErrorCode::Timeout);
}

#[test]
fn only_own_copyable_gestures_outside_the_library_and_trash_are_read() {
    let mut world = world();
    let f = Fixture::new(&world, definition(&[]));
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    let read = |desk: &mut Desk, world: &World, id: Uuid| take(desk, world, &f, STATUS, json!({"item_id": id.to_string()}));
    assert_eq!(code(read(&mut desk, &world, SOUND)), ErrorCode::InvalidArguments);
    assert_eq!(code(read(&mut desk, &world, Uuid::from_u128(1))), ErrorCode::NotFound);
    // a link reads its original
    let link = Uuid::from_u128(0x11CC);
    let mut linked = item(&world, GESTURE);
    linked.id = link;
    linked.asset_type = 24;
    linked.asset_id = GESTURE;
    world.inventory.add_items(vec![linked]);
    assert_eq!(read(&mut desk, &world, link).expect("through the link")["item_id"], json!(GESTURE));
    // no Copy for the user
    let original = item(&world, GESTURE);
    let mut no_copy = original.clone();
    no_copy.owner_mask &= !actions::COPY;
    world.inventory.add_items(vec![no_copy]);
    assert_eq!(code(read(&mut desk, &world, GESTURE)), ErrorCode::NotAllowed);
    // in the Trash
    let mut trashed = original.clone();
    trashed.parent = actions::system(&world.inventory, 14).expect("the Trash");
    world.inventory.add_items(vec![trashed]);
    assert_eq!(code(read(&mut desk, &world, GESTURE)), ErrorCode::NotAllowed);
    // in the Library
    let library = Uuid::from_u128(0x11B0);
    world.inventory.folders.insert(
        library,
        Folder {
            info: InvFolder {
                id: library,
                name: "Library".into(),
                ..Default::default()
            },
            children: vec![],
            items: vec![],
            state: FetchState::Fetched,
            library: true,
        },
    );
    let mut shared = original;
    shared.parent = library;
    world.inventory.add_items(vec![shared]);
    assert_eq!(code(read(&mut desk, &world, GESTURE)), ErrorCode::NotAllowed);
    // RLVa @showinv refuses every inventory read
    let mut locked = self::world();
    locked.rlva.configure(true);
    locked.rlva_console_input("@showinv=n");
    assert_eq!(code(read(&mut desk, &locked, GESTURE)), ErrorCode::PermissionDenied);
}

#[test]
fn a_play_runs_once_under_its_own_key_and_its_end_is_followed() {
    let world = world();
    let mut f = Fixture::new(&world, definition(&[]));
    let mut desk = desk_with(allow_all());
    let rev = revision(&mut desk, &world, &f);
    let played = change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "play-1")).expect("played");
    let key = playback(&played);
    assert_ne!(key, GESTURE, "never the item's key");
    assert_eq!(played["started"], true);
    assert_eq!(played["state"], "done");
    assert!(f.manager.is_playing(key) && !f.manager.is_playing(GESTURE));
    assert!(f.manager.active.contains(&GESTURE), "its activation is untouched");
    assert_eq!(status(&mut desk, &world, &f)["playback"]["state"], "playing");
    // one playback at a time; the same request answers the first receipt
    assert_eq!(
        code(change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "play-2"))),
        ErrorCode::Busy
    );
    let again = change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "play-1")).expect("duplicate");
    assert_eq!(again["duplicate"], true);
    // the request id is required
    assert_eq!(
        code(take(
            &mut desk,
            &world,
            &f,
            PLAY,
            json!({"item_id": GESTURE.to_string(), "revision": rev})
        )),
        ErrorCode::InvalidArguments
    );
    // its end, once the manager no longer plays it
    let _ = f.manager.stop(key);
    desk.tick_avatar(&ctx(&world, &f), Instant::now());
    assert_eq!(status(&mut desk, &world, &f)["playback"]["state"], "finished");
    let stop = change(&mut desk, &world, &mut f, STOP, json!({"playback_id": key.to_string()})).expect("ended");
    assert_eq!(stop["stopped"], false);
    assert_eq!(
        code(change(
            &mut desk,
            &world,
            &mut f,
            STOP,
            json!({"playback_id": Uuid::new_v4().to_string()})
        )),
        ErrorCode::NotFound
    );
    // a stop of a playback under way stops it
    let played = change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "play-3")).expect("played");
    let key = playback(&played);
    let stop = change(&mut desk, &world, &mut f, STOP, json!({"playback_id": key.to_string()})).expect("stopped");
    assert_eq!(stop["stopped"], true);
    assert!(!f.manager.is_playing(key));
    assert_eq!(status(&mut desk, &world, &f)["playback"]["state"], "stopped");
    assert!(desk.gestures().take_stops().is_empty(), "the stop itself was the app's");
}

#[test]
fn a_play_is_refused_when_stale_too_long_disabled_or_under_sendgesture() {
    let mut world = world();
    let mut f = Fixture::new(&world, definition(&[]));
    let mut desk = desk_with(allow_all());
    // never inspected on this login, then another revision
    assert_eq!(
        code(change(&mut desk, &world, &mut f, PLAY, play_args("gr-0", "a"))),
        ErrorCode::Stale
    );
    let rev = revision(&mut desk, &world, &f);
    assert_eq!(
        code(change(&mut desk, &world, &mut f, PLAY, play_args("gr-1", "b"))),
        ErrorCode::Stale
    );
    // the gesture edited since its revision
    f.manager.entries.get_mut(&GESTURE).expect("entry").definition = definition(&["changed"]);
    assert_eq!(
        code(change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "c"))),
        ErrorCode::Stale
    );
    // more than 64 steps
    let mut long = definition(&[]);
    long.steps = (0..65)
        .map(|_| Step::Wait {
            seconds: 0.0,
            flags: WAIT_TIME,
        })
        .collect();
    f.manager.entries.get_mut(&GESTURE).expect("entry").definition = long;
    let read = status(&mut desk, &world, &f);
    assert_eq!(read["playable"], false);
    assert_eq!(read["steps"].as_array().map(Vec::len), Some(64));
    let rev = read["revision"].as_str().expect("revision").to_owned();
    assert_eq!(
        code(change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "d"))),
        ErrorCode::NotAllowed
    );
    // gestures turned off in the Gestures window
    f.manager.entries.get_mut(&GESTURE).expect("entry").definition = definition(&[]);
    f.manager.enabled = false;
    let rev = revision(&mut desk, &world, &f);
    assert_eq!(
        code(change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "e"))),
        ErrorCode::NotAllowed
    );
    f.manager.enabled = true;
    // RLVa @sendgesture
    world.rlva.configure(true);
    world.rlva_console_input("@sendgesture=n");
    let read = status(&mut desk, &world, &f);
    assert!(read["why_not"].to_string().contains("@sendgesture"), "{read}");
    let rev = read["revision"].as_str().expect("revision").to_owned();
    assert_eq!(
        code(change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "f"))),
        ErrorCode::NotAllowed
    );
}

#[test]
fn a_gesture_that_says_something_always_asks_and_never_runs_a_command_or_dodges_rlv() {
    let world = world();
    let mut f = Fixture::new(&world, definition(&["Hello there", "/28574 T4"]));
    let mut desk = desk_with(allow_all());
    let rev = revision(&mut desk, &world, &f);
    let (c, answer) = call(PLAY, play_args(&rev, "lines-1"));
    let now = Instant::now();
    desk.take(c, &ctx(&world, &f), now);
    let q = desk.question(now).expect("asked even on Allow");
    assert!(q.summary.contains("Hello there") && q.summary.contains("28574"), "{}", q.summary);
    desk.decide(q.number, true);
    run(&mut desk, &world, &mut f, now);
    assert_eq!(answered(&answer).expect("played")["started"], true);
    // without a line, Allow plays at once
    let mut quiet = Fixture::new(&world, definition(&[]));
    let mut desk = desk_with(allow_all());
    let rev = revision(&mut desk, &world, &quiet);
    let (c, answer) = call(PLAY, play_args(&rev, "quiet-1"));
    desk.take(c, &ctx(&world, &quiet), now);
    assert!(desk.question(now).is_none());
    run(&mut desk, &world, &mut quiet, now);
    assert!(answered(&answer).is_ok());
    // a line the chat bar would run as a viewer command
    for command in ["/standup", "tp2cam"] {
        let f = Fixture::new(&world, definition(&[command]));
        let mut desk = desk_with(allow_all());
        let read = status(&mut desk, &world, &f);
        assert_eq!(read["playable"], false, "{command}");
        assert!(read["why_not"].to_string().contains("viewer command"), "{read}");
    }
    // RLVa would block or redirect a line
    for (restriction, line) in [
        ("@sendchat=n", "Hello there"),
        ("@redirchat:123=n", "Hello there"),
        ("@sendchannel=n", "/28574 T4"),
    ] {
        let mut world = self::world();
        world.rlva.configure(true);
        world.rlva_console_input(restriction);
        let mut f = Fixture::new(&world, definition(&[line]));
        let mut desk = desk_with(allow_all());
        let read = status(&mut desk, &world, &f);
        assert_eq!(read["playable"], false, "{restriction}");
        let rev = read["revision"].as_str().expect("revision").to_owned();
        assert_eq!(
            code(change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "rlv"))),
            ErrorCode::NotAllowed,
            "{restriction}"
        );
    }
}

#[test]
fn the_playback_stops_when_its_permission_access_login_or_time_goes() {
    let world = world();
    let mut f = Fixture::new(&world, definition(&[]));
    let mut desk = desk_with(allow_all());
    let rev = revision(&mut desk, &world, &f);
    let key = playback(&change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "r-1")).expect("played"));
    // Gestures set to Never
    let mut never = allow_all();
    never.set(Class::Gestures, Level::Never);
    desk.set_permissions(never);
    desk.tick_avatar(&ctx(&world, &f), Instant::now());
    assert_eq!(desk.gestures().take_stops(), [key]);
    assert_eq!(status(&mut desk, &world, &f)["playback"]["state"], "revoked");
    // 120 seconds
    desk.set_permissions(allow_all());
    let key = playback(&change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "r-2")).expect("played"));
    desk.tick_avatar(&ctx(&world, &f), Instant::now() + Duration::from_secs(121));
    assert_eq!(desk.gestures().take_stops(), [key]);
    assert_eq!(status(&mut desk, &world, &f)["playback"]["state"], "expired");
    // a login change
    let key = playback(&change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "r-3")).expect("played"));
    let login = desk.login();
    desk.set_login(login + 1, Some("vs-other".into()));
    assert_eq!(desk.gestures().take_stops(), [key]);
    // access turned off
    let mut desk = desk_with(allow_all());
    let rev = revision(&mut desk, &world, &f);
    let key = playback(&change(&mut desk, &world, &mut f, PLAY, play_args(&rev, "r-4")).expect("played"));
    desk.close();
    assert_eq!(desk.gestures().take_stops(), [key]);
}

fn start(desk: &mut Desk, world: &World, f: &mut Fixture) -> String {
    let started = change(desk, world, f, pose::START, json!({})).expect("started");
    assert_eq!(started["revision"], 0);
    started["pose_session"].as_str().expect("session").to_owned()
}

fn head_turn(angle: f32) -> [f32; 4] {
    Quat::from_rotation_z(angle).to_array()
}

fn head(desk: &mut Desk, world: &World, f: &Fixture) -> Quat {
    let joints = take(desk, world, f, pose::JOINTS, json!({"joints": ["mHead"]})).expect("joints");
    let q: Vec<f32> = joints["joints"]["mHead"]
        .as_array()
        .expect("head")
        .iter()
        .map(|v| v.as_f64().unwrap_or_default() as f32)
        .collect();
    Quat::from_xyzw(q[0], q[1], q[2], q[3])
}

#[test]
fn a_pose_session_holds_the_avatar_and_turns_joints_at_its_revision() {
    let world = world();
    let mut f = Fixture::new(&world, definition(&[]));
    let mut desk = desk_with(allow_all());
    let before = take(&mut desk, &world, &f, pose::STATUS, json!({})).expect("status");
    assert_eq!(before["active"], false);
    let rest = take(&mut desk, &world, &f, pose::JOINTS, json!({"joints": ["mHead"]})).expect("joints");
    assert_eq!(rest["source"], "avatar");
    let session = start(&mut desk, &world, &mut f);
    assert!(session.starts_with("ps-"));
    assert!(desk.posing().layer(false).is_some(), "shown for the avatar");
    let read = take(&mut desk, &world, &f, pose::STATUS, json!({})).expect("status");
    assert_eq!((read["active"].as_bool(), read["revision"].as_u64()), (Some(true), Some(0)));
    assert_eq!(read["shown_to_others"], false);
    // a second start is refused
    assert_eq!(code(change(&mut desk, &world, &mut f, pose::START, json!({}))), ErrorCode::Conflict);
    let turned = change(
        &mut desk,
        &world,
        &mut f,
        pose::UPDATE,
        json!({"pose_session": session, "revision": 0, "rotations": {"mHead": head_turn(0.4)}}),
    )
    .expect("turned");
    assert_eq!((turned["revision"].as_u64(), turned["joints_changed"].as_u64()), (Some(1), Some(1)));
    assert!(head(&mut desk, &world, &f).dot(Quat::from_rotation_z(0.4)).abs() > 0.9999);
    let all = take(&mut desk, &world, &f, pose::JOINTS, json!({})).expect("all");
    assert_eq!(all["source"], "session");
    assert!(all["count"].as_u64().unwrap_or_default() > 100, "{}", all["count"]);
    assert!(all["joints"].get("mPelvis").is_none());
    // a stale revision, the pelvis, a rotation not of unit length, an
    // unknown joint, a collision volume, too many joints, another session
    let update = |rotations: Value, revision: u64| json!({"pose_session": session, "revision": revision, "rotations": rotations});
    let cases = [
        (update(json!({"mHead": head_turn(0.1)}), 0), ErrorCode::Stale),
        (update(json!({"mPelvis": head_turn(0.1)}), 1), ErrorCode::NotAllowed),
        (update(json!({"mHead": [0.0, 0.0, 0.5, 0.5]}), 1), ErrorCode::InvalidArguments),
        (update(json!({"mNope": head_turn(0.1)}), 1), ErrorCode::InvalidArguments),
        (update(json!({"CHEST": head_turn(0.1)}), 1), ErrorCode::InvalidArguments),
        (
            json!({"pose_session": "ps-x", "revision": 1, "rotations": {"mHead": head_turn(0.1)}}),
            ErrorCode::NotFound,
        ),
    ];
    for (args, expected) in cases {
        assert_eq!(
            code(change(&mut desk, &world, &mut f, pose::UPDATE, args.clone())),
            expected,
            "{args}"
        );
    }
    let many: serde_json::Map<String, Value> = world
        .avatar_lib
        .rig
        .canonical_names
        .iter()
        .filter(|n| n.starts_with('m') && *n != "mPelvis")
        .take(65)
        .map(|n| (n.clone(), json!(head_turn(0.0))))
        .collect();
    assert_eq!(
        code(change(&mut desk, &world, &mut f, pose::UPDATE, update(Value::Object(many), 1))),
        ErrorCode::InvalidArguments
    );
}

#[test]
fn undo_reset_and_stop_follow_the_session() {
    let world = world();
    let mut f = Fixture::new(&world, definition(&[]));
    let mut desk = desk_with(allow_all());
    let session = start(&mut desk, &world, &mut f);
    let at = |revision: u64| json!({"pose_session": session, "revision": revision});
    let nothing = change(&mut desk, &world, &mut f, pose::UNDO, at(0)).expect("nothing to undo");
    assert_eq!(nothing["undone"], false);
    for (revision, angle) in [(0, 0.3), (1, 0.6)] {
        change(
            &mut desk,
            &world,
            &mut f,
            pose::UPDATE,
            json!({"pose_session": session, "revision": revision, "rotations": {"mHead": head_turn(angle)}}),
        )
        .expect("turned");
    }
    let undone = change(&mut desk, &world, &mut f, pose::UNDO, at(2)).expect("undone");
    assert_eq!(undone["revision"], 3);
    assert!(head(&mut desk, &world, &f).dot(Quat::from_rotation_z(0.3)).abs() > 0.9999);
    let reset = change(&mut desk, &world, &mut f, pose::RESET, at(3)).expect("reset");
    assert_eq!(reset["joints_changed"], 0);
    assert!(head(&mut desk, &world, &f).dot(Quat::IDENTITY).abs() > 0.9999);
    let read = take(&mut desk, &world, &f, pose::STATUS, json!({})).expect("status");
    assert_eq!(read["undo_steps"], 2, "the history stays after a reset");
    let stopped = change(&mut desk, &world, &mut f, pose::STOP, json!({"pose_session": session})).expect("stopped");
    assert_eq!(stopped["stopped"], true);
    assert!(desk.posing().layer(false).is_none());
    let read = take(&mut desk, &world, &f, pose::STATUS, json!({})).expect("status");
    assert_eq!(read["last_session"]["ended"], "stopped");
}

#[test]
fn the_viewer_poser_a_teleport_or_a_login_ends_or_refuses_a_session() {
    let mut world = world();
    let mut f = Fixture::new(&world, definition(&[]));
    let mut desk = desk_with(allow_all());
    // the viewer's poser poses the user: a start is refused
    assert!(desk.posing().layer(true).is_none());
    assert_eq!(
        code(change(&mut desk, &world, &mut f, pose::START, json!({}))),
        ErrorCode::NotAllowed
    );
    assert!(desk.posing().layer(false).is_none());
    // it takes over a session under way
    start(&mut desk, &world, &mut f);
    assert!(desk.posing().layer(true).is_none());
    let read = take(&mut desk, &world, &f, pose::STATUS, json!({})).expect("status");
    assert_eq!(read["last_session"]["ended"], "superseded_by_viewer_poser");
    assert!(desk.posing().layer(false).is_none());
    // a teleport
    start(&mut desk, &world, &mut f);
    world.teleporting = true;
    desk.tick_avatar(&ctx(&world, &f), Instant::now());
    world.teleporting = false;
    assert!(desk.posing().layer(false).is_none());
    // Pose set to Never
    start(&mut desk, &world, &mut f);
    let mut never = allow_all();
    never.set(Class::Pose, Level::Never);
    desk.set_permissions(never);
    desk.tick_avatar(&ctx(&world, &f), Instant::now());
    assert!(desk.posing().layer(false).is_none());
    desk.set_permissions(allow_all());
    // a login change
    start(&mut desk, &world, &mut f);
    let login = desk.login();
    desk.set_login(login + 1, Some("vs-other".into()));
    assert!(desk.posing().layer(false).is_none());
}

/// A save, asked and allowed, then written by its worker.
fn save(desk: &mut Desk, world: &World, f: &mut Fixture, session: &str, name: &str, request: &str) -> Result<Value, ToolError> {
    let (c, answer) = call(
        pose::SAVE,
        json!({"pose_session": session, "revision": 1, "name": name, "client_request_id": request}),
    );
    let now = Instant::now();
    desk.take(c, &ctx(world, f), now);
    if let Ok(result) = answer.try_recv() {
        return result;
    }
    let q = desk.question(now).expect("always asks");
    assert!(q.summary.contains(name), "{}", q.summary);
    desk.decide(q.number, true);
    run(desk, world, f, now);
    waited(desk, world, f, &answer)
}

#[test]
fn presets_list_hand_presets_and_rotation_only_files_without_paths_and_load() {
    let world = world();
    let mut f = Fixture::new(&world, definition(&[]));
    let dir = f.dir();
    let mut desk = desk_with(allow_all());
    let session = start(&mut desk, &world, &mut f);
    change(
        &mut desk,
        &world,
        &mut f,
        pose::UPDATE,
        json!({"pose_session": session, "revision": 0, "rotations": {"mHead": head_turn(0.5)}}),
    )
    .expect("turned");
    // a save always asks, writes a new file and never overwrites one
    let saved = save(&mut desk, &world, &mut f, &session, "Look", "save-1").expect("saved");
    assert_eq!(saved["saved"], true);
    let file = dir.join("Look.aurora-pose.json");
    let bytes = std::fs::read(&file).expect("written");
    assert_eq!(
        code(save(&mut desk, &world, &mut f, &session, "Look", "save-2")),
        ErrorCode::Conflict
    );
    assert_eq!(std::fs::read(&file).expect("kept"), bytes);
    assert_eq!(
        code(save(&mut desk, &world, &mut f, &session, "../out", "save-3")),
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        code(take(
            &mut desk,
            &world,
            &f,
            pose::SAVE,
            json!({"pose_session": session, "revision": 1, "name": "x"})
        )),
        ErrorCode::InvalidArguments,
        "a save needs its request id"
    );
    // a file that moves a joint, one too large, something else
    let mut moved: Value = serde_json::from_slice(&bytes).expect("json");
    for joint in moved["joints"].as_array_mut().expect("joints") {
        if joint["name"] == "mHead" {
            joint["translation"] = json!([0.0, 0.1, 0.0]);
        }
    }
    std::fs::write(dir.join("Moved.aurora-pose.json"), serde_json::to_vec(&moved).expect("bytes")).expect("written");
    std::fs::write(dir.join("Big.aurora-pose.json"), vec![b' '; pose::PRESET_BYTES + 1]).expect("written");
    std::fs::write(dir.join("notes.txt"), b"hello").expect("written");
    let (c, answer) = call(pose::PRESETS, json!({}));
    desk.take(c, &ctx(&world, &f), Instant::now());
    let listing = waited(&mut desk, &world, &f, &answer).expect("listed");
    let rows = listing["presets"].as_array().expect("presets");
    assert_eq!(rows.iter().filter(|p| p["kind"] == "hand_preset").count(), 5);
    let look = rows.iter().find(|p| p["name"] == "Look").expect("the saved pose").clone();
    assert_eq!(look["loadable"], true);
    let moved = rows
        .iter()
        .find(|p| p["why_not"] == "it moves joints")
        .expect("the moved pose")
        .clone();
    assert_eq!(moved["loadable"], false);
    assert_eq!(rows.len(), 7, "{listing}");
    let hand = rows.iter().find(|p| p["preset_id"] == "hand-pointing").expect("pointing").clone();
    let text = listing.to_string();
    assert!(
        !text.contains(dir.to_string_lossy().as_ref()) && !text.contains(".aurora-pose.json"),
        "{text}"
    );
    // load the file back after a reset, a hand preset, a stale revision
    change(
        &mut desk,
        &world,
        &mut f,
        pose::RESET,
        json!({"pose_session": session, "revision": 1}),
    )
    .expect("reset");
    let load = |preset: &Value, revision: u64| json!({"pose_session": session, "revision": revision, "preset_id": preset["preset_id"], "preset_revision": preset["preset_revision"]});
    let loaded = change(&mut desk, &world, &mut f, pose::LOAD, load(&look, 2)).expect("loaded");
    assert_eq!(loaded["joints_changed"], 1);
    assert!(head(&mut desk, &world, &f).dot(Quat::from_rotation_z(0.5)).abs() > 0.9999);
    let loaded = change(&mut desk, &world, &mut f, pose::LOAD, load(&hand, 3)).expect("hand preset");
    assert!(loaded["joints_changed"].as_u64().unwrap_or_default() > 1);
    assert_eq!(
        code(change(&mut desk, &world, &mut f, pose::LOAD, load(&moved, 4))),
        ErrorCode::NotAllowed
    );
    let mut wrong = look.clone();
    wrong["preset_revision"] = json!("0");
    assert_eq!(
        code(change(&mut desk, &world, &mut f, pose::LOAD, load(&wrong, 4))),
        ErrorCode::Stale
    );
}

#[test]
fn gestures_and_poses_ask_in_ask_before_changes_and_are_refused_read_only() {
    let world = world();
    let f = Fixture::new(&world, definition(&[]));
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let now = Instant::now();
    let rev = revision(&mut desk, &world, &f);
    for (tool, args) in [(PLAY, play_args(&rev, "ask-1")), (pose::START, json!({}))] {
        let (c, answer) = call(tool, args);
        desk.take(c, &ctx(&world, &f), now);
        assert!(answer.try_recv().is_err(), "{tool} waits for the user");
        let q = desk.question(now).expect("asks");
        desk.decide(q.number, false);
        let _ = desk.tick(&IDLE, now);
        assert_eq!(code(answered(&answer)), ErrorCode::PermissionDenied, "{tool}");
    }
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    let (c, answer) = call(pose::START, json!({}));
    desk.take(c, &ctx(&world, &f), now);
    assert_eq!(code(answered(&answer)), ErrorCode::PermissionDenied);
}

/// The sidecar's MCP layer → a real 127.0.0.1 socket → the bridge (Ask
/// before changes) → the desk, the dialog allowed: a gesture read, then
/// played, then a pose session started.
#[test]
fn an_assistant_inspects_and_plays_a_gesture_then_poses_end_to_end() {
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
    let folder = dir.path().to_path_buf();
    let world = world();
    let mut f = Fixture::new(&world, definition(&[]));
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        let mut ask = |line: &str| -> Value { serde_json::from_str(&server.handle_line(line).expect("answer")).expect("json") };
        ask(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code"}}}"#,
        );
        let read = ask(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"gesture_status","arguments":{{"item_id":"{GESTURE}"}}}}}}"#
        ));
        let revision = read["result"]["structuredContent"]["revision"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let played = ask(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"gesture_play","arguments":{{"item_id":"{GESTURE}","revision":"{revision}","client_request_id":"e2e-gesture-1"}}}}}}"#
        ));
        let posed = ask(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"pose_start","arguments":{}}}"#);
        (read, played, posed)
    });
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let mut questions = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        while let Ok(pending) = calls.try_recv() {
            desk.take(pending, &ctx(&world, &f), now);
        }
        if let Some(q) = desk.question(now) {
            questions.push(q.summary.clone());
            desk.decide(q.number, true);
        }
        run(&mut desk, &world, &mut f, now);
        desk.tick_avatar(&ctx(&world, &f), now);
        std::thread::sleep(Duration::from_millis(5));
    }
    let (read, played, posed) = assistant.join().expect("assistant");
    assert_eq!(read["result"]["isError"], false, "{read}");
    no_asset_ids(&read, &world);
    assert_eq!(played["result"]["isError"], false, "{played}");
    assert_eq!(played["result"]["structuredContent"]["started"], true, "{played}");
    assert_eq!(posed["result"]["isError"], false, "{posed}");
    assert_eq!(questions.len(), 2, "{questions:?}");
    assert!(desk.posing().layer(false).is_some());
    bridge.stop();
}

#[test]
fn the_demo_recipe_reads_plays_poses_loads_and_saves() {
    let world = world();
    assert!(posing::demo::ready(&world));
    let (tool, args) = gestures::demo::status();
    assert_eq!(tool, STATUS);
    assert_eq!(args["item_id"], json!(GESTURE.to_string()));
    let (tool, args) = gestures::demo::play_after(&json!({"revision": "gr-1"})).expect("play");
    assert_eq!(tool, PLAY);
    assert!(args["client_request_id"].is_string());
    let (tool, _) = posing::demo::start();
    assert_eq!(tool, pose::START);
    let (tool, args) = posing::demo::turn_after(&json!({"pose_session": "ps-1", "revision": 0})).expect("turn");
    assert_eq!(tool, pose::UPDATE);
    assert!(args["rotations"]["mHead"].is_array());
    let listing = json!({"presets": [{"preset_id": "hand-pointing", "preset_revision": "builtin-1"}]});
    let (tool, args) = posing::demo::load_after(&listing, "ps-1", 1).expect("load");
    assert_eq!((tool, args["revision"].as_u64()), (pose::LOAD, Some(1)));
    let (tool, args) = posing::demo::save("ps-1", 2, "20261114");
    assert_eq!(tool, pose::SAVE);
    assert!(pose::valid_name(args["name"].as_str().unwrap_or_default()));
    let (tool, _) = posing::demo::status();
    assert_eq!(tool, pose::STATUS);
}
