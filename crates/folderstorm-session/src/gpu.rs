//! Background-safe GPU trouble (session-host plan, section 6.2 and rules R4
//! and R5 of section 6.3).
//!
//! The human's past bug: with two viewers open, the inactive one closed
//! almost at once when the other became active, after a box saying the
//! graphics card was lost (Aurora's `app-gpu-lost`). A driver reset (TDR)
//! loses the device of every program on the card, so each viewer behind
//! took the loss of the one in front: counted towards its own ladder
//! (`app/gpu_recovery.rs`: the second loss within minutes saves the safe
//! graphics level, the third closes), or, with the recovery off, its box
//! and its close. A stall behind (Windows serves the window in front first)
//! raised the topmost "the card does not answer" box over the active
//! character, one click away from closing the one behind.
//!
//! Here, a window that is not in front (not focused, minimised or
//! occluded) never shows either box and never acts on a loss: the loss
//! waits, the session goes on (network, chat, sound), and the window
//! recovers when it comes to the front, without the loss counting towards
//! the safe level or the close. A surface that is merely out of date or
//! lost (a resize, a minimise, a full-screen switch) is not a device loss:
//! the renderer makes the swapchain again and nothing here is involved.

/// Where the window is, for the GPU rules.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Place {
    /// Focused and shown: the user is looking at it (until a frame says
    /// otherwise: Aurora's own behaviour).
    #[default]
    Front,
    /// Not focused, or covered (another window in front).
    Behind,
    /// Minimised: a new window made now would open off the screen.
    Minimised,
}

/// Whether the window has the keyboard, for these rules and for the
/// session host: a window exists (`window`: its own focus, None before it
/// is made) and has the focus. `focus_events`: what the window's focus
/// events said so far, which a viewer seeds with true before any came: a
/// character started behind, whose window never got the keyboard, would
/// otherwise count as in front (a driver reset then takes Aurora's counted
/// ladder) and claim the focus in its first report (the host would then
/// bring it forward).
pub fn has_keyboard(focus_events: bool, window: Option<bool>) -> bool {
    focus_events && window == Some(true)
}

/// `forced_behind`: the demo's "another viewer in front"
/// (`AURORA_DEMO_SESSION_BEHIND`).
pub fn place(focused: bool, minimised: bool, occluded: bool, forced_behind: bool) -> Place {
    if minimised {
        Place::Minimised
    } else if !focused || occluded || forced_behind {
        Place::Behind
    } else {
        Place::Front
    }
}

/// What to do with a device loss this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loss {
    /// In front, with no loss waiting: Aurora's own ladder (recover, the
    /// safe level, close), as before.
    Upstream,
    /// Not in front: nothing yet, no box, no count, no save, no close.
    Wait,
    /// Back in front with a loss that waited: recover on a new window and
    /// device, with the settings as they are, without counting it.
    Recover,
}

/// The decision for a loss. `waited`: this loss has already been left to
/// wait. `recovers`: the recovery is on (Preferences › Graphics, or
/// `AURORA_GPU_RECOVERY`) and no headset is in use; when it is off, a loss
/// that waited takes Aurora's own path once in front, where the user sees
/// its box.
pub fn on_loss(place: Place, waited: bool, recovers: bool) -> Loss {
    match place {
        Place::Behind | Place::Minimised => Loss::Wait,
        Place::Front if waited && recovers => Loss::Recover,
        Place::Front => Loss::Upstream,
    }
}

/// Whether the "the card does not answer" box may show now: in front only.
/// Behind, the stall is still logged and noted in the crash session; the
/// box comes if the stall remains once the window is in front.
pub fn stall_notice_allowed(place: Place) -> bool {
    place == Place::Front
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_place_follows_focus_minimise_and_cover() {
        assert_eq!(place(true, false, false, false), Place::Front);
        assert_eq!(place(false, false, false, false), Place::Behind);
        assert_eq!(place(true, false, true, false), Place::Behind);
        assert_eq!(place(true, false, false, true), Place::Behind);
        assert_eq!(place(true, true, false, false), Place::Minimised);
        assert_eq!(place(false, true, true, true), Place::Minimised);
    }

    #[test]
    fn a_window_that_never_got_the_keyboard_is_behind() {
        // the viewer's seed says focused; the window never had the keyboard
        let focused = has_keyboard(true, Some(false));
        assert!(!focused);
        assert_eq!(place(focused, false, false, false), Place::Behind);
        assert_eq!(on_loss(place(focused, false, false, false), false, true), Loss::Wait);
        assert!(!stall_notice_allowed(place(focused, false, false, false)));
        // no window yet: never focused (the first report claims nothing)
        assert!(!has_keyboard(true, None));
        // a focus event that took the keyboard away wins over the window
        assert!(!has_keyboard(false, Some(true)));
        assert!(has_keyboard(true, Some(true)));
        assert_eq!(place(has_keyboard(true, Some(true)), false, false, false), Place::Front);
    }

    #[test]
    fn a_loss_behind_waits_and_never_counts_saves_or_closes() {
        for place in [Place::Behind, Place::Minimised] {
            for waited in [false, true] {
                for recovers in [false, true] {
                    assert_eq!(on_loss(place, waited, recovers), Loss::Wait, "{place:?}");
                }
            }
        }
    }

    #[test]
    fn back_in_front_a_waited_loss_recovers_uncounted() {
        assert_eq!(on_loss(Place::Front, true, true), Loss::Recover);
        // the recovery off: the user's choice, in front, with its box
        assert_eq!(on_loss(Place::Front, true, false), Loss::Upstream);
    }

    #[test]
    fn a_loss_in_front_follows_aurora_s_ladder() {
        assert_eq!(on_loss(Place::Front, false, true), Loss::Upstream);
        assert_eq!(on_loss(Place::Front, false, false), Loss::Upstream);
    }

    #[test]
    fn the_stall_box_shows_in_front_only() {
        assert!(stall_notice_allowed(Place::Front));
        assert!(!stall_notice_allowed(Place::Behind));
        assert!(!stall_notice_allowed(Place::Minimised));
    }
}
