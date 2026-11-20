//! The drawn side of the launch rules of `folderstorm_session::launch`:
//! what a button that may start a viewer shows and does, in the bar and in
//! the host's window.
//!
//! The rules keep their state in egui's memory, one per window of the
//! host or of a viewer (a bar, a card and a line are drawn by egui in the
//! same process): [`armed`] says whether a button asks "Launch?" now, and
//! [`settle`] turns the click on it into "run" or "ask again". The pointer
//! leaving the button, or the time running out, reverts it.

use ::folderstorm_session::bar::{Character, State};
use ::folderstorm_session::launch::{Arming, Choice, Press};
pub use ::folderstorm_session::launch::{Surface, Target, starts_viewer};
use aurora_i18n::tr;
use std::time::Duration;

/// What the bar keeps between two frames: the button waiting for its
/// second click, and the character picked in the list.
#[derive(Clone, Copy, Debug, Default)]
pub struct Memory {
    pub arming: Arming,
    pub choice: Choice,
    /// The demo's states were set up (once, when the characters came).
    seeded: bool,
}

fn id() -> egui::Id {
    egui::Id::unique("folderstorm-launch-memory")
}

/// The time, in seconds on egui's clock.
pub fn now(ctx: &egui::Context) -> f64 {
    ctx.input(|i| i.time)
}

/// Reduced motion ("Reduce animations", `theme::apply_motion`): egui's own
/// transitions last no time at all.
pub fn still(ctx: &egui::Context) -> bool {
    ctx.global_style().animation_time <= 0.0
}

pub fn load(ctx: &egui::Context) -> Memory {
    ctx.data_mut(|d| d.get_temp::<Memory>(id())).unwrap_or_default()
}

pub fn store(ctx: &egui::Context, memory: Memory) {
    ctx.data_mut(|d| d.insert_temp(id(), memory));
}

/// What an armed button says.
pub fn confirm_label() -> &'static str {
    tr!("folderstorm-quick-confirm")
}

/// What an armed button's hover says.
pub fn confirm_help(name: String) -> String {
    tr!("folderstorm-quick-confirm-help", name = name)
}

/// This button asks "Launch?" now.
pub fn armed(ctx: &egui::Context, target: Target) -> bool {
    load(ctx).arming.armed(target, now(ctx))
}

/// After a button was drawn: `response` is its response, `starts`: what it
/// does starts a viewer. True when its action is to be carried out now:
/// always for a button that does not start one, at the second click for
/// one that does. A first click arms the button instead; the pointer
/// leaving an armed one reverts it.
pub fn settle(ctx: &egui::Context, response: &egui::Response, target: Target, starts: bool) -> bool {
    let mut memory = load(ctx);
    let at = now(ctx);
    memory.arming.expire(at);
    let run = if response.clicked() {
        memory.arming.press(target, starts, at) == Press::Run
    } else {
        false
    };
    keep_armed(ctx, &mut memory, response.rect, target, at);
    store(ctx, memory);
    run
}

/// An armed button: reverted when the pointer is on something else, else
/// repainted when its time is up.
pub fn keep_armed(ctx: &egui::Context, memory: &mut Memory, rect: egui::Rect, target: Target, at: f64) {
    if !memory.arming.armed(target, at) {
        return;
    }
    let left = ctx.input(|i| i.pointer.latest_pos()).is_some_and(|pos| !rect.contains(pos));
    if left {
        memory.arming.pointer_left(target);
        ctx.request_repaint();
    } else if let Some(left) = memory.arming.remaining(at) {
        ctx.request_repaint_after(Duration::from_secs_f64(left.max(0.0) + 0.02));
    }
}

/// The bar's button of a character (docked in a viewer, or in the host's
/// compact window).
pub fn bar_button(slot: u8) -> Target {
    Target {
        surface: Surface::Bar,
        slot: usize::from(slot),
    }
}

/// Before the bar is drawn: what ran out is forgotten, a click anywhere
/// calms the login button, the choice of a character that logged in is
/// dropped, and the demo's states are set.
pub fn begin(ctx: &egui::Context, memory: &mut Memory, state: &State) {
    let at = now(ctx);
    memory.arming.expire(at);
    if ctx.input(|i| i.pointer.any_pressed()) {
        memory.choice.calm();
    }
    if memory.choice.slot(state).is_none() {
        memory.choice.clear();
    }
    if !memory.seeded && !state.characters.is_empty() {
        memory.seeded = true;
        if demo_states() {
            seed(memory, state, at);
        }
    }
}

/// After the bar was drawn: the repaints its animations need.
pub fn finish(ctx: &egui::Context, memory: &Memory) {
    let at = now(ctx);
    if memory.choice.pulsing(at) {
        if still(ctx) {
            if let Some(left) = memory.choice.pulse_left(at) {
                ctx.request_repaint_after(Duration::from_secs_f64(left.max(0.0) + 0.02));
            }
        } else {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }
}

/// `AURORA_DEMO_SESSION_HOST_BAR=docked-states` (or `popped-states`):
/// the demo's bar starts in the states of the launch rules, and holds
/// them until something is clicked.
fn demo_states() -> bool {
    static STATES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *STATES.get_or_init(|| std::env::var(super::host::DEMO_BAR_VARIABLE).is_ok_and(|value| super::host::demo_bar_states(&value)))
}

/// The last character that has no viewer is armed ("Launch?"), the first
/// one not logged in is picked (its button names it and pulses).
fn seed(memory: &mut Memory, state: &State, at: f64) {
    let starters: Vec<&Character> = state
        .buttons()
        .into_iter()
        .filter_map(|button| match button {
            ::folderstorm_session::bar::Button::Character(c) if ::folderstorm_session::launch::starts_viewer(c.quick) => Some(c),
            _ => None,
        })
        .collect();
    if let Some(last) = starters.last().filter(|_| starters.len() > 1) {
        memory.arming.arm(bar_button(last.slot), at);
        memory.arming.hold();
    }
    if let Some(first) = state.next_login() {
        memory.choice.pick(first.slot, at);
        memory.choice.hold();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> egui::Context {
        egui::Context::default()
    }

    #[test]
    fn the_memory_is_kept_between_two_frames_of_a_context() {
        let ctx = ctx();
        let mut memory = load(&ctx);
        memory.choice.pick(2, 1.0);
        store(&ctx, memory);
        assert!(load(&ctx).choice.pulsing(1.5));
        assert!(!armed(&ctx, bar_button(2)));
    }

    #[test]
    fn the_demo_arms_the_last_character_without_a_viewer_and_picks_the_first() {
        use ::folderstorm_session::bar::{Presence, QuickAction};
        use ::folderstorm_session::host::Quick;
        let character = |slot: u8, presence, quick| Character {
            slot,
            name: format!("demo-{}", slot + 1),
            presence,
            quick,
            ..Character::default()
        };
        let state = State {
            docked: true,
            characters: vec![
                character(0, Presence::Active, Quick::Show),
                character(1, Presence::Inactive, Quick::Switch),
                character(2, Presence::Out, Quick::Launch),
                character(3, Presence::Out, Quick::Launch),
            ],
            quick: (0..4).map(QuickAction::Character).collect(),
            ..State::default()
        };
        let mut memory = Memory::default();
        seed(&mut memory, &state, 5.0);
        assert!(memory.arming.armed(bar_button(3), 500.0), "held: it waits to be looked at");
        assert!(!memory.arming.armed(bar_button(2), 5.0));
        assert_eq!(memory.choice.slot(&state), Some(2));
        assert!(memory.choice.pulsing(500.0));
        // one character without a viewer: nothing is armed, it is picked
        let one = State {
            characters: state.characters[..3].to_vec(),
            ..state
        };
        let mut memory = Memory::default();
        seed(&mut memory, &one, 5.0);
        assert!(memory.arming.which(5.0).is_none());
        assert_eq!(memory.choice.slot(&one), Some(2));
    }

    #[test]
    fn reduced_motion_is_the_themes_own_signal() {
        let ctx = ctx();
        assert!(!still(&ctx));
        crate::theme::apply_motion(&ctx, true);
        assert!(still(&ctx));
        crate::theme::apply_motion(&ctx, false);
        assert!(!still(&ctx));
    }
}
