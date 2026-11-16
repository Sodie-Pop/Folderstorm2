//! The *Saved workspaces* group of the tab (checkpoint 6), found by the
//! window's search so that it is drawn alone, above the fold.

use super::*;
use crate::folderstorm::workspaces::words::workspace_holds;

/// The tab with the layout "Wide" saved and in use, searched for its
/// workspaces.
fn searched() -> Window {
    let mut w = Window::new();
    w.save_as("Wide");
    w.st.query = "workspace".into();
    w.settle();
    w
}

fn save_workspace(w: &mut Window, name: &str) {
    // (under a selected workspace's settings: the page is scrolled to it)
    w.reveal(tr!("fs-workspaces-name-hint"));
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
    w.reveal(tr!("fs-workspaces-error-taken"));
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
    let texts = drawn_found("preferences-workspaces", 3, s, false, "workspace");
    for expected in [
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
        assert!(texts.contains(&expected), "“{expected}” is drawn");
    }
    assert!(!texts.iter().any(|text| text == tr!("common-conversations")), "never Conversations");
}

mod context {
    //! The workspace's context (row 1.7): its folded section, the graphics
    //! profile and camera pickers, the HUD boxes, *Forget them*.

    use super::*;
    use crate::camera::{CameraPreset, CameraShot};
    use crate::folderstorm::workspaces::context::{Camera, Hud, HudView, Huds};

    const ME: uuid::Uuid = uuid::Uuid::from_u128(7);

    fn radar() -> Hud {
        Hud {
            item: uuid::Uuid::from_u128(9001),
            point: 35,
        }
    }

    /// [`searched`] with two graphics profiles, a camera preset, logged in
    /// as `ME` wearing the HUD "Radar", and the workspace "Photos" saved.
    fn photos() -> Window {
        let mut w = searched();
        w.s.graphics_saved.insert("Fast".into(), Default::default());
        w.s.graphics_saved.insert("My photo profile".into(), Default::default());
        w.s.camera_shots
            .push(CameraShot::at_avatar("Portrait".into(), [-2.0, 0.0, 0.5], [0.0, 0.0, 0.5]));
        w.shell.account = Some(ME);
        w.shell.huds = vec![radar()];
        w.shell.names.insert(radar().item, "Radar".into());
        save_workspace(&mut w, "Photos");
        w
    }

    fn photos_workspace(w: &Window) -> crate::folderstorm::workspaces::workspace::Workspace {
        w.store().workspace(0).expect("saved").clone()
    }

    /// Picks `choice` in the combo box showing `shown`.
    fn pick(w: &mut Window, shown: &str, choice: &str) {
        w.reveal(shown);
        w.click(shown);
        let at = w
            .run(Vec::new())
            .iter()
            .filter(|t| t.source == choice)
            .map(|t| t.rect)
            .next_back()
            .unwrap_or_else(|| panic!("“{choice}” is offered"));
        w.click_at(at.center());
    }

    #[test]
    fn the_context_is_folded_until_chosen_and_chosen_in_its_section() {
        let mut w = photos();
        w.reveal(tr!("fs-context-section"));
        assert!(!w.shows(tr!("fs-context-graphics-keep")), "folded: no context yet");
        w.click(tr!("fs-context-section"));
        w.settle();
        // the graphics profile, by name
        pick(&mut w, tr!("fs-context-graphics-keep"), "My photo profile");
        assert_eq!(photos_workspace(&w).graphics.as_deref(), Some("My photo profile"));
        assert!(w.changed);
        // the camera: a view, then a preset
        pick(&mut w, tr!("fs-context-camera-keep"), CameraPreset::Front.label());
        assert_eq!(photos_workspace(&w).camera, Some(Camera::View(CameraPreset::Front)));
        pick(
            &mut w,
            CameraPreset::Front.label(),
            &tr!("fs-context-camera-preset", name = "Portrait"),
        );
        assert_eq!(photos_workspace(&w).camera, Some(Camera::Preset("Portrait".into())));
        // a HUD worn now, ticked: added with its point for this account
        w.reveal("Radar");
        w.click("Radar");
        assert_eq!(
            photos_workspace(&w).huds,
            Some(Huds {
                account: ME,
                items: vec![radar()]
            })
        );
        // what it holds is said under its name
        assert!(w.shows(&workspace_holds(&photos_workspace(&w))));
        // taken off: still listed by its name, ticked; unticked, gone
        w.shell.huds.clear();
        w.settle();
        w.reveal("Radar");
        w.click("Radar");
        assert_eq!(photos_workspace(&w).huds, None);
        assert!(w.shows(tr!("fs-context-huds-none")));
    }

    #[test]
    fn a_profile_or_preset_gone_is_shown_as_such_and_another_account_s_huds_can_be_forgotten() {
        let mut w = photos();
        let index = 0;
        assert!(w.s.folderstorm_workspaces.set_workspace_graphics(index, Some("Deleted profile")));
        assert!(
            w.s.folderstorm_workspaces
                .set_workspace_camera(index, Some(Camera::Preset("Old shot".into())))
        );
        let theirs = Huds {
            account: uuid::Uuid::from_u128(8),
            items: vec![radar()],
        };
        assert!(w.s.folderstorm_workspaces.set_workspace_huds(index, Some(theirs)));
        w.settle();
        // (the section stays as the user left it: folded when it had none)
        w.reveal(tr!("fs-context-section-set"));
        w.click(tr!("fs-context-section-set"));
        w.settle();
        w.reveal(&tr!("fs-context-gone", name = "Deleted profile"));
        assert!(w.shows(&tr!("fs-context-gone", name = "Deleted profile")));
        let preset = tr!("fs-context-camera-preset", name = "Old shot");
        w.reveal(&tr!("fs-context-gone", name = preset.as_str()));
        assert!(w.shows(&tr!("fs-context-gone", name = preset.as_str())));
        w.reveal(&tr!("fs-context-huds-other", count = 1));
        assert!(w.shows(&tr!("fs-context-huds-other", count = 1)));
        assert!(!w.shows("Radar"), "this account's HUDs wait until those are forgotten");
        w.click(tr!("fs-context-huds-forget"));
        assert_eq!(photos_workspace(&w).huds, None);
        w.reveal("Radar");
        assert!(w.shows("Radar"));
    }

    /// The picture of the section open, for a workspace with a context
    /// (`preferences-workspace-context.png`).
    #[test]
    fn the_context_section_is_drawn() {
        let mut s = Settings::default();
        s.graphics_saved.insert("My photo profile".into(), Default::default());
        s.folderstorm_workspaces = serde_json::from_value(serde_json::json!({
            "workspaces": [{
                "schema": 1, "name": "Photos", "open": ["worldmap"],
                "graphics": "My photo profile",
                "camera": { "view": "Front" },
                "huds": { "account": ME.to_string(), "items": [
                    { "item": radar().item.to_string(), "point": 35 },
                    { "item": "00000000-0000-0000-0000-000000002333", "point": 31 },
                ]},
            }],
            "active_workspace": "Photos",
        }))
        .expect("read");
        let huds = HudView {
            account: Some(ME),
            worn: vec![(
                Hud {
                    item: uuid::Uuid::from_u128(9100),
                    point: 33,
                },
                "Camera HUD".into(),
            )],
            names: [(radar().item, "Radar".to_owned())].into_iter().collect(),
        };
        let texts = drawn_scrolled("preferences-workspace-context", 3, s, false, "workspace", Some(huds), 420.0);
        for expected in [
            tr!("fs-context-section-set").to_owned(),
            tr!("fs-context-graphics").to_owned(),
            "My photo profile".to_owned(),
            tr!("fs-context-camera").to_owned(),
            CameraPreset::Front.label().to_owned(),
            tr!("fs-context-huds").to_owned(),
            "Radar".to_owned(),
            tr!("fs-context-hud-gone").to_owned(),
            "Camera HUD".to_owned(),
        ] {
            assert!(texts.contains(&expected), "“{expected}” is drawn: {texts:?}");
        }
    }
}
