//! The desk's side of checkpoint 7: a character's colour and alias and
//! the condensed layout are part of the host's settings, are checked when
//! the file is read, and tell the viewers nothing.

use super::*;
use crate::identity::{AliasError, Colour, MAX_ALIAS};

fn desk() -> Desk {
    Desk::new(Config::demo(2))
}

fn saves(effects: &[Effect]) -> usize {
    effects.iter().filter(|e| matches!(e, Effect::Save)).count()
}

#[test]
fn a_colour_and_an_alias_are_saved_and_told_to_no_viewer() {
    let now = Instant::now();
    let mut desk = desk();
    for slot in 0..2 {
        desk.launch(slot, slot == 0);
        desk.connected(slot, 1, now);
    }
    desk.take_effects();
    desk.set_colour(1, Colour::Teal);
    assert_eq!(desk.set_alias(1, "  Shop keeper "), Ok(()));
    desk.set_condensed(true);
    let effects = desk.take_effects();
    assert_eq!(saves(&effects), 3);
    assert!(
        effects.iter().all(|e| matches!(e, Effect::Save)),
        "nothing but the file: {effects:?}"
    );
    assert_eq!(
        desk.slot(1).map(|s| (s.colour, s.alias.as_deref())),
        Some((Colour::Teal, Some("Shop keeper")))
    );
    assert_eq!(desk.slot(0).map(|s| (s.colour, s.alias.as_deref())), Some((Colour::None, None)));
    // the same again: nothing to write
    desk.set_colour(1, Colour::Teal);
    assert_eq!(desk.set_alias(1, "Shop keeper"), Ok(()));
    desk.set_condensed(true);
    assert!(desk.take_effects().is_empty());
    // what is saved is what the next host starts with
    let config = desk.config();
    let text = serde_json::to_string(&config).expect("json");
    assert!(
        text.contains("\"colour\":\"teal\"") && text.contains("\"alias\":\"Shop keeper\""),
        "{text}"
    );
    assert!(text.contains("\"condensed\":true"));
    let next = Desk::new(serde_json::from_str(&text).expect("json"));
    assert!(next.condensed());
    assert_eq!(
        next.slot(1).map(|s| (s.colour, s.alias.as_deref())),
        Some((Colour::Teal, Some("Shop keeper")))
    );
    // an empty text takes the alias away
    assert_eq!(desk.set_alias(1, "   "), Ok(()));
    assert_eq!(desk.slot(1).and_then(|s| s.alias.clone()), None);
    assert_eq!(saves(&desk.take_effects()), 1);
    assert!(!serde_json::to_string(&desk.config()).expect("json").contains("alias"));
}

#[test]
fn a_text_that_is_no_alias_changes_nothing() {
    let mut desk = desk();
    assert_eq!(desk.set_alias(0, "Main"), Ok(()));
    desk.take_effects();
    assert_eq!(desk.set_alias(0, &"x".repeat(MAX_ALIAS + 1)), Err(AliasError::TooLong));
    assert_eq!(desk.set_alias(0, "two\nlines"), Err(AliasError::Character('\n')));
    assert_eq!(desk.set_alias(0, "a\u{202E}b"), Err(AliasError::Character('\u{202E}')));
    assert_eq!(desk.slot(0).and_then(|s| s.alias.clone()), Some("Main".to_owned()));
    assert!(desk.take_effects().is_empty());
    // a slot without a character: nothing, and no error to show
    assert_eq!(desk.set_alias(4, "Nobody"), Ok(()));
    desk.set_colour(4, Colour::Red);
    assert!(desk.take_effects().is_empty());
}

#[test]
fn an_older_file_reads_with_no_colour_no_alias_and_the_full_layout() {
    let text = r#"{"slots":[{"slot":0,"profile":"first"},{"slot":1,"profile":"alt","standby":"Economy"}],"stage":true}"#;
    let desk = Desk::new(serde_json::from_str(text).expect("an older file"));
    assert!(!desk.condensed());
    assert_eq!(desk.slots().count(), 2);
    assert!(desk.slots().all(|(_, s)| s.colour == Colour::None && s.alias.is_none()));
}

#[test]
fn the_file_s_aliases_and_colours_are_checked_when_read() {
    let text = format!(
        r#"{{"slots":[
            {{"slot":0,"profile":"first","colour":"pink","alias":"  Main  "}},
            {{"slot":1,"profile":"alt","colour":"infrared","alias":"bad\u0007bell"}},
            {{"slot":2,"profile":"third","alias":"{}"}},
            {{"slot":3,"profile":"fourth","alias":""}}
        ],"condensed":true}}"#,
        "y".repeat(MAX_ALIAS + 1)
    );
    let desk = Desk::new(serde_json::from_str(&text).expect("the file reads"));
    let got: Vec<(Colour, Option<&str>)> = desk.slots().map(|(_, s)| (s.colour, s.alias.as_deref())).collect();
    assert_eq!(
        got,
        [
            (Colour::Pink, Some("Main")),
            // a colour of a later build, an alias with a control character
            (Colour::None, None),
            // too long: dropped, not cut into another name
            (Colour::None, None),
            (Colour::None, None),
        ]
    );
    assert!(desk.condensed());
}

#[test]
fn removing_a_character_takes_its_colour_and_alias_with_it() {
    let mut desk = desk();
    desk.set_colour(1, Colour::Blue);
    assert_eq!(desk.set_alias(1, "Alt"), Ok(()));
    assert!(desk.remove(1));
    assert_eq!(desk.add("someone-else"), Ok(1));
    assert_eq!(desk.slot(1).map(|s| (s.colour, s.alias.clone())), Some((Colour::None, None)));
}
