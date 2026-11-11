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
        self.assignment.map(|a| (a.role == Role::Background && !focused, a.behind))
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
