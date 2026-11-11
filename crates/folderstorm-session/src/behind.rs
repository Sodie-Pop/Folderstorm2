//! "3D while another Folderstorm2 window is in front" (session-host plan,
//! section 6.1, checkpoint 1): Off, Limited or Normal.
//!
//! Without a session host, a window is *behind* when it has not the focus
//! and another Folderstorm2 window on the same card has it, as the
//! instances registry says (`instances.rs`, `Peers::in_front`), the same
//! test as the registry's own `BEHIND_FPS`. Switching to a browser is not
//! "behind": the character in front of its user keeps drawing.
//!
//! - **Off**: the 3D pauses (the busy card's paused state: empty draw lists,
//!   3D targets shrunk), the loop and the interface go on at a steady low
//!   rate, never zero: chat, IMs, teleports and region crossings keep
//!   working. The default of a named profile (decision 1 of the plan).
//! - **Limited**: the 3D is drawn, at that same low rate. Monitors (stage 2)
//!   will draw it smaller and only while one shows it.
//! - **Normal**: today's behaviour (the registry's 10 fps and the
//!   background cap). The main profile's default: it keeps what it did.

use serde::{Deserialize, Serialize};

/// The policy of a window behind another Folderstorm2 window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Behind3d {
    Off,
    Limited,
    #[default]
    Normal,
}

impl Behind3d {
    /// Off for a named profile, Normal for the main one.
    pub fn default_for(named_profile: bool) -> Self {
        if named_profile { Self::Off } else { Self::Normal }
    }
}

/// Frames per second of a window behind with Off or Limited: the setting's
/// range and default. Never zero (rule R1: the loop and the presentation
/// never stop).
pub const FPS_MIN: u32 = 2;
pub const FPS_MAX: u32 = 15;
pub const FPS_DEFAULT: u32 = 5;

/// Whether the window counts as behind: not focused while another
/// Folderstorm2 window is in front.
pub fn is_behind(focused: bool, other_in_front: bool) -> bool {
    !focused && other_in_front
}

/// Whether the 3D drawing pauses. `exempt`: something needs the 3D drawn
/// (a photo, a headset, a timed run).
pub fn paused(policy: Behind3d, behind: bool, exempt: bool) -> bool {
    policy == Behind3d::Off && behind && !exempt
}

/// The frame cap (0 = none) once the policy is applied: Off and Limited
/// hold a window behind to `fps` (kept in [`FPS_MIN`]..=[`FPS_MAX`]), or
/// lower if the cap already is; Normal and a window in front keep `cap`.
pub fn cap(cap: f32, policy: Behind3d, behind: bool, fps: u32, exempt: bool) -> f32 {
    if !behind || exempt || policy == Behind3d::Normal {
        return cap;
    }
    let fps = fps.clamp(FPS_MIN, FPS_MAX) as f32;
    if cap > 0.0 { cap.min(fps) } else { fps }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_profiles_start_off_and_the_main_one_keeps_today_s_behaviour() {
        assert_eq!(Behind3d::default_for(true), Behind3d::Off);
        assert_eq!(Behind3d::default_for(false), Behind3d::Normal);
        assert_eq!(Behind3d::default(), Behind3d::Normal);
    }

    #[test]
    fn behind_means_another_folderstorm2_window_in_front() {
        assert!(is_behind(false, true));
        assert!(!is_behind(true, true), "focused: it is the one in front");
        assert!(!is_behind(false, false), "a browser in front is not another viewer");
    }

    #[test]
    fn off_pauses_the_3d_behind_only() {
        assert!(paused(Behind3d::Off, true, false));
        assert!(!paused(Behind3d::Off, false, false));
        assert!(!paused(Behind3d::Off, true, true), "a photo or a headset is drawn");
        assert!(!paused(Behind3d::Limited, true, false));
        assert!(!paused(Behind3d::Normal, true, false));
    }

    #[test]
    fn the_rate_behind_is_low_and_never_zero() {
        // no cap at all (0): the policy's rate
        assert_eq!(cap(0.0, Behind3d::Off, true, 5, false), 5.0);
        assert_eq!(cap(60.0, Behind3d::Limited, true, 5, false), 5.0);
        // a lower cap already: kept
        assert_eq!(cap(3.0, Behind3d::Off, true, 5, false), 3.0);
        // out of range values come back in it: never 0, never fast
        assert_eq!(cap(0.0, Behind3d::Off, true, 0, false), FPS_MIN as f32);
        assert_eq!(cap(0.0, Behind3d::Off, true, 500, false), FPS_MAX as f32);
        // in front, Normal, or exempt: unchanged
        assert_eq!(cap(60.0, Behind3d::Off, false, 5, false), 60.0);
        assert_eq!(cap(60.0, Behind3d::Normal, true, 5, false), 60.0);
        assert_eq!(cap(0.0, Behind3d::Off, true, 5, true), 0.0);
    }
}
