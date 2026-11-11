//! Checkpoint 4 of the session host, "comfort" (session-host plan,
//! section 13): what the host itself does for the global hotkeys and for
//! "New character profile…". The rules are the `folderstorm-session`
//! crate's (`hotkeys`, `copy`, the desk's quick actions); the keys are
//! taken in [`super::hotkeys`], the files written in
//! `folderstorm_characters/profile_copy.rs`, the window's side is
//! `ui/comfort.rs`.

use super::Host;
use super::hotkeys::Registrar;
use crate::folderstorm_characters::{profile, profile_copy};
use ::folderstorm_session::copy::Source;
use ::folderstorm_session::host::AddError;
use ::folderstorm_session::hotkeys::{Action, State};
use ::folderstorm_session::profile::{NameError, ProfileName};
use std::time::Instant;

/// What the host keeps for checkpoint 4.
#[derive(Default)]
pub(super) struct Comfort {
    /// The global hotkeys held, and what the system answered for each.
    pub hotkeys: Registrar,
}

/// A profile made by "New character profile…".
pub(super) struct NewProfile {
    pub name: ProfileName,
    pub created: profile_copy::Created,
    /// It is in the list now (false: the list is full).
    pub listed: bool,
}

/// Why "New character profile…" made nothing.
pub(super) enum NewProfileError {
    Name(NameError),
    Copy(profile_copy::CopyError),
}

impl Host {
    /// Registers the hotkeys of the settings in place of those held: at
    /// the start, and whenever the user changes them.
    pub(super) fn apply_hotkeys(&mut self) {
        self.comfort.hotkeys.apply(self.desk.hotkeys());
    }

    /// A hotkey as the window shows it.
    pub(super) fn hotkey_state(&self, action: Action) -> State {
        let config = self.desk.hotkeys();
        let plan = config.plan();
        State::of(&plan[action.index()], config.enabled, self.comfort.hotkeys.outcome(action))
    }

    /// How many hotkeys do not work for a reason the user can do
    /// something about.
    pub(super) fn hotkey_problems(&self) -> usize {
        Action::all().filter(|action| self.hotkey_state(*action).problem()).count()
    }

    /// What a hotkey does (also the demo's "Try" button): the desk
    /// decides, [`Host::tick`] then carries it out.
    pub(super) fn hotkey(&mut self, action: Action, now: Instant) {
        match self.desk.hotkey(action, now) {
            Some(slot) => log::info!("session host: hotkey {action:?}, character {}", slot + 1),
            None => log::info!("session host: hotkey {action:?}, nothing to do"),
        }
    }

    /// The hotkeys pressed since the last call are carried out; true when
    /// there was one.
    pub(super) fn pressed_hotkeys(&mut self, now: Instant) -> bool {
        let pressed = self.comfort.hotkeys.pressed();
        for action in &pressed {
            self.hotkey(*action, now);
        }
        !pressed.is_empty()
    }

    /// The named profiles there are, to make a new one from.
    pub(super) fn profiles(&self) -> Vec<ProfileName> {
        profile_copy::existing(&profile::main_config_dir())
    }

    /// "New character profile…": makes the profile `name` from `source`
    /// and adds it to the list. Refused while that profile runs, here or
    /// in a viewer started by hand, and when it exists.
    pub(super) fn new_profile(&mut self, name: &str, source: &Source) -> Result<NewProfile, NewProfileError> {
        let name = ProfileName::parse(name).map_err(NewProfileError::Name)?;
        // (a viewer the host is starting may not hold its lease yet)
        let on_the_desk = |profile: &ProfileName| self.desk.slots().any(|(_, s)| s.has_process() && s.profile.key() == profile.key());
        let running = |profile: &ProfileName| on_the_desk(profile) || profile::lease_held(profile);
        let created = profile_copy::create(&profile::main_config_dir(), source, &name, &running).map_err(|error| {
            if let profile_copy::CopyError::Io(e) = &error {
                log::warn!("session host: the profile {} was not made ({e})", name.key());
            }
            NewProfileError::Copy(error)
        })?;
        log::info!(
            "session host: profile {} made from {} ({:?}, {} other files)",
            name.key(),
            match source {
                Source::Main => "the main profile".to_owned(),
                Source::Profile(from) => format!("the profile {}", from.key()),
                Source::Defaults => "the defaults".to_owned(),
            },
            created.preferences,
            created.files
        );
        let listed = match self.desk.add(name.shown()) {
            Ok(_) | Err(AddError::Duplicate) => true,
            Err(AddError::Full | AddError::Name(_)) => false,
        };
        Ok(NewProfile { name, created, listed })
    }
}

#[cfg(test)]
mod tests {
    use super::super::chat_ui::tests::host;
    use super::*;
    use ::folderstorm_session::host::Effect;
    use ::folderstorm_session::hotkeys::Config;

    #[test]
    fn every_hotkey_has_a_state_and_bad_choices_are_counted() {
        let mut host = host();
        host.apply_hotkeys();
        assert_eq!(host.hotkey_problems(), 0, "the defaults are sound");
        assert!(host.desk.set_hotkey(Action::Next, "banana"));
        assert!(host.desk.set_hotkey(Action::Previous, "Ctrl+Alt+1"));
        host.apply_hotkeys();
        assert!(matches!(host.hotkey_state(Action::Next), State::Invalid(_)));
        assert_eq!(host.hotkey_state(Action::Previous), State::Duplicate(Action::Character(0)));
        assert_eq!(host.hotkey_problems(), 2);
        let off = Config {
            enabled: false,
            ..Config::default()
        };
        assert!(host.desk.set_hotkeys(off));
        host.apply_hotkeys();
        assert_eq!(host.hotkey_state(Action::Character(0)), State::Disabled);
        assert_eq!(host.hotkey_problems(), 0);
    }

    #[test]
    fn a_hotkey_goes_through_the_desk_and_never_starts_two_viewers() {
        let mut host = host();
        let now = Instant::now();
        // nothing pressed: nothing done
        assert!(!host.pressed_hotkeys(now));
        // (the desk's effects are read here, not carried out: no process)
        host.hotkey(Action::Character(0), now);
        host.hotkey(Action::Character(0), now);
        host.hotkey(Action::Next, now);
        let spawns = host
            .desk
            .take_effects()
            .into_iter()
            .filter(|e| matches!(e, Effect::Spawn { .. }))
            .count();
        assert_eq!(spawns, 1);
    }
}
