//! Folderstorm: the navigation tools (phase 3, part 4) on the demo plaza:
//! the rules of a move, a take-off or landing and a teleport (bounds,
//! region, sitting and flying, RLVa), the navigator's ends and stop reasons,
//! the desk's rules for them (the class, the teleport that always asks and
//! needs its id, lanes and the rate limit, stop, the login and access), and
//! end to end over a real socket.

use super::change_tests::{ctx, demo_inventory};
use super::changes::{Planned, Work};
use super::desk::{Bake, Desk};
use super::navigation::{self, Act, Frame, Mode, Move, Native, PilotId, Reason, Stage};
use super::tools::Ctx;
use crate::demo::DEMO_AGENT;
use crate::world::World;
use crate::world::inventory::Inventory;
use aurora_net::{NetEvent, ParcelInfo, RegionHandle, parcel_flags as pf};
use folderstorm_mcp_bridge::protocol::navigation::{FLYING_SET, MOVE_TO, STATUS, STOP, TELEPORT};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{Class, ErrorCode, Level, Permissions, Preset, ToolError};
use glam::{DVec3, Vec3};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// Where our avatar stands on the plaza.
const ME: Vec3 = Vec3::new(120.0, 120.0, 25.0);

fn plaza() -> (World, RegionHandle) {
    let mut world = World::new(Arc::new(crate::scene::avatar::AvatarLibrary::load()));
    world.agent_id = DEMO_AGENT;
    let mut handle = 0;
    for ev in crate::demo::events() {
        match &ev {
            NetEvent::MainRegionChanged { handle: h } => handle = *h,
            NetEvent::RegionHandshake(_) => {}
            _ => continue,
        }
        let _ = world.apply(ev);
    }
    world.agent.position = ME;
    (world, handle)
}

/// The plaza's parcel, where flying is allowed.
fn flyable(world: &mut World) {
    world.parcel = Some(Arc::new(ParcelInfo {
        local_id: 7,
        name: "Aurora Plaza".into(),
        flags: pf::ALLOW_FLY,
        ..Default::default()
    }));
}

fn restrict(world: &mut World, commands: &str) {
    world.rlva.configure(true);
    world.rlva_console_input(&format!("@{commands}"));
}

fn cx<'a>(inv: &'a Inventory, world: &'a World) -> Ctx<'a> {
    let mut cx = ctx(inv);
    cx.nav = Stage {
        world: Some(world),
        in_world: true,
        native: Native::default(),
    };
    cx
}

fn plan(world: &World, tool: &str, args: Value) -> Result<Planned, ToolError> {
    let inv = demo_inventory();
    super::changes::plan(tool, &args, &cx(&inv, world))
}

fn refused(world: &World, tool: &str, args: Value) -> ErrorCode {
    match plan(world, tool, args.clone()) {
        Ok(p) => panic!("{tool} {args} was planned: {:?}", p.work),
        Err(e) => e.code,
    }
}

fn move_of(p: &Planned) -> Move {
    match &p.work {
        Work::Navigate(navigation::Work::Move(mv)) => mv.clone(),
        other => panic!("not a move: {other:?}"),
    }
}

fn origin(handle: RegionHandle) -> DVec3 {
    let (x, y) = aurora_net::handle_to_origin(handle);
    DVec3::new(x as f64, y as f64, 0.0)
}

#[test]
fn a_move_stays_in_its_region_within_64_m() {
    let (world, handle) = plaza();
    let p = plan(&world, MOVE_TO, json!({"position": [130.0, 120.0, 25.0]})).expect("planned");
    let mv = move_of(&p);
    assert_eq!((mv.mode, mv.region, mv.stop_distance), (Mode::Walk, handle, 1.0));
    assert_eq!(mv.timeout, Duration::from_secs(30));
    assert!(p.summary.contains("10 m"), "{}", p.summary);
    // a global position in the region is the same point
    let global = origin(handle) + DVec3::new(130.0, 120.0, 25.0);
    let p = plan(&world, MOVE_TO, json!({"global_position": global.to_array()})).expect("planned");
    assert_eq!(move_of(&p).target, Vec3::new(130.0, 120.0, 25.0));
    // a walk is measured flat, a flight in 3D
    let p = plan(
        &world,
        MOVE_TO,
        json!({"position": [170.0, 120.0, 80.0], "stop_distance": 2.5, "timeout_seconds": 5}),
    );
    let mv = move_of(&p.expect("a walk 50 m away"));
    assert_eq!((mv.stop_distance, mv.timeout), (2.5, Duration::from_secs(5)));
    // outside the region, too far, wrong arguments
    for (args, code) in [
        (json!({"position": [300.0, 120.0, 25.0]}), ErrorCode::NotAllowed),
        (json!({"position": [-1.0, 120.0, 25.0]}), ErrorCode::NotAllowed),
        (json!({"position": [185.0, 120.0, 25.0]}), ErrorCode::NotAllowed),
        (json!({"position": [130.0, 120.0, 5000.0]}), ErrorCode::InvalidArguments),
        (json!({"position": [130.0, 120.0]}), ErrorCode::InvalidArguments),
        (json!({"position": [130.0, "a", 25.0]}), ErrorCode::InvalidArguments),
        (json!({}), ErrorCode::InvalidArguments),
        (
            json!({"position": [130.0, 120.0, 25.0], "global_position": global.to_array()}),
            ErrorCode::InvalidArguments,
        ),
        (
            json!({"position": [130.0, 120.0, 25.0], "mode": "run"}),
            ErrorCode::InvalidArguments,
        ),
        (
            json!({"position": [130.0, 120.0, 25.0], "stop_distance": 5}),
            ErrorCode::InvalidArguments,
        ),
        (
            json!({"position": [130.0, 120.0, 25.0], "timeout_seconds": 31}),
            ErrorCode::InvalidArguments,
        ),
        (
            json!({"position": [130.0, 120.0, 25.0], "timeout_seconds": 0}),
            ErrorCode::InvalidArguments,
        ),
        (json!({"position": [130.0, 120.0, 25.0], "speed": 2}), ErrorCode::InvalidArguments),
    ] {
        assert_eq!(refused(&world, MOVE_TO, args.clone()), code, "{args}");
    }
    // already there: nothing to do, no dialog
    let p = plan(&world, MOVE_TO, json!({"position": [120.5, 120.0, 25.0]})).expect("planned");
    assert!(matches!(p.work, Work::Nothing));
    assert_eq!(p.reply["arrived"], true);
}

#[test]
fn not_ready_teleporting_or_logged_out() {
    let (mut world, _) = plaza();
    let inv = demo_inventory();
    let mut c = cx(&inv, &world);
    c.nav.in_world = false;
    let e = super::changes::plan(MOVE_TO, &json!({"position": [130.0, 120.0, 25.0]}), &c).expect_err("login screen");
    assert_eq!(e.code, ErrorCode::NotReady);
    world.teleporting = true;
    assert_eq!(
        refused(&world, TELEPORT, json!({"global_position": [256000.0, 256000.0, 25.0]})),
        ErrorCode::NotReady
    );
    let mut c = cx(&inv, &world);
    c.session = super::tools::Session::LoggedOut;
    let e = super::changes::plan(FLYING_SET, &json!({"flying": true}), &c).expect_err("logged out");
    assert_eq!(e.code, ErrorCode::NotReady);
}

#[test]
fn sitting_flying_and_the_viewer_s_own_walks() {
    let (mut world, _) = plaza();
    let walk = json!({"position": [130.0, 120.0, 25.0]});
    let fly = json!({"position": [130.0, 120.0, 40.0], "mode": "fly"});
    // a walk of the viewer's own is never interrupted
    let inv = demo_inventory();
    let mut c = cx(&inv, &world);
    c.nav.native.pilot = true;
    assert_eq!(super::changes::plan(MOVE_TO, &walk, &c).expect_err("busy").code, ErrorCode::Busy);
    assert_eq!(
        super::changes::plan(FLYING_SET, &json!({"flying": true}), &c)
            .expect_err("busy")
            .code,
        ErrorCode::Busy
    );
    c.nav.native = Native {
        pilot: false,
        blocks_walk: true,
    };
    assert_eq!(
        super::changes::plan(MOVE_TO, &walk, &c).expect_err("held").code,
        ErrorCode::NotAllowed
    );
    // no parcel known yet: no take-off, as the Fly button
    assert_eq!(refused(&world, MOVE_TO, fly.clone()), ErrorCode::NotAllowed);
    assert_eq!(refused(&world, FLYING_SET, json!({"flying": true})), ErrorCode::NotAllowed);
    flyable(&mut world);
    assert_eq!(move_of(&plan(&world, MOVE_TO, fly.clone()).expect("a flight")).mode, Mode::Fly);
    let p = plan(&world, FLYING_SET, json!({"flying": true})).expect("take off");
    assert!(matches!(p.work, Work::Navigate(navigation::Work::Flying(true))));
    // flying: no walk; landing; already flying needs nothing
    world.agent.flying = true;
    assert_eq!(refused(&world, MOVE_TO, walk.clone()), ErrorCode::NotAllowed);
    assert!(matches!(
        plan(&world, FLYING_SET, json!({"flying": true})).expect("nothing").work,
        Work::Nothing
    ));
    let p = plan(&world, FLYING_SET, json!({"flying": false})).expect("land");
    assert!(matches!(p.work, Work::Navigate(navigation::Work::Flying(false))));
    assert_eq!(move_of(&plan(&world, MOVE_TO, fly.clone()).expect("a flight")).mode, Mode::Fly);
    // sitting
    world.agent.flying = false;
    world.agent.ground_sit = true;
    assert_eq!(refused(&world, MOVE_TO, walk), ErrorCode::NotAllowed);
    assert_eq!(refused(&world, FLYING_SET, json!({"flying": true})), ErrorCode::NotAllowed);
    assert_eq!(refused(&world, FLYING_SET, json!({"flying": "yes"})), ErrorCode::InvalidArguments);
}

#[test]
fn rlv_restrictions_refuse_as_the_viewer_does() {
    let walk = json!({"position": [130.0, 120.0, 25.0]});
    // @fly: no take-off
    let (mut world, handle) = plaza();
    flyable(&mut world);
    restrict(&mut world, "fly=n");
    assert_eq!(refused(&world, FLYING_SET, json!({"flying": true})), ErrorCode::NotAllowed);
    assert_eq!(
        refused(&world, MOVE_TO, json!({"position": [130.0, 120.0, 40.0], "mode": "fly"})),
        ErrorCode::NotAllowed
    );
    // @tplocal: no walk farther than its limit, as a double click
    let (mut world, _) = plaza();
    restrict(&mut world, "tplocal:2=n");
    assert_eq!(refused(&world, MOVE_TO, walk.clone()), ErrorCode::NotAllowed);
    assert!(
        plan(&world, MOVE_TO, json!({"position": [121.5, 120.0, 25.0]})).is_ok(),
        "within 2 m"
    );
    // @tploc: no teleport to another region; this region stays possible
    let (mut world, _) = plaza();
    restrict(&mut world, "tploc=n");
    let far = origin(handle) + DVec3::new(1000.0, 1000.0, 30.0);
    assert_eq!(
        refused(&world, TELEPORT, json!({"global_position": far.to_array()})),
        ErrorCode::NotAllowed
    );
    let near = origin(handle) + DVec3::new(100.0, 100.0, 30.0);
    assert!(plan(&world, TELEPORT, json!({"global_position": near.to_array()})).is_ok());
    // @sittp: a local teleport farther than its limit
    let (mut world, _) = plaza();
    restrict(&mut world, "sittp:5=n");
    assert_eq!(
        refused(&world, TELEPORT, json!({"global_position": near.to_array()})),
        ErrorCode::NotAllowed
    );
    // @unsit while seated: neither a teleport nor a walk
    let (mut world, _) = plaza();
    restrict(&mut world, "unsit=n");
    world.agent.ground_sit = true;
    assert_eq!(
        refused(&world, TELEPORT, json!({"global_position": near.to_array()})),
        ErrorCode::NotAllowed
    );
}

#[test]
fn a_teleport_finds_its_region() {
    let (mut world, handle) = plaza();
    let o = origin(handle);
    // in the avatar's region
    let (h, local, name) = navigation::teleport_target(&world, o + DVec3::new(10.5, 20.0, 30.0));
    assert_eq!((h, local), (handle, Vec3::new(10.5, 20.0, 30.0)));
    assert!(name.is_some_and(|n| !n.is_empty()));
    // a region of the world map, of variable size
    let grid = ((o.x as u32) / 256 + 10, (o.y as u32) / 256);
    world.map.sims.insert(
        grid,
        crate::world::worldmap::SimInfo {
            name: "Far Var".into(),
            access: 13,
            flags: 0,
            image: uuid::Uuid::nil(),
            size_x: 512,
            size_y: 512,
        },
    );
    let at = DVec3::new(grid.0 as f64 * 256.0 + 300.0, grid.1 as f64 * 256.0 + 40.0, 22.0);
    let (h, local, name) = navigation::teleport_target(&world, at);
    assert_eq!(h, aurora_net::origin_to_handle(grid.0 * 256, grid.1 * 256));
    assert_eq!((local, name.as_deref()), (Vec3::new(300.0, 40.0, 22.0), Some("Far Var")));
    // nowhere the viewer knows: the 256 m slot under it
    let (h, local, name) = navigation::teleport_target(&world, DVec3::new(1000.0 * 256.0 + 7.0, 3.0 * 256.0 + 9.0, 50.0));
    assert_eq!(h, aurora_net::origin_to_handle(256_000, 768));
    assert_eq!((local, name), (Vec3::new(7.0, 9.0, 50.0), None));
    // the plan names it in the dialog, and checks the arguments
    let p = plan(&world, TELEPORT, json!({"global_position": at.to_array()})).expect("planned");
    assert!(p.summary.contains("Far Var"), "{}", p.summary);
    assert_eq!(p.reply["region"], "Far Var");
    for args in [
        json!({"global_position": [1.0, 2.0, 5000.0]}),
        json!({"global_position": [-1.0, 2.0, 30.0]}),
        json!({"global_position": [1.0, 2.0]}),
        json!({}),
        json!({"global_position": [1.0, 2.0, 3.0], "region": "x"}),
    ] {
        assert_eq!(refused(&world, TELEPORT, args.clone()), ErrorCode::InvalidArguments, "{args}");
    }
}

// --- the navigator and the desk ---

fn call(tool: &str, args: Value, ask: bool) -> (Pending, Receiver<Result<Value, ToolError>>) {
    Pending::detached(tool, args, ask, Some("Claude Code"))
}

fn answered(answer: &Receiver<Result<Value, ToolError>>) -> Result<Value, ToolError> {
    answer.try_recv().expect("answered")
}

const IDLE: Bake = Bake {
    outfit_change_pending: false,
    cof_version: 3,
    appearance_cof_version: 3,
    server_bakes: true,
};

fn levels(navigation: Level) -> Permissions {
    let mut p = Preset::ReadOnly.permissions();
    p.set(Class::Navigation, navigation);
    p
}

fn desk_with(permissions: Permissions) -> Desk {
    let mut desk = Desk::default();
    desk.set_permissions(permissions);
    desk
}

/// A frame where the avatar stands at `position` of `handle`, nothing else
/// happening.
fn frame(handle: RegionHandle, position: Vec3, pilot: Option<PilotId>) -> Frame {
    Frame {
        pilot,
        position,
        region: Some(handle),
        global: Some(origin(handle) + position.as_dvec3()),
        flying: false,
        sitting: false,
        teleporting: false,
        teleport_failed: false,
        manual: false,
        in_world: true,
        location_hidden: false,
    }
}

/// A walk 10 m east, allowed and started by "the app": its answer, its
/// operation and its autopilot.
fn walking(desk: &mut Desk, world: &World, now: Instant) -> (Receiver<Result<Value, ToolError>>, String, PilotId) {
    let inv = demo_inventory();
    let (c, answer) = call(MOVE_TO, json!({"position": [130.0, 120.0, 25.0], "timeout_seconds": 10}), false);
    desk.take(c, &cx(&inv, world), now);
    let run = desk.tick(&IDLE, now).into_iter().next().expect("allowed");
    let operation = run.operation_id.clone().expect("named");
    let planned = super::changes::plan(&run.tool, &run.arguments, &cx(&inv, world)).expect("planned");
    let pilot = PilotId(now);
    desk.start_move(run, move_of(&planned), pilot, now);
    (answer, operation, pilot)
}

fn state(desk: &mut Desk, world: &World, operation: &str) -> String {
    let inv = demo_inventory();
    let (c, answer) = call("assistant_operation_status", json!({"operation_id": operation}), false);
    desk.take(c, &cx(&inv, world), Instant::now());
    answered(&answer).expect("found")["receipt"]["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn last_reason(desk: &Desk) -> String {
    desk.navigator().last().map(|l| l.reason.code().to_owned()).unwrap_or_default()
}

#[test]
fn a_move_arrives_and_says_so() {
    let (world, handle) = plaza();
    let now = Instant::now();
    let mut desk = desk_with(levels(Level::Allow));
    let (answer, operation, pilot) = walking(&mut desk, &world, now);
    assert_eq!(state(&mut desk, &world, &operation), "running");
    // still walking: nothing
    assert!(desk.navigate(&frame(handle, ME, Some(pilot)), now).is_empty());
    assert!(answer.try_recv().is_err());
    // the autopilot stopped there by itself
    let acts = desk.navigate(&frame(handle, Vec3::new(129.2, 120.3, 25.0), None), now);
    assert!(acts.is_empty(), "nothing to stop: {acts:?}");
    let done = answered(&answer).expect("arrived");
    assert_eq!(done["arrived"], true);
    assert!(done["distance_left"].as_f64().is_some_and(|d| d < 1.0));
    assert_eq!(done["state"], "done");
    assert_eq!(state(&mut desk, &world, &operation), "done");
    assert_eq!(last_reason(&desk), "arrived");
}

#[test]
fn every_reason_a_move_stops() {
    let (world, handle) = plaza();
    let now = Instant::now();
    let here = |pilot| frame(handle, ME, pilot);
    let other = PilotId(now + Duration::from_millis(5));
    // (what the frame shows, the reason, whether its autopilot is stopped)
    let cases: Vec<(Box<dyn Fn(PilotId) -> (Frame, Instant)>, &str)> = vec![
        (
            Box::new(|p| {
                (
                    Frame {
                        manual: true,
                        ..here(Some(p))
                    },
                    now,
                )
            }),
            "user_input",
        ),
        (Box::new(|_| (here(Some(other)), now)), "replaced"),
        (Box::new(|p| (here(Some(p)), now + Duration::from_secs(11))), "timeout"),
        (
            Box::new(|p| {
                (
                    Frame {
                        teleporting: true,
                        ..here(Some(p))
                    },
                    now,
                )
            }),
            "teleported",
        ),
        (
            Box::new(|p| {
                (
                    Frame {
                        region: Some(handle + 1),
                        ..here(Some(p))
                    },
                    now,
                )
            }),
            "region_changed",
        ),
        (
            Box::new(|p| {
                (
                    Frame {
                        sitting: true,
                        ..here(Some(p))
                    },
                    now,
                )
            }),
            "sat_down",
        ),
        (
            Box::new(|p| {
                (
                    Frame {
                        in_world: false,
                        ..here(Some(p))
                    },
                    now,
                )
            }),
            "left_world",
        ),
        (
            Box::new(|_| {
                (
                    Frame {
                        flying: true,
                        ..here(None)
                    },
                    now,
                )
            }),
            "flight_changed",
        ),
        (
            Box::new(|_| {
                (
                    Frame {
                        sitting: true,
                        ..here(None)
                    },
                    now,
                )
            }),
            "sat_down",
        ),
        (
            Box::new(|_| {
                (
                    Frame {
                        manual: true,
                        ..here(None)
                    },
                    now,
                )
            }),
            "user_input",
        ),
        (Box::new(|_| (here(None), now)), "stopped_short"),
    ];
    for (make, reason) in cases {
        let mut desk = desk_with(levels(Level::Allow));
        let (answer, operation, pilot) = walking(&mut desk, &world, now);
        let (f, at) = make(pilot);
        let acts = desk.navigate(&f, at);
        // only its own autopilot is ever named: the app stops it if it still runs
        assert_eq!(acts, vec![Act::StopPilot(pilot)], "{reason}");
        let e = answered(&answer).expect_err(reason);
        assert_eq!(e.code, ErrorCode::Cancelled, "{reason}");
        assert!(e.message.starts_with("stopped: "), "{reason}: {e}");
        assert_eq!(last_reason(&desk), reason);
        assert_eq!(state(&mut desk, &world, &operation), "stopped", "{reason}: it was sent");
    }
}

#[test]
fn stop_by_operation_id_and_only_navigation() {
    let (world, handle) = plaza();
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(levels(Level::Allow));
    let (answer, operation, pilot) = walking(&mut desk, &world, now);
    let (c, stop) = call(STOP, json!({"operation_id": operation}), false);
    desk.take(c, &cx(&inv, &world), now);
    let stopped = answered(&stop).expect("stopped");
    assert_eq!(
        (stopped["stopped"].as_bool(), stopped["state"].as_str()),
        (Some(true), Some("stopped"))
    );
    assert_eq!(answered(&answer).expect_err("ended").code, ErrorCode::Cancelled);
    assert_eq!(last_reason(&desk), "assistant_stop");
    assert_eq!(desk.navigate(&frame(handle, ME, Some(pilot)), now), vec![Act::StopPilot(pilot)]);
    // again: it had ended
    let (c, again) = call(STOP, json!({"operation_id": operation}), false);
    desk.take(c, &cx(&inv, &world), now);
    assert_eq!(answered(&again).expect("answered")["stopped"], json!(null));
    // an unknown operation, and one that is not a navigation
    let (c, unknown) = call(STOP, json!({"operation_id": "op-0123456789abcdef0123456789abcdef"}), false);
    desk.take(c, &cx(&inv, &world), now);
    assert_eq!(answered(&unknown).expect_err("unknown").code, ErrorCode::NotFound);
    desk.set_permissions({
        let mut p = levels(Level::Allow);
        p.set(Class::Edit, Level::Ask);
        p
    });
    let (c, _rename) = call("inventory_create_folder", json!({"name": "x"}), false);
    desk.take(c, &cx(&inv, &world), now);
    let folder = desk.ledger().with(|s| s.recent(None, 1)[0].operation_id.clone());
    let (c, wrong) = call(STOP, json!({"operation_id": folder}), false);
    desk.take(c, &cx(&inv, &world), now);
    assert_eq!(answered(&wrong).expect_err("not navigation").code, ErrorCode::NotFound);
    // waiting for the dialog: cancelled, the dialog closes
    let mut desk = desk_with(levels(Level::Ask));
    let (c, asked) = call(MOVE_TO, json!({"position": [125.0, 120.0, 25.0]}), true);
    desk.take(c, &cx(&inv, &world), now);
    let q = desk.question(now).expect("a dialog");
    assert!(q.summary.contains("Walk to") && q.summary.contains("5 m"), "{}", q.summary);
    let waiting = desk.ledger().with(|s| s.recent(None, 1)[0].operation_id.clone());
    let (c, stop) = call(STOP, json!({"operation_id": waiting}), false);
    desk.take(c, &cx(&inv, &world), now);
    assert_eq!(answered(&stop).expect("cancelled")["cancelled"], true);
    assert_eq!(answered(&asked).expect_err("ended").code, ErrorCode::Cancelled);
    assert!(desk.question(now).is_none(), "its dialog closed");
}

#[test]
fn never_a_login_change_or_access_off_stop_a_running_move() {
    let (world, handle) = plaza();
    let now = Instant::now();
    // the class set to Never while it walks
    let mut desk = desk_with(levels(Level::Allow));
    let (answer, _, pilot) = walking(&mut desk, &world, now);
    desk.set_permissions(levels(Level::Ask));
    assert!(
        desk.navigate(&frame(handle, ME, Some(pilot)), now).is_empty(),
        "Ask: the approved move goes on"
    );
    desk.set_permissions(levels(Level::Never));
    assert_eq!(desk.navigate(&frame(handle, ME, Some(pilot)), now), vec![Act::StopPilot(pilot)]);
    assert_eq!(answered(&answer).expect_err("stopped").code, ErrorCode::Cancelled);
    assert_eq!(last_reason(&desk), "permissions_changed");
    // another login
    let mut desk = desk_with(levels(Level::Allow));
    let (answer, _, pilot) = walking(&mut desk, &world, now);
    desk.set_login(1, None);
    assert_eq!(answered(&answer).expect_err("stopped").code, ErrorCode::Cancelled);
    assert_eq!(desk.navigate(&frame(handle, ME, Some(pilot)), now), vec![Act::StopPilot(pilot)]);
    assert!(desk.navigator().last().is_none(), "the old login's outcome is gone");
    // access turned off
    let mut desk = desk_with(levels(Level::Allow));
    let (answer, _, pilot) = walking(&mut desk, &world, now);
    desk.close();
    assert!(answered(&answer).expect_err("stopped").message.contains("turned off"));
    assert_eq!(desk.navigate(&frame(handle, ME, Some(pilot)), now), vec![Act::StopPilot(pilot)]);
    assert_eq!(last_reason(&desk), "access_off");
}

fn teleport_args(world: &World, id: &str) -> Value {
    let to = origin(world.main_region.expect("a region")) + DVec3::new(60.0, 70.0, 30.0);
    json!({"global_position": to.to_array(), "client_request_id": id})
}

/// A teleport asked, allowed and submitted by "the app".
fn teleporting(desk: &mut Desk, world: &World, id: &str, now: Instant) -> (Receiver<Result<Value, ToolError>>, String) {
    let inv = demo_inventory();
    let (c, answer) = call(TELEPORT, teleport_args(world, id), false);
    desk.take(c, &cx(&inv, world), now);
    let q = desk.question(now).expect("a teleport always asks");
    desk.decide(q.number, true);
    let run = desk.tick(&IDLE, now).into_iter().next().expect("allowed");
    let operation = run.operation_id.clone().expect("named");
    let planned = super::changes::plan(&run.tool, &run.arguments, &cx(&inv, world)).expect("planned");
    let Work::Navigate(navigation::Work::Teleport(teleport)) = planned.work else {
        panic!("a teleport")
    };
    desk.start_teleport(run, teleport, now);
    (answer, operation)
}

#[test]
fn a_teleport_always_asks_and_needs_its_id() {
    let (world, _) = plaza();
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(levels(Level::Allow));
    let mut args = teleport_args(&world, "x");
    args.as_object_mut().expect("object").remove("client_request_id");
    let (c, answer) = call(TELEPORT, args, false);
    desk.take(c, &cx(&inv, &world), now);
    let e = answered(&answer).expect_err("refused");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
    assert!(e.message.contains("client_request_id"), "{e}");
    // on Allow, made in the process with no ask flag: it asks all the same
    let (c, answer) = call(TELEPORT, teleport_args(&world, "tp-1"), false);
    desk.take(c, &cx(&inv, &world), now);
    let q = desk.question(now).expect("asks on Allow");
    assert!(q.summary.contains("60") && q.summary.contains("70"), "{}", q.summary);
    desk.decide(q.number, false);
    desk.tick(&IDLE, now);
    assert_eq!(answered(&answer).expect_err("refused").code, ErrorCode::PermissionDenied);
    // a move on Allow does not ask
    let (c, _walk) = call(MOVE_TO, json!({"position": [125.0, 120.0, 25.0]}), false);
    desk.take(c, &cx(&inv, &world), now);
    assert!(desk.question(now).is_none());
    assert_eq!(desk.tick(&IDLE, now).len(), 1);
    // Never refuses it before any dialog
    let mut desk = desk_with(levels(Level::Never));
    let (c, answer) = call(TELEPORT, teleport_args(&world, "tp-2"), false);
    desk.take(c, &cx(&inv, &world), now);
    assert_eq!(answered(&answer).expect_err("never").code, ErrorCode::PermissionDenied);
    assert!(desk.question(now).is_none());
}

#[test]
fn a_teleport_is_followed_to_its_end_and_never_sent_twice() {
    let (world, handle) = plaza();
    let inv = demo_inventory();
    let now = Instant::now();
    let target = Vec3::new(60.0, 70.0, 30.0);
    // arrived at the target
    let mut desk = desk_with(levels(Level::Allow));
    let (answer, operation) = teleporting(&mut desk, &world, "tp-a", now);
    assert!(
        desk.navigate(
            &Frame {
                teleporting: true,
                ..frame(handle, ME, None)
            },
            now
        )
        .is_empty()
    );
    assert!(answer.try_recv().is_err());
    desk.navigate(&frame(handle, target + Vec3::new(1.0, 0.0, -4.0), None), now);
    let done = answered(&answer).expect("arrived");
    assert_eq!((done["arrived"].as_bool(), done["at_target"].as_bool()), (Some(true), Some(true)));
    assert_eq!(state(&mut desk, &world, &operation), "done");
    // the same client_request_id again: the receipt, nothing sent
    let (c, again) = call(TELEPORT, teleport_args(&world, "tp-a"), false);
    desk.take(c, &cx(&inv, &world), now);
    let duplicate = answered(&again).expect("the first receipt");
    assert_eq!(
        (duplicate["duplicate"].as_bool(), duplicate["operation_id"].as_str()),
        (Some(true), Some(operation.as_str()))
    );
    assert!(desk.question(now).is_none() && desk.tick(&IDLE, now).is_empty(), "never sent twice");
    // too soon after the last one, then fine
    let (c, soon) = call(TELEPORT, teleport_args(&world, "tp-b"), false);
    desk.take(c, &cx(&inv, &world), now + Duration::from_secs(5));
    let e = answered(&soon).expect_err("busy");
    assert_eq!(e.code, ErrorCode::Busy);
    assert!(e.message.contains("15 seconds"), "{e}");
    let later = now + Duration::from_secs(16);
    let (c, _ok) = call(TELEPORT, teleport_args(&world, "tp-c"), false);
    desk.take(c, &cx(&inv, &world), later);
    assert!(desk.question(later).is_some(), "asks");
    // one at a time: another while that one waits for its dialog
    let (c, second) = call(TELEPORT, teleport_args(&world, "tp-d"), false);
    desk.take(c, &cx(&inv, &world), later);
    assert_eq!(answered(&second).expect_err("busy").code, ErrorCode::Busy);
}

#[test]
fn a_teleport_fails_is_cancelled_or_unconfirmed() {
    let (world, handle) = plaza();
    let inv = demo_inventory();
    let now = Instant::now();
    let flying = |f: Frame| Frame { teleporting: true, ..f };
    // the simulator refuses it
    let mut desk = desk_with(levels(Level::Allow));
    let (answer, operation) = teleporting(&mut desk, &world, "f", now);
    desk.teleport_failed("Destination is full");
    desk.navigate(&flying(frame(handle, ME, None)), now);
    let e = answered(&answer).expect_err("failed");
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(e.message.contains("Destination is full"), "{e}");
    assert_eq!(state(&mut desk, &world, &operation), "failed");
    // the stop asks the viewer to cancel; the cancel takes effect
    let mut desk = desk_with(levels(Level::Allow));
    let (answer, operation) = teleporting(&mut desk, &world, "c", now);
    let (c, stop) = call(STOP, json!({"operation_id": operation}), false);
    desk.take(c, &cx(&inv, &world), now);
    let asked = answered(&stop).expect("asked");
    assert_eq!(
        (asked["cancel_requested"].as_bool(), asked["state"].as_str()),
        (Some(true), Some("running"))
    );
    assert_eq!(desk.navigate(&flying(frame(handle, ME, None)), now), vec![Act::CancelTeleport]);
    assert!(answer.try_recv().is_err(), "not ended by asking");
    desk.navigate(
        &Frame {
            teleport_failed: true,
            ..frame(handle, ME, None)
        },
        now,
    );
    assert_eq!(answered(&answer).expect_err("cancelled").code, ErrorCode::Cancelled);
    assert_eq!(state(&mut desk, &world, &operation), "stopped");
    assert_eq!(last_reason(&desk), "teleport_cancelled");
    // no end in its time
    let mut desk = desk_with(levels(Level::Allow));
    let (answer, operation) = teleporting(&mut desk, &world, "u", now);
    desk.navigate(&flying(frame(handle, ME, None)), now + Duration::from_secs(61));
    assert_eq!(answered(&answer).expect_err("unconfirmed").code, ErrorCode::Timeout);
    assert_eq!(state(&mut desk, &world, &operation), "unconfirmed");
    // landed elsewhere (a landing point): done, not at the target
    let mut desk = desk_with(levels(Level::Allow));
    let (answer, _) = teleporting(&mut desk, &world, "l", now);
    desk.navigate(&frame(handle, Vec3::new(10.0, 10.0, 22.0), None), now);
    let done = answered(&answer).expect("arrived");
    assert_eq!(done["at_target"], false);
    assert!(done["distance_from_target"].as_f64().is_some_and(|d| d > 50.0));
}

#[test]
fn moves_are_one_at_a_time() {
    let (world, _) = plaza();
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(levels(Level::Allow));
    let (_answer, _, _) = walking(&mut desk, &world, now);
    for (tool, args) in [
        (MOVE_TO, json!({"position": [125.0, 120.0, 25.0]})),
        (FLYING_SET, json!({"flying": false})),
    ] {
        let (c, answer) = call(tool, args, false);
        desk.take(c, &cx(&inv, &world), now);
        assert_eq!(answered(&answer).expect_err("busy").code, ErrorCode::Busy, "{tool}");
    }
    // a teleport goes on its own lane (it ends the walk when it leaves)
    let (c, _tp) = call(TELEPORT, teleport_args(&world, "t"), false);
    desk.take(c, &cx(&inv, &world), now);
    assert!(desk.question(now).is_some());
}

#[test]
fn status_gives_the_place_the_move_and_how_the_last_ended() {
    let (mut world, handle) = plaza();
    let now = Instant::now();
    let mut desk = desk_with(levels(Level::Allow));
    let read = |desk: &mut Desk, world: &World| {
        let inv = demo_inventory();
        let (c, answer) = call(STATUS, json!({}), false);
        desk.take(c, &cx(&inv, world), now);
        answered(&answer).expect("status")
    };
    let s = read(&mut desk, &world);
    assert_eq!(s["active"], json!([]));
    assert_eq!(s["last"], json!(null));
    assert_eq!((s["flying"].as_bool(), s["teleporting"].as_bool()), (Some(false), Some(false)));
    assert!(s["location"]["position"].is_object(), "{s}");
    assert_eq!(s["teleport_wait_seconds"], 0.0);
    let (_answer, operation, _pilot) = walking(&mut desk, &world, now);
    let s = read(&mut desk, &world);
    assert_eq!(s["active"][0]["operation_id"], operation.as_str());
    assert_eq!(s["active"][0]["kind"], "walk");
    assert!(s["active"][0]["distance_left"].as_f64().is_some_and(|d| (d - 10.0).abs() < 0.01));
    desk.navigate(&frame(handle, Vec3::new(129.5, 120.0, 25.0), None), now);
    let s = read(&mut desk, &world);
    assert_eq!(s["last"]["reason"], "arrived");
    assert_eq!(s["last"]["outcome"], "arrived");
    assert!(s["last"]["target"].is_array());
    // @showloc: no positions, no targets
    restrict(&mut world, "showloc=n");
    let s = read(&mut desk, &world);
    assert_eq!(s["last"]["positions_hidden_by_rlv"], true);
    assert!(s["last"].get("target").is_none() && s["last"].get("position").is_none(), "{s}");
    // Read on Never refuses it
    desk.set_permissions({
        let mut p = levels(Level::Allow);
        p.set(Class::Read, Level::Never);
        p
    });
    let inv = demo_inventory();
    let (c, answer) = call(STATUS, json!({}), false);
    desk.take(c, &cx(&inv, &world), now);
    assert_eq!(answered(&answer).expect_err("never").code, ErrorCode::PermissionDenied);
}

#[test]
fn reasons_name_themselves_and_reach_a_move_s_error() {
    for r in [
        Reason::UserInput,
        Reason::Timeout,
        Reason::StoppedShort,
        Reason::TeleportFailed(Some("x".into())),
        Reason::Unconfirmed,
    ] {
        assert!(!r.code().is_empty() && !r.message().is_empty());
    }
    assert!(navigation::reached(
        Mode::Walk,
        1.0,
        Vec3::new(10.0, 0.0, 0.0),
        Vec3::new(8.6, 0.0, 9.0)
    ));
    assert!(!navigation::reached(
        Mode::Walk,
        1.0,
        Vec3::new(10.0, 0.0, 0.0),
        Vec3::new(8.4, 0.0, 0.0)
    ));
    assert!(!navigation::reached(
        Mode::Fly,
        1.0,
        Vec3::new(10.0, 0.0, 0.0),
        Vec3::new(10.0, 0.0, 3.0)
    ));
    assert_eq!(navigation::distance(Mode::Walk, Vec3::ZERO, Vec3::new(3.0, 4.0, 12.0)), 5.0);
    assert_eq!(navigation::distance(Mode::Fly, Vec3::ZERO, Vec3::new(3.0, 4.0, 12.0)), 13.0);
    let mut input = crate::agent::MoveInput::default();
    assert!(!navigation::manual_input(&input, false));
    assert!(navigation::manual_input(&input, true), "mouse steering");
    input.strafe_left = true;
    assert!(navigation::manual_input(&input, false));
}

/// End to end over a real socket: the sidecar's walk answers at once
/// (`asking`), the user allows it, the app starts it, the assistant polls
/// `navigation_status` until it arrived; a teleport retried with the same
/// `client_request_id` is submitted once.
#[test]
fn an_assistant_walks_and_teleports_through_the_sidecar() {
    use folderstorm_mcp::connect::Viewer;
    use folderstorm_mcp_bridge::Bridge;
    use folderstorm_mcp_bridge::server::Config;
    use std::sync::atomic::{AtomicBool, Ordering};

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
    let (world, handle) = plaza();
    let to = origin(handle) + DVec3::new(60.0, 70.0, 30.0);
    let folder = dir.path().to_path_buf();
    let allow = Arc::new(AtomicBool::new(false));
    let allow_now = allow.clone();
    let assistant = std::thread::spawn(move || {
        let mut viewer = Viewer::new(Some(folder), None);
        let early = viewer.call(MOVE_TO, json!({"position": [130.0, 120.0, 25.0]})).expect("at once");
        let operation = early["operation_id"].as_str().unwrap_or_default().to_owned();
        let first = early["state"].as_str().unwrap_or_default().to_owned();
        allow_now.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut last = Value::Null;
        while Instant::now() < deadline {
            let status = viewer.call(STATUS, json!({})).expect("status");
            if status["last"]["operation_id"] == operation.as_str() {
                last = status["last"].clone();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // a teleport, its answer "lost", sent again with the same id
        let args = json!({"global_position": to.to_array(), "client_request_id": "tp-e2e"});
        let one = viewer.call(TELEPORT, args.clone()).expect("at once");
        let two = viewer.call(TELEPORT, args).expect("the receipt");
        (first, last, one, two)
    });
    let inv = demo_inventory();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let mut walking: Option<PilotId> = None;
    let mut submitted = 0;
    let deadline = Instant::now() + Duration::from_secs(40);
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        let session = bridge.viewer_session();
        desk.set_login(1, session.clone());
        let mut c = cx(&inv, &world);
        c.viewer_session = session;
        while let Ok(pending) = calls.try_recv() {
            desk.take(pending, &c, now);
        }
        if allow.load(Ordering::SeqCst)
            && let Some(q) = desk.question(now)
        {
            desk.decide(q.number, true);
        }
        for run in desk.tick(&IDLE, now) {
            let planned = super::changes::plan(&run.tool, &run.arguments, &c).expect("planned");
            match planned.work {
                Work::Navigate(navigation::Work::Move(mv)) => {
                    let pilot = PilotId(now);
                    walking = Some(pilot);
                    desk.start_move(run, mv, pilot, now);
                }
                Work::Navigate(navigation::Work::Teleport(teleport)) => {
                    submitted += 1;
                    desk.start_teleport(run, teleport, now);
                }
                other => panic!("not a navigation: {other:?}"),
            }
        }
        // the autopilot arrives on the next frame; the teleport lands
        let position = if walking.take().is_some() {
            Vec3::new(129.5, 120.0, 25.0)
        } else {
            ME
        };
        desk.navigate(&frame(handle, position, None), now);
        std::thread::sleep(Duration::from_millis(5));
    }
    let (first, last, one, two) = assistant.join().expect("assistant");
    assert_eq!(first, "asking");
    assert_eq!(last["reason"], "arrived", "{last}");
    assert_eq!(one["early"], true);
    assert_eq!(two["duplicate"], true, "{two}");
    assert_eq!(two["operation_id"], one["operation_id"]);
    assert_eq!(submitted, 1, "never sent twice");
    bridge.stop();
}
