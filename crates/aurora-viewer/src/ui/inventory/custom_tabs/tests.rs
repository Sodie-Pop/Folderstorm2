use super::*;
use crate::ui::inventory::view::{SearchField, ViewCache};
use crate::ui::widgets::elide_tests::Frames;

fn tab(id: u64, name: &str) -> CustomTab {
    CustomTab {
        id,
        name: name.into(),
        ..Default::default()
    }
}

fn texture_filters() -> Filters {
    Filters {
        types: 1 << 0,
        ..Default::default()
    }
}

fn prefs_with(names: &[&str]) -> InventoryPreferences {
    let mut prefs = InventoryPreferences::default();
    for name in names {
        add(&mut prefs.custom_tabs, name, Filters::default(), String::new()).expect("room");
    }
    prefs
}

fn agent() -> Uuid {
    Uuid::from_u128(2)
}

fn demo_inventory() -> Inventory {
    let mut inv = Inventory::default();
    crate::world::inventory::demo::seed(&mut inv, agent());
    inv
}

/// The items a custom tab lists in the demo inventory.
fn listed(inv: &Inventory, tab: &CustomTab) -> Vec<String> {
    let facts = Facts {
        agent: agent(),
        ..Default::default()
    };
    let mut view = ViewCache::default();
    view.search(
        inv,
        inv.root,
        &tab.search,
        &InventoryPreferences::default(),
        &tab.filters,
        CUSTOM_TAB,
        None,
        &facts,
        agent(),
        view::now(),
    );
    let mut names: Vec<String> = view
        .results
        .iter()
        .filter_map(|id| inv.items.get(id))
        .map(|it| it.name.clone())
        .collect();
    names.sort();
    names
}

fn name_of_item(inv: &Inventory, n: u128) -> String {
    inv.items[&Uuid::from_u128(n)].name.clone()
}

// ---- persistence ----

#[test]
fn custom_tabs_round_trip_through_the_saved_preferences() {
    let mut prefs = InventoryPreferences::default();
    let advanced = Filters {
        smart: Some(smart::SmartQuery::from_quick("hat+red", SearchField::Name)),
        ..Default::default()
    };
    add(&mut prefs.custom_tabs, "Textures", texture_filters(), "sky".into()).expect("room");
    add(&mut prefs.custom_tabs, "Rules", advanced, String::new()).expect("room");
    let saved = serde_json::to_string(&prefs).expect("serialize");
    let loaded: InventoryPreferences = serde_json::from_str(&saved).expect("deserialize");
    assert_eq!(loaded, prefs);
    assert_eq!(loaded.custom_tabs[0].filters.types, 1);
    assert_eq!(loaded.custom_tabs[0].search, "sky");
    assert!(loaded.custom_tabs[1].filters.smart.is_some());
}

#[test]
fn old_settings_without_custom_tabs_load_with_none() {
    let old: InventoryPreferences = serde_json::from_str(r#"{"show_recent":false,"separate_searches":true}"#).expect("old file");
    assert!(old.custom_tabs.is_empty());
    assert!(!old.show_recent && old.separate_searches);
    assert_eq!(InventoryPreferences::default().custom_tabs, Vec::new());
}

#[test]
fn a_tab_missing_fields_takes_their_defaults() {
    let loaded: InventoryPreferences =
        serde_json::from_str(r#"{"custom_tabs":[{"id":4,"name":"Old"},{"id":5,"name":"F","filters":{"types":2}}]}"#).expect("partial");
    assert_eq!(loaded.custom_tabs[0].filters, Filters::default());
    assert_eq!(loaded.custom_tabs[0].search, "");
    assert_eq!(loaded.custom_tabs[1].filters.types, 2);
    assert_eq!(loaded.custom_tabs[1].filters.links, view::Links::Include);
}

#[test]
fn hand_edited_ids_and_names_are_repaired() {
    let mut tabs = vec![tab(0, "A"), tab(3, "B"), tab(3, "C"), tab(4, "  ")];
    assert!(normalize(&mut tabs));
    let ids: std::collections::HashSet<u64> = tabs.iter().map(|t| t.id).collect();
    assert_eq!(ids.len(), 4);
    assert!(!ids.contains(&0));
    assert!(tabs.iter().all(|t| !t.name.is_empty()));
    assert!(!normalize(&mut tabs), "a clean list is left alone");
}

// ---- the list ----

#[test]
fn names_are_clean_and_unique() {
    assert_eq!(clean_name("  Hats\n and  "), "Hats  and");
    assert_eq!(clean_name(&"é".repeat(MAX_NAME + 9)).chars().count(), MAX_NAME);
    let mut tabs = Vec::new();
    for _ in 0..3 {
        add(&mut tabs, "Hats", Filters::default(), String::new()).expect("room");
    }
    assert_eq!(
        tabs.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Hats", "Hats 2", "Hats 3"]
    );
    assert_eq!(unique_name(&tabs, "hats"), "hats 4", "the case is ignored");
    let long = "x".repeat(MAX_NAME);
    add(&mut tabs, &long, Filters::default(), String::new()).expect("room");
    let again = unique_name(&tabs, &long);
    assert!(again.chars().count() <= MAX_NAME && again.ends_with(" 2"));
}

#[test]
fn tabs_stop_at_the_limit() {
    let mut tabs = Vec::new();
    for n in 0..MAX_TABS {
        assert!(add(&mut tabs, &format!("Tab {n}"), Filters::default(), String::new()).is_some());
    }
    assert!(add(&mut tabs, "More", Filters::default(), String::new()).is_none());
    let id = tabs[0].id;
    assert!(clone_tab(&mut tabs, id, "Copy").is_none());
    assert_eq!(tabs.len(), MAX_TABS);
    assert!(delete(&mut tabs, id));
    let first = tabs[0].id;
    assert!(clone_tab(&mut tabs, first, "Copy").is_some());
}

#[test]
fn rename_checks_the_name_and_keeps_the_id() {
    let mut tabs = vec![tab(1, "Hats"), tab(2, "Shoes")];
    assert_eq!(rename(&mut tabs, 1, "   "), Err(NameError::Empty));
    assert_eq!(rename(&mut tabs, 1, "SHOES"), Err(NameError::Taken));
    assert_eq!(rename(&mut tabs, 9, "Gloves"), Err(NameError::Missing));
    assert_eq!(rename(&mut tabs, 1, "HATS"), Ok(()), "a tab may change its own case");
    assert_eq!(rename(&mut tabs, 1, "  Caps "), Ok(()));
    assert_eq!((tabs[0].id, tabs[0].name.as_str()), (1, "Caps"));
}

#[test]
fn clone_copies_filters_and_search_right_after_the_tab() {
    let mut tabs = Vec::new();
    let first = add(&mut tabs, "Textures", texture_filters(), "sky".into()).expect("room");
    add(&mut tabs, "Other", Filters::default(), String::new()).expect("room");
    let copy = clone_tab(&mut tabs, first, "Textures copy").expect("room");
    assert_eq!(
        tabs.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Textures", "Textures copy", "Other"]
    );
    assert_ne!(copy, first);
    assert_eq!(tabs[1].filters, tabs[0].filters);
    assert_eq!(tabs[1].search, "sky");
    let second = clone_tab(&mut tabs, first, "Textures copy").expect("room");
    assert_eq!(tabs[1].id, second);
    assert_eq!(tabs[1].name, "Textures copy 2");
    assert_eq!(clone_tab(&mut tabs, 99, "x"), None);
}

#[test]
fn delete_removes_only_that_tab() {
    let mut tabs = vec![tab(1, "A"), tab(2, "B"), tab(3, "C")];
    assert!(delete(&mut tabs, 2));
    assert!(!delete(&mut tabs, 2));
    assert_eq!(tabs.iter().map(|t| t.id).collect::<Vec<_>>(), [1, 3]);
    // ids of deleted tabs are not reused while a later tab exists
    assert_eq!(add(&mut tabs, "D", Filters::default(), String::new()), Some(4));
}

// ---- what a tab lists ----

#[test]
fn a_custom_tab_applies_its_filters_and_search_to_the_demo_inventory() {
    let inv = demo_inventory();
    let everything = listed(&inv, &tab(1, "All"));
    assert!(everything.len() > 5, "the demo inventory has items: {everything:?}");

    let textures = CustomTab {
        filters: texture_filters(),
        ..tab(1, "Textures")
    };
    let only_textures = listed(&inv, &textures);
    assert!(only_textures.contains(&name_of_item(&inv, 8104)));
    assert!(!only_textures.contains(&name_of_item(&inv, 8105)), "the sound is not a texture");
    assert!(only_textures.len() < everything.len());

    let media = CustomTab {
        filters: Filters {
            types: (1 << 0) | (1 << 1),
            ..Default::default()
        },
        ..tab(2, "Media")
    };
    let listed_media = listed(&inv, &media);
    for n in [8104, 8105] {
        assert!(listed_media.contains(&name_of_item(&inv, n)));
    }
    assert!(!listed_media.contains(&name_of_item(&inv, 8100)), "the object is not media");

    let searched = CustomTab {
        search: "sound".into(),
        ..tab(3, "Search")
    };
    assert_eq!(listed(&inv, &searched), [name_of_item(&inv, 8105)]);

    let both = CustomTab {
        filters: texture_filters(),
        search: "sound".into(),
        ..tab(4, "Both")
    };
    assert!(listed(&inv, &both).is_empty(), "filters and search both apply");
}

#[test]
fn a_custom_tab_keeps_advanced_rules() {
    let inv = demo_inventory();
    let filters = Filters {
        smart: Some(smart::SmartQuery::from_quick("sound", SearchField::Name)),
        ..Default::default()
    };
    let rules = CustomTab {
        filters,
        ..tab(1, "Rules")
    };
    assert_eq!(listed(&inv, &rules), [name_of_item(&inv, 8105)]);
}

// ---- the window ----

#[test]
fn selecting_a_custom_tab_loads_and_remembers_its_filters_and_search() {
    let mut prefs = prefs_with(&["Textures", "Notes"]);
    prefs.custom_tabs[0].filters = texture_filters();
    prefs.custom_tabs[0].search = "sky".into();
    let mut st = InventoryUi::default();
    controls::init_filters(&mut st, &prefs);
    st.search = "typed in All".into();

    select(&mut st, &prefs, SLOT_BASE);
    assert_eq!(st.tab, CUSTOM_TAB);
    assert!(selected(&st, &prefs, SLOT_BASE) && !selected(&st, &prefs, SLOT_BASE + 1) && !selected(&st, &prefs, 0));
    assert_eq!(st.filters, texture_filters());
    assert_eq!(st.search, "sky");
    assert!(st.result_view(), "a custom tab lists results");
    assert_eq!(name_of(&st), Some("Textures"));

    // edits are remembered by the tab, in the preferences
    st.filters.permissions = rules::COPY;
    st.search = "sky blue".into();
    sync(&mut st, &mut prefs);
    assert_eq!(prefs.custom_tabs[0].filters.permissions, rules::COPY);
    assert_eq!(prefs.custom_tabs[0].search, "sky blue");

    // another custom tab has its own
    select(&mut st, &prefs, SLOT_BASE + 1);
    assert_eq!(st.tab, CUSTOM_TAB);
    assert_eq!(st.filters, Filters::default());
    assert_eq!(st.search, "");
    assert_eq!(name_of(&st), Some("Notes"));
    st.search = "todo".into();
    sync(&mut st, &mut prefs);

    // leaving for All brings its search back, and the custom tabs keep theirs
    select(&mut st, &prefs, 0);
    assert_eq!((st.tab, active(&st)), (0, None));
    assert_eq!(st.search, "todo", "the search box is shared unless each tab keeps its own");
    st.search = "typed in All".into();
    select(&mut st, &prefs, SLOT_BASE);
    assert_eq!(st.filters.permissions, rules::COPY);
    assert_eq!(st.search, "sky blue");
    select(&mut st, &prefs, SLOT_BASE + 1);
    assert_eq!(st.search, "todo");
    assert_eq!(prefs.custom_tabs[0].search, "sky blue");
}

#[test]
fn custom_tabs_leave_the_built_in_tabs_and_their_defaults_alone() {
    let mut prefs = prefs_with(&["Mine"]);
    prefs.filters_for_mut(2).hours = 70;
    let defaults = prefs.clone();
    let mut st = InventoryUi::default();
    controls::init_filters(&mut st, &prefs);
    set_tab(&mut st, &prefs, 4);
    st.filters.types = 1 << 6;
    select(&mut st, &prefs, SLOT_BASE);
    st.filters.types = 1 << 3;
    sync(&mut st, &mut prefs);
    select(&mut st, &prefs, 4);
    assert_eq!(st.filters.types, 1 << 6, "Favorites keeps its live finder");
    select(&mut st, &prefs, SLOT_BASE);
    assert_eq!(st.filters.types, 1 << 3);
    assert_eq!(prefs.tab_filter_defaults, defaults.tab_filter_defaults);
    assert_eq!(prefs.filter_defaults, defaults.filter_defaults);
}

#[test]
fn advanced_rules_and_their_text_are_remembered() {
    let mut prefs = prefs_with(&["Rules"]);
    let mut st = InventoryUi::default();
    select(&mut st, &prefs, SLOT_BASE);
    st.search = "hat".into();
    st.enter_advanced(SearchField::Name);
    sync(&mut st, &mut prefs);
    assert!(prefs.custom_tabs[0].filters.smart.is_some());
    assert_eq!(prefs.custom_tabs[0].search, "hat", "the quick text waits with the rules");

    let mut other = InventoryUi::default();
    select(&mut other, &prefs, SLOT_BASE);
    assert!(other.advanced && other.filters.smart.is_some());
    other.leave_advanced();
    assert_eq!(other.search, "hat");
}

#[test]
fn temporary_modes_are_not_kept_by_the_tab() {
    let mut prefs = prefs_with(&["Mine"]);
    prefs.custom_tabs[0].filters = texture_filters();
    let before = prefs.clone();
    let mut st = InventoryUi::default();
    select(&mut st, &prefs, SLOT_BASE);
    // Find all links on this tab goes to All, which leaves the tab as it is
    st.find_links(Uuid::from_u128(1));
    sync(&mut st, &mut prefs);
    assert_eq!(st.tab, 0);
    assert_eq!(prefs, before);
    // a similar search set aside on the tab itself is not written back
    select(&mut st, &prefs, SLOT_BASE);
    st.saved_filters = Some(st.filters.clone());
    st.filters.types = 1 << 6;
    sync(&mut st, &mut prefs);
    assert_eq!(prefs, before);
}

#[test]
fn a_tab_deleted_elsewhere_sends_the_window_back_to_all() {
    let mut prefs = prefs_with(&["A", "B"]);
    let mut st = InventoryUi::default();
    select(&mut st, &prefs, SLOT_BASE + 1);
    let id = prefs.custom_tabs[1].id;
    assert!(delete(&mut prefs.custom_tabs, id));
    sync(&mut st, &mut prefs);
    assert_eq!(st.tab, 0);
    assert_eq!(prefs.custom_tabs.len(), 1);
}

#[test]
fn deleting_the_shown_tab_goes_back_to_all() {
    let mut prefs = prefs_with(&["A", "B"]);
    let mut st = InventoryUi::default();
    select(&mut st, &prefs, SLOT_BASE);
    let (first, second) = (prefs.custom_tabs[0].id, prefs.custom_tabs[1].id);
    delete_and_leave(&mut st, &mut prefs, second);
    assert_eq!(active(&st), Some(first), "another tab's deletion changes nothing here");
    delete_and_leave(&mut st, &mut prefs, first);
    assert_eq!((st.tab, active(&st)), (0, None));
    assert!(prefs.custom_tabs.is_empty());
}

#[test]
fn cloning_a_built_in_tab_makes_a_custom_one_with_what_it_shows() {
    let mut prefs = InventoryPreferences::default();
    let mut st = InventoryUi::default();
    controls::init_filters(&mut st, &prefs);
    st.filters.types = 1 << 6;
    st.search = "box".into();
    clone_builtin(&mut st, &mut prefs, 0);
    assert_eq!(prefs.custom_tabs.len(), 1);
    assert_eq!(prefs.custom_tabs[0].name, "Inventory copy");
    assert_eq!(prefs.custom_tabs[0].filters.types, 1 << 6);
    assert_eq!(prefs.custom_tabs[0].search, "box");
    assert_eq!(
        (st.tab, active(&st)),
        (CUSTOM_TAB, Some(prefs.custom_tabs[0].id)),
        "the clone is shown"
    );
    // All itself is untouched, and comes back as it was
    select(&mut st, &prefs, 0);
    assert_eq!(st.filters.types, 1 << 6);

    // Recent: its period becomes an age; Worn: its rule
    prefs.recent_hours = 48;
    clone_builtin(&mut st, &mut prefs, 2);
    let recent = &prefs.custom_tabs[1];
    assert_eq!((recent.filters.age, recent.filters.hours), (view::Age::Newer, 48));
    prefs.recent_hours = 0;
    clone_builtin(&mut st, &mut prefs, 2);
    assert_eq!(prefs.custom_tabs[2].filters.age, view::Age::SinceLogout);
    set_tab(&mut st, &prefs, 0);
    st.filters = Filters::default();
    st.search.clear();
    clone_builtin(&mut st, &mut prefs, 3);
    let worn = prefs.custom_tabs.last().expect("worn clone");
    assert!(worn.filters.smart.as_ref().is_some_and(|q| q.uses(smart::Field::Worn)));
}

#[test]
fn cloning_a_custom_tab_shows_the_copy() {
    let mut prefs = prefs_with(&["Hats"]);
    prefs.custom_tabs[0].filters = texture_filters();
    let mut st = InventoryUi::default();
    select(&mut st, &prefs, SLOT_BASE);
    st.search = "fedora".into();
    let id = prefs.custom_tabs[0].id;
    clone_custom(&mut st, &mut prefs, id);
    assert_eq!(
        prefs.custom_tabs.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Hats", "Hats copy"]
    );
    assert_eq!(prefs.custom_tabs[0].search, "fedora", "the original keeps what was typed");
    assert_eq!(prefs.custom_tabs[1].search, "fedora");
    assert_eq!(active(&st), Some(prefs.custom_tabs[1].id));
    assert_eq!(st.filters, texture_filters());
}

#[test]
fn only_inventory_tabs_can_be_cloned_from_the_built_in_ones() {
    assert!([0, 2, 3, 4].into_iter().all(has_menu));
    assert!(!has_menu(super::RECEIVED_TAB));
    assert!(has_menu(SLOT_BASE) && has_menu(SLOT_BASE + 3));
}

// ---- the tab strips ----

fn run_window<'a>(
    frames: &'a mut Frames,
    classic: bool,
    inv: &'a mut Inventory,
    st: &'a mut InventoryUi,
    prefs: &'a mut InventoryPreferences,
    events: Vec<egui::Event>,
) -> Vec<crate::ui::widgets::elide_tests::Text> {
    let p = crate::theme::Theme::default().palette();
    frames.run(events, |ui| {
        ui.set_max_size(Vec2::new(420.0, 700.0));
        if classic {
            inventory_contents(ui, &p, &Icons::default(), inv, st, prefs, &Facts::default(), &mut vec![]);
        } else {
            aurora::contents(
                ui,
                &p,
                &Icons::default(),
                inv,
                st,
                prefs,
                &Facts::default(),
                &std::collections::HashMap::new(),
                &mut vec![],
            );
        }
        controls::dialogs(ui.ctx(), &p, st, prefs);
    })
}

fn click(pos: egui::Pos2, button: egui::PointerButton) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos,
            button,
            pressed: false,
            modifiers: Default::default(),
        },
    ]
}

#[test]
fn both_windows_show_custom_tabs_select_them_and_offer_customize() {
    for classic in [true, false] {
        let mut frames = Frames::new();
        crate::theme::Theme::default().apply(&frames.ctx, 1.0);
        let mut inv = demo_inventory();
        let mut prefs = prefs_with(&["Textures", "Notes"]);
        prefs.custom_tabs[0].filters = texture_filters();
        let mut st = InventoryUi::default();
        for _ in 0..3 {
            run_window(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        }
        let customize = aurora_i18n::tr!("inventory-custom-tab-customize");
        let tip_button_shown = |texts: &[crate::ui::widgets::elide_tests::Text]| texts.iter().any(|t| t.source == customize);

        let texts = run_window(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        assert!(!tip_button_shown(&texts), "classic={classic}: no Customize tab… on a built-in tab");
        let label = texts
            .iter()
            .find(|t| t.source == "Textures")
            .unwrap_or_else(|| panic!("classic={classic}: custom tab label"));
        assert!(texts.iter().any(|t| t.source == "Notes"));
        let pos = label.rect.center();
        run_window(
            &mut frames,
            classic,
            &mut inv,
            &mut st,
            &mut prefs,
            click(pos, egui::PointerButton::Primary),
        );
        let texts = run_window(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        assert_eq!(st.tab, CUSTOM_TAB, "classic={classic}");
        assert_eq!(st.filters, texture_filters());
        if classic {
            assert!(tip_button_shown(&texts), "classic: Customize tab… beside the search box");
        }

        // the filter editor edits the tab: a change goes to its entry
        customize_open(&mut st);
        run_window(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        st.filters.permissions = rules::COPY;
        run_window(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        assert_eq!(prefs.custom_tabs[0].filters.permissions, rules::COPY, "classic={classic}");
        let texts = run_window(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        let title = aurora_i18n::tr!("inventory-custom-tab-filters-title", name = "Textures");
        assert!(
            texts.iter().any(|t| t.source == title),
            "classic={classic}: the editor is named after the tab"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t.source == aurora_i18n::tr!("ui-inventory-controls-keep-by-default")),
            "classic={classic}: no keep by default on a custom tab"
        );
    }
}

fn entry_label(entry: &str) -> &'static str {
    match entry {
        "customize" => aurora_i18n::tr!("inventory-custom-tab-customize"),
        "rename" => aurora_i18n::tr!("inventory-custom-tab-rename"),
        "clone" => aurora_i18n::tr!("inventory-custom-tab-clone"),
        _ => aurora_i18n::tr!("inventory-custom-tab-delete"),
    }
}

fn customize_open(st: &mut InventoryUi) {
    st.filters_open = false;
    customize(st);
    assert!(st.filters_open);
}

#[test]
fn the_menu_of_a_tab_customizes_renames_clones_and_deletes() {
    for classic in [true, false] {
        let mut frames = Frames::new();
        crate::theme::Theme::default().apply(&frames.ctx, 1.0);
        let mut inv = demo_inventory();
        let mut prefs = prefs_with(&["Hats", "Shoes"]);
        let mut st = InventoryUi::default();
        for _ in 0..3 {
            run_window(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        }
        let open_menu = |frames: &mut Frames, inv: &mut Inventory, st: &mut InventoryUi, prefs: &mut InventoryPreferences, name: &str| {
            let texts = run_window(frames, classic, inv, st, prefs, vec![]);
            let pos = texts
                .iter()
                .find(|t| t.source == name)
                .unwrap_or_else(|| panic!("classic={classic}: tab {name}"))
                .rect
                .center();
            run_window(frames, classic, inv, st, prefs, click(pos, egui::PointerButton::Secondary));
            run_window(frames, classic, inv, st, prefs, vec![])
        };
        let choose = |frames: &mut Frames,
                      inv: &mut Inventory,
                      st: &mut InventoryUi,
                      prefs: &mut InventoryPreferences,
                      texts: Vec<crate::ui::widgets::elide_tests::Text>,
                      label: &str| {
            let pos = texts
                .iter()
                .find(|t| t.source == label)
                .unwrap_or_else(|| panic!("classic={classic}: menu entry {label}"))
                .rect
                .center();
            run_window(frames, classic, inv, st, prefs, click(pos, egui::PointerButton::Primary));
            run_window(frames, classic, inv, st, prefs, vec![]);
            run_window(frames, classic, inv, st, prefs, vec![])
        };

        // Customize on an inactive tab selects it and opens the editor
        let texts = open_menu(&mut frames, &mut inv, &mut st, &mut prefs, "Shoes");
        for entry in ["customize", "rename", "clone", "delete"] {
            let label = entry_label(entry);
            assert!(texts.iter().any(|t| t.source == label), "classic={classic}: {entry}");
        }
        choose(
            &mut frames,
            &mut inv,
            &mut st,
            &mut prefs,
            texts,
            aurora_i18n::tr!("inventory-custom-tab-customize"),
        );
        assert_eq!((st.tab, name_of(&st)), (CUSTOM_TAB, Some("Shoes")), "classic={classic}");
        assert!(st.filters_open, "classic={classic}");

        // Clone
        let texts = open_menu(&mut frames, &mut inv, &mut st, &mut prefs, "Hats");
        choose(
            &mut frames,
            &mut inv,
            &mut st,
            &mut prefs,
            texts,
            aurora_i18n::tr!("inventory-custom-tab-clone"),
        );
        assert_eq!(
            prefs.custom_tabs.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["Hats", "Hats copy", "Shoes"],
            "classic={classic}"
        );
        assert_eq!(name_of(&st), Some("Hats copy"));

        // Rename opens its dialog
        let texts = open_menu(&mut frames, &mut inv, &mut st, &mut prefs, "Shoes");
        let texts = choose(
            &mut frames,
            &mut inv,
            &mut st,
            &mut prefs,
            texts,
            aurora_i18n::tr!("inventory-custom-tab-rename"),
        );
        assert!(
            texts
                .iter()
                .any(|t| t.source == aurora_i18n::tr!("inventory-custom-tab-rename-title")),
            "classic={classic}: rename dialog"
        );
        st.custom_tabs.rename = None;

        // Delete asks first, and Cancel keeps the tab
        let texts = open_menu(&mut frames, &mut inv, &mut st, &mut prefs, "Shoes");
        let texts = choose(
            &mut frames,
            &mut inv,
            &mut st,
            &mut prefs,
            texts,
            aurora_i18n::tr!("inventory-custom-tab-delete"),
        );
        assert!(
            texts
                .iter()
                .any(|t| t.source == aurora_i18n::tr!("inventory-custom-tab-delete-question", name = "Shoes")),
            "classic={classic}: delete question"
        );
        assert_eq!(prefs.custom_tabs.len(), 3);
        assert!(st.custom_tabs.delete.is_some());
        // confirming (the dialog's button is exercised by its state here)
        let (id, _) = st.custom_tabs.delete.take().expect("pending");
        delete_and_leave(&mut st, &mut prefs, id);
        assert_eq!(prefs.custom_tabs.len(), 2);
        assert_eq!(prefs.custom_tabs[1].name, "Hats copy");
    }
}

#[test]
fn the_built_in_tabs_offer_clone_only() {
    // Legacy: right click "Recent"
    let mut frames = Frames::new();
    crate::theme::Theme::default().apply(&frames.ctx, 1.0);
    let mut inv = demo_inventory();
    let mut prefs = InventoryPreferences::default();
    let mut st = InventoryUi::default();
    for _ in 0..3 {
        run_window(&mut frames, true, &mut inv, &mut st, &mut prefs, vec![]);
    }
    let texts = run_window(&mut frames, true, &mut inv, &mut st, &mut prefs, vec![]);
    let pos = texts
        .iter()
        .find(|t| t.source == aurora_i18n::tr!("common-recent"))
        .expect("Recent")
        .rect
        .center();
    run_window(
        &mut frames,
        true,
        &mut inv,
        &mut st,
        &mut prefs,
        click(pos, egui::PointerButton::Secondary),
    );
    let texts = run_window(&mut frames, true, &mut inv, &mut st, &mut prefs, vec![]);
    assert!(texts.iter().any(|t| t.source == aurora_i18n::tr!("inventory-custom-tab-clone")));
    for entry in ["customize", "rename", "delete"] {
        let label = entry_label(entry);
        assert!(!texts.iter().any(|t| t.source == label), "{entry} is for custom tabs");
    }
}

#[test]
fn the_rename_dialog_renames_and_refuses_bad_names() {
    let mut frames = Frames::new();
    crate::theme::Theme::default().apply(&frames.ctx, 1.0);
    let p = crate::theme::Theme::default().palette();
    let mut prefs = prefs_with(&["Hats", "Shoes"]);
    let mut st = InventoryUi::default();
    select(&mut st, &prefs, SLOT_BASE);
    let id = prefs.custom_tabs[0].id;
    st.custom_tabs.rename = Some(Rename {
        id,
        text: "Shoes".into(),
        focus: false,
    });
    for _ in 0..3 {
        frames.run(vec![], |ui| frame(ui.ctx(), &p, &mut st, &mut prefs));
    }
    let texts = frames.run(vec![], |ui| frame(ui.ctx(), &p, &mut st, &mut prefs));
    assert!(
        texts
            .iter()
            .any(|t| t.source == aurora_i18n::tr!("inventory-custom-tab-name-taken"))
    );
    // the name in the dialog is what it keeps
    st.custom_tabs.rename.as_mut().expect("still open").text = "  Caps ".into();
    let accept = frames
        .run(vec![], |ui| frame(ui.ctx(), &p, &mut st, &mut prefs))
        .into_iter()
        .find(|t| t.source == aurora_i18n::tr!("common-rename"))
        .expect("Rename button")
        .rect
        .center();
    frames.run(click(accept, egui::PointerButton::Primary), |ui| {
        frame(ui.ctx(), &p, &mut st, &mut prefs)
    });
    assert_eq!(prefs.custom_tabs[0].name, "Caps");
    assert!(st.custom_tabs.rename.is_none());
    assert_eq!(name_of(&st), Some("Caps"), "the title of the shown tab follows");
}
