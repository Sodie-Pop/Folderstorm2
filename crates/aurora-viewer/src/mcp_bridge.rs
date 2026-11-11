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

pub mod ask;
#[cfg(test)]
mod change_tests;
pub mod changes;
pub mod desk;
pub mod setup;
#[cfg(test)]
mod tests;
pub mod tools;

use crate::world::World;
pub use desk::Desk;
use folderstorm_mcp_bridge::server::{Activity, Config, Pending, VIEWER_TIMEOUT};
use folderstorm_mcp_bridge::{Bridge, LocalAssistant, Preset, ToolError};
use std::collections::HashSet;
use std::sync::mpsc::Receiver;
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
    /// "Use these settings for this session only": the settings in force
    /// instead of the saved ones until the viewer quits.
    session: Option<LocalAssistant>,
    /// `--mcp-api` (and the demo's `AURORA_DEMO_MCP`) applied once.
    started: bool,
    /// `AURORA_DEMO_MCP=ask`: the demo's own request was made.
    demo_asked: bool,
    /// The bridge could not start for the access turned on now; tried
    /// again when access is turned off and on.
    failure: Option<String>,
}

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);

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
}

pub fn status() -> Status {
    with(|rt| Status {
        port: rt.running.as_ref().map(|r| r.bridge.port()),
        failure: rt.failure.clone(),
        activity: rt.running.as_ref().map(|r| r.bridge.activity()).unwrap_or_default(),
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

/// `AURORA_DEMO_MCP=ask`: the session asks before every change, and the
/// demo makes one request of its own so that the dialog shows.
fn demo_asks() -> bool {
    std::env::var("AURORA_DEMO_MCP").is_ok_and(|v| v == "ask")
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
        }
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
        if !wanted.enabled {
            rt.failure = None;
            rt.running = None;
            // the waiting calls end with the bridge ("the call was dropped")
            rt.desk = Desk::default();
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
        let now = Instant::now();
        let mut cx = None;
        for _ in 0..PER_FRAME {
            let Ok(call) = running.calls.try_recv() else { break };
            let cx = cx.get_or_insert_with(|| view.context());
            rt.desk.take(call, cx, now);
        }
        if view.demo && !rt.demo_asked && demo_asks() && !view.world.inventory.root.is_nil() {
            rt.demo_asked = true;
            let (call, _answer) = Pending::detached(
                "inventory_create_folder",
                serde_json::json!({"name": "Sorted by the assistant"}),
                true,
                Some("Demo assistant"),
            );
            rt.desk.take(call, cx.get_or_insert_with(|| view.context()), now);
        }
        rt.desk.tick(&view.bake(), now)
    })
}

/// Plan a change again just before it runs: the inventory may have changed
/// while the user read the dialog.
pub fn replan(call: &Pending, view: &View) -> Result<changes::Planned, ToolError> {
    changes::plan(&call.tool, &call.arguments, &view.context())
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
    let Some(outcome) = desk::outcome(event) else { return };
    with(|rt| rt.desk.observe(outcome));
}

/// Stop the bridge and delete its discovery file (the viewer quits).
pub fn shutdown() {
    with(|rt| {
        rt.desk = Desk::default();
        rt.running = None;
    });
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
