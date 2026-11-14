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

pub mod ask;
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
pub mod inventory_edits;
pub mod inventory_reads;
pub mod journal;
pub mod landmark;
pub mod login;
pub mod picture;
pub mod plans; // Folderstorm: batch plans (mcp-receipts.md, section 6)
pub mod setup;
pub mod studio;
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
    /// The demo's requests waiting for their answer, logged when it comes.
    demo_answers: Vec<(&'static str, Receiver<Result<serde_json::Value, ToolError>>)>,
    /// Folderstorm: `AURORA_DEMO_MCP=content`: the demo's reads, saved
    /// once they answer.
    demo_reads: Vec<Receiver<Result<serde_json::Value, ToolError>>>,
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

/// Where the discovery file goes: the settings folder in use.
pub fn discovery_dir() -> std::path::PathBuf {
    crate::settings::config_dir()
}

/// `AURORA_DEMO_MCP=ask` (or `camera`, `plan`): the session asks before
/// every change, and the demo makes requests of its own so that the dialog
/// shows.
fn demo_asks() -> bool {
    std::env::var("AURORA_DEMO_MCP").is_ok_and(|v| v == "ask" || v == "camera" || v == "plan" || v == "content")
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
    /// Folderstorm: the items open in the notecard window or the script
    /// editor, which content saves leave alone.
    pub content_open: HashSet<Uuid>,
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
            content: content::Limits {
                open: self.content_open.clone(),
                ..content::Limits::of(&world.rlva)
            },
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
        let session = running.bridge.viewer_session();
        rt.desk.set_login(generation, session.clone());
        rt.desk.set_permissions(wanted.permissions);
        let now = Instant::now();
        rt.studio.observe(view.camera, now);
        let mut cx = None;
        let context = || tools::Ctx {
            viewer_session: session.clone(),
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
            && (!demo_camera() || view.stage.in_world)
            && (!content::demo::wanted() || view.world.inventory.items.contains_key(&content::demo::NOTES));
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
            } else {
                vec![("inventory_create_folder", serde_json::json!({"name": "Sorted by the assistant"}))]
            };
            for (tool, arguments) in requests {
                let (call, answer) = Pending::detached(tool, arguments, true, Some("Demo assistant"));
                rt.desk.take(call, cx.get_or_insert_with(context), now);
                rt.demo_answers.push((tool, answer));
            }
        }
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
        rt.demo_answers.retain(|(tool, answer)| match answer.try_recv() {
            Ok(result) => {
                log_demo_answer(tool, &result);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        rt.studio.tick(&view.camera, now);
        rt.desk.tick_reads(view.world, now);
        rt.desk.tick(&view.bake(), now)
    })
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

/// Folderstorm: the scripts the assistant saved keep their new asset on
/// the app's inventory (`Content::hold`), before the frame's calls.
pub fn content_hold(inventory: &mut crate::world::inventory::Inventory) {
    with(|rt| rt.desk.content().hold(inventory));
}

/// Folderstorm: whether an assistant's save of `item` is under way.
pub fn content_saving(item: Uuid) -> bool {
    with(|rt| rt.desk.content_saving(item))
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
    let call = with(|rt| {
        let Some((generation, session)) = rt.rebind(view) else {
            call.answer(Err(ToolError::new(ErrorCode::PermissionDenied, desk::ACCESS_OFF)));
            return None;
        };
        rt.desk.recheck(call, generation, session, &cx, Instant::now())
    })?;
    match changes::plan(&call.tool, &call.arguments, &cx) {
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
