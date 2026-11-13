//! Folderstorm: the local assistant's navigation (folderstorm/plans/mcp.md,
//! "Phase 3, part 4: navigation"), after Folderstorm (C++)'s
//! `doc/plans/mcp-navigation.md`. Two halves:
//!
//! - **The rules** ([`plan`]): a move, a take-off or landing, a teleport,
//!   checked as the viewer checks the same action of the user (the double
//!   click on the ground and its RLVa rule, the Fly button's
//!   `autopilot::can_takeoff`, the teleport's `rlva_allows_command`), with
//!   the assistant's own bounds on top (same region, 64 m, sitting, a walk of
//!   the viewer's own never interrupted). Nothing here sends anything: the
//!   app starts the autopilot, flies or teleports (`app/local_assistant.rs`).
//! - **The navigator** ([`Navigator`]), held by the desk: the assistant's
//!   move or teleport under way, followed once per frame from what the frame
//!   shows ([`Frame`]), just before the autopilot runs. It ends a move when
//!   it arrives, or stops it, saying why ([`Reason`]): its timeout, the
//!   user's input, another walk, a teleport, the permissions, the login. It
//!   only ever stops the autopilot it started ([`PilotId`]). A teleport is
//!   submitted once and never sent again; its end is observed, not assumed.

use super::changes::{Planned, Work as ChangeWork};
use super::tools::{Ctx, Session, err};
use crate::world::World;
use folderstorm_mcp_bridge::protocol::navigation::{
    FLYING_SET, HEIGHT_MAX, MOVE_DISTANCE_MAX, MOVE_TIMEOUT_DEFAULT, MOVE_TIMEOUT_MAX, MOVE_TO, STOP_DISTANCE,
    STOP_DISTANCE_DEFAULT, TELEPORT, TELEPORT_INTERVAL, TELEPORT_WAIT,
};
use folderstorm_mcp_bridge::receipts::{Receipt, State};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use glam::{DVec3, Vec3};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// A move that stopped this close to its target (beyond its stop distance)
/// counts as arrived: the avatar coasts a little once the autopilot stops.
const ARRIVAL_MARGIN: f32 = 0.5;
/// A flight's arrival also needs the height within this (the autopilot's
/// hover rule, `AutoPilot::reached`), plus the margin.
const HOVER_HEIGHT: f32 = 0.2;
/// A teleport that lands this close to its target (flat) is at the target;
/// farther, a landing point or a telehub moved it.
pub const AT_TARGET: f64 = 5.0;

/// What the viewer itself is doing with the avatar, for the rules.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Native {
    /// An autopilot runs: a double-click walk, Go to, follow, or the
    /// assistant's own move.
    pub pilot: bool,
    /// The viewer holds the avatar where it is (a seat or the movelock's
    /// anchor, `movelock::blocks_walk`).
    pub blocks_walk: bool,
}

/// What the navigation rules read, gathered by the app each frame.
#[derive(Clone, Copy, Default)]
pub struct Stage<'a> {
    pub world: Option<&'a World>,
    /// The world is on screen (not the login screen).
    pub in_world: bool,
    pub native: Native,
}

/// How a move goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Walk,
    Fly,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Walk => "walk",
            Mode::Fly => "fly",
        }
    }
}

/// A move, planned.
#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    /// Region metres, the avatar's centre there (as `world_location`'s
    /// position).
    pub target: Vec3,
    pub region: u64,
    pub mode: Mode,
    pub stop_distance: f32,
    pub timeout: Duration,
}

/// A teleport, planned: the `TeleportTo` the double-click teleport sends.
#[derive(Debug, Clone, PartialEq)]
pub struct Teleport {
    pub handle: u64,
    pub local: Vec3,
    pub global: DVec3,
    pub look_at: Vec3,
    /// What the teleport screen says while it goes.
    pub label: String,
}

impl Teleport {
    pub fn command(&self) -> aurora_net::NetCommand {
        aurora_net::NetCommand::TeleportTo {
            handle: self.handle,
            position: self.local,
            look_at: self.look_at,
        }
    }
}

/// What the app runs for a planned navigation.
#[derive(Debug, Clone, PartialEq)]
pub enum Work {
    Move(Move),
    /// Take off (true) or land.
    Flying(bool),
    Teleport(Teleport),
}

fn invalid(message: impl Into<String>) -> ToolError {
    err(ErrorCode::InvalidArguments, message)
}

fn not_allowed(message: impl Into<String>) -> ToolError {
    err(ErrorCode::NotAllowed, message)
}

fn known(args: &Value, keys: &[&str]) -> Result<(), ToolError> {
    match args.as_object() {
        Some(a) if a.keys().all(|k| keys.contains(&k.as_str())) => Ok(()),
        _ => Err(invalid("unknown argument")),
    }
}

/// `[x, y, z]`, finite.
fn vector(args: &Value, key: &str) -> Result<Option<DVec3>, ToolError> {
    let value = &args[key];
    if value.is_null() {
        return Ok(None);
    }
    let bad = || invalid(format!("`{key}` must be three finite numbers [x, y, z]"));
    let list = value.as_array().filter(|l| l.len() == 3).ok_or_else(bad)?;
    let mut v = [0.0; 3];
    for (slot, item) in v.iter_mut().zip(list) {
        *slot = item.as_f64().filter(|n| n.is_finite()).ok_or_else(bad)?;
    }
    Ok(Some(DVec3::from_array(v)))
}

fn number(args: &Value, key: &str, (min, max): (f64, f64), default: f64) -> Result<f64, ToolError> {
    match &args[key] {
        Value::Null => Ok(default),
        v => v
            .as_f64()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| invalid(format!("`{key}` must be {min} to {max}"))),
    }
}

/// Where the avatar is, or why the tools cannot move it now.
fn place<'a>(cx: &Ctx<'a>) -> Result<&'a World, ToolError> {
    if cx.session == Session::LoggedOut {
        return Err(err(ErrorCode::NotReady, "not logged in"));
    }
    let world = cx.nav.world.ok_or_else(|| err(ErrorCode::NotReady, "the world is not loaded"))?;
    if !cx.nav.in_world || world.main_region.is_none() {
        return Err(err(ErrorCode::NotReady, "not in a region yet"));
    }
    if world.teleporting {
        return Err(err(ErrorCode::NotReady, "a teleport is under way: try again once it ends"));
    }
    Ok(world)
}

/// The avatar's region: its handle, origin and size in metres.
fn region(world: &World) -> Option<(u64, (u32, u32), (f32, f32))> {
    let handle = world.main_region?;
    let size = world
        .main()
        .map_or((256.0, 256.0), |r| (r.heightmap.size_x as f32, r.heightmap.size_y as f32));
    Some((handle, aurora_net::handle_to_origin(handle), size))
}

/// "(x, y, z) in “Region”", as the dialog names a place.
fn place_label(region: Option<&str>, at: DVec3) -> String {
    let (x, y, z) = (format!("{:.0}", at.x), format!("{:.0}", at.y), format!("{:.0}", at.z));
    match region.filter(|r| !r.is_empty()) {
        Some(region) => aurora_i18n::tr!("fs-mcp-nav-place", x = x, y = y, z = z, region = region.to_owned()),
        None => aurora_i18n::tr!("fs-mcp-nav-place-global", x = x, y = y, z = z),
    }
}

/// Plan one navigation change, checking it as the viewer would.
pub fn plan(tool: &str, args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    match tool {
        MOVE_TO => plan_move(args, cx),
        FLYING_SET => plan_flying(args, cx),
        TELEPORT => plan_teleport(args, cx),
        _ => Err(err(ErrorCode::UnknownTool, "unknown tool")),
    }
}

fn planned(work: Work, summary: String, reply: Value) -> Planned {
    Planned {
        work: ChangeWork::Navigate(work),
        summary,
        reply,
        created: None,
    }
}

fn nothing(reply: Value) -> Planned {
    Planned {
        work: ChangeWork::Nothing,
        summary: String::new(),
        reply,
        created: None,
    }
}

/// A walk of the viewer's own (double click, Go to, follow) is never
/// interrupted by the assistant.
fn native_busy() -> ToolError {
    err(
        ErrorCode::Busy,
        "the avatar is already walking or following (the viewer's own autopilot): it is not interrupted; try again once it stops",
    )
}

/// The distance a move covers: flat for a walk (the autopilot's ground
/// rule), in 3D for a flight.
pub fn distance(mode: Mode, from: Vec3, to: Vec3) -> f32 {
    match mode {
        Mode::Walk => (to - from).truncate().length(),
        Mode::Fly => to.distance(from),
    }
}

/// Whether a move stopped at `position` arrived at its target.
pub fn reached(mode: Mode, stop_distance: f32, target: Vec3, position: Vec3) -> bool {
    let flat = (target - position).truncate().length() <= stop_distance + ARRIVAL_MARGIN;
    match mode {
        Mode::Walk => flat,
        Mode::Fly => flat && (target.z - position.z).abs() <= HOVER_HEIGHT.max(stop_distance) + ARRIVAL_MARGIN,
    }
}

/// `navigation_move_to`: a point of the avatar's region, 64 m at most,
/// where the viewer would walk or fly the avatar itself.
fn plan_move(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["position", "global_position", "mode", "stop_distance", "timeout_seconds"])?;
    let world = place(cx)?;
    let Some((handle, origin, size)) = region(world) else {
        return Err(err(ErrorCode::NotReady, "not in a region yet"));
    };
    let origin = DVec3::new(origin.0 as f64, origin.1 as f64, 0.0);
    let local = match (vector(args, "position")?, vector(args, "global_position")?) {
        (Some(_), Some(_)) => return Err(invalid("give `position` or `global_position`, not both")),
        (Some(local), None) => local,
        (None, Some(global)) => global - origin,
        (None, None) => return Err(invalid("give the point as `position` (region metres) or `global_position`")),
    };
    if !(0.0..size.0 as f64).contains(&local.x) || !(0.0..size.1 as f64).contains(&local.y) {
        return Err(not_allowed(
            "the point is not in the avatar's region: a move stays in its region (use navigation_teleport to go farther)",
        ));
    }
    if !(0.0..=HEIGHT_MAX).contains(&local.z) {
        return Err(invalid(format!("the height must be 0 to {HEIGHT_MAX} m")));
    }
    let mode = match args["mode"].as_str() {
        None if args["mode"].is_null() => Mode::Walk,
        Some("walk") => Mode::Walk,
        Some("fly") => Mode::Fly,
        _ => return Err(invalid("`mode` is walk or fly")),
    };
    let stop_distance = number(args, "stop_distance", STOP_DISTANCE, STOP_DISTANCE_DEFAULT)? as f32;
    let timeout = match &args["timeout_seconds"] {
        Value::Null => MOVE_TIMEOUT_DEFAULT,
        v => v
            .as_u64()
            .filter(|n| (1..=MOVE_TIMEOUT_MAX).contains(n))
            .ok_or_else(|| invalid(format!("`timeout_seconds` must be 1 to {MOVE_TIMEOUT_MAX}")))?,
    };
    let agent = &world.agent;
    let target = local.as_vec3();
    let away = distance(mode, agent.position, target);
    if f64::from(away) > MOVE_DISTANCE_MAX {
        return Err(not_allowed(format!(
            "the point is {away:.0} m away: a move goes {MOVE_DISTANCE_MAX:.0} m at most (use navigation_teleport to go farther)"
        )));
    }
    if agent.is_sitting() || cx.nav.native.blocks_walk {
        return Err(not_allowed("the avatar is sitting: it stays seated (stand up first)"));
    }
    if cx.nav.native.pilot {
        return Err(native_busy());
    }
    match mode {
        Mode::Walk if agent.flying => {
            return Err(not_allowed(
                "the avatar is flying: walk only on the ground (land first with navigation_flying_set, or use mode fly)",
            ));
        }
        Mode::Fly if !agent.flying && !crate::autopilot::can_takeoff(world) => {
            return Err(not_allowed(
                "taking off is not allowed here (no-fly land, or an RLV @fly restriction)",
            ));
        }
        _ => {}
    }
    // LLToolPie::walkToClickedLocation under RLVa: where a local teleport
    // may not go (@tplocal, @sittp, @unsit while seated), no walk either
    if !world.rlva_can_teleport_global(origin + local) {
        return Err(not_allowed(
            "an RLV restriction (@tplocal, @sittp or @unsit) does not let the avatar go there, as for a double click on the ground",
        ));
    }
    let reply = json!({
        "mode": mode.name(),
        "target": [target.x, target.y, target.z],
        "distance": away,
    });
    if away <= stop_distance {
        return Ok(nothing(json!({
            "arrived": true,
            "moved": false,
            "distance_left": away,
            "mode": mode.name(),
        })));
    }
    let label = place_label(Some(&world.region_name()), local);
    let distance = format!("{away:.0}");
    let summary = match mode {
        Mode::Walk => aurora_i18n::tr!("fs-mcp-ask-nav-walk", place = label, distance = distance),
        Mode::Fly => aurora_i18n::tr!("fs-mcp-ask-nav-fly", place = label, distance = distance),
    };
    Ok(planned(
        Work::Move(Move {
            target,
            region: handle,
            mode,
            stop_distance,
            timeout: Duration::from_secs(timeout),
        }),
        summary,
        reply,
    ))
}

/// `navigation_flying_set`: the Fly button (LLAgent::setFlying's rules).
fn plan_flying(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["flying"])?;
    let flying = args["flying"]
        .as_bool()
        .ok_or_else(|| invalid("`flying` is required: true to take off, false to land"))?;
    let world = place(cx)?;
    if flying == world.agent.flying {
        return Ok(nothing(json!({"flying": flying, "changed": false})));
    }
    if cx.nav.native.pilot {
        return Err(native_busy());
    }
    if flying && !crate::autopilot::can_takeoff(world) {
        return Err(not_allowed(if world.agent.is_sitting() {
            "the avatar is sitting: it cannot take off"
        } else {
            "taking off is not allowed here (no-fly land, or an RLV @fly restriction)"
        }));
    }
    let summary = if flying {
        aurora_i18n::tr!("fs-mcp-ask-nav-takeoff").to_owned()
    } else {
        aurora_i18n::tr!("fs-mcp-ask-nav-land").to_owned()
    };
    Ok(planned(Work::Flying(flying), summary, json!({"flying": flying})))
}

/// The region a global point lies in, as far as the viewer knows it: a
/// region it holds (variable sizes included), then the world map's, else
/// the 256 m slot under it, which a variable region's simulator accepts
/// as its own (`autopilot::teleport_destination`). Its handle, the point in
/// it, and its name when known.
pub fn teleport_target(world: &World, global: DVec3) -> (u64, Vec3, Option<String>) {
    let inside = |origin: (u32, u32), size: (f64, f64)| {
        let (x, y) = (global.x - origin.0 as f64, global.y - origin.1 as f64);
        ((0.0..size.0).contains(&x) && (0.0..size.1).contains(&y)).then(|| Vec3::new(x as f32, y as f32, global.z as f32))
    };
    for r in world.regions.values() {
        let origin = aurora_net::handle_to_origin(r.handle);
        let size = (r.heightmap.size_x as f64, r.heightmap.size_y as f64);
        if let Some(local) = inside(origin, size) {
            let name = Some(r.name.clone()).filter(|n| !n.is_empty());
            let name = name.or_else(|| (Some(r.handle) == world.main_region).then(|| world.region_name()));
            return (r.handle, local, name);
        }
    }
    for (&(gx, gy), sim) in &world.map.sims {
        let origin = (gx.saturating_mul(256), gy.saturating_mul(256));
        let size = (sim.size_x.max(256) as f64, sim.size_y.max(256) as f64);
        if let Some(local) = inside(origin, size) {
            return (
                aurora_net::origin_to_handle(origin.0, origin.1),
                local,
                Some(sim.name.clone()).filter(|n| !n.is_empty()),
            );
        }
    }
    let slot = (
        ((global.x / 256.0).floor() as u32).saturating_mul(256),
        ((global.y / 256.0).floor() as u32).saturating_mul(256),
    );
    let local = Vec3::new(
        (global.x - slot.0 as f64) as f32,
        (global.y - slot.1 as f64) as f32,
        global.z as f32,
    );
    (aurora_net::origin_to_handle(slot.0, slot.1), local, None)
}

/// `navigation_teleport`: one `TeleportTo` to a global position, refused
/// where the viewer's own teleport is (RLVa at the send boundary).
fn plan_teleport(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["global_position"])?;
    let global = vector(args, "global_position")?.ok_or_else(|| invalid("`global_position` is required: [x, y, z]"))?;
    if global.x < 0.0 || global.y < 0.0 || global.x > f64::from(u32::MAX) || global.y > f64::from(u32::MAX) {
        return Err(invalid("`global_position` x and y must be 0 or more, within the grid"));
    }
    if !(0.0..=HEIGHT_MAX).contains(&global.z) {
        return Err(invalid(format!("the height must be 0 to {HEIGHT_MAX} m")));
    }
    let world = place(cx)?;
    let (handle, local, name) = teleport_target(world, global);
    let yaw = world.agent.yaw;
    let teleport = Teleport {
        handle,
        local,
        global,
        look_at: Vec3::new(yaw.cos(), yaw.sin(), 0.0),
        label: match &name {
            Some(region) => format!("{region} ({:.0}, {:.0}, {:.0})", local.x, local.y, local.z),
            None => format!("({:.0}, {:.0}, {:.0})", global.x, global.y, global.z),
        },
    };
    // the send boundary's own rule: @tploc, @tplocal, @sittp, @unsit
    if !world.rlva_allows_command(&teleport.command(), Uuid::nil()) {
        return Err(not_allowed(
            "an RLV restriction (@tploc, @tplocal, @sittp or @unsit) refuses this teleport",
        ));
    }
    let place = match &name {
        Some(region) => place_label(Some(region), local.as_dvec3()),
        None => place_label(None, global),
    };
    let summary = aurora_i18n::tr!("fs-mcp-ask-nav-teleport", place = place);
    let mut reply = json!({
        "global_position": [global.x, global.y, global.z],
        "region_position": [local.x, local.y, local.z],
    });
    if let Some(region) = name {
        reply["region"] = json!(region);
    }
    Ok(planned(Work::Teleport(teleport), summary, reply))
}

/// The user moves the avatar: the keys, the joystick, Stop and mouse
/// steering, as `update_autopilot` reads them to stop a walk (LLAgent::
/// moveAt & co → stopAutoPilot).
pub fn manual_input(i: &crate::agent::MoveInput, steering: bool) -> bool {
    i.forward
        || i.back
        || i.turn_left
        || i.turn_right
        || i.strafe_left
        || i.strafe_right
        || i.up
        || i.down
        || i.stop
        || i.joystick_yaw.abs() > 0.001
        || i.joystick_pitch.abs() > 0.001
        || steering
}

/// The autopilot the assistant started, by the instant it started: a walk
/// the user starts later is another one, never stopped by the assistant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PilotId(pub Instant);

/// What a frame shows, just before the autopilot runs.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub pilot: Option<PilotId>,
    /// Region metres.
    pub position: Vec3,
    pub region: Option<u64>,
    pub global: Option<DVec3>,
    pub flying: bool,
    pub sitting: bool,
    pub teleporting: bool,
    /// The last teleport failed or was cancelled (`World::tp_failed`).
    pub teleport_failed: bool,
    /// The user moves the avatar this frame ([`manual_input`]).
    pub manual: bool,
    pub in_world: bool,
    /// RLVa `@showloc`: positions are left out of the answers.
    pub location_hidden: bool,
}

/// What the navigator asks of the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Stop this autopilot, if it still runs (`stop_autopilot`).
    StopPilot(PilotId),
    /// Cancel the teleport under way, as the teleport screen's Cancel.
    CancelTeleport,
}

/// Why an assistant's move or teleport ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    Arrived,
    UserInput,
    Replaced,
    Timeout,
    AssistantStop,
    UserStop,
    PermissionsChanged,
    AccessOff,
    LoginChanged,
    Teleported,
    RegionChanged,
    SatDown,
    FlightChanged,
    StoppedShort,
    LeftWorld,
    /// A teleport: the simulator refused it (its reason when known).
    TeleportFailed(Option<String>),
    /// A teleport cancelled on request.
    TeleportCancelled,
    /// A teleport with no end in its time.
    Unconfirmed,
}

impl Reason {
    pub fn code(&self) -> &'static str {
        match self {
            Reason::Arrived => "arrived",
            Reason::UserInput => "user_input",
            Reason::Replaced => "replaced",
            Reason::Timeout => "timeout",
            Reason::AssistantStop => "assistant_stop",
            Reason::UserStop => "user_stop",
            Reason::PermissionsChanged => "permissions_changed",
            Reason::AccessOff => "access_off",
            Reason::LoginChanged => "login_changed",
            Reason::Teleported => "teleported",
            Reason::RegionChanged => "region_changed",
            Reason::SatDown => "sat_down",
            Reason::FlightChanged => "flight_changed",
            Reason::StoppedShort => "stopped_short",
            Reason::LeftWorld => "left_world",
            Reason::TeleportFailed(_) => "teleport_failed",
            Reason::TeleportCancelled => "teleport_cancelled",
            Reason::Unconfirmed => "unconfirmed",
        }
    }

    /// In plain words, for the assistant.
    pub fn message(&self) -> String {
        match self {
            Reason::Arrived => "it arrived".into(),
            Reason::UserInput => "the user took control of the avatar (a movement key, the joystick or mouse steering)".into(),
            Reason::Replaced => "the user started another walk or follow, which goes on".into(),
            Reason::Timeout => "its timeout_seconds ran out before it arrived".into(),
            Reason::AssistantStop => "stopped by the assistant".into(),
            Reason::UserStop => "stopped by the user in Preferences › Privacy › Local assistant".into(),
            Reason::PermissionsChanged => "the user no longer allows the assistant to move the avatar".into(),
            Reason::AccessOff => "local assistant access was turned off".into(),
            Reason::LoginChanged => "the viewer's login changed (logout, another login or character)".into(),
            Reason::Teleported => "the avatar teleported".into(),
            Reason::RegionChanged => "the avatar is in another region now".into(),
            Reason::SatDown => "the avatar sat down".into(),
            Reason::FlightChanged => "the avatar took off or landed".into(),
            Reason::StoppedShort => {
                "the walk ended before the target: an obstacle or a target out of reach (the autopilot came no nearer), or the viewer stopped it (Escape, a camera reset)".into()
            }
            Reason::LeftWorld => "the viewer left the world".into(),
            Reason::TeleportFailed(Some(why)) => format!("the teleport failed: {why}"),
            Reason::TeleportFailed(None) => "the teleport failed or was cancelled".into(),
            Reason::TeleportCancelled => "the teleport was cancelled on request; the avatar stayed".into(),
            Reason::Unconfirmed => format!(
                "the teleport did not end within {} seconds; it may still complete",
                TELEPORT_WAIT.as_secs()
            ),
        }
    }

    /// The error the move's or teleport's own call ends with (None: done).
    /// Something was sent, so its receipt says `stopped` (a cancel) or
    /// `failed` or `unconfirmed`, never `cancelled`.
    fn error(&self) -> Option<ToolError> {
        let code = match self {
            Reason::Arrived => return None,
            Reason::TeleportFailed(_) => ErrorCode::Failed,
            Reason::Unconfirmed => ErrorCode::Timeout,
            _ => ErrorCode::Cancelled,
        };
        Some(ToolError::new(code, format!("stopped: {}", self.message())))
    }
}

/// The assistant's move under way.
struct Moving {
    call: Pending,
    operation_id: String,
    mv: Move,
    pilot: PilotId,
    started: Instant,
    deadline: Instant,
}

/// The assistant's teleport, submitted.
struct Teleporting {
    call: Pending,
    operation_id: String,
    teleport: Teleport,
    started: Instant,
    deadline: Instant,
    cancel_requested: bool,
    failure: Option<String>,
}

/// How the last move or teleport of the assistant ended, for
/// `navigation_status`.
#[derive(Debug, Clone)]
pub struct Last {
    pub operation_id: String,
    pub kind: &'static str,
    pub reason: Reason,
    /// Region metres for a move, global for a teleport.
    pub target: DVec3,
    /// Where the avatar was when it ended (region metres for a move,
    /// global for a teleport), when known.
    pub position: Option<DVec3>,
    pub distance_left: Option<f64>,
    pub started: Instant,
    pub ended: Instant,
    /// `@showloc` when it ended: no positions.
    pub hidden: bool,
}

impl Last {
    fn to_json(&self, now: Instant, hidden: bool) -> Value {
        let mut out = json!({
            "operation_id": self.operation_id,
            "kind": self.kind,
            "outcome": if self.reason == Reason::Arrived { "arrived" } else if matches!(self.reason, Reason::TeleportFailed(_)) { "failed" } else if self.reason == Reason::Unconfirmed { "unconfirmed" } else { "stopped" },
            "reason": self.reason.code(),
            "message": self.reason.message(),
            "took_seconds": round(self.ended.saturating_duration_since(self.started).as_secs_f64()),
            "ended_seconds_ago": round(now.saturating_duration_since(self.ended).as_secs_f64()),
        });
        if hidden || self.hidden {
            out["positions_hidden_by_rlv"] = json!(true);
        } else {
            out["target"] = json!(self.target.to_array().map(round));
            if let Some(at) = self.position {
                out["position"] = json!(at.to_array().map(round));
            }
            if let Some(left) = self.distance_left {
                out["distance_left"] = json!(round(left));
            }
        }
        out
    }
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The assistant's moves and teleports under way, held by the desk.
#[derive(Default)]
pub struct Navigator {
    moving: Option<Moving>,
    teleport: Option<Teleporting>,
    last: Option<Last>,
    /// When the assistant last submitted a teleport (the rate limit).
    last_teleport: Option<Instant>,
    /// What the app does next frame.
    acts: Vec<Act>,
}

impl Navigator {
    /// A move started: the autopilot `pilot` walks or flies to `mv`'s target.
    pub fn start_move(&mut self, call: Pending, mv: Move, pilot: PilotId, now: Instant) {
        call.receipt(Receipt::mark_sent);
        let operation_id = call.operation_id.clone().unwrap_or_default();
        self.moving = Some(Moving {
            call,
            operation_id,
            deadline: now + mv.timeout,
            mv,
            pilot,
            started: now,
        });
    }

    /// A teleport was submitted (it is never sent again).
    pub fn start_teleport(&mut self, call: Pending, teleport: Teleport, now: Instant) {
        call.receipt(Receipt::mark_sent);
        let operation_id = call.operation_id.clone().unwrap_or_default();
        self.last_teleport = Some(now);
        self.teleport = Some(Teleporting {
            call,
            operation_id,
            teleport,
            started: now,
            deadline: now + TELEPORT_WAIT,
            cancel_requested: false,
            failure: None,
        });
    }

    /// How long before the assistant may submit another teleport (zero:
    /// now). A teleport under way waits for its end.
    pub fn teleport_wait(&self, now: Instant) -> Option<Duration> {
        if self.teleport.is_some() {
            return None;
        }
        Some(
            self.last_teleport
                .map_or(Duration::ZERO, |at| (at + TELEPORT_INTERVAL).saturating_duration_since(now)),
        )
    }

    /// One at a time on each lane, as the windows do: a move or a take-off
    /// while another move of the assistant is live (`live` names the other
    /// live navigation operations of this login and their tools); a
    /// teleport while another is live, under way, or too soon after the
    /// last one.
    pub fn lane(&self, tool: &str, live: &[(String, String)], now: Instant) -> Result<(), ToolError> {
        let busy = |m: String| Err(err(ErrorCode::Busy, m));
        if tool == TELEPORT {
            if live.iter().any(|(_, t)| t == TELEPORT) {
                return busy("another teleport of yours is waiting or under way: one at a time".into());
            }
            return match self.teleport_wait(now) {
                None => busy("another teleport of yours is under way: one at a time".into()),
                Some(wait) if !wait.is_zero() => busy(format!(
                    "teleports are at least {} seconds apart: try again in {} s",
                    TELEPORT_INTERVAL.as_secs(),
                    wait.as_secs_f32().ceil()
                )),
                Some(_) => Ok(()),
            };
        }
        if self.moving.is_some() || live.iter().any(|(_, t)| t == MOVE_TO || t == FLYING_SET) {
            return busy("another move of yours is waiting or under way: navigation_stop it first, or wait for its end".into());
        }
        Ok(())
    }

    /// Stop the move `operation_id` (the assistant's stop or cancel, or the
    /// user's Stop), or ask the viewer to cancel the teleport. None: neither
    /// is under way.
    pub fn stop(&mut self, operation_id: &str, by_user: bool, now: Instant) -> Option<Value> {
        if self.moving.as_ref().is_some_and(|m| m.operation_id == operation_id) {
            let reason = if by_user { Reason::UserStop } else { Reason::AssistantStop };
            self.end_move(reason, None, now);
            return Some(json!({"operation_id": operation_id, "stopped": true, "cancelled": false, "state": State::Stopped}));
        }
        let teleport = self.teleport.as_mut().filter(|t| t.operation_id == operation_id)?;
        if !teleport.cancel_requested {
            teleport.cancel_requested = true;
            self.acts.push(Act::CancelTeleport);
        }
        Some(json!({
            "operation_id": operation_id,
            "stopped": false,
            "cancelled": false,
            "cancel_requested": true,
            "state": State::Running,
            "message": "a teleport already submitted cannot be recalled: the viewer was asked to cancel it as its teleport screen's Cancel would, and it may still complete. navigation_status says where the avatar ended up",
        }))
    }

    /// The login changed: what was under way belongs to the old one.
    pub fn login_changed(&mut self, now: Instant) {
        self.end_move(Reason::LoginChanged, None, now);
        self.end_teleport(Reason::LoginChanged, None, now);
        self.last = None;
        self.last_teleport = None;
    }

    /// Access turned off: the move stops (nothing more is sent), a teleport
    /// already submitted is followed no more.
    pub fn close(&mut self, now: Instant) {
        self.end_move(Reason::AccessOff, None, now);
        if let Some(t) = self.teleport.take() {
            t.call.answer(Err(ToolError::new(
                ErrorCode::Timeout,
                "local assistant access was turned off: the teleport was submitted and may still complete",
            )));
        }
    }

    /// The permissions no longer allow the move under way: it stops.
    pub fn refuse_move(&mut self, reason: Reason, now: Instant) {
        self.end_move(reason, None, now);
    }

    /// The tool of the move under way, if any (for the permission review).
    pub fn moving_tool(&self) -> Option<&str> {
        self.moving.as_ref().map(|m| m.call.tool.as_str())
    }

    /// The simulator refused the teleport (`TeleportFailed`).
    pub fn teleport_failed(&mut self, reason: &str) {
        if let Some(t) = &mut self.teleport {
            t.failure = Some(folderstorm_mcp_bridge::receipts::clean(reason, 200));
        }
    }

    /// Once per frame, just before the autopilot runs: follow the move and
    /// the teleport under way; returns what the app does now.
    pub fn tick(&mut self, frame: &Frame, now: Instant) -> Vec<Act> {
        if let Some(m) = &self.moving {
            let ours = frame.pilot == Some(m.pilot);
            let reason = if !ours {
                Some(self.gone(m, frame))
            } else if frame.manual {
                Some(Reason::UserInput)
            } else if !frame.in_world {
                Some(Reason::LeftWorld)
            } else if frame.region != Some(m.mv.region) {
                Some(Reason::RegionChanged)
            } else if frame.teleporting {
                Some(Reason::Teleported)
            } else if frame.sitting {
                Some(Reason::SatDown)
            } else if now >= m.deadline {
                Some(Reason::Timeout)
            } else {
                None
            };
            if let Some(reason) = reason {
                self.end_move(reason, Some(frame), now);
            }
        }
        if let Some(t) = &mut self.teleport {
            let reason = if let Some(why) = t.failure.take() {
                Some(Reason::TeleportFailed(Some(why)))
            } else if frame.teleporting {
                (now >= t.deadline).then_some(Reason::Unconfirmed)
            } else if frame.teleport_failed {
                Some(if t.cancel_requested {
                    Reason::TeleportCancelled
                } else {
                    Reason::TeleportFailed(None)
                })
            } else {
                Some(Reason::Arrived)
            };
            if let Some(reason) = reason {
                self.end_teleport(reason, Some(frame), now);
            }
        }
        std::mem::take(&mut self.acts)
    }

    /// Why the move ended when its autopilot is gone (or another one runs).
    fn gone(&self, m: &Moving, frame: &Frame) -> Reason {
        if frame.region == Some(m.mv.region) && reached(m.mv.mode, m.mv.stop_distance, m.mv.target, frame.position) {
            Reason::Arrived
        } else if frame.teleporting || self.teleport.is_some() {
            Reason::Teleported
        } else if !frame.in_world {
            Reason::LeftWorld
        } else if frame.region != Some(m.mv.region) {
            Reason::RegionChanged
        } else if frame.sitting {
            Reason::SatDown
        } else if frame.manual {
            Reason::UserInput
        } else if frame.pilot.is_some() {
            Reason::Replaced
        } else if (m.mv.mode == Mode::Walk) == frame.flying {
            Reason::FlightChanged
        } else {
            Reason::StoppedShort
        }
    }

    /// The move ends: its call answers, its autopilot stops if it still
    /// runs (never another one), and `last` keeps how it ended.
    fn end_move(&mut self, reason: Reason, frame: Option<&Frame>, now: Instant) {
        let Some(m) = self.moving.take() else { return };
        if reason != Reason::Arrived {
            self.acts.push(Act::StopPilot(m.pilot));
        }
        let same_region = frame.is_some_and(|f| f.region == Some(m.mv.region));
        let position = frame.filter(|_| same_region).map(|f| f.position);
        let left = position.map(|p| distance(m.mv.mode, p, m.mv.target));
        let hidden = frame.is_some_and(|f| f.location_hidden);
        self.last = Some(Last {
            operation_id: m.operation_id.clone(),
            kind: m.mv.mode.name(),
            reason: reason.clone(),
            target: m.mv.target.as_dvec3(),
            position: position.map(Vec3::as_dvec3),
            distance_left: left.map(f64::from),
            started: m.started,
            ended: now,
            hidden,
        });
        match reason.error() {
            None => {
                let mut reply = json!({"arrived": true, "mode": m.mv.mode.name()});
                if !hidden {
                    if let Some(p) = position {
                        reply["position"] = json!([p.x, p.y, p.z]);
                    }
                    if let Some(left) = left {
                        reply["distance_left"] = json!(left);
                    }
                }
                m.call.answer(Ok(reply));
            }
            Some(e) => m.call.answer(Err(e)),
        }
    }

    /// The teleport ends (arrived, failed, cancelled, unconfirmed, or
    /// followed no more).
    fn end_teleport(&mut self, reason: Reason, frame: Option<&Frame>, now: Instant) {
        let Some(t) = self.teleport.take() else { return };
        let position = frame.and_then(|f| f.global);
        let left = position.map(|p| (p - t.teleport.global).truncate().length());
        let hidden = frame.is_some_and(|f| f.location_hidden);
        self.last = Some(Last {
            operation_id: t.operation_id.clone(),
            kind: "teleport",
            reason: reason.clone(),
            target: t.teleport.global,
            position,
            distance_left: left,
            started: t.started,
            ended: now,
            hidden,
        });
        match reason.error() {
            None => {
                let mut reply = json!({"arrived": true, "at_target": left.is_some_and(|d| d <= AT_TARGET)});
                if !hidden {
                    if let Some(p) = position {
                        reply["global_position"] = json!(p.to_array().map(round));
                    }
                    if let Some(left) = left {
                        reply["distance_from_target"] = json!(round(left));
                    }
                }
                t.call.answer(Ok(reply));
            }
            Some(e) => t.call.answer(Err(e)),
        }
    }

    /// `navigation_status`'s own part: what is under way, how the last one
    /// ended, and when the next teleport may go.
    pub fn status(&self, world: Option<&World>, now: Instant) -> Value {
        let hidden = world.is_some_and(|w| w.rlva.has("showloc"));
        let mut active = Vec::new();
        if let Some(m) = &self.moving {
            let mut entry = json!({
                "operation_id": m.operation_id,
                "kind": m.mv.mode.name(),
                "state": State::Running,
                "seconds_left": round(m.deadline.saturating_duration_since(now).as_secs_f64()),
            });
            if hidden {
                entry["positions_hidden_by_rlv"] = json!(true);
            } else {
                entry["target"] = json!([m.mv.target.x, m.mv.target.y, m.mv.target.z]);
                if let Some(world) = world.filter(|w| w.main_region == Some(m.mv.region)) {
                    entry["distance_left"] = json!(round(f64::from(distance(m.mv.mode, world.agent.position, m.mv.target))));
                }
            }
            active.push(entry);
        }
        if let Some(t) = &self.teleport {
            let mut entry = json!({
                "operation_id": t.operation_id,
                "kind": "teleport",
                "state": State::Running,
                "submitted_seconds_ago": round(now.saturating_duration_since(t.started).as_secs_f64()),
                "cancel_requested": t.cancel_requested,
            });
            if !hidden {
                entry["target"] = json!(t.teleport.global.to_array().map(round));
            }
            active.push(entry);
        }
        json!({
            "active": active,
            "last": self.last.as_ref().map(|l| l.to_json(now, hidden)),
            "teleport_wait_seconds": self.teleport_wait(now).map(|w| w.as_secs_f32().ceil()),
        })
    }

    #[cfg(test)]
    pub fn last(&self) -> Option<&Last> {
        self.last.as_ref()
    }
}

/// `navigation_status`: the location as `world_location` gives it, what the
/// avatar does, and the navigator's part.
pub fn status(cx: &Ctx, nav: &Navigator, ours: bool, now: Instant) -> Result<Value, ToolError> {
    if cx.session == Session::LoggedOut {
        return Err(err(ErrorCode::NotReady, "not logged in"));
    }
    let world = cx.nav.world;
    let mut out = nav.status(world, now);
    let Some(world) = world else {
        return Err(err(ErrorCode::NotReady, "the world is not loaded"));
    };
    let observation = super::world::Observation {
        world,
        demo: cx.session == Session::Demo,
        object_props: &[],
        sounds: None,
    };
    out["location"] = match super::world::answer("world_location", &json!({}), &observation) {
        Ok(location) => location,
        Err(e) => json!({"error": e.code, "message": e.message}),
    };
    out["teleporting"] = json!(world.teleporting);
    out["flying"] = json!(world.agent.flying);
    out["sitting"] = json!(world.agent.is_sitting());
    out["can_take_off"] = json!(world.agent.flying || crate::autopilot::can_takeoff(world));
    out["native_movement"] = json!(cx.nav.native.pilot && !ours);
    Ok(out)
}
