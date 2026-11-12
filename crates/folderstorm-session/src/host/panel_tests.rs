//! The desk's side of checkpoint 6: the panels that have a window of
//! their own are part of the host's settings, and closing one of those
//! windows docks its panel and does nothing else.

use super::*;
use crate::panels::{Panel, PanelConfig, Place};

fn desk() -> Desk {
    Desk::new(Config::demo(2))
}

fn saves(desk: &mut Desk) -> usize {
    desk.take_effects().iter().filter(|e| matches!(e, Effect::Save)).count()
}

#[test]
fn popping_a_panel_out_and_back_is_saved() {
    let mut desk = desk();
    assert!(!desk.panels().out(Panel::Chat));
    desk.set_panel_out(Panel::Chat, true);
    assert!(desk.panels().out(Panel::Chat));
    assert_eq!(saves(&mut desk), 1);
    desk.set_panel_out(Panel::Chat, true);
    assert_eq!(saves(&mut desk), 0, "nothing changed: nothing to write");
    let place = Place {
        position: Some([40, 60]),
        size: [640, 480],
    };
    desk.panel_placed(Panel::Chat, place);
    desk.panel_placed(Panel::Chat, place);
    assert_eq!(saves(&mut desk), 1);
    assert_eq!(
        desk.config().panels.chat,
        PanelConfig {
            out: true,
            place: Some(place)
        }
    );
    // what is saved is what the next host starts with
    let next = Desk::new(desk.config());
    assert!(next.panels().out(Panel::Chat) && !next.panels().out(Panel::Monitors));
    assert_eq!(next.panels().get(Panel::Chat).place, Some(place));
}

#[test]
fn closing_a_panel_s_window_never_quits_the_host_or_a_character() {
    let now = Instant::now();
    let mut desk = desk();
    for slot in 0..2 {
        desk.launch(slot, slot == 0);
        desk.connected(slot, 1, now);
    }
    desk.set_panel_out(Panel::Controls, true);
    desk.set_panel_out(Panel::Monitors, true);
    desk.take_effects();
    desk.panel_closed(Panel::Controls);
    let effects = desk.take_effects();
    assert!(matches!(effects.as_slice(), [Effect::Save]), "{effects:?}");
    assert!(!desk.panels().out(Panel::Controls));
    assert!(desk.panels().out(Panel::Monitors));
    assert_eq!(desk.live(), 2);
    assert_eq!(desk.active(), Some(0));
    assert!(!desk.quitting());
    desk.panel_closed(Panel::Controls);
    assert!(desk.take_effects().is_empty());
}

#[test]
fn a_file_with_a_place_no_window_has_starts_the_panel_at_its_first_size() {
    let mut config = Config::demo(1);
    config.panels.monitors = PanelConfig {
        out: true,
        place: Some(Place {
            position: Some([0, 0]),
            size: [1, 1],
        }),
    };
    let desk = Desk::new(config);
    assert_eq!(desk.panels().get(Panel::Monitors), PanelConfig { out: true, place: None });
    // a settings file from before checkpoint 6 reads, panels docked
    let old: Config = serde_json::from_str(r#"{"slots": [], "stage": true}"#).expect("reads");
    assert_eq!(old.panels, crate::panels::Config::default());
}
