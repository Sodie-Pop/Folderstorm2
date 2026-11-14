//! Folderstorm: the viewer's side of the local assistant bridge
//! (FEATURES.md 4.1, folderstorm/plans/mcp.md). The listener, the token,
//! the discovery file and the permission checks live in the
//! `folderstorm-mcp-bridge` crate; this module starts and stops it from the
//! settings, keeps the "this session only" copy of them, and answers the
//! calls on the frame thread, a few per frame, without ever blocking it.
//!
//! Changes (phase 2) are planned here ([`changes`]), wait for the user in the
//! Ask dialog when the permissions say so ([`ask`]), then go back to the app
//! (`app/local_assistant.rs`), which runs them through the inventory
//! window's and Appearance's own paths. The call waits in [`Desk`] until the
//! server confirms the change ([`observe`]), all polled once per frame.
//!
//! The camera, pictures and thumbnails (phase 3) are planned in [`camera`],
//! sized and encoded in [`picture`], and wait in [`studio::Studio`] for the
//! camera to stop, for their photo or for the gallery's snapshot. The studio
//! decides its waiting calls again by the desk's rules, on every frame,
//! just before a photo starts ([`photo_step`]) and just before a thumbnail
//! is sent ([`thumbnail_send`]).
//!
//! The desk decides every waiting call again with the permissions and the
//! login in force ([`desk`], [`login`]). The bridge names each login with a
//! `viewer_session` that the sidecar pins: a call pinned to another login is
//! refused with `session_changed` (folderstorm/plans/mcp-receipts.md,
//! section 2).
//!
//! Every change the desk takes gets a receipt (section 4): the desk keeps
//! them for the login, Preferences shows them in Recent activity, and
//! `assistant_operation_status` reads them back.
//!
//! Navigation (phase 3, part 4) is planned in [`navigation`]; the app starts
//! the move or the teleport, and the desk's navigator follows it once per
//! frame just before the autopilot runs ([`navigation_frame`]).

pub mod ask;
pub mod building; // Folderstorm: building (mcp-building.md)
#[cfg(test)]
mod building_tests;
pub mod camera;
#[cfg(test)]
mod camera_tests;
#[cfg(test)]
mod change_tests;
pub mod changes;
pub mod content; // Folderstorm: content editing (mcp-content.md)
#[cfg(test)]
mod content_tests;
pub mod desk;
#[cfg(test)]
mod gap_tests; // Folderstorm: phase 3, part 2 (inventory gaps)
#[cfg(test)]
mod gesture_tests;
pub mod gestures; // Folderstorm: gestures and own posing (mcp-gestures.md)
pub mod inventory_edits;
pub mod inventory_reads;
pub mod journal;
pub mod landmark;
pub mod login;
pub mod navigation; // Folderstorm: phase 3, part 4 (navigation)
#[cfg(test)]
mod navigation_tests;
pub mod objects; // Folderstorm: object inspection and lifecycle (mcp-objects.md)
#[cfg(test)]
mod objects_tests;
pub mod picture;
pub mod plans; // Folderstorm: batch plans (mcp-receipts.md, section 6)
pub mod posing; // Folderstorm: own posing (mcp-gestures.md)
pub mod script_feed; // Folderstorm: script output feed (mcp-task-scripts.md)
pub mod script_testing; // Folderstorm: script testing (mcp-script-testing.md)
#[cfg(test)]
mod script_testing_tests;
pub mod setup;
pub mod studio;
pub mod task_scripts; // Folderstorm: scripts inside objects (mcp-task-scripts.md)
#[cfg(test)]
mod task_scripts_tests;
#[cfg(test)]
mod tests;
pub mod tools;
pub mod world; // Folderstorm: phase 3, part 3 (world observation)
#[cfg(test)]
mod world_tests;

use crate::world::World;
pub use desk::Desk;
use folderstorm_mcp_bridge::protocol::assistant as plans_tool;
use folderstorm_mcp_bridge::receipts::Receipt;
use folderstorm_mcp_bridge::server::{Activity, Config, Pending, VIEWER_TIMEOUT};
use folderstorm_mcp_bridge::{Bridge, ErrorCode, LocalAssistant, Preset, ToolError};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;
use uuid::Uuid;

/// Calls taken per frame at most; the rest wait for the next frames.
const PER_FRAME: usize = 4;

struct Running {
    bridge: Bridge,
    calls: Receiver<Pending>,
}

#[derive(Default)]
struct Runtime {
    running: Option<Running>,
    /// Calls waiting for the user, the server or the avatar.
    desk: Desk,
    /// Camera moves, pictures and thumbnails waiting (phase 3).
    studio: studio::Studio,
    /// The login generation the desk binds its calls to.
    logins: login::Logins,
    /// "Use these settings for this session only": the settings in force
    /// instead of the saved ones until the viewer quits.
    session: Option<LocalAssistant>,
    /// `--mcp-api` (and the demo's `AURORA_DEMO_MCP`) applied once.
    started: bool,
    /// `AURORA_DEMO_MCP=ask|camera`: the demo's own requests were made.
    demo_asked: bool,
    /// Folderstorm: `AURORA_DEMO_MCP=navigation`'s step: 0 walking, 1 the
    /// walk answered (the teleport goes), 2 teleporting, 3 done.
    demo_step: u8,
    /// The demo's requests waiting for their answer, logged when it comes.
    demo_answers: Vec<(&'static str, Receiver<Result<serde_json::Value, ToolError>>)>,
    /// Folderstorm: `AURORA_DEMO_MCP=content`: the demo's reads, saved
    /// once they answer.
    demo_reads: Vec<Receiver<Result<serde_json::Value, ToolError>>>,
    /// Folderstorm: `AURORA_DEMO_MCP=objects`: the demo's listing, which a
    /// put follows once it answers.
    demo_listing: Option<Receiver<Result<serde_json::Value, ToolError>>>,
    /// Folderstorm: `AURORA_DEMO_MCP=task-scripts`: the demo's step and the
    /// answer it waits for, and the script it works on.
    demo_task: Option<(u8, Receiver<Result<serde_json::Value, ToolError>>)>,
    demo_task_item: Option<String>,
    /// Folderstorm: `AURORA_DEMO_MCP=script-testing`: the demo's step and
    /// the answer it waits for.
    demo_test: Option<(u8, Receiver<Result<serde_json::Value, ToolError>>)>,
    /// Folderstorm: `AURORA_DEMO_MCP=building`: the demo's step and the
    /// answer it waits for.
    demo_build: Option<(u8, Receiver<Result<serde_json::Value, ToolError>>)>,
    /// Folderstorm: `AURORA_DEMO_MCP=gestures`: the demo's step, the answer
    /// it waits for, and the pose session with its revision.
    demo_gesture: Option<(u8, Receiver<Result<serde_json::Value, ToolError>>)>,
    demo_pose: Option<(String, u64)>,
    /// The bridge could not start for the access turned on now; tried
    /// again when access is turned off and on.
    failure: Option<String>,
    /// Items the assistant just created, until the app files them
    /// (phase 3: no rename box in the inventory for them).
    created: Vec<Uuid>,
}

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);

impl Runtime {
    /// The login in force now, told to the bridge: its generation and
    /// `viewer_session`. None: the bridge is not running.
    fn rebind(&mut self, view: &View) -> Option<(u64, Option<String>)> {
        let running = self.running.as_ref()?;
        let generation = self.logins.observe(view.world.login.as_ref());
        running.bridge.set_login(generation, view.logged_in());
        Some((generation, running.bridge.viewer_session()))
    }

    /// Folderstorm: just before a studio call acts, with the login in force
    /// now: the studio decides its waiting calls again by the desk's rules,
    /// and those that ask now go back to the desk's dialog.
    fn review_studio(&mut self, view: &View, now: Instant) {
        let session = self.rebind(view).map(|(generation, session)| {
            self.desk.set_login(generation, session.clone());
            session
        });
        let back = self.studio.review(&self.desk);
        if back.is_empty() {
            return;
        }
        let cx = tools::Ctx {
            viewer_session: session.flatten(),
            ..view.context()
        };
        for call in back {
            self.desk.take(call, &cx, now);
        }
    }
}

fn with<R>(f: impl FnOnce(&mut Runtime) -> R) -> R {
    let mut guard = RUNTIME.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(Runtime::default))
}

/// What Preferences shows.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub port: Option<u16>,
    pub failure: Option<String>,
    pub activity: Vec<Activity>,
    /// Folderstorm: this login's receipts, oldest first.
    pub receipts: Vec<ReceiptLine>,
}

/// Folderstorm: a receipt as Recent activity shows it.
#[derive(Debug, Clone)]
pub struct ReceiptLine {
    pub operation_id: String,
    pub tool: String,
    pub summary: String,
    pub state: folderstorm_mcp_bridge::receipts::State,
    pub client: Option<String>,
    /// When the call arrived on the frame thread.
    pub asked: std::time::SystemTime,
}

pub fn status() -> Status {
    with(|rt| Status {
        port: rt.running.as_ref().map(|r| r.bridge.port()),
        failure: rt.failure.clone(),
        activity: rt.running.as_ref().map(|r| r.bridge.activity()).unwrap_or_default(),
        receipts: rt.desk.ledger().with(|store| {
            store
                .all()
                .map(|r| ReceiptLine {
                    operation_id: r.operation_id.clone(),
                    tool: r.tool.clone(),
                    summary: r.summary.clone(),
                    state: r.state,
                    client: r.client.clone(),
                    asked: r.asked,
                })
                .collect()
        }),
    })
}

/// The session-only settings, when they are in force.
pub fn session() -> Option<LocalAssistant> {
    with(|rt| rt.session)
}

/// Use `settings` for this session only (Some), or the saved ones again.
pub fn set_session(settings: Option<LocalAssistant>) {
    with(|rt| rt.session = settings);
}

/// Where the discovery file goes: the main profile's settings folder,
/// whatever this process's profile, so that the sidecar finds every
/// character's viewer in one place (session routing,
/// folderstorm/plans/mcp-routing.md, section 1). The pid in the file's name
/// keeps them apart.
pub fn discovery_dir() -> std::path::PathBuf {
    crate::folderstorm_characters::profile::main_config_dir()
}

/// Who this viewer is, for the sidecar (session routing): its profile,
/// managed by a session host or not, and its avatar's name while logged in
/// (the bridge gives the name only while Read is Allow).
fn identity(view: &View) -> folderstorm_mcp_bridge::server::Identity {
    folderstorm_mcp_bridge::server::Identity {
        profile: crate::folderstorm_characters::profile::current().map(|p| p.shown().to_owned()),
        managed: crate::cli::get().managed,
        character: view.logged_in().then(|| view.world.own_name()).filter(|n| !n.is_empty()),
    }
}

/// `AURORA_DEMO_MCP=ask` (or `camera`, `plan`, `navigation`, `content`,
/// `objects`, `task-scripts`, `script-testing`): the session asks before
/// every change, and the demo makes requests of its own so that the dialog
/// shows.
fn demo_asks() -> bool {
    std::env::var("AURORA_DEMO_MCP").is_ok_and(|v| {
        v == "ask"
            || v == "camera"
            || v == "plan"
            || v == "navigation"
            || v == "content"
            || v == "objects"
            || v == "task-scripts"
            || v == "script-testing"
            || v == "building" // Folderstorm: mcp-building.md
            || v == "gestures" // Folderstorm: mcp-gestures.md
    })
}

/// Folderstorm: `AURORA_DEMO_MCP=navigation`: the demo walks the avatar 8 m
/// east, then teleports it back near where it started (both asked), then
/// logs `navigation_status`.
fn demo_navigation() -> bool {
    std::env::var("AURORA_DEMO_MCP").is_ok_and(|v| v == "navigation")
}

/// `AURORA_DEMO_MCP=plan`: the demo previews a 3-entry move and executes
/// it, so that the dialog lists a plan's rows.
fn demo_plan() -> bool {
    std::env::var("AURORA_DEMO_MCP").is_ok_and(|v| v == "plan")
}

/// `AURORA_DEMO_MCP=camera`: the demo's requests frame the avatar and take
/// a picture (saved in the photo folder) instead of creating a folder.
fn demo_camera() -> bool {
    std::env::var("AURORA_DEMO_MCP").is_ok_and(|v| v == "camera")
}

/// What the frame thread reads for the calls, gathered by the app.
pub struct View<'a> {
    pub world: &'a World,
    pub demo: bool,
    /// The folders locked in the inventory window.
    pub protected: &'a HashSet<Uuid>,
    /// An outfit change is still on its way (Appearance waits for the
    /// server, or its detaches have not landed yet).
    pub appearance_busy: bool,
    /// What the camera tools read (phase 3).
    pub stage: camera::Stage,
    /// Where the camera is now.
    pub camera: camera::Pose,
    /// Object names and owners the viewer already holds (Area Search, the
    /// hover tips, the selection), for `world_nearby_objects`.
    pub object_props: Vec<&'a HashMap<Uuid, aurora_net::build::ObjectProps>>,
    /// The Sound Explorer's history, for `world_audio_sources`.
    pub sounds: Option<&'a crate::scene::sounds::history::History>,
    /// Folderstorm: what the viewer itself does with the avatar, for the
    /// navigation tools.
    pub navigation: navigation::Native,
    /// Folderstorm: the items open in the notecard window or the script
    /// editor, which content saves leave alone.
    pub content_open: HashSet<Uuid>,
    /// Folderstorm: what the gesture and pose tools read (mcp-gestures.md).
    pub avatar: gestures::Avatar<'a>,
}

impl View<'_> {
    /// What the tools read this frame.
    pub fn context(&self) -> tools::Ctx<'_> {
        let world = self.world;
        let session = if self.demo {
            tools::Session::Demo
        } else if world.login.is_some() {
            tools::Session::LoggedIn
        } else {
            tools::Session::LoggedOut
        };
        tools::Ctx {
            inv: &world.inventory,
            session,
            worn: worn_items(world),
            attachment_points: world.worn_attachment_points(),
            inventory_locked: world.rlva.has("showinv"),
            protected: self.protected.clone(),
            outfit_points: world.outfit_attachment_points(),
            stage: self.stage,
            viewer_session: None,
            identity: Default::default(),
            nav: navigation::Stage {
                world: Some(world),
                in_world: self.stage.in_world,
                native: self.navigation,
            },
            content: content::Limits {
                open: self.content_open.clone(),
                ..content::Limits::of(&world.rlva)
            },
            objects: objects::Scene {
                world: Some(world),
                props: &self.object_props,
            },
            avatar: self.avatar.clone(),
        }
    }

    /// What the world observations read this frame.
    pub fn observation(&self) -> world::Observation<'_> {
        world::Observation {
            world: self.world,
            demo: self.demo,
            object_props: &self.object_props,
            sounds: self.sounds,
        }
    }

    /// Logged in, or the demo: a login the calls can be bound to.
    fn logged_in(&self) -> bool {
        self.demo || self.world.login.is_some()
    }

    /// Where the last outfit change stands, for `wait_for_bake`.
    pub fn bake(&self) -> desk::Bake {
        let world = self.world;
        let cof_version = crate::world::appearance::cof(&world.inventory)
            .and_then(|cof| world.inventory.folders.get(&cof))
            .map_or(-1, |f| f.info.version);
        desk::Bake {
            outfit_change_pending: self.appearance_busy,
            cof_version,
            appearance_cof_version: world.appearance_cof_version(&world.agent_id),
            server_bakes: !self.demo && !world.bakes_locally(),
        }
    }
}

/// Once per frame: start or stop the bridge as the settings say, give it
/// the permissions in force, take a few waiting calls and move the waiting
/// ones on. Returns the changes to run now, by the app.
pub fn frame(saved: &LocalAssistant, view: &View) -> Vec<Pending> {
    with(|rt| {
        if !rt.started {
            rt.started = true;
            let demo_on = view.demo && std::env::var_os("AURORA_DEMO_MCP").is_some();
            if crate::cli::get().mcp_api || demo_on {
                // forced on for this session; the saved setting is untouched
                let mut session = LocalAssistant { enabled: true, ..*saved };
                if demo_on && demo_asks() {
                    session.permissions = Preset::AskBeforeChanges.permissions();
                }
                rt.session = Some(session);
                log::info!("local assistant access turned on for this session only (--mcp-api)");
            }
        }
        let wanted = rt.session.unwrap_or(*saved);
        // Folderstorm: the script output feed captures while access is on
        // and Read is Allow (mcp-task-scripts.md)
        script_feed::set_capturing(wanted.enabled && wanted.permissions.check(folderstorm_mcp_bridge::Class::Read).is_ok());
        let generation = rt.logins.observe(view.world.login.as_ref());
        if !wanted.enabled {
            rt.failure = None;
            // the calls waiting, queued or not, are refused, then the
            // bridge stops
            if let Some(running) = rt.running.take() {
                while let Ok(call) = running.calls.try_recv() {
                    call.answer(Err(ToolError::new(ErrorCode::PermissionDenied, desk::ACCESS_OFF)));
                }
            }
            rt.desk.close();
            rt.studio.close();
            return Vec::new();
        }
        if rt.running.is_none() && rt.failure.is_none() {
            let config = Config {
                discovery_dir: Some(discovery_dir()),
                pid: std::process::id(),
                version: aurora_version::version().to_owned(),
                permissions: wanted.permissions,
                viewer_timeout: VIEWER_TIMEOUT,
            };
            match Bridge::start(config) {
                Ok((bridge, calls)) => rt.running = Some(Running { bridge, calls }),
                Err(e) => {
                    log::warn!("local assistant bridge not started: {e}");
                    rt.failure = Some(e.kind().to_string());
                }
            }
        }
        let Some(running) = &rt.running else { return Vec::new() };
        if running.bridge.permissions() != wanted.permissions {
            running.bridge.set_permissions(wanted.permissions);
        }
        // before any call: the desk decides again with what is in force
        running.bridge.set_login(generation, view.logged_in());
        let me = identity(view);
        running.bridge.set_identity(me.clone());
        // (viewer_status names the character only while Read is Allow, as
        // the health does)
        let me = folderstorm_mcp_bridge::server::Identity {
            character: me
                .character
                .filter(|_| wanted.permissions.check(folderstorm_mcp_bridge::Class::Read).is_ok()),
            ..me
        };
        let session = running.bridge.viewer_session();
        rt.desk.set_login(generation, session.clone());
        rt.desk.set_permissions(wanted.permissions);
        let now = Instant::now();
        rt.studio.observe(view.camera, now);
        let mut cx = None;
        let context = || tools::Ctx {
            viewer_session: session.clone(),
            identity: me.clone(),
            ..view.context()
        };
        // the studio's waiting calls follow the desk's rules too
        for call in rt.studio.review(&rt.desk) {
            rt.desk.take(call, cx.get_or_insert_with(context), now);
        }
        for _ in 0..PER_FRAME {
            let Ok(call) = running.calls.try_recv() else { break };
            if world::answers(&call.tool) {
                // reads of what the viewer shows, answered at once once the
                // desk has checked the login and the levels
                let Some(call) = rt.desk.admit(call) else { continue };
                let result = world::answer(&call.tool, &call.arguments, &view.observation());
                call.answer(result);
                continue;
            }
            let cx = cx.get_or_insert_with(context);
            rt.desk.take(call, cx, now);
        }
        let demo_ready = !view.world.inventory.root.is_nil()
            && (!(demo_camera() || demo_navigation()) || view.stage.in_world)
            && (!content::demo::wanted() || view.world.inventory.items.contains_key(&content::demo::NOTES))
            && (!objects::demo::wanted() || objects::demo::ready(view.world))
            && (!task_scripts::demo::wanted() || task_scripts::demo::ready(view.world))
            && (!script_testing::demo::wanted() || script_testing::demo::ready(view.world))
            && (!building::demo::wanted() || (view.stage.in_world && building::demo::ready(view.world)))
            && (!gestures::demo::wanted() || (view.stage.in_world && posing::demo::ready(view.world)));
        if view.demo && !rt.demo_asked && demo_asks() && demo_ready {
            rt.demo_asked = true;
            let requests = if demo_camera() {
                vec![
                    ("camera_preset", serde_json::json!({"preset": "full_body"})),
                    (
                        "camera_snapshot",
                        serde_json::json!({"source": "world", "width": 512, "height": 512, "save": true}),
                    ),
                ]
            } else if demo_navigation() {
                // Folderstorm: 8 m east, on the ground
                let p = view.world.agent.position;
                vec![(
                    folderstorm_mcp_bridge::protocol::navigation::MOVE_TO,
                    serde_json::json!({"position": [p.x + 8.0, p.y, p.z]}),
                )]
            } else if demo_plan() {
                // Folderstorm: a plan, previewed now (a read answers at once)
                let ids: Vec<String> = [8100u128, 8102, 8103].iter().map(|n| Uuid::from_u128(*n).to_string()).collect();
                let (call, answer) = Pending::detached(
                    plans_tool::PREVIEW_MOVE,
                    serde_json::json!({"ids": ids, "folder_id": Uuid::from_u128(8001).to_string()}),
                    false,
                    Some("Demo assistant"),
                );
                rt.desk.take(call, cx.get_or_insert_with(context), now);
                let plan = answer.try_recv().ok().and_then(Result::ok);
                log::info!("local assistant demo: plan previewed: {plan:?}");
                let plan_id = plan.as_ref().and_then(|p| p["plan_id"].as_str()).unwrap_or_default().to_owned();
                vec![(plans_tool::EXECUTE_PLAN, serde_json::json!({"plan_id": plan_id}))]
            } else if content::demo::wanted() {
                // Folderstorm: content editing: the reads now, each saved
                // once it answers (below)
                for (tool, arguments) in content::demo::reads() {
                    let (call, answer) = Pending::detached(tool, arguments, false, Some("Demo assistant"));
                    rt.desk.take(call, cx.get_or_insert_with(context), now);
                    rt.demo_reads.push(answer);
                }
                Vec::new()
            } else if gestures::demo::wanted() {
                // Folderstorm: the demo gesture read now (if seeded), else
                // the pose session first
                let (step, (tool, arguments)) = if view.world.inventory.items.contains_key(&gestures::demo::GESTURE) {
                    (0, gestures::demo::status())
                } else {
                    (2, posing::demo::start())
                };
                let (call, answer) = Pending::detached(tool, arguments, step != 0, Some("Demo assistant"));
                rt.desk.take(call, cx.get_or_insert_with(context), now);
                rt.demo_gesture = Some((step, answer));
                Vec::new()
            } else if building::demo::wanted() {
                // Folderstorm: the read now, then a move, a rez and a next owner change
                let (tool, arguments) = building::demo::get();
                let (call, answer) = Pending::detached(tool, arguments, false, Some("Demo assistant"));
                rt.desk.take(call, cx.get_or_insert_with(context), now);
                rt.demo_build = Some((0, answer));
                Vec::new()
            } else if script_testing::demo::wanted() {
                // Folderstorm: the listing now, then a touch, the dialog and a recipe
                let (tool, arguments) = script_testing::demo::list();
                let (call, answer) = Pending::detached(tool, arguments, false, Some("Demo assistant"));
                rt.desk.take(call, cx.get_or_insert_with(context), now);
                rt.demo_test = Some((0, answer));
                Vec::new()
            } else if task_scripts::demo::wanted() {
                // Folderstorm: the listing now, then a read, a save and a reset
                let (tool, arguments) = task_scripts::demo::list();
                let (call, answer) = Pending::detached(tool, arguments, false, Some("Demo assistant"));
                rt.desk.take(call, cx.get_or_insert_with(context), now);
                rt.demo_task = Some((0, answer));
                Vec::new()
            } else if objects::demo::wanted() {
                // Folderstorm: the listing now, the put once it answers (below)
                let (call, answer) = Pending::detached(objects::demo::LIST, objects::demo::list(), false, Some("Demo assistant"));
                rt.desk.take(call, cx.get_or_insert_with(context), now);
                rt.demo_listing = Some(answer);
                Vec::new()
            } else {
                vec![("inventory_create_folder", serde_json::json!({"name": "Sorted by the assistant"}))]
            };
            for (tool, arguments) in requests {
                let (call, answer) = Pending::detached(tool, arguments, true, Some("Demo assistant"));
                rt.desk.take(call, cx.get_or_insert_with(context), now);
                rt.demo_answers.push((tool, answer));
            }
        }
        let mut answered = Vec::new();
        // Folderstorm: a content read answered: the demo saves an edit
        for read in std::mem::take(&mut rt.demo_reads) {
            match read.try_recv() {
                Ok(result) => {
                    // the text itself is never logged
                    match &result {
                        Ok(read) => log::info!(
                            "local assistant demo: inventory_content_get answered {} bytes, editable {}",
                            read["bytes"],
                            read["editable"]
                        ),
                        Err(e) => log::info!("local assistant demo: inventory_content_get ended with {e}"),
                    }
                    if let Some((tool, arguments)) = result.ok().as_ref().and_then(content::demo::save_after) {
                        let (call, answer) = Pending::detached(tool, arguments, true, Some("Demo assistant"));
                        rt.desk.take(call, cx.get_or_insert_with(context), now);
                        rt.demo_answers.push((tool, answer));
                    }
                }
                Err(TryRecvError::Empty) => rt.demo_reads.push(read),
                Err(TryRecvError::Disconnected) => {}
            }
        }
        // Folderstorm: the demo's listing answered: a put into the same prim
        if let Some(listing) = rt.demo_listing.take() {
            match listing.try_recv() {
                Ok(result) => {
                    match &result {
                        Ok(page) => log::info!("local assistant demo: object_contents_list answered {} items", page["total"]),
                        Err(e) => log::info!("local assistant demo: object_contents_list ended with {e}"),
                    }
                    if let Some((tool, arguments)) = result.ok().as_ref().and_then(objects::demo::put_after) {
                        let (call, answer) = Pending::detached(tool, arguments, true, Some("Demo assistant"));
                        rt.desk.take(call, cx.get_or_insert_with(context), now);
                        rt.demo_answers.push((tool, answer));
                    }
                }
                Err(TryRecvError::Empty) => rt.demo_listing = Some(listing),
                Err(TryRecvError::Disconnected) => {}
            }
        }
        // Folderstorm: the task script demo's next step
        if let Some((step, answer)) = rt.demo_task.take() {
            match answer.try_recv() {
                Ok(result) => {
                    log_demo_task(step, &result);
                    let next = result.ok().and_then(|value| match step {
                        0 => task_scripts::demo::get_after(&value).map(|n| (1, n)),
                        1 => {
                            rt.demo_task_item = value["item_id"].as_str().map(str::to_owned);
                            task_scripts::demo::save_after(&value).map(|n| (2, n))
                        }
                        2 => Some((3, task_scripts::demo::list())),
                        3 => rt
                            .demo_task_item
                            .as_deref()
                            .and_then(|item| task_scripts::demo::reset_after(&value, item))
                            .map(|n| (4, n)),
                        _ => None,
                    });
                    if let Some((step, (tool, arguments))) = next {
                        let ask = matches!(step, 2 | 4);
                        let (call, answer) = Pending::detached(tool, arguments, ask, Some("Demo assistant"));
                        rt.desk.take(call, cx.get_or_insert_with(context), now);
                        rt.demo_task = Some((step, answer));
                    }
                }
                Err(TryRecvError::Empty) => rt.demo_task = Some((step, answer)),
                Err(TryRecvError::Disconnected) => {}
            }
        }
        // Folderstorm: the script testing demo's next step
        if let Some((step, answer)) = rt.demo_test.take() {
            match answer.try_recv() {
                Ok(result) => {
                    let running = step == 6 && result.as_ref().is_ok_and(|v| v["state"] == "running");
                    if !running {
                        log_demo_test(step, &result);
                    }
                    let next = result.ok().and_then(|value| match step {
                        0 => Some((1, script_testing::demo::touch())),
                        1 => Some((2, script_testing::demo::dialogs())),
                        2 => script_testing::demo::reply_after(&value).map(|n| (3, n)),
                        3 => Some((4, script_testing::demo::list())),
                        4 => script_testing::demo::begin_after(&value, script_testing::demo::near(view.world)).map(|n| (5, n)),
                        5 => value["run_id"].as_str().map(|id| (6, script_testing::demo::status(id))),
                        6 if running => value["run_id"].as_str().map(|id| (6, script_testing::demo::status(id))),
                        _ => None,
                    });
                    if let Some((step, (tool, arguments))) = next {
                        let ask = matches!(step, 1 | 3 | 5);
                        let (call, answer) = Pending::detached(tool, arguments, ask, Some("Demo assistant"));
                        rt.desk.take(call, cx.get_or_insert_with(context), now);
                        rt.demo_test = Some((step, answer));
                    }
                }
                Err(TryRecvError::Empty) => rt.demo_test = Some((step, answer)),
                Err(TryRecvError::Disconnected) => {}
            }
        }
        // Folderstorm: the building demo's next step
        if let Some((step, answer)) = rt.demo_build.take() {
            match answer.try_recv() {
                Ok(result) => {
                    log_demo_build(step, &result);
                    let next = result.ok().and_then(|value| match step {
                        0 => building::demo::move_after(&value).map(|n| (1, n)),
                        1 => Some((2, building::demo::rez(view.world.agent.position))),
                        2 => Some((3, building::demo::permissions())),
                        3 => building::demo::next_owner_after(&value).map(|n| (4, n)),
                        _ => None,
                    });
                    if let Some((step, (tool, arguments))) = next {
                        let ask = step != 3;
                        let (call, answer) = Pending::detached(tool, arguments, ask, Some("Demo assistant"));
                        rt.desk.take(call, cx.get_or_insert_with(context), now);
                        rt.demo_build = Some((step, answer));
                    }
                }
                Err(TryRecvError::Empty) => rt.demo_build = Some((step, answer)),
                Err(TryRecvError::Disconnected) => {}
            }
        }
        // Folderstorm: the gesture and posing demo's next step
        if let Some((step, answer)) = rt.demo_gesture.take() {
            match answer.try_recv() {
                Ok(result) => {
                    log_demo_gesture(step, &result);
                    let pose = rt.demo_pose.clone();
                    let next = result.ok().and_then(|value| match step {
                        0 => gestures::demo::play_after(&value).map(|n| (1, n)),
                        1 => Some((2, posing::demo::start())),
                        2 => {
                            rt.demo_pose = value["pose_session"].as_str().map(|s| (s.to_owned(), 0));
                            posing::demo::turn_after(&value).map(|n| (3, n))
                        }
                        3 => {
                            rt.demo_pose = pose.map(|(s, _)| (s, value["revision"].as_u64().unwrap_or_default()));
                            Some((4, posing::demo::presets()))
                        }
                        4 => pose.and_then(|(s, r)| posing::demo::load_after(&value, &s, r)).map(|n| (5, n)),
                        5 => {
                            let revision = value["revision"].as_u64().unwrap_or_default();
                            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
                            pose.map(|(s, _)| (6, posing::demo::save(&s, revision, &stamp)))
                        }
                        6 => Some((7, posing::demo::status())),
                        _ => None,
                    });
                    if let Some((step, (tool, arguments))) = next {
                        let ask = !matches!(step, 4 | 7);
                        let (call, answer) = Pending::detached(tool, arguments, ask, Some("Demo assistant"));
                        rt.desk.take(call, cx.get_or_insert_with(context), now);
                        rt.demo_gesture = Some((step, answer));
                    }
                }
                Err(TryRecvError::Empty) => rt.demo_gesture = Some((step, answer)),
                Err(TryRecvError::Disconnected) => {}
            }
        }
        rt.demo_answers.retain(|(tool, answer)| match answer.try_recv() {
            Ok(result) => {
                log_demo_answer(tool, &result);
                answered.push(*tool);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        // Folderstorm: the navigation demo's next steps
        if view.demo && demo_navigation() {
            use folderstorm_mcp_bridge::protocol::navigation::{MOVE_TO, STATUS, TELEPORT};
            let cx = cx.get_or_insert_with(context);
            if rt.demo_step == 0 && answered.contains(&MOVE_TO) {
                rt.demo_step = 1;
                if let Some(here) = view.world.agent_global() {
                    let to = here + glam::DVec3::new(-14.0, 10.0, 0.0);
                    let arguments = serde_json::json!({"global_position": [to.x, to.y, to.z], "client_request_id": "demo-teleport-1"});
                    let (call, answer) = Pending::detached(TELEPORT, arguments, true, Some("Demo assistant"));
                    rt.desk.take(call, cx, now);
                    rt.demo_answers.push((TELEPORT, answer));
                    rt.demo_step = 2;
                }
            } else if rt.demo_step == 2 && answered.contains(&TELEPORT) {
                rt.demo_step = 3;
                let (call, answer) = Pending::detached(STATUS, serde_json::json!({}), false, Some("Demo assistant"));
                rt.desk.take(call, cx, now);
                if let Ok(result) = answer.try_recv() {
                    log_demo_answer(STATUS, &result);
                }
            }
        }
        rt.studio.tick(&view.camera, now);
        rt.desk.tick_reads(view.world, now);
        // Folderstorm: the gesture and pose tools' waits, playback and session
        if rt.desk.avatar_busy() {
            rt.desk.tick_avatar(cx.get_or_insert_with(context), now);
        }
        rt.desk.tick(&view.bake(), now)
    })
}

/// Folderstorm (session routing): the Ask dialog's open question, for a
/// managed viewer's Attention list, so that the host's inbox shows it while
/// this character is in the background: its number, the program's name and
/// what it asks. None when nothing is asked.
pub fn open_question() -> Option<desk::Question> {
    with(|rt| rt.desk.question(Instant::now()))
}

/// Folderstorm: once per frame, just before the autopilot runs: the
/// assistant's move or teleport under way is followed from what the frame
/// shows; returns what the app does now (phase 3, part 4).
pub fn navigation_frame(frame: &navigation::Frame) -> Vec<navigation::Act> {
    with(|rt| rt.desk.navigate(frame, Instant::now()))
}

/// Folderstorm: the app started the assistant's move with autopilot `pilot`.
pub fn navigation_moving(call: Pending, mv: navigation::Move, pilot: navigation::PilotId) {
    with(|rt| rt.desk.start_move(call, mv, pilot, Instant::now()));
}

/// Folderstorm: the app submitted the assistant's teleport.
pub fn navigation_teleporting(call: Pending, teleport: navigation::Teleport) {
    with(|rt| rt.desk.start_teleport(call, teleport, Instant::now()));
}

/// Folderstorm: how long before the assistant may submit a teleport (None:
/// one is under way).
pub fn teleport_wait() -> Option<std::time::Duration> {
    with(|rt| rt.desk.teleport_wait(Instant::now()))
}

/// Folderstorm: Preferences' Stop: the operation is cancelled if it was not
/// sent yet (folderstorm/plans/mcp-receipts.md, section 5).
pub fn cancel(operation_id: &str) {
    with(|rt| {
        if let Err(e) = rt.desk.cancel(operation_id, true) {
            log::info!("local assistant: nothing to stop: {e}");
        }
    });
}

/// Folderstorm: whether `item` was just created by the local assistant
/// (asked once, when the app files it: it then neither selects nor renames
/// it in the inventory, as Folderstorm (C++) opens no editor for it).
pub fn created_by_assistant(item: Uuid) -> bool {
    with(|rt| match rt.created.iter().position(|id| *id == item) {
        Some(index) => {
            rt.created.remove(index);
            true
        }
        None => false,
    })
}

/// Folderstorm: what the waiting content reads need fetched: the app sends
/// a notecard's `PreviewInventoryItem` or a script's `LoadScript`.
pub fn wanted_content() -> Vec<content::Fetch> {
    with(|rt| rt.desk.wanted_content())
}

/// Folderstorm: the app's fetch `request` could not be sent.
pub fn content_fetch_failed(request: Uuid, reason: String) {
    with(|rt| rt.desk.content().fetch_failed(request, reason));
}

/// Folderstorm: a notecard's asset as the demo serves it (the demo's
/// `PreviewInventoryItem` does not pass through [`observe`]).
pub fn content_demo_notecard(item: Uuid, asset: Uuid, bytes: Vec<u8>) {
    with(|rt| rt.desk.content().notecard_loaded(item, asset, &Ok(bytes)));
}

/// Folderstorm: the scripts the assistant saved (item, new asset), for the
/// app's inventory.
pub fn content_updates() -> Vec<(Uuid, Uuid)> {
    with(|rt| rt.desk.content().take_updates())
}

/// Folderstorm: whether an assistant's save of `item` is under way.
pub fn content_saving(item: Uuid) -> bool {
    with(|rt| rt.desk.content_saving(item))
}

/// Folderstorm: what the object tools need sent now: an object's
/// properties or its contents (mcp-objects.md).
pub fn object_wants() -> Vec<objects::Want> {
    with(|rt| rt.desk.objects().wanted())
}

/// Folderstorm: the app sends an object change now (`send`, planned with
/// `reply`): the viewer watches for its outcome.
pub fn object_sending(call: Pending, send: &objects::Send, reply: serde_json::Value, world: &World) {
    with(|rt| rt.desk.objects().sending(call, send, reply, world, Instant::now()));
}

/// Folderstorm: an association of the object tools, recorded now.
pub fn object_associate(call: Pending, reply: serde_json::Value) {
    with(|rt| rt.desk.objects().associate(call, reply));
}

/// Folderstorm: what the waiting task script reads need fetched, and the
/// running states to ask for (mcp-task-scripts.md).
pub fn task_script_wants() -> (Vec<task_scripts::Fetch>, Vec<task_scripts::Query>) {
    with(|rt| {
        let scripts = rt.desk.task_scripts();
        (scripts.wanted(), scripts.wanted_queries())
    })
}

/// Folderstorm: a task script fetch could not be sent.
pub fn task_script_fetch_failed(source_request: Uuid, reason: &str) {
    with(|rt| rt.desk.task_scripts().fetch_failed(source_request, reason));
}

/// Folderstorm: the app sends a task script change now; a save's upload
/// request id comes back.
pub fn task_script_sending(call: Pending, send: &task_scripts::Send, reply: serde_json::Value) -> Option<Uuid> {
    with(|rt| rt.desk.task_scripts().sending(call, send, reply, Instant::now()))
}

/// Folderstorm: the task scripts saved since the last call: prim, item and
/// new asset, for the Contents tab.
pub fn task_script_updates() -> Vec<(Uuid, Uuid, Uuid)> {
    with(|rt| rt.desk.task_scripts().take_updates())
}

/// Folderstorm: whether a save of that script is under way.
pub fn task_script_saving(object: Uuid, item: Uuid) -> bool {
    with(|rt| rt.desk.task_scripts().saving(object, item))
}

/// Folderstorm: the sends the test run under way wants now
/// (mcp-script-testing.md).
pub fn script_test_sends() -> Vec<(u64, script_testing::Send)> {
    with(|rt| rt.desk.testing().wanted())
}

/// Folderstorm: a test run's send `token` left, or could not.
pub fn script_test_sent(token: u64, result: Result<(), String>) {
    with(|rt| rt.desk.testing().sent(token, result, Instant::now()));
}

/// Folderstorm: a planned interaction (touch, dialog answer, channel line)
/// was sent, or could not be.
pub fn interaction_sent(call: Pending, interaction: &script_testing::Interaction, reply: serde_json::Value, result: Result<(), ToolError>) {
    with(|rt| rt.desk.testing().interacted(call, interaction, reply, result, Instant::now()));
}

/// Folderstorm: a planned test run starts.
pub fn script_test_start(call: Pending, recipe: script_testing::Recipe) {
    with(|rt| rt.desk.testing().start(call, recipe, Instant::now()));
}

/// Folderstorm: the app sends a building change now (`send`, planned with
/// `reply`): the viewer watches for its outcome (mcp-building.md).
pub fn build_sending(call: Pending, send: &building::Send, reply: serde_json::Value, world: &World) {
    with(|rt| rt.desk.build_sending(call, send, reply, world, Instant::now()));
}

/// Folderstorm: the gestures the gesture tools want loaded now (the app
/// loads them as the Gestures window does, never activating them).
pub fn gesture_loads() -> Vec<Uuid> {
    with(|rt| rt.desk.gestures().take_loads())
}

/// Folderstorm: the assistant's playbacks the app stops now (a
/// revocation, a login change, access off, a teleport, 120 s).
pub fn gesture_stops() -> Vec<Uuid> {
    with(|rt| rt.desk.gestures().take_stops())
}

/// Folderstorm: the app started the planned play under `key`.
pub fn gesture_started(call: Pending, key: Uuid, item: Uuid, reply: serde_json::Value) {
    with(|rt| rt.desk.gestures().started(call, key, item, reply, Instant::now()));
}

/// Folderstorm: the app stopped the playback under `key`.
pub fn gesture_stopped(call: Pending, key: Uuid, reply: serde_json::Value) {
    with(|rt| rt.desk.gestures().stopped(call, key, reply));
}

/// Folderstorm: the assistant's pose of the user's avatar for this frame
/// (`native`: the viewer's poser poses the user, and wins).
pub fn pose_layer(native: bool) -> Option<std::sync::Arc<crate::ik_edit::poser::SubjectPose>> {
    with(|rt| rt.desk.posing().layer(native))
}

/// Folderstorm: run a planned pose change now.
pub fn pose_run(call: Pending, work: posing::Work, reply: serde_json::Value, world: &World) {
    with(|rt| rt.desk.posing().run(call, work, reply, world, Instant::now()));
}

/// Folderstorm: the landmark assets `inventory_landmark` waits for (the app
/// asks the landmark cache for them, phase 3).
pub fn wanted_landmarks() -> Vec<Uuid> {
    with(|rt| rt.desk.wanted_landmarks())
}

/// Folderstorm: whether the Image window's job `request` is the local
/// assistant's `inventory_set_thumbnail` (it then needs no window).
pub fn image_job(request: Uuid) -> bool {
    with(|rt| rt.desk.image_job(request))
}

/// Folderstorm: the Image window's job `request` ended (an error: why).
pub fn image_finished(request: Uuid, error: Option<&str>) {
    if let Some(error) = error {
        with(|rt| rt.desk.image_failed(request, error));
    }
}

/// Decide and plan a change again just before it is sent: the permissions,
/// the login or the inventory may have changed while the user read the
/// dialog. None: the call was answered, or it went back to the dialog.
pub fn replan(call: Pending, view: &View) -> Option<(Pending, changes::Planned)> {
    let cx = view.context();
    let (call, object_plan) = with(|rt| {
        let Some((generation, session)) = rt.rebind(view) else {
            call.answer(Err(ToolError::new(ErrorCode::PermissionDenied, desk::ACCESS_OFF)));
            return None;
        };
        let call = rt.desk.recheck(call, generation, session, &cx, Instant::now())?;
        // Folderstorm: an object change is planned again against its
        // snapshot, the prim unchanged since (mcp-objects.md)
        let planned = objects::handles(&call.tool).then(|| rt.desk.objects().plan(&call, &cx, Instant::now(), true));
        // Folderstorm: and a task script change (mcp-task-scripts.md)
        let planned = planned.or_else(|| task_scripts::is_change(&call.tool).then(|| rt.desk.task_script_plan(&call, &cx, Instant::now())));
        // Folderstorm: and a script testing change (mcp-script-testing.md)
        let planned =
            planned.or_else(|| script_testing::is_change(&call.tool).then(|| rt.desk.script_test_plan(&call, &cx, Instant::now())));
        // Folderstorm: and a building change (mcp-building.md)
        let planned = planned.or_else(|| building::is_change(&call.tool).then(|| rt.desk.building_plan(&call, &cx, Instant::now())));
        // Folderstorm: and a gesture or pose change (mcp-gestures.md)
        let planned =
            planned.or_else(|| (gestures::is_change(&call.tool) || posing::is_change(&call.tool)).then(|| rt.desk.avatar_plan(&call, &cx)));
        Some((call, planned))
    })?;
    match object_plan.unwrap_or_else(|| changes::plan(&call.tool, &call.arguments, &cx)) {
        Ok(planned) => Some((call, planned)),
        Err(e) => {
            call.answer(Err(e));
            None
        }
    }
}

/// A change starts: what the server answers while the app sends it belongs
/// to `call` ([`observe`]).
pub fn begin(call: Pending, planned: &changes::Planned, outfit: bool) {
    with(|rt| rt.desk.begin(call, planned, outfit, Instant::now()));
}

/// The change is sent: `request` is the id the window's or Appearance's
/// pending change took (None when it did not start, `failure` saying why).
pub fn sent(request: Option<Uuid>, failure: Option<String>) {
    with(|rt| rt.desk.sent(request, failure));
}

/// Every inventory and outfit answer of the server passes here first, so
/// that a waiting change learns its outcome.
pub fn observe(event: &aurora_net::NetEvent) {
    // Folderstorm: the simulator's reason, for the assistant's teleport
    if let aurora_net::NetEvent::TeleportFailed { reason } = event {
        with(|rt| rt.desk.teleport_failed(reason));
        return;
    }
    // Folderstorm: objects' properties and contents (mcp-objects.md); a
    // listing also tells the task script snapshots (mcp-task-scripts.md)
    if matches!(
        event,
        aurora_net::NetEvent::ObjectProperties(_) | aurora_net::NetEvent::TaskInventory { .. }
    ) {
        return with(|rt| {
            rt.desk.task_script_observe(event, Instant::now());
            rt.desk.objects().observe(event, Instant::now());
        });
    }
    // Folderstorm: scripts inside objects: their source, association, save
    // and running state (mcp-task-scripts.md)
    match event {
        aurora_net::NetEvent::ScriptLoaded { target, .. } | aurora_net::NetEvent::ScriptSaved { target, .. } if !target.object.is_nil() => {
            return with(|rt| rt.desk.task_script_observe(event, Instant::now()));
        }
        aurora_net::NetEvent::ScriptExperiences { .. } | aurora_net::NetEvent::ScriptRunning { .. } => {
            return with(|rt| {
                rt.desk.task_script_observe(event, Instant::now());
                // Folderstorm: a test run's running assertions (mcp-script-testing.md)
                rt.desk.testing().observe(event);
            });
        }
        _ => {}
    }
    if let aurora_net::NetEvent::InventoryCreated(item) = event {
        with(|rt| {
            if rt.desk.created(item) {
                rt.created.truncate(15);
                rt.created.insert(0, item.id);
            }
        });
        return;
    }
    // Folderstorm: a content read's text (mcp-content.md)
    match event {
        aurora_net::NetEvent::InventoryPreview { item, asset, result } => {
            return with(|rt| rt.desk.content().notecard_loaded(*item, *asset, result));
        }
        aurora_net::NetEvent::ScriptLoaded { request, target, result } if target.object.is_nil() => {
            return with(|rt| rt.desk.content().script_loaded(*request, result));
        }
        _ => {}
    }
    let Some(outcome) = desk::outcome(event) else { return };
    with(|rt| rt.desk.observe(outcome));
}

/// Stop the bridge and delete its discovery file (the viewer quits).
pub fn shutdown() {
    with(|rt| {
        rt.desk.close();
        rt.studio = studio::Studio::default();
        rt.running = None;
    });
}

/// Folderstorm: the task script demo's step answered: what it did, never
/// the source.
fn log_demo_task(step: u8, result: &Result<serde_json::Value, ToolError>) {
    match result {
        Ok(value) => {
            let mut value = value.clone();
            if let Some(object) = value.as_object_mut() {
                object.remove("text");
                object.remove("items");
            }
            log::info!("local assistant demo: task script step {step} answered {value}");
        }
        Err(e) => log::info!("local assistant demo: task script step {step} ended with {e}"),
    }
}

/// Folderstorm: the script testing demo's step answered: what it did,
/// never the grid's text.
fn log_demo_test(step: u8, result: &Result<serde_json::Value, ToolError>) {
    match result {
        Ok(value) => {
            let mut value = value.clone();
            if let Some(object) = value.as_object_mut() {
                object.remove("items");
                object.remove("dialogs");
                object.remove("steps");
                object.remove("runtime_errors");
                object.remove("note");
            }
            log::info!("local assistant demo: script testing step {step} answered {value}");
        }
        Err(e) => log::info!("local assistant demo: script testing step {step} ended with {e}"),
    }
}

/// Folderstorm: the building demo's step answered: what it did, never the
/// grid's text.
fn log_demo_build(step: u8, result: &Result<serde_json::Value, ToolError>) {
    match result {
        Ok(value) => {
            let mut value = value.clone();
            if let Some(object) = value.as_object_mut() {
                object.remove("faces");
                object.remove("name");
                object.remove("note");
            }
            log::info!("local assistant demo: building step {step} answered {value}");
        }
        Err(e) => log::info!("local assistant demo: building step {step} ended with {e}"),
    }
}

/// Folderstorm: the gesture and posing demo's step answered: what it did,
/// never the gesture's text nor the joints.
fn log_demo_gesture(step: u8, result: &Result<serde_json::Value, ToolError>) {
    match result {
        Ok(value) => {
            let mut value = value.clone();
            if let Some(object) = value.as_object_mut() {
                for key in ["steps", "name", "trigger", "replacement", "note", "presets", "joints"] {
                    object.remove(key);
                }
            }
            log::info!("local assistant demo: gestures step {step} answered {value}");
        }
        Err(e) => log::info!("local assistant demo: gestures step {step} ended with {e}"),
    }
}

/// The demo's request answered: what it did, never the picture's bytes.
fn log_demo_answer(tool: &str, result: &Result<serde_json::Value, ToolError>) {
    match result {
        Ok(value) => {
            let mut value = value.clone();
            if let Some(object) = value.as_object_mut() {
                object.remove(folderstorm_mcp_bridge::protocol::IMAGE_KEY);
            }
            log::info!("local assistant demo: {tool} answered {value}");
        }
        Err(e) => log::info!("local assistant demo: {tool} ended with {e}"),
    }
}

/// A camera move was made: its call answers once the camera stops. Bound,
/// as the calls below, to the login it was decided under (`replan`).
pub fn wait_move(call: Pending, reply: serde_json::Value) {
    call.receipt(Receipt::mark_sent); // Folderstorm: receipts
    with(|rt| rt.studio.wait_move(call, reply, rt.desk.login(), Instant::now()));
}

/// A picture to take once the camera stops.
pub fn queue_photo(call: Pending, request: picture::Request) {
    call.receipt(Receipt::mark_sent);
    with(|rt| rt.studio.queue_photo(call, request, rt.desk.login(), Instant::now()));
}

/// In the frame's drawing: whether to start a picture's photo now, decided
/// again just before with the login in force.
pub fn photo_step(drawing: bool, view: &View) -> studio::PhotoStep {
    with(|rt| {
        let now = Instant::now();
        rt.review_studio(view, now);
        rt.studio.photo_step(drawing, now)
    })
}

/// The picture's photo started, planned as `plan`.
pub fn photo_started(plan: picture::Plan) {
    with(|rt| rt.studio.photo_started(plan));
}

/// The picture's photo came back: encoded (and saved in `folder`) on a
/// worker.
pub fn photo_drawn(size: (u32, u32), pixels: Vec<u8>, folder: std::path::PathBuf) {
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string();
    with(|rt| rt.studio.photo_drawn(size, pixels, folder, stamp));
}

/// A thumbnail of the assistant started through the gallery's snapshot.
pub fn thumbnail_started(call: Pending, reply: serde_json::Value) {
    call.receipt(Receipt::mark_sent);
    with(|rt| rt.studio.thumbnail_started(call, reply, rt.desk.login(), Instant::now()));
}

/// Folderstorm: just before the gallery's snapshot is sent as an entry's
/// picture: decided again with the login in force. False: it is the
/// assistant's thumbnail, refused or asking again, and the app stops it.
pub fn thumbnail_send(view: &View) -> bool {
    with(|rt| {
        rt.review_studio(view, Instant::now());
        rt.studio.thumbnail_send()
    })
}

/// Whether the assistant's thumbnail is under way.
pub fn thumbnail_busy() -> bool {
    with(|rt| rt.studio.thumbnail_busy())
}

/// The gallery's snapshot ended (`app/inventory_thumbnail.rs`): the
/// assistant's thumbnail, if it was one, learns how.
pub fn thumbnail_done(error: Option<&str>) {
    with(|rt| rt.studio.thumbnail_done(error));
}

/// The field of view an exact shot replaced, kept until the camera is reset.
pub fn fov_before() -> Option<f32> {
    with(|rt| rt.studio.fov_before)
}

pub fn set_fov_before(fov: Option<f32>) {
    with(|rt| rt.studio.fov_before = fov);
}

/// LLPanelWearableOutfitItem::updateItem, as `ui/appearance/items.rs`
/// reads it: clothing and body parts follow the Current Outfit's links,
/// objects are worn when the simulator reports them attached.
fn worn_items(world: &World) -> HashSet<Uuid> {
    use crate::world::appearance;
    let inv = &world.inventory;
    let mut worn = world.worn_attachment_items();
    if let Some(cof) = appearance::cof(inv) {
        worn.extend(
            appearance::folder_links(inv, cof)
                .into_iter()
                .filter(|link| !link.folder && inv.items.get(&link.target).is_some_and(|it| it.asset_type != 6))
                .map(|link| link.target),
        );
    }
    worn
}
