//! The desk's side of checkpoint 10: the settings the settings' window
//! adds are saved, read back and read from a file of checkpoint 9; a
//! character's settings live through its launches; two cards swap their
//! places only when nothing running changes number.

use super::*;
use crate::identity::Colour;
use crate::ipc::{AttentionKind, LineKind, SessionKind};
use crate::settings::{Alerts, Attention, ChatShown, Reading, Section};

fn desk() -> Desk {
    Desk::new(Config::demo(3))
}

fn saves(effects: &[Effect]) -> usize {
    effects.iter().filter(|e| matches!(e, Effect::Save)).count()
}

/// A settings file as checkpoint 9 wrote it: none of this checkpoint's
/// fields.
const CHECKPOINT_9: &str = r#"{
  "slots": [
    {"slot": 0, "profile": "Alpha", "standby": "Warm", "sound": "All", "colour": "teal", "alias": "Shop"},
    {"slot": 2, "profile": "Beta", "standby": "Economy", "sound": "Muted",
     "monitor": {"size": "Medium", "rate": "One"},
     "monitor_window": {"pinned": true, "chat": true}}
  ],
  "stage": false,
  "chat_sound": true,
  "condensed": true,
  "mute_background": true,
  "voice_follows": false,
  "monitors": {"cycling": {"open": true, "interval_s": 12}}
}"#;

#[test]
fn a_file_of_checkpoint_9_reads_with_nothing_lost_and_the_new_defaults() {
    let config: Config = serde_json::from_str(CHECKPOINT_9).expect("a file of checkpoint 9 reads");
    let desk = Desk::new(config);
    // what it had
    assert!(!desk.stage() && desk.chat_sound() && desk.condensed());
    assert!(desk.mute_background() && !desk.voice_follows());
    assert!(desk.monitors().cycling.open);
    let main = desk.slot(0).expect("the first character");
    assert_eq!(
        (main.colour, main.alias.as_deref(), main.sound),
        (Colour::Teal, Some("Shop"), BackgroundSound::All)
    );
    let alt = desk.slot(2).expect("the third character");
    assert_eq!(alt.standby, Standby::Economy);
    assert!(alt.monitor.is_some() && alt.monitor_window.pinned && alt.monitor_window.chat);
    // what it had not: the defaults, everything as it was before
    assert_eq!(desk.chat_shown(), ChatShown::default());
    assert_eq!(desk.attention(), Attention::default());
    assert_eq!(desk.reading(), Reading::default());
    assert_eq!(desk.settings_window().section, Section::Toolbar);
    assert!(main.attention && alt.attention);
    assert_eq!((main.alerts, alt.alerts), (Alerts::default(), Alerts::default()));
    // written again, the defaults of a character are left out of the file
    let text = serde_json::to_string(&desk.config()).expect("json");
    assert!(!text.contains("\"attention\":true") && !text.contains("\"alerts\""), "{text}");
}

#[test]
fn the_new_settings_are_saved_and_read_back() {
    let mut desk = desk();
    desk.take_effects();
    let shown = ChatShown {
        groups: false,
        ..ChatShown::default()
    };
    desk.set_chat_shown(shown);
    let attention = Attention {
        teleports: false,
        newest_first: true,
        ..Attention::default()
    };
    desk.set_attention(attention);
    desk.set_reading(Reading {
        auto_read_after_s: 90,
        mark_displayed: false,
        ..Reading::default()
    });
    desk.set_attention_included(1, false);
    let alerts = Alerts {
        groups: false,
        ..Alerts::default()
    };
    desk.set_alerts(2, alerts);
    desk.edit_settings_window(|window| window.section = Section::Chat);
    assert_eq!(saves(&desk.take_effects()), 6);
    // the same again: nothing to write
    desk.set_chat_shown(shown);
    desk.set_attention(attention);
    desk.set_attention_included(1, false);
    desk.set_alerts(2, alerts);
    desk.edit_settings_window(|window| window.section = Section::Chat);
    assert!(desk.take_effects().is_empty());
    // a delay out of bounds is held to them
    desk.set_reading(Reading {
        auto_read_after_s: 1,
        ..desk.reading()
    });
    assert_eq!(desk.reading().auto_read_after_s, crate::settings::AUTO_READ_MIN_S);
    let text = serde_json::to_string(&desk.config()).expect("json");
    let next = Desk::new(serde_json::from_str(&text).expect("json"));
    assert_eq!(next.chat_shown(), shown);
    assert_eq!(next.attention(), attention);
    assert!(!next.reading().mark_displayed);
    assert_eq!(next.settings_window().section, Section::Chat);
    assert_eq!(next.slot(1).map(|s| s.attention), Some(false));
    assert_eq!(next.slot(2).map(|s| s.alerts), Some(alerts));
}

#[test]
fn attention_and_the_alerts_follow_each_character_s_choice() {
    let mut desk = desk();
    assert!(desk.attention_lists(1, AttentionKind::Teleport));
    desk.set_attention_included(1, false);
    assert!(!desk.attention_lists(1, AttentionKind::Teleport));
    assert!(desk.attention_lists(0, AttentionKind::Teleport));
    desk.set_attention(Attention {
        inventory: false,
        ..Attention::default()
    });
    assert!(!desk.attention_lists(0, AttentionKind::Inventory));
    assert!(desk.attention_lists(0, AttentionKind::Script));
    // the alerts: never for the active character, by each one's choice
    let now = Instant::now();
    desk.launch(0, true);
    desk.connected(0, 1, now);
    desk.switch(0, now);
    desk.set_alerts(
        2,
        Alerts {
            groups: false,
            ..Alerts::default()
        },
    );
    if desk.active() == Some(0) {
        assert!(!desk.alert_wanted(0, SessionKind::Resident, LineKind::Said));
    }
    assert!(desk.alert_wanted(1, SessionKind::Group, LineKind::Said));
    assert!(!desk.alert_wanted(2, SessionKind::Group, LineKind::Said));
    assert!(desk.alert_wanted(2, SessionKind::Resident, LineKind::Said));
    assert!(!desk.alert_wanted(1, SessionKind::Nearby, LineKind::Said));
}

#[test]
fn a_character_s_own_settings_live_through_its_launches() {
    let now = Instant::now();
    let mut desk = desk();
    desk.set_colour(1, Colour::Orange);
    assert_eq!(desk.set_alias(1, "Shop"), Ok(()));
    desk.set_attention_included(1, false);
    desk.set_alerts(
        1,
        Alerts {
            ims: false,
            ..Alerts::default()
        },
    );
    desk.edit_monitor_window(1, |window| window.pinned = true);
    for launch in 1..=2 {
        desk.launch(1, false);
        desk.connected(1, launch, now);
        let s = desk.slot(1).expect("the character");
        assert_eq!(s.launch(), launch);
        assert_eq!((s.colour, s.alias.as_deref()), (Colour::Orange, Some("Shop")), "launch {launch}");
        assert!(!s.attention && !s.alerts.ims && s.monitor_window.pinned);
        desk.exited(1, launch, true, now);
    }
}

#[test]
fn two_cards_swap_their_places_only_when_nothing_running_changes_number() {
    let now = Instant::now();
    let mut desk = desk();
    // each place keeps its own count of launches
    desk.launch(0, false);
    desk.exited(0, 1, true, now);
    desk.set_colour(0, Colour::Teal);
    desk.take_effects();
    assert_eq!(desk.neighbour_card(0, true), None);
    assert_eq!(desk.neighbour_card(0, false), Some(1));
    assert!(desk.can_swap(0, 1));
    assert!(desk.swap_slots(0, 1));
    assert_eq!(saves(&desk.take_effects()), 1);
    assert_eq!(desk.slot(0).map(|s| s.profile.shown().to_owned()), Some("demo-2".into()));
    assert_eq!(
        desk.slot(1).map(|s| (s.profile.shown().to_owned(), s.colour)),
        Some(("demo-1".into(), Colour::Teal))
    );
    assert_eq!(desk.slot(0).map(Slot::launch), Some(1), "place 0 counted one launch");
    assert_eq!(desk.slot(1).map(Slot::launch), Some(0));
    // a character with a viewer, or with a monitor open, does not move
    desk.launch(2, false);
    assert!(!desk.can_swap(1, 2) && !desk.swap_slots(2, 1));
    desk.set_monitor(0, Some(monitor::Spec::default()));
    assert!(!desk.can_swap(0, 1));
    desk.set_monitor(0, None);
    assert!(desk.can_swap(0, 1));
    // nor with an empty place, nor with itself
    desk.remove(1);
    assert!(!desk.can_swap(0, 1) && !desk.can_swap(0, 0));
    assert_eq!(desk.neighbour_card(0, false), Some(2));
}
