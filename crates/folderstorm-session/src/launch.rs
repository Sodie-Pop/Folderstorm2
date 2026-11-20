//! How the host's buttons start a viewer without surprises: two small
//! rules, both pure, drawn by `folderstorm_characters::launch`.
//!
//! - **Confirm before a new viewer** ([`Arming`]). A quick-action button
//!   that would start a viewer that is not running ([`starts_viewer`])
//!   does not do so on the first click: the button is *armed* and asks
//!   "Launch?", and a second click within [`CONFIRM_SECS`] starts the
//!   viewer. It reverts when the time runs out, when the pointer leaves
//!   the button, and when another button is used. A button that only
//!   switches to a running character is never armed: one click. The
//!   global hotkeys do not go through here: they stay immediate.
//! - **Choose, then log in** ([`Choice`]). Picking a character that is not
//!   logged in from the bar's list does not start it: the list selects, the
//!   "open a login" button names the character and pulses for
//!   [`PULSE_SECS`] ([`PULSE_PERIOD_SECS`] a pulse) until it, or anything
//!   else, is clicked, and the button starts it.
//!
//! Times are seconds on any clock that only goes forward (egui's own
//! `input.time`); nothing here reads a clock.

use crate::bar::{Character, Presence, State};
use crate::host::Quick;

/// How long an armed button waits for its second click.
pub const CONFIRM_SECS: f64 = 4.0;
/// A pulse of the login button, from dim to bright and back.
pub const PULSE_PERIOD_SECS: f64 = 1.2;
/// How long the login button pulses after a character was chosen.
pub const PULSE_SECS: f64 = 6.0;

/// The quick action starts a viewer that is not running (the one the
/// hotkeys press too, but a hotkey never asks).
pub fn starts_viewer(quick: Quick) -> bool {
    matches!(quick, Quick::Launch | Quick::Restart)
}

/// The places a quick action is drawn: the bar (docked in a viewer, or in
/// the host's compact window), the host window's row of quick actions, its
/// cards and its condensed lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Bar,
    Strip,
    Card,
    Line,
}

/// One button: where it is drawn and whose it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub surface: Surface,
    pub slot: usize,
}

/// What a press on a button that may start a viewer comes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// Do what the button does.
    Run,
    /// The button is armed: it asks again, nothing is done.
    Armed,
}

/// Which button, if any, waits for its second click, and since when.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Arming {
    armed: Option<(Target, f64)>,
    /// Never times out (the demo's frozen state) until something is used.
    held: bool,
}

impl Arming {
    /// The button is armed at `now`.
    pub fn armed(&self, target: Target, now: f64) -> bool {
        self.armed.is_some_and(|(armed, since)| armed == target && self.live(since, now))
    }

    fn live(&self, since: f64, now: f64) -> bool {
        self.held || (0.0..CONFIRM_SECS).contains(&(now - since))
    }

    /// The button that is armed at `now`, if any.
    pub fn which(&self, now: f64) -> Option<Target> {
        self.armed.filter(|(_, since)| self.live(*since, now)).map(|(target, _)| target)
    }

    /// The seconds the armed button has left, None when nothing is armed.
    pub fn remaining(&self, now: f64) -> Option<f64> {
        let (_, since) = self.armed?;
        self.live(since, now)
            .then_some(if self.held { CONFIRM_SECS } else { CONFIRM_SECS - (now - since) })
    }

    /// Forgets an armed button whose time ran out.
    pub fn expire(&mut self, now: f64) {
        if self.armed.is_some_and(|(_, since)| !self.live(since, now)) {
            self.disarm();
        }
    }

    /// Nothing waits any more.
    pub fn disarm(&mut self) {
        self.armed = None;
        self.held = false;
    }

    /// Arms a button, whichever was armed before. (Tests and the demo.)
    pub fn arm(&mut self, target: Target, now: f64) {
        self.armed = Some((target, now));
        self.held = false;
    }

    /// The armed button stays armed until something is used (the demo).
    pub fn hold(&mut self) {
        self.held = self.armed.is_some();
    }

    /// A click on a button. `starts`: the button would start a viewer
    /// ([`starts_viewer`]); any other press just runs, and disarms the
    /// button that was waiting (another button was used).
    pub fn press(&mut self, target: Target, starts: bool, now: f64) -> Press {
        if !starts {
            self.disarm();
            return Press::Run;
        }
        if self.armed(target, now) {
            self.disarm();
            Press::Run
        } else {
            self.arm(target, now);
            Press::Armed
        }
    }

    /// The pointer is not on the button any more: it reverts.
    pub fn pointer_left(&mut self, target: Target) {
        if self.armed.is_some_and(|(armed, _)| armed == target) {
            self.disarm();
        }
    }
}

/// The character the user picked in the bar's list although it is not
/// logged in, and whether its button still pulses.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Choice {
    picked: Option<(u8, f64)>,
    /// A click was seen: no more pulse.
    calm: bool,
    /// The pulse never stops (the demo's frozen state) until a click.
    held: bool,
}

impl Choice {
    /// A character was picked at `now`.
    pub fn pick(&mut self, slot: u8, now: f64) {
        self.picked = Some((slot, now));
        self.calm = false;
        self.held = false;
    }

    /// The choice is over (it was logged in, or something else was).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The pulse goes on while nothing is held (the demo).
    pub fn hold(&mut self) {
        self.held = self.picked.is_some();
    }

    /// Something was clicked: the pulse stops, the choice stays.
    pub fn calm(&mut self) {
        self.calm = true;
        self.held = false;
    }

    /// The slot picked, if it is still one "open a login" can open: a
    /// choice that went stale (the character logged in, or is closing) is
    /// no choice.
    pub fn slot(&self, state: &State) -> Option<u8> {
        let (slot, _) = self.picked?;
        state.logins().any(|c| c.slot == slot).then_some(slot)
    }

    /// The character picked, as the state has it now.
    pub fn target<'a>(&self, state: &'a State) -> Option<&'a Character> {
        let slot = self.slot(state)?;
        state.character(slot)
    }

    /// Whether the button is drawing attention to itself at `now`.
    pub fn pulsing(&self, now: f64) -> bool {
        let Some((_, since)) = self.picked else {
            return false;
        };
        !self.calm && (self.held || (0.0..PULSE_SECS).contains(&(now - since)))
    }

    /// How bright the glow is at `now`, 0 to 1, along a soft wave of
    /// [`PULSE_PERIOD_SECS`] that starts at its dimmest; None when the
    /// button does not pulse. `still`: reduced motion, a steady glow.
    pub fn glow(&self, now: f64, still: bool) -> Option<f32> {
        if !self.pulsing(now) {
            return None;
        }
        if still {
            return Some(0.7);
        }
        let (_, since) = self.picked?;
        let phase = ((now - since) / PULSE_PERIOD_SECS).fract();
        Some((0.5 - 0.5 * (phase * std::f64::consts::TAU).cos()) as f32)
    }

    /// The seconds until the pulse ends, for a repaint.
    pub fn pulse_left(&self, now: f64) -> Option<f64> {
        let (_, since) = self.picked?;
        self.pulsing(now)
            .then_some(if self.held { PULSE_SECS } else { PULSE_SECS - (now - since) })
    }
}

/// Whether picking this character in the list only selects it: it is not
/// logged in and there is something for "open a login" to do for it. Any
/// other pick is the character's quick action, at once.
pub fn picks_only_select(character: &Character, state: &State) -> bool {
    character.presence == Presence::Out && state.logins().any(|c| c.slot == character.slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Colour;

    const A: Target = Target {
        surface: Surface::Bar,
        slot: 2,
    };
    const B: Target = Target {
        surface: Surface::Bar,
        slot: 3,
    };

    fn character(slot: u8, presence: Presence, quick: Quick) -> Character {
        Character {
            slot,
            name: format!("demo-{}", slot + 1),
            real: None,
            colour: Colour::None,
            presence,
            quick,
            monitor: false,
        }
    }

    fn state() -> State {
        State {
            docked: true,
            characters: vec![
                character(0, Presence::Active, Quick::Show),
                character(1, Presence::Inactive, Quick::Switch),
                character(2, Presence::Out, Quick::Launch),
                character(3, Presence::Out, Quick::Restart),
                character(4, Presence::Out, Quick::Nothing),
            ],
            ..State::default()
        }
    }

    #[test]
    fn only_a_button_that_starts_a_viewer_asks() {
        for quick in [Quick::Launch, Quick::Restart] {
            assert!(starts_viewer(quick), "{quick:?}");
        }
        for quick in [Quick::Switch, Quick::Show, Quick::LogIn, Quick::Wait, Quick::Nothing] {
            assert!(!starts_viewer(quick), "{quick:?}");
        }
    }

    #[test]
    fn the_first_click_arms_and_a_second_one_in_time_runs() {
        let mut arming = Arming::default();
        assert!(!arming.armed(A, 0.0));
        assert_eq!(arming.press(A, true, 10.0), Press::Armed);
        assert!(arming.armed(A, 10.0) && arming.armed(A, 13.9));
        assert!(!arming.armed(B, 10.5), "only the button that was clicked");
        assert_eq!(arming.remaining(11.0), Some(3.0));
        assert_eq!(arming.press(A, true, 13.9), Press::Run);
        assert!(!arming.armed(A, 13.9), "used: nothing waits");
        assert_eq!(arming.remaining(13.9), None);
    }

    #[test]
    fn it_reverts_when_the_time_runs_out() {
        let mut arming = Arming::default();
        arming.press(A, true, 10.0);
        assert!(!arming.armed(A, 14.0), "four seconds: over");
        assert_eq!(arming.remaining(14.5), None);
        // a click after the time is a first click again
        assert_eq!(arming.press(A, true, 20.0), Press::Armed);
        arming.expire(21.0);
        assert!(arming.armed(A, 21.0), "still in time");
        arming.expire(24.5);
        assert_eq!(arming.remaining(10.0), None, "expired: forgotten");
        // a clock that went back never leaves one armed for ever
        arming.arm(A, 100.0);
        assert!(!arming.armed(A, 50.0));
    }

    #[test]
    fn it_reverts_when_the_pointer_leaves_or_another_button_is_used() {
        let mut arming = Arming::default();
        arming.press(A, true, 0.0);
        arming.pointer_left(B);
        assert!(arming.armed(A, 1.0), "another button's pointer is not this one's");
        arming.pointer_left(A);
        assert!(!arming.armed(A, 1.0));
        // another button that starts a viewer arms itself, not the first
        arming.press(A, true, 0.0);
        assert_eq!(arming.press(B, true, 1.0), Press::Armed);
        assert!(!arming.armed(A, 1.0) && arming.armed(B, 1.0));
        // a button that does not start one just runs, and disarms
        assert_eq!(arming.press(A, false, 2.0), Press::Run);
        assert!(!arming.armed(B, 2.0));
        // the same slot on another surface is another button
        let strip = Target {
            surface: Surface::Strip,
            ..A
        };
        arming.press(A, true, 0.0);
        assert!(!arming.armed(strip, 0.5));
    }

    #[test]
    fn a_held_arming_waits_until_something_is_used() {
        let mut arming = Arming::default();
        arming.arm(A, 0.0);
        arming.hold();
        assert!(arming.armed(A, 500.0));
        arming.expire(500.0);
        assert!(arming.armed(A, 500.0));
        assert_eq!(arming.press(A, true, 501.0), Press::Run);
        assert!(!arming.armed(A, 501.0));
    }

    #[test]
    fn a_character_that_is_not_logged_in_is_chosen_and_one_that_is_runs_at_once() {
        let state = state();
        let only = |slot: u8| picks_only_select(state.character(slot).expect("a character"), &state);
        assert!(!only(0) && !only(1), "in the world: its quick action");
        assert!(only(2) && only(3), "not logged in: chosen");
        assert!(!only(4), "closing: nothing to open, so nothing to choose");
    }

    #[test]
    fn a_choice_pulses_for_six_seconds_and_stops_at_a_click() {
        let state = state();
        let mut choice = Choice::default();
        assert_eq!(choice.target(&state), None);
        assert!(!choice.pulsing(0.0) && choice.glow(0.0, false).is_none());
        choice.pick(2, 100.0);
        assert_eq!(choice.target(&state).map(|c| c.slot), Some(2));
        assert!(choice.pulsing(100.0) && choice.pulsing(105.9));
        assert!(!choice.pulsing(106.0), "six seconds: over, the choice stays");
        assert_eq!(choice.slot(&state), Some(2));
        // a click stops the pulse at once
        choice.pick(3, 200.0);
        assert!(choice.pulsing(201.0));
        choice.calm();
        assert!(!choice.pulsing(201.0) && choice.glow(201.0, false).is_none());
        assert_eq!(choice.slot(&state), Some(3));
        // a new pick pulses again
        choice.pick(2, 300.0);
        assert!(choice.pulsing(300.5));
        choice.clear();
        assert_eq!(choice.slot(&state), None);
    }

    #[test]
    fn the_glow_is_a_soft_wave_of_a_second_and_a_fifth_and_still_when_motion_is_reduced() {
        let mut choice = Choice::default();
        choice.pick(2, 10.0);
        let at = |t: f64| choice.glow(10.0 + t, false).expect("pulsing");
        assert!(at(0.0) < 0.01, "it starts dim");
        assert!((at(0.6) - 1.0).abs() < 0.01, "bright at half a period");
        assert!(at(1.2) < 0.01, "and dim again after 1.2 s");
        assert!((at(0.3) - 0.5).abs() < 0.01);
        assert!((0..60).all(|i| (0.0..=1.0).contains(&at(f64::from(i) * 0.1))));
        // reduced motion: one steady value, as long as the pulse would last
        let steady: Vec<f32> = [0.0, 0.3, 0.6, 2.0, 5.9]
            .into_iter()
            .filter_map(|t| choice.glow(10.0 + t, true))
            .collect();
        assert_eq!(steady.len(), 5);
        assert!(steady.iter().all(|g| *g == steady[0] && *g > 0.0));
        assert_eq!(choice.glow(16.5, true), None);
        assert!((choice.pulse_left(11.0).expect("pulsing") - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_choice_that_went_stale_is_no_choice() {
        let mut state = state();
        let mut choice = Choice::default();
        choice.pick(2, 0.0);
        // it logged in
        state.characters[2].presence = Presence::Inactive;
        state.characters[2].quick = Quick::Switch;
        assert_eq!(choice.slot(&state), None);
        // a character the desk does not have
        choice.pick(4, 0.0);
        assert_eq!(choice.slot(&state), None, "closing: nothing to open");
        choice.pick(0, 0.0);
        assert_eq!(choice.slot(&state), None, "in the world");
    }

    #[test]
    fn a_held_choice_pulses_until_a_click() {
        let mut choice = Choice::default();
        choice.hold();
        assert!(!choice.pulsing(0.0), "nothing picked: nothing to hold");
        choice.pick(2, 0.0);
        choice.hold();
        assert!(choice.pulsing(1000.0));
        choice.calm();
        assert!(!choice.pulsing(1000.0));
    }
}
