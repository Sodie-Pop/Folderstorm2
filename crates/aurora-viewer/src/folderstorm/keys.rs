//! Folderstorm: the fork's own keyboard actions, carried by one variant of
//! Aurora's `keybinds::Action` (`Action::Folderstorm`), so that a new one
//! is added here and not in `keybinds.rs`.
//!
//! They are listed in Preferences › Shortcuts like Aurora's, under their
//! own section, and are saved with the others. **None has a default key**
//! (folderstorm/plans/workspaces.md section 9.4, as in Folderstorm (C++):
//! "Ship without taking over existing shortcuts",
//! doc/plans/workspace-enhancements.md, 3): the user chooses them, so no
//! shortcut of the viewer is taken, and Aurora's own warning tells when a
//! chosen key is already another action's. A test holds the default table
//! to that.

use crate::keybinds::{Action, Binding};
use aurora_i18n::tr;
use serde::{Deserialize, Serialize};

/// One of the fork's keyboard actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Key {
    /// The layouts' switcher, shown or put away.
    LayoutSwitcher,
    /// The one of the toolbar toggle's two layouts that is not in use.
    LayoutToggle,
    /// *Previous arrangement*.
    PreviousArrangement,
    /// The favorite layout after the one in use.
    NextFavoriteLayout,
    /// The favorite layout before the one in use.
    PreviousFavoriteLayout,
}

/// Every key, in the order of the Shortcuts page.
pub const KEYS: [Key; 5] = [
    Key::LayoutSwitcher,
    Key::LayoutToggle,
    Key::PreviousArrangement,
    Key::NextFavoriteLayout,
    Key::PreviousFavoriteLayout,
];

impl Key {
    /// The i18n key of its label.
    pub fn label_key(self) -> &'static str {
        match self {
            Key::LayoutSwitcher => "fs-keys-layout-switcher",
            Key::LayoutToggle => "fs-keys-layout-toggle",
            Key::PreviousArrangement => "fs-layouts-previous",
            Key::NextFavoriteLayout => "fs-keys-next-favorite",
            Key::PreviousFavoriteLayout => "fs-keys-previous-favorite",
        }
    }
}

/// Our section of the Shortcuts page (the line of `keybinds::sections`).
pub fn section() -> (&'static str, Vec<(Action, &'static str)>) {
    (
        tr!("fs-keys-section"),
        KEYS.iter()
            .map(|key| (Action::Folderstorm(*key), aurora_i18n::text(key.label_key())))
            .collect(),
    )
}

/// The default table with our actions added, each without a key (the line
/// of `KeyBindings::us_places`).
pub fn with_unbound(mut defaults: Vec<(Action, [Option<Binding>; 2])>) -> Vec<(Action, [Option<Binding>; 2])> {
    defaults.extend(KEYS.iter().map(|key| (Action::Folderstorm(*key), [None, None])));
    defaults
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybinds::{Input, KeyBindings, Mods, all_actions, is_held, label};
    use std::collections::HashSet;

    fn ours(action: Action) -> bool {
        matches!(action, Action::Folderstorm(_))
    }

    fn mods(binding: &Binding) -> Mods {
        Mods {
            ctrl: binding.ctrl,
            shift: binding.shift,
            alt: binding.alt,
        }
    }

    /// Our actions come without a key: nothing of the viewer's default
    /// table is taken, in any keyboard layout's defaults.
    #[test]
    fn our_actions_have_no_default_key() {
        let defaults = KeyBindings::default();
        for key in KEYS {
            let action = Action::Folderstorm(key);
            assert_eq!(defaults.get(action), [None, None], "{key:?}");
            assert!(defaults.map.iter().any(|(listed, _)| *listed == action), "{key:?} is in the table");
            assert!(!is_held(action), "{key:?} triggers on a press");
        }
    }

    /// With our actions in the table, every default binding of the viewer
    /// still does what it did: the same action on its press, the same
    /// actions while held. (The table without ours is the same table with
    /// our lines taken out.)
    #[test]
    fn the_viewers_default_bindings_do_what_they_did() {
        let with = KeyBindings::default();
        let mut without = with.clone();
        without.map.retain(|(action, _)| !ours(*action));
        assert_eq!(with.map.len(), without.map.len() + KEYS.len());
        let mut checked = 0;
        for (action, bindings) in &without.map {
            for binding in bindings.iter().flatten() {
                let m = mods(binding);
                assert_eq!(
                    with.triggered(&binding.input, m),
                    without.triggered(&binding.input, m),
                    "{action:?}"
                );
                assert!(!with.triggered(&binding.input, m).is_some_and(ours), "{action:?}");
                let down = HashSet::from([binding.input.clone()]);
                for other in all_actions().filter(|other| !ours(*other)) {
                    assert_eq!(with.held(other, &down, m), without.held(other, &down, m), "{action:?} / {other:?}");
                }
                assert_eq!(with.users(binding), without.users(binding), "{action:?}");
                checked += 1;
            }
        }
        assert!(checked > 100, "the viewer's table was walked ({checked} bindings)");
    }

    /// A settings file from before our actions gains them without a key,
    /// and keeps every key it had.
    #[test]
    fn older_settings_gain_our_actions_without_a_key() {
        let mut old = KeyBindings::default();
        old.map.retain(|(action, _)| !ours(*action));
        let before = old.clone();
        let new = old.sanitized();
        for (action, bindings) in &before.map {
            assert_eq!(new.get(*action), *bindings, "{action:?}");
        }
        for key in KEYS {
            assert_eq!(new.get(Action::Folderstorm(key)), [None, None]);
        }
    }

    /// A key the user chose is saved and read back, and triggers its
    /// action; cleared, it triggers nothing.
    #[test]
    fn a_chosen_key_survives_a_save_and_triggers_its_action() {
        let mut keys = KeyBindings::default();
        let chord = Binding {
            input: Input::key(winit::keyboard::KeyCode::F7),
            ctrl: true,
            shift: false,
            alt: true,
        };
        assert!(keys.users(&chord).is_empty(), "a free chord for the test");
        let action = Action::Folderstorm(Key::LayoutToggle);
        keys.set(action, 0, Some(chord.clone()));
        let saved = serde_json::to_string(&keys).expect("saved");
        assert!(saved.contains(r#"{"Folderstorm":"LayoutToggle"}"#), "{saved}");
        let read: KeyBindings = serde_json::from_str(&saved).expect("read");
        let read = read.sanitized();
        assert_eq!(read.get(action)[0], Some(chord.clone()));
        assert_eq!(read.triggered(&chord.input, mods(&chord)), Some(action));
        assert_eq!(read.triggered(&chord.input, Mods::default()), None, "not without its modifiers");
        assert_eq!(read.users(&chord), [action]);
        let mut cleared = read;
        cleared.set(action, 0, None);
        assert_eq!(cleared.triggered(&chord.input, mods(&chord)), None);
    }

    /// A build without our actions (upstream's, an older one of ours)
    /// reads the same file and loses only our lines.
    #[test]
    fn a_build_without_our_actions_drops_only_them() {
        #[derive(Debug, Deserialize, PartialEq)]
        enum Theirs {
            Fly,
            Mouselook,
        }
        let saved =
            serde_json::json!([["Fly", [null, null]], [{ "Folderstorm": "LayoutToggle" }, [null, null]], ["Mouselook", [null, null]]]);
        let read = crate::tolerant::from_value::<Vec<(Theirs, [Option<Binding>; 2])>>(saved, "test").expect("read");
        let actions: Vec<_> = read.value.iter().map(|(action, _)| action).collect();
        assert_eq!(actions, [&Theirs::Fly, &Theirs::Mouselook]);
        assert_eq!(read.dropped.len(), 1);
    }

    /// Each action is listed once in the Shortcuts page, with a label in
    /// every language.
    #[test]
    fn each_action_is_listed_with_a_label_in_every_language() {
        let listed: Vec<Action> = all_actions().filter(|action| ours(*action)).collect();
        assert_eq!(listed, KEYS.map(Action::Folderstorm));
        for key in KEYS {
            assert_ne!(label(Action::Folderstorm(key)), "?", "{key:?}");
            for (language, _) in aurora_i18n::available_languages() {
                assert_ne!(
                    aurora_i18n::text_in(language, key.label_key()),
                    key.label_key(),
                    "{key:?} in {language}"
                );
                assert_ne!(aurora_i18n::text_in(language, "fs-keys-section"), "fs-keys-section", "{language}");
            }
        }
    }
}
