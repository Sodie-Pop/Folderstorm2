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
//! Aurora's files reach this through small hooks marked `// Folderstorm:`.

pub mod profile;

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
