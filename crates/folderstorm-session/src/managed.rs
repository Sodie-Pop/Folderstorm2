//! A viewer run by the session host: what it makes of the host's
//! assignments (session-host plan, sections 6.1, 6.3 and 10).
//!
//! - **Role over focus (R3).** The active character is in front for every
//!   rule that used the keyboard focus (frame cap, texture release, busy
//!   pause, the instances registry), even while the user types in the
//!   host's window. A window that has the keyboard is in front too,
//!   whatever its role: the user clicked it, and the host follows.
//! - **Behind** is the host's word, not the instances registry's: a
//!   background character that has not the keyboard.
//! - **Sound and voice** follow the assignment alone. Before the first one,
//!   voice is not allowed: another character may hold it.
//! - **Host gone**: [`Managed::release`], and everything is as in a viewer
//!   started by hand: focus, the profile's own choices, its saved sound and
//!   voice settings. Nothing was saved while managed, so nothing is undone.

use crate::behind::Behind3d;
use crate::ipc::{Assignment, Role, Standby};
use crate::monitor::Order;

/// The host's hold on this viewer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Managed {
    /// A host launched this viewer and is still there.
    linked: bool,
    assignment: Option<Assignment>,
}

impl Managed {
    /// The host's bootstrap was accepted.
    pub fn link(&mut self) {
        self.linked = true;
        self.assignment = None;
    }

    /// The host is gone (its pipe ended): unmanaged from now on.
    pub fn release(&mut self) {
        *self = Self::default();
    }

    pub fn linked(&self) -> bool {
        self.linked
    }

    /// Takes an assignment; false when it is older than, or the same as,
    /// the one held (dropped), or when no host is linked.
    pub fn assign(&mut self, assignment: Assignment) -> bool {
        if !self.linked || self.assignment.is_some_and(|held| assignment.seq <= held.seq) {
            return false;
        }
        self.assignment = Some(assignment);
        true
    }

    /// The assignment held, for the status ([`Assignment::seq`], 0: none).
    pub fn seq(&self) -> u64 {
        self.assignment.map_or(0, |a| a.seq)
    }

    pub fn role(&self) -> Option<Role> {
        self.assignment.map(|a| a.role)
    }

    pub fn standby(&self) -> Option<Standby> {
        self.assignment.map(|a| a.standby)
    }

    /// In front, for the rules that used the keyboard focus.
    pub fn in_front(&self, focused: bool) -> bool {
        focused || self.role() == Some(Role::Active)
    }

    /// Behind for the 3D policy, with that policy; None when the host has
    /// said nothing (the viewer then asks the instances registry, with its
    /// profile's own policy).
    pub fn behind(&self, focused: bool) -> Option<(bool, Behind3d)> {
        self.assignment.map(|a| (a.role.background() && !focused, a.behind))
    }

    /// The monitor's pictures the host asks for (checkpoint 5): only of a
    /// background character that has not the keyboard. One the user went
    /// to draws its 3D in its own window: nothing is taken for the host.
    pub fn monitor(&self, focused: bool) -> Option<Order> {
        self.assignment.filter(|_| !focused).and_then(|a| a.role.monitor())
    }

    /// Every sound is cut for now.
    pub fn sound_muted(&self) -> bool {
        self.assignment.is_some_and(|a| a.sound_muted)
    }

    /// Voice may connect and transmit: always without a host; with one,
    /// only when its assignment says so.
    pub fn voice_allowed(&self) -> bool {
        !self.linked || self.assignment.is_some_and(|a| a.voice_allowed)
    }

    /// What the host is told of the voice: given up only once it is not
    /// allowed any more **and** the voice module says it is off
    /// (`voice_on`): until then this viewer may still be transmitting, and
    /// the host must not give the voice to another character.
    pub fn voice_report(&self, voice_on: bool) -> bool {
        self.voice_allowed() || voice_on
    }

    /// Whether taking `next` takes the voice away: it was allowed, and
    /// will not be. The voice is then turned off at once, not at the next
    /// frame (frames are rare in a minimised window).
    pub fn revokes_voice(&self, next: &Assignment) -> bool {
        self.linked && self.voice_allowed() && !next.voice_allowed && self.assignment.is_none_or(|held| next.seq > held.seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(seq: u64) -> Assignment {
        Assignment {
            seq,
            role: Role::Active,
            behind: Behind3d::Off,
            standby: Standby::Warm,
            sound_muted: false,
            voice_allowed: true,
        }
    }

    fn background(seq: u64) -> Assignment {
        Assignment {
            seq,
            role: Role::Background,
            behind: Behind3d::Off,
            standby: Standby::Economy,
            sound_muted: true,
            voice_allowed: false,
        }
    }

    #[test]
    fn a_viewer_started_by_hand_follows_its_focus_and_its_own_settings() {
        let m = Managed::default();
        assert!(!m.linked());
        assert!(m.in_front(true));
        assert!(!m.in_front(false));
        assert_eq!(m.behind(false), None, "the instances registry decides");
        assert!(!m.sound_muted());
        assert!(m.voice_allowed());
        assert_eq!(m.seq(), 0);
    }

    #[test]
    fn nothing_is_taken_from_a_host_that_is_not_linked() {
        let mut m = Managed::default();
        assert!(!m.assign(background(1)));
        assert!(!m.sound_muted());
        assert!(m.voice_allowed());
    }

    #[test]
    fn linked_but_not_assigned_yet_voice_waits() {
        let mut m = Managed::default();
        m.link();
        assert!(!m.voice_allowed(), "another character may hold the voice");
        assert!(!m.sound_muted());
        assert_eq!(m.behind(false), None);
        assert!(!m.in_front(false));
    }

    #[test]
    fn the_active_character_is_in_front_without_the_keyboard() {
        let mut m = Managed::default();
        m.link();
        assert!(m.assign(active(1)));
        // the user types in the host's window: no focus here
        assert!(m.in_front(false));
        assert_eq!(m.behind(false), Some((false, Behind3d::Off)));
        assert!(!m.sound_muted());
        assert!(m.voice_allowed());
    }

    #[test]
    fn a_background_character_is_behind_muted_and_without_voice() {
        let mut m = Managed::default();
        m.link();
        assert!(m.assign(background(1)));
        assert!(!m.in_front(false));
        assert_eq!(m.behind(false), Some((true, Behind3d::Off)));
        assert!(m.sound_muted());
        assert!(!m.voice_allowed());
        assert_eq!(m.standby(), Some(Standby::Economy));
        // clicked by the user before the host has followed: drawn at once,
        // but the sound and the voice wait for the host's word
        assert!(m.in_front(true));
        assert_eq!(m.behind(true), Some((false, Behind3d::Off)));
        assert!(m.sound_muted());
        assert!(!m.voice_allowed());
    }

    #[test]
    fn the_voice_is_given_up_only_once_it_is_off() {
        let mut m = Managed::default();
        m.link();
        assert!(!m.revokes_voice(&background(1)), "it never had it");
        assert!(m.assign(active(1)));
        assert!(m.voice_report(false));
        // the background assignment takes it away: turned off at once
        assert!(m.revokes_voice(&background(2)));
        assert!(!m.revokes_voice(&active(2)));
        assert!(!m.revokes_voice(&background(1)), "an old one is dropped anyway");
        assert!(m.assign(background(2)));
        // not allowed any more, but still connected: not given up yet
        assert!(m.voice_report(true));
        assert!(!m.voice_report(false));
    }

    fn watched(seq: u64, interval_ms: u32) -> Assignment {
        Assignment {
            role: Role::Monitor(Order {
                size: crate::monitor::Size::Large,
                interval_ms,
            }),
            standby: Standby::Warm,
            ..background(seq)
        }
    }

    #[test]
    fn a_watched_character_is_a_background_one_that_takes_pictures() {
        let mut m = Managed::default();
        m.link();
        assert_eq!(m.monitor(false), None, "nothing is asked before the host says so");
        // Off → Monitor: still behind, 3D off, muted, without voice
        assert!(m.assign(background(1)));
        assert_eq!(m.monitor(false), None);
        assert!(m.assign(watched(2, 1000)));
        assert_eq!(m.monitor(false).map(|o| o.interval_ms), Some(1000));
        assert!(!m.in_front(false));
        assert_eq!(m.behind(false), Some((true, Behind3d::Off)), "the 3D stays off between pictures");
        assert!(m.sound_muted());
        assert!(!m.voice_allowed());
        // the user clicks its window: it draws there, nothing for the host
        assert_eq!(m.monitor(true), None);
        assert_eq!(m.behind(true), Some((false, Behind3d::Off)));
        // Monitor → Front: the host made it the active character
        assert!(m.assign(active(3)));
        assert_eq!(m.monitor(false), None);
        assert!(m.in_front(false));
        // Front → Monitor → Off (the monitor closed, or the host is hidden)
        assert!(m.assign(watched(4, 500)));
        assert_eq!(m.monitor(false).map(|o| o.interval_ms), Some(500));
        assert!(m.assign(background(5)));
        assert_eq!(m.monitor(false), None);
        assert_eq!(m.behind(false), Some((true, Behind3d::Off)));
        // an old order that comes late does not bring the monitor back
        assert!(!m.assign(watched(4, 500)));
        assert_eq!(m.monitor(false), None);
        // an order is never faster than a monitor's fastest rate
        assert!(m.assign(watched(6, 0)));
        assert_eq!(m.monitor(false).map(|o| o.interval_ms), Some(crate::monitor::MIN_INTERVAL_MS));
        // the host gone: no monitor
        m.release();
        assert_eq!(m.monitor(false), None);
    }

    #[test]
    fn older_and_repeated_assignments_are_dropped() {
        let mut m = Managed::default();
        m.link();
        assert!(m.assign(active(5)));
        assert!(!m.assign(background(5)), "the same number again");
        assert!(!m.assign(background(4)), "an older one, late");
        assert_eq!((m.seq(), m.role()), (5, Some(Role::Active)));
        assert!(m.assign(background(6)));
        assert_eq!((m.seq(), m.role()), (6, Some(Role::Background)));
    }

    #[test]
    fn host_gone_the_viewer_is_its_own_again() {
        let mut m = Managed::default();
        m.link();
        assert!(m.assign(background(3)));
        m.release();
        assert_eq!(m, Managed::default());
        assert!(!m.sound_muted());
        assert!(m.voice_allowed());
        assert_eq!(m.behind(false), None);
        // nothing of a host that is gone is taken afterwards
        assert!(!m.assign(active(9)));
    }
}
