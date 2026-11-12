//! Folderstorm: several characters on one PC, checkpoint 1 of the session
//! host (`folderstorm/plans/session-host.md`, FEATURES.md 5.1). The rules
//! are in the `folderstorm-session` crate; this module holds what the
//! viewer keeps of them:
//! - [`profile`]: `--profile <name>`, its folders, its lease, its link
//!   hand-over scope, its log name and window title;
//! - [`Settings`]: Preferences › Graphics › "3D while another Folderstorm2
//!   window is in front", and whether the profile shares the main cache;
//! - [`State`]: what the frames keep of it (the `impl App` glue is in
//!   `app/folderstorm_characters.rs`).
//!
//! Checkpoint 2, the session host:
//! - [`host`]: `aurora-viewer --session-host`, the window that starts up to
//!   five characters, switches between them and watches over them;
//! - [`managed`]: `--managed`, a viewer's link to the host that started it;
//! - [`transport`]: the private local endpoint between the two.
//!
//! Checkpoint 3, the shared chat:
//! - [`chat_export`]: what a managed viewer tells the host of its
//!   conversations, read from the world as its own window shows it.
//!
//! Checkpoint 4, comfort:
//! - [`profile_copy`]: "New character profile…", the files of a new
//!   profile made from another one (the host's quick actions and global
//!   hotkeys are in [`host`]).
//!
//! Checkpoint 5, monitors:
//! - [`Monitoring`]: what a managed viewer keeps of the pictures it takes
//!   for the host (the `impl App` glue is in
//!   `app/folderstorm_characters/monitor.rs`; the host's side in [`host`]).
//!
//! Aurora's files reach this through small hooks marked `// Folderstorm:`.

pub mod chat_export;
pub mod host;
pub mod managed;
pub mod profile;
pub mod profile_copy;
pub mod transport;

use ::folderstorm_session::behind::{self, Behind3d};
use serde::{Deserialize, Serialize};

/// This profile's choices, saved in its own settings file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// What this window does while another Folderstorm2 window is in front:
    /// Off for a named profile, Normal (as before) for the main one.
    pub behind_3d: Behind3d,
    /// Frames per second behind with Off or Limited (2 to 15).
    pub behind_fps: u32,
    /// A named profile reads and writes the main profile's asset cache
    /// instead of its own (applies at the next start).
    pub share_main_cache: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            behind_3d: Behind3d::default_for(profile::current().is_some()),
            behind_fps: behind::FPS_DEFAULT,
            share_main_cache: false,
        }
    }
}

/// What the frames keep: whether the window is behind another Folderstorm2
/// window (as of the last frame), the demo's stand-in for that other
/// window, and a device loss left to wait while not in front.
#[derive(Debug, Default)]
pub struct State {
    /// Behind another Folderstorm2 window, as of the last frame.
    pub behind: bool,
    /// The 3D is paused by the Off policy, as of the last frame.
    pub paused: bool,
    /// `AURORA_DEMO_SESSION_BEHIND`: ended by a click in the window.
    pub demo_released: bool,
    /// Where the window is for the GPU rules, as of the last frame.
    pub place: ::folderstorm_session::gpu::Place,
    /// A GPU device loss is waiting for the window to come to the front.
    pub loss_waiting: bool,
    /// A GPU stall behind whose box was held back has been logged.
    pub stall_held: bool,
    /// The session host's hold on this viewer (`--managed`).
    pub managed: ::folderstorm_session::managed::Managed,
    /// What the host is told of this viewer.
    pub report: Report,
    /// What the host is told of this viewer's conversations.
    pub chat: chat_export::Export,
    /// The session host asked this viewer to quit.
    pub quit_by_host: bool,
    /// The window's own focus as last read from it. A frame takes the
    /// window out of the app while it is drawn: the rules asked during the
    /// frame go by what the window said just before.
    pub window_focus: std::cell::Cell<Option<bool>>,
    /// The monitor's pictures taken for the session host (checkpoint 5).
    pub monitor: Monitoring,
}

/// What a managed viewer keeps of its monitor (plan section 12): when the
/// next picture is due, and the one asked of the renderer and not
/// answered yet. No picture is kept here: it goes to the link at once.
#[derive(Debug, Default)]
pub struct Monitoring {
    pub schedule: ::folderstorm_session::monitor::Schedule,
    /// The size of the picture asked of the renderer, until its answer.
    pub asked: Option<(u32, u32)>,
    /// The host asked for pictures as of the last frame.
    pub ordered: bool,
    /// The number of the last picture sent.
    pub seq: u64,
    /// What the host was last told of the monitor.
    pub state: ::folderstorm_session::monitor::State,
    /// Pictures sent, and the time their frames took in all, for the log.
    pub sent: u64,
    pub waited: std::time::Duration,
}

/// The window's focus for the session's rules: as the window says when it
/// is there to ask (`window`: None while a frame holds it, or before it is
/// made), which is then remembered; otherwise as it last said. None: no
/// window ever said anything.
pub fn window_focus(remembered: &std::cell::Cell<Option<bool>>, window: Option<bool>) -> Option<bool> {
    if window.is_some() {
        remembered.set(window);
    }
    remembered.get()
}

/// What a managed viewer keeps of its reports to the host.
#[derive(Debug, Default)]
pub struct Report {
    /// Changes at every login and logout.
    pub generation: u32,
    /// Logged in (or logging in) as of the last report.
    pub logged_in: bool,
    /// The last status looked at, and when; the last one sent, and when.
    pub checked: Option<std::time::Instant>,
    pub sent: Option<(::folderstorm_session::ipc::Status, std::time::Instant)>,
    /// The host asked for the window before there was one: brought forward
    /// (on that stage, if any) as soon as it exists.
    pub show: Option<Option<::folderstorm_session::ipc::Stage>>,
}

/// `AURORA_DEMO_SESSION_BEHIND=1|off|limited|normal`, demo only: the window
/// counts as behind another Folderstorm2 window (the instances registry is
/// off in demo mode) until a click in it; `1` keeps this profile's policy,
/// the others replace it for the run.
pub fn demo_behind() -> Option<Option<Behind3d>> {
    static VALUE: std::sync::OnceLock<Option<Option<Behind3d>>> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var_os("AURORA_DEMO")?;
        parse_demo(&std::env::var("AURORA_DEMO_SESSION_BEHIND").ok()?)
    })
}

fn parse_demo(value: &str) -> Option<Option<Behind3d>> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "on" | "yes" => Some(None),
        "off" => Some(Some(Behind3d::Off)),
        "limited" => Some(Some(Behind3d::Limited)),
        "normal" => Some(Some(Behind3d::Normal)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_variable_keeps_or_replaces_the_policy() {
        assert_eq!(parse_demo("1"), Some(None));
        assert_eq!(parse_demo(" Off "), Some(Some(Behind3d::Off)));
        assert_eq!(parse_demo("limited"), Some(Some(Behind3d::Limited)));
        assert_eq!(parse_demo("normal"), Some(Some(Behind3d::Normal)));
        assert_eq!(parse_demo("0"), None);
        assert_eq!(parse_demo(""), None);
    }

    #[test]
    fn the_window_s_focus_is_remembered_while_a_frame_holds_the_window() {
        let remembered = std::cell::Cell::new(None);
        // no window yet: nothing is known, never focused
        assert_eq!(window_focus(&remembered, None), None);
        // the window says it has the keyboard, then the frame takes it
        assert_eq!(window_focus(&remembered, Some(true)), Some(true));
        assert_eq!(window_focus(&remembered, None), Some(true), "during the frame: as it last said");
        // it lost the keyboard between two frames
        assert_eq!(window_focus(&remembered, Some(false)), Some(false));
        assert_eq!(window_focus(&remembered, None), Some(false));
    }

    #[test]
    fn the_main_profile_keeps_today_s_behaviour_by_default() {
        // (unit tests run as the main profile)
        let s = Settings::default();
        assert_eq!(s.behind_3d, Behind3d::Normal);
        assert_eq!(s.behind_fps, behind::FPS_DEFAULT);
        assert!(!s.share_main_cache);
        // an older settings file without the group reads as the defaults
        let read: Settings = serde_json::from_str("{}").expect("defaults");
        assert_eq!(read, s);
    }
}
