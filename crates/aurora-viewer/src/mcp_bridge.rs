//! Folderstorm: the viewer's side of the local assistant bridge
//! (FEATURES.md 4.1, folderstorm/plans/mcp.md). The listener, the token,
//! the discovery file and the permission checks live in the
//! `folderstorm-mcp-bridge` crate; this module starts and stops it from the
//! settings, keeps the "this session only" copy of them, and answers the
//! calls on the frame thread, a few per frame, without ever blocking it.

pub mod setup;
#[cfg(test)]
mod tests;
pub mod tools;

use crate::world::World;
use folderstorm_mcp_bridge::server::{Activity, Config, Pending, VIEWER_TIMEOUT};
use folderstorm_mcp_bridge::{Bridge, LocalAssistant, ToolError};
use std::collections::HashSet;
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, PoisonError};
use uuid::Uuid;

/// Calls answered per frame at most; the rest wait for the next frames.
const PER_FRAME: usize = 4;

struct Running {
    bridge: Bridge,
    calls: Receiver<Pending>,
}

#[derive(Default)]
struct Runtime {
    running: Option<Running>,
    /// "Use these settings for this session only": the settings in force
    /// instead of the saved ones until the viewer quits.
    session: Option<LocalAssistant>,
    /// `--mcp-api` (and the demo's `AURORA_DEMO_MCP`) applied once.
    started: bool,
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

/// Once per frame: start or stop the bridge as the settings say, give it
/// the permissions in force, and answer a few waiting calls.
pub fn frame(saved: &LocalAssistant, world: &World, demo: bool) {
    with(|rt| {
        if !rt.started {
            rt.started = true;
            let demo_on = demo && std::env::var_os("AURORA_DEMO_MCP").is_some();
            if crate::cli::get().mcp_api || demo_on {
                // forced on for this session; the saved setting is untouched
                rt.session = Some(LocalAssistant { enabled: true, ..*saved });
                log::info!("local assistant access turned on for this session only (--mcp-api)");
            }
        }
        let wanted = rt.session.unwrap_or(*saved);
        if !wanted.enabled {
            rt.failure = None;
            rt.running = None;
            return;
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
        let Some(running) = &rt.running else { return };
        if running.bridge.permissions() != wanted.permissions {
            running.bridge.set_permissions(wanted.permissions);
        }
        answer_waiting(&running.calls, world, demo);
    });
}

/// Stop the bridge and delete its discovery file (the viewer quits).
pub fn shutdown() {
    with(|rt| rt.running = None);
}

fn answer_waiting(calls: &Receiver<Pending>, world: &World, demo: bool) {
    let mut cx = None;
    for _ in 0..PER_FRAME {
        let Ok(call) = calls.try_recv() else { break };
        let cx = cx.get_or_insert_with(|| context(world, demo));
        let result: Result<_, ToolError> = tools::answer(&call.tool, &call.arguments, cx);
        call.answer(result);
    }
}

/// What the tools read this frame.
fn context(world: &World, demo: bool) -> tools::Ctx<'_> {
    let inv = &world.inventory;
    let session = if demo {
        tools::Session::Demo
    } else if world.login.is_some() {
        tools::Session::LoggedIn
    } else {
        tools::Session::LoggedOut
    };
    tools::Ctx {
        inv,
        session,
        worn: worn_items(world),
        attachment_points: world.worn_attachment_points(),
        inventory_locked: world.rlva.has("showinv"),
    }
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
