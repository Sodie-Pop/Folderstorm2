//! The *Saved workspaces* group of the tab (checkpoint 6), found by the
//! window's search so that it is drawn alone, above the fold.

use super::*;
use crate::folderstorm::workspaces::words::workspace_holds;

/// The tab with the layout "Wide" saved and in use, searched for its
/// workspaces.
fn searched() -> Window {
    let mut w = Window::new();
    w.save_as("Wide");
    w.st.query = "workspaces".into();
    w.settle();
    w
}

fn save_workspace(w: &mut Window, name: &str) {
    w.click(tr!("fs-workspaces-name-hint"));
    w.type_text(name);
    w.click(tr!("fs-workspaces-save"));
    w.settle();
}

#[test]
fn the_tab_saves_a_workspace_and_edits_its_layout_and_its_windows() {
    let mut w = searched();
    assert!(w.shows(tr!("fs-workspaces-empty")));
    w.changed = false;
    save_workspace(&mut w, "Sorting");
    assert!(w.changed, "the settings are saved");
    let saved = w.store().workspace(0).expect("saved").clone();
    assert_eq!((saved.name.as_str(), saved.layout.as_deref()), ("Sorting", None));
    assert!(w.shows("Sorting") && w.shows(tr!("fs-layouts-active")));
    assert!(w.shows(&workspace_holds(&saved)), "what it does");
    assert!(!w.shows(tr!("fs-workspaces-empty")));
    // a window ticked: in its list, and the workspace no longer describes
    // what is open
    w.click(tr!("common-places"));
    assert!(w.store().workspace(0).expect("saved").open.contains("places"));
    assert!(w.shows(tr!("fs-layouts-modified")));
    w.click(tr!("common-places"));
    assert!(w.store().workspace(0).expect("saved").open.is_empty());
    // its layout: the one in use is kept, or a layout of the list
    w.click(tr!("fs-workspaces-layout-keep"));
    let wide = w
        .run(Vec::new())
        .iter()
        .filter(|t| t.source == "Wide")
        .map(|t| t.rect)
        .next_back()
        .expect("offered");
    w.click_at(wide.center());
    assert_eq!(w.store().workspace(0).expect("saved").layout.as_deref(), Some("Wide"));
}

#[test]
fn a_workspace_is_saved_with_the_layout_in_use_when_asked() {
    let mut w = searched();
    w.click(&tr!("fs-workspaces-with-layout", name = "Wide"));
    save_workspace(&mut w, "Photos");
    assert_eq!(w.store().workspace(0).expect("saved").layout.as_deref(), Some("Wide"));
    // a name that cannot be used is refused with the reason
    save_workspace(&mut w, " photos ");
    assert!(w.shows(tr!("fs-workspaces-error-taken")));
    assert_eq!(w.store().workspaces.len(), 1);
}

#[test]
fn a_workspace_is_applied_renamed_starred_and_deleted_after_a_question() {
    let mut w = searched();
    save_workspace(&mut w, "Sorting");
    w.s.folderstorm_workspaces.active_workspace = None;
    w.changed = false;
    w.click(tr!("fs-layouts-apply"));
    w.settle();
    assert_eq!(w.store().active_workspace.as_deref(), Some("Sorting"));
    assert!(w.changed);
    assert!(w.shows(&tr!("fs-workspaces-applied", name = "Sorting")), "what the apply did");
    // the star, after the two arrows
    let star = w.rect_of(tr!("common-delete")).expect("the actions").right_center() + vec2(85.0, 0.0);
    w.click_at(star);
    assert!(w.store().is_favorite_workspace("Sorting"));
    // renamed: the star and the mark follow
    w.click(tr!("fs-layouts-rename"));
    w.type_text("Tidying");
    w.click(tr!("fs-layouts-rename"));
    w.settle();
    assert!(w.store().find_workspace("Tidying").is_some() && w.store().is_favorite_workspace("Tidying"));
    assert_eq!(w.store().active_workspace.as_deref(), Some("Tidying"));
    // Delete asks first
    w.click(tr!("common-delete"));
    assert!(w.shows(&tr!("fs-workspaces-ask-delete", name = "Tidying")));
    assert_eq!(w.store().workspaces.len(), 1);
    w.click(tr!("common-cancel"));
    assert_eq!(w.store().workspaces.len(), 1);
    w.click(tr!("common-delete"));
    w.click(tr!("common-delete"));
    assert!(w.store().workspaces.is_empty() && w.store().favorites.is_empty());
    assert_eq!(w.store().layouts.len(), 1, "the layouts are not touched");
}

/// The picture of the group with a workspace in use, another one, and an
/// entry from a newer version (`preferences-workspaces.png`).
#[test]
fn the_workspaces_group_is_drawn() {
    use crate::folderstorm::workspaces::windows::ADDITIONAL_INVENTORY;
    let s = Settings {
        folderstorm_workspaces: serde_json::from_value(serde_json::json!({
            "layouts": [
                Layout::new("2 monitors", Some(wide()), None, Vec::new()),
                Layout::new("1 monitor", Some(Default::default()), None, Vec::new()),
            ],
            "workspaces": [
                { "schema": 1, "name": "Sorting", "layout": "1 monitor", "open": ["inventory", ADDITIONAL_INVENTORY, "people"] },
                { "schema": 1, "name": "Exploring", "open": ["minimap", "worldmap", "places"] },
                { "schema": 7, "name": "From a later version" },
            ],
            "active_workspace": "Sorting",
            "favorites": [{ "workspace": "Exploring" }],
        }))
        .expect("read"),
        ..Default::default()
    };
    let sorting = s.folderstorm_workspaces.workspace(0).expect("read").clone();
    let texts = drawn_found("preferences-workspaces", 3, s, false, "workspaces");
    for expected in [
        tr!("fs-workspaces-group").to_owned(),
        tr!("fs-workspaces-list").to_owned(),
        "Sorting".to_owned(),
        "Exploring".to_owned(),
        "From a later version".to_owned(),
        tr!("fs-layouts-newer").to_owned(),
        workspace_holds(&sorting),
        tr!("fs-workspaces-layout").to_owned(),
        "1 monitor".to_owned(),
        tr!("fs-workspaces-windows").to_owned(),
        tr!("common-inventory").to_owned(),
        tr!("fs-workspaces-save-new").to_owned(),
        tr!("fs-workspaces-save").to_owned(),
    ] {
        assert!(texts.iter().any(|text| *text == expected), "“{expected}” is drawn");
    }
    assert!(!texts.iter().any(|text| text == tr!("common-conversations")), "never Conversations");
}
