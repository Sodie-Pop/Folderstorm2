use super::super::folders::tests::{BODY, BOOTS, CLOTHING, DELETED, OLD, SHIRT, account, id, inventory};
use super::*;
use crate::ui::widgets::elide_tests::{Frames, Text};

fn facts() -> Facts {
    Facts {
        agent: account(),
        ..Default::default()
    }
}

fn run(
    frames: &mut Frames,
    classic: bool,
    inv: &mut Inventory,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    events: Vec<egui::Event>,
) -> Vec<Text> {
    let p = crate::theme::Theme::default().palette();
    let facts = facts();
    frames.run(events, |ui| {
        ui.set_max_size(Vec2::new(420.0, 700.0));
        if classic {
            super::super::super::inventory_contents(ui, &p, &Icons::default(), inv, st, prefs, &facts, &mut vec![]);
        } else {
            super::super::super::aurora::contents(
                ui,
                &p,
                &Icons::default(),
                inv,
                st,
                prefs,
                &facts,
                &std::collections::HashMap::new(),
                &mut vec![],
            );
        }
        super::super::super::controls::dialogs(ui.ctx(), &p, st, prefs);
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

fn has(texts: &[Text], source: &str) -> bool {
    texts.iter().any(|t| t.source == source)
}

/// An inventory row: its name, maybe followed by its rights.
fn row(texts: &[Text], name: &str) -> bool {
    texts.iter().any(|t| t.source == name || t.source.starts_with(&format!("{name} ")))
}

fn at(texts: &[Text], source: &str) -> egui::Pos2 {
    texts
        .iter()
        .find(|t| t.source == source)
        .unwrap_or_else(|| panic!("{source} is not drawn"))
        .rect
        .center()
}

fn scoped_prefs(folders: &[u128]) -> InventoryPreferences {
    let mut prefs = InventoryPreferences::default();
    add(&mut prefs.custom_tabs, "Wardrobe", Filters::default(), String::new()).expect("room");
    folders::set(&mut prefs.custom_tabs[0], account(), folders.iter().map(|n| id(*n)).collect());
    prefs
}

fn saved(prefs: &InventoryPreferences) -> Vec<Uuid> {
    folders::of(&prefs.custom_tabs[0], account()).to_vec()
}

#[test]
fn the_customize_dialog_lists_and_edits_the_tabs_folders() {
    for classic in [true, false] {
        let mut frames = Frames::new();
        crate::theme::Theme::default().apply(&frames.ctx, 1.0);
        let mut inv = inventory();
        let mut prefs = scoped_prefs(&[CLOTHING, DELETED]);
        let mut st = InventoryUi::default();
        select(&mut st, &prefs, SLOT_BASE);
        customize(&mut st);
        for _ in 0..3 {
            run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        }
        let texts = run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        assert!(has(&texts, aurora_i18n::tr!("inventory-folder-tab-heading")), "classic={classic}");
        let path = super::super::super::inventory_path(&inv, id(CLOTHING));
        assert!(has(&texts, &path), "classic={classic}: the folder by its path");
        assert!(has(&texts, aurora_i18n::tr!("inventory-folder-tab-deleted-folder")));
        assert!(has(&texts, aurora_i18n::tr!("inventory-folder-tab-status-missing")));
        for label in [
            aurora_i18n::tr!("inventory-folder-tab-add"),
            aurora_i18n::tr!("inventory-folder-tab-replace"),
            aurora_i18n::tr!("inventory-folder-tab-clear"),
        ] {
            assert!(has(&texts, label), "classic={classic}: {label}");
        }
        // the list shows the scope only, and says that a folder is gone
        assert!(row(&texts, "Shirt") && !row(&texts, "Unrelated"), "classic={classic}");
        let note = aurora_i18n::tr!("inventory-folder-tab-note-gone", count = 1);
        assert!(has(&texts, &note), "classic={classic}: the note above the list");

        // Add selected folders
        st.selection = [id(BODY)].into();
        let texts = run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        let add = at(&texts, aurora_i18n::tr!("inventory-folder-tab-add"));
        run(
            &mut frames,
            classic,
            &mut inv,
            &mut st,
            &mut prefs,
            click(add, egui::PointerButton::Primary),
        );
        assert_eq!(saved(&prefs), [id(CLOTHING), id(DELETED), id(BODY)], "classic={classic}");
        let texts = run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        assert!(row(&texts, "Skin"), "classic={classic}: the added folder is listed");

        // the note forgets the deleted folder
        let texts = run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        let remove = at(&texts, aurora_i18n::tr!("inventory-folder-tab-note-remove"));
        run(
            &mut frames,
            classic,
            &mut inv,
            &mut st,
            &mut prefs,
            click(remove, egui::PointerButton::Primary),
        );
        assert_eq!(saved(&prefs), [id(CLOTHING), id(BODY)], "classic={classic}");
        let texts = run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        assert!(!has(&texts, &note), "classic={classic}: no note once none is gone");

        // an item in the selection: nothing to add
        st.selection = [id(BODY), id(SHIRT)].into();
        let texts = run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        let replace = at(&texts, aurora_i18n::tr!("inventory-folder-tab-replace"));
        run(
            &mut frames,
            classic,
            &mut inv,
            &mut st,
            &mut prefs,
            click(replace, egui::PointerButton::Primary),
        );
        assert_eq!(saved(&prefs), [id(CLOTHING), id(BODY)], "classic={classic}: disabled");

        // Show all folders
        let texts = run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        let clear = at(&texts, aurora_i18n::tr!("inventory-folder-tab-clear"));
        run(
            &mut frames,
            classic,
            &mut inv,
            &mut st,
            &mut prefs,
            click(clear, egui::PointerButton::Primary),
        );
        assert!(saved(&prefs).is_empty(), "classic={classic}");
        run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        let texts = run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        assert!(has(&texts, aurora_i18n::tr!("inventory-folder-tab-all")), "classic={classic}");
        assert!(row(&texts, "Unrelated"), "classic={classic}: the whole inventory again");
    }
}

#[test]
fn a_tabs_menu_scopes_it_to_the_selected_folders() {
    for classic in [true, false] {
        let mut frames = Frames::new();
        crate::theme::Theme::default().apply(&frames.ctx, 1.0);
        let mut inv = inventory();
        let mut prefs = scoped_prefs(&[]);
        let mut st = InventoryUi::default();
        for _ in 0..3 {
            run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);
        }
        let open_menu = |frames: &mut Frames, inv: &mut Inventory, st: &mut InventoryUi, prefs: &mut InventoryPreferences| {
            let texts = run(frames, classic, inv, st, prefs, vec![]);
            let tab = at(&texts, "Wardrobe");
            run(frames, classic, inv, st, prefs, click(tab, egui::PointerButton::Secondary));
            run(frames, classic, inv, st, prefs, vec![])
        };
        let choose = |frames: &mut Frames,
                      inv: &mut Inventory,
                      st: &mut InventoryUi,
                      prefs: &mut InventoryPreferences,
                      texts: &[Text],
                      label: &str| {
            run(
                frames,
                classic,
                inv,
                st,
                prefs,
                click(at(texts, label), egui::PointerButton::Primary),
            );
            run(frames, classic, inv, st, prefs, vec![]);
        };

        st.selection = [id(BOOTS), id(BODY)].into();
        let texts = open_menu(&mut frames, &mut inv, &mut st, &mut prefs);
        for label in [
            aurora_i18n::tr!("inventory-folder-tab-add"),
            aurora_i18n::tr!("inventory-folder-tab-replace"),
            aurora_i18n::tr!("inventory-folder-tab-clear"),
        ] {
            assert!(has(&texts, label), "classic={classic}: {label}");
        }
        choose(
            &mut frames,
            &mut inv,
            &mut st,
            &mut prefs,
            &texts,
            aurora_i18n::tr!("inventory-folder-tab-replace"),
        );
        assert_eq!(saved(&prefs), [id(BODY), id(BOOTS)], "classic={classic}");

        st.selection = [id(CLOTHING)].into();
        let texts = open_menu(&mut frames, &mut inv, &mut st, &mut prefs);
        choose(
            &mut frames,
            &mut inv,
            &mut st,
            &mut prefs,
            &texts,
            aurora_i18n::tr!("inventory-folder-tab-add"),
        );
        assert_eq!(saved(&prefs), [id(BODY), id(BOOTS), id(CLOTHING)], "classic={classic}");

        // a folder in the Trash cannot be chosen
        st.selection = [id(OLD)].into();
        let texts = open_menu(&mut frames, &mut inv, &mut st, &mut prefs);
        choose(
            &mut frames,
            &mut inv,
            &mut st,
            &mut prefs,
            &texts,
            aurora_i18n::tr!("inventory-folder-tab-replace"),
        );
        assert_eq!(saved(&prefs).len(), 3, "classic={classic}: disabled");
        // a disabled entry leaves the menu open: Escape closes it
        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![escape]);
        run(&mut frames, classic, &mut inv, &mut st, &mut prefs, vec![]);

        let texts = open_menu(&mut frames, &mut inv, &mut st, &mut prefs);
        choose(
            &mut frames,
            &mut inv,
            &mut st,
            &mut prefs,
            &texts,
            aurora_i18n::tr!("inventory-folder-tab-clear"),
        );
        assert!(saved(&prefs).is_empty(), "classic={classic}");
    }
}

#[test]
fn save_as_tab_makes_a_scoped_tab_named_after_the_first_folder() {
    let inv = inventory();
    let mut prefs = InventoryPreferences::default();
    let mut st = InventoryUi::default();
    let made = save_as_tab(&inv, &mut st, &mut prefs, account(), &[id(CLOTHING), id(BODY)]).expect("a new tab");
    let tab = &prefs.custom_tabs[0];
    assert_eq!((tab.id, tab.name.as_str()), (made, "Body"));
    assert_eq!(folders::of(tab, account()), [id(BODY), id(CLOTHING)]);
    assert_eq!(tab.filters, Filters::default());
    assert_eq!((st.tab, name_of(&st)), (CUSTOM_TAB, Some("Body")), "shown");
    assert_eq!(
        st.custom_tabs.rename.as_ref().map(|r| (r.id, r.text.as_str())),
        Some((made, "Body"))
    );
    save_as_tab(&inv, &mut st, &mut prefs, account(), &[id(BODY)]).expect("another");
    assert_eq!(prefs.custom_tabs[1].name, "Body 2");

    // what cannot become a tab
    assert!(!can_save(&inv, &st, &prefs, account(), &[id(SHIRT)]), "an item");
    assert!(!can_save(&inv, &st, &prefs, account(), &[id(OLD)]), "in the Trash");
    assert!(!can_save(&inv, &st, &prefs, Uuid::nil(), &[id(BODY)]), "no account");
    let folder_window = InventoryUi {
        scope: id(CLOTHING),
        ..Default::default()
    };
    assert!(!can_save(&inv, &folder_window, &prefs, account(), &[id(BODY)]), "a folder window");
    while prefs.custom_tabs.len() < MAX_TABS {
        add(&mut prefs.custom_tabs, "More", Filters::default(), String::new());
    }
    assert!(!can_save(&inv, &st, &prefs, account(), &[id(BODY)]), "no room");
    assert_eq!(save_as_tab(&inv, &mut st, &mut prefs, account(), &[id(BODY)]), None);
}

#[test]
fn the_folder_menu_offers_save_as_inventory_tab_for_folders_only() {
    let inv = inventory();
    let prefs = InventoryPreferences::default();
    let mut st = InventoryUi {
        selection: [id(CLOTHING), id(BODY)].into(),
        ..Default::default()
    };
    let entry = |st: &InventoryUi, at: u128| {
        super::super::super::context_catalog::build(&inv, id(at), st, &prefs, &facts())
            .entries
            .into_iter()
            .find(|e| e.id == "folder-tab")
    };
    let offered = entry(&st, CLOTHING).expect("on a folder");
    assert!(offered.enabled);
    assert_eq!(offered.label, aurora_i18n::tr!("inventory-folder-tab-save"));
    st.selection = [id(SHIRT)].into();
    assert!(entry(&st, SHIRT).is_none(), "not on an item");
    st.selection = [id(CLOTHING), id(SHIRT)].into();
    assert!(!entry(&st, CLOTHING).expect("on a folder").enabled, "an item in the selection");
}

#[test]
fn a_tab_scoped_on_another_account_shows_everything_with_a_note() {
    let mut frames = Frames::new();
    crate::theme::Theme::default().apply(&frames.ctx, 1.0);
    let mut inv = inventory();
    let mut prefs = scoped_prefs(&[]);
    folders::set(&mut prefs.custom_tabs[0], id(8), vec![id(BODY)]);
    let mut st = InventoryUi::default();
    select(&mut st, &prefs, SLOT_BASE);
    for _ in 0..3 {
        run(&mut frames, true, &mut inv, &mut st, &mut prefs, vec![]);
    }
    let texts = run(&mut frames, true, &mut inv, &mut st, &mut prefs, vec![]);
    assert!(has(&texts, aurora_i18n::tr!("inventory-folder-tab-elsewhere")));
    assert!(row(&texts, "Unrelated") && row(&texts, "Shirt"));
}
