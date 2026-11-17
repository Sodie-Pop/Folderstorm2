//! The background of the margins in Preferences (row 1.8): the default's
//! group on Workspaces › Margins, and a layout's own under the selected
//! layout on Saved.

use super::*;
use crate::folderstorm::backgrounds::record::{Background, Fit, Kind, Picture};

fn chosen() -> Background {
    Background {
        kind: Kind::Picture,
        colour: [24, 30, 52, 255],
        picture: Some(Picture {
            file: "0123456789abcdef.png".into(),
            name: "dusk.png".into(),
        }),
        fit: Fit::Fill,
        span: false,
        brightness: 80,
    }
}

/// The default's group, with a picture chosen and the viewport off
/// (`preferences-background.png`).
#[test]
fn the_background_group_is_drawn() {
    let mut s = Settings::default();
    s.folderstorm_workspaces.background = chosen();
    let texts = drawn("preferences-background", tab::MARGINS, s, false);
    // (the group's title in small capitals)
    let title = tr!("fs-background-group").to_uppercase();
    assert!(texts.iter().any(|t| t.to_uppercase() == title), "the group's title");
    for expected in [
        tr!("fs-margins-off"),
        tr!("fs-margins-turn-on"),
        tr!("fs-background-kind"),
        tr!("fs-background-kind-none"),
        tr!("fs-background-kind-colour"),
        tr!("fs-background-kind-picture"),
        tr!("fs-background-picture"),
        tr!("fs-background-choose"),
        "dusk.png",
        tr!("fs-background-fit"),
        tr!("fs-background-fit-fill"),
        tr!("fs-background-span"),
        tr!("fs-background-colour"),
        tr!("fs-background-brightness"),
    ] {
        assert!(texts.iter().any(|t| t == expected), "“{expected}” is drawn");
    }
    assert!(crate::ui::icons::global("photo-filled").is_some(), "the icon is embedded");
    // the viewport on: no notice; nothing chosen: only the kind's row
    let mut s = Settings::default();
    s.folderstorm_viewport.enabled = true;
    let texts = drawn("preferences-background-none", tab::MARGINS, s.clone(), false);
    assert!(texts.iter().any(|t| t == tr!("fs-background-kind")));
    assert!(!texts.iter().any(|t| t == tr!("fs-margins-off")));
    assert!(
        !texts.iter().any(|t| t == tr!("fs-background-fit")),
        "the picture's rows wait for a picture"
    );
    // the search still finds them
    let texts = drawn_found("preferences-background-found", tab::MARGINS, s, false, "wallpaper");
    assert!(texts.iter().any(|t| t == tr!("fs-background-picture")));
}

#[test]
fn the_default_is_chosen_in_its_group() {
    let mut w = Window::new();
    w.s.folderstorm_viewport.enabled = true;
    w.st.open(Page::Workspaces, tab::MARGINS);
    w.st.query = "wallpaper".into();
    w.settle();
    w.changed = false;
    w.click(tr!("fs-background-kind-colour"));
    assert_eq!(w.store().background.kind, Kind::Colour);
    assert!(w.changed, "the settings are saved");
    w.click(tr!("fs-background-kind-picture"));
    assert_eq!(w.store().background.kind, Kind::Picture);
    assert!(w.shows(tr!("fs-background-none-chosen")));
    w.click(tr!("fs-background-kind-none"));
    assert_eq!(w.store().background.kind, Kind::None);
}

#[test]
fn a_layout_takes_its_own_background_and_gives_it_back() {
    let mut w = Window::new();
    w.s.folderstorm_workspaces.background = chosen();
    w.save_as("Wide");
    assert!(w.store().layout(0).expect("saved").background.is_none(), "saved with the default");
    assert!(w.shows(tr!("fs-background-layout-default")));
    w.click(tr!("fs-background-layout-own"));
    assert_eq!(
        w.store().layout(0).expect("saved").background.as_ref(),
        Some(&chosen()),
        "a copy of the default"
    );
    // its own choices, under it: a colour
    w.click(tr!("fs-background-kind-colour"));
    let own = w.store().layout(0).expect("saved").background.clone().expect("its own");
    assert_eq!(own.kind, Kind::Colour);
    assert_eq!(w.store().background, chosen(), "the default untouched");
    assert_eq!(
        crate::folderstorm::backgrounds::in_effect(w.store()).kind,
        Kind::Colour,
        "in effect while in use"
    );
    w.click(tr!("fs-background-layout-default"));
    assert!(w.store().layout(0).expect("saved").background.is_none());
    assert_eq!(crate::folderstorm::backgrounds::in_effect(w.store()), &chosen());
}

/// The selected layout with its own background (`preferences-layout-background.png`).
#[test]
fn a_layout_s_own_background_is_drawn() {
    let mut layout = Layout::new("Wall", Some(wide()), None, Vec::new());
    layout.background = Some(Background {
        fit: Fit::Tile,
        ..chosen()
    });
    let mut s = Settings::default();
    s.folderstorm_workspaces.add(layout).expect("added");
    let texts = drawn("preferences-layout-background", tab::SAVED, s.clone(), false);
    // the layout in use is selected: its background under its actions
    for expected in [
        "Wall",
        tr!("fs-background-layout"),
        tr!("fs-background-layout-own"),
        tr!("fs-background-kind-picture"),
        "dusk.png",
    ] {
        assert!(texts.iter().any(|t| t == expected), "“{expected}” is drawn");
    }
    // further down the editor
    let texts = drawn_scrolled("preferences-layout-background-down", tab::SAVED, s, false, "", None, 400.0);
    for expected in [tr!("fs-background-fit-tile"), tr!("fs-background-brightness")] {
        assert!(texts.iter().any(|t| t == expected), "“{expected}” is drawn");
    }
}
