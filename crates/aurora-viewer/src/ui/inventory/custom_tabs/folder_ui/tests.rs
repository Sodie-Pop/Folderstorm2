use super::super::folders::tests::{BODY, BOOTS, CLOTHING, DELETED, LIBRARY_CLOTHES, OLD, OTHER, SHIRT, TRASH, account, id, inventory};
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

fn can_save(inv: &Inventory, st: &InventoryUi, prefs: &InventoryPreferences, account: Uuid, ids: &[Uuid]) -> bool {
    offer(inv, st, prefs, account, ids).create.is_ok()
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

fn catalog(inv: &Inventory, st: &InventoryUi, prefs: &InventoryPreferences, at: u128) -> super::super::super::context_catalog::Catalog {
    super::super::super::context_catalog::build(inv, id(at), st, prefs, &facts())
}

fn entry(catalog: &super::super::super::context_catalog::Catalog, entry: &str) -> Option<crate::ui::inventory::menu_layout::Entry> {
    catalog.entries.iter().find(|e| e.id == entry).cloned()
}

#[test]
fn the_folder_menu_offers_create_tab_and_add_to_for_folders_only() {
    let inv = inventory();
    let mut prefs = scoped_prefs(&[CLOTHING]);
    add(&mut prefs.custom_tabs, "Looks", Filters::default(), String::new()).expect("room");
    let (wardrobe, looks) = (prefs.custom_tabs[0].id, prefs.custom_tabs[1].id);
    let mut st = InventoryUi {
        selection: [id(CLOTHING), id(BODY)].into(),
        ..Default::default()
    };
    let c = catalog(&inv, &st, &prefs, CLOTHING);
    let create = entry(&c, "folder-tab").expect("on a folder");
    assert!(create.enabled);
    assert_eq!(create.label, aurora_i18n::tr!("inventory-folder-tab-create"));
    let group = entry(&c, "folder-tab-add").expect("Add to");
    assert!(group.enabled && group.group());
    assert_eq!(group.label, aurora_i18n::tr!("inventory-folder-tab-add-to"));
    // every custom tab by name, then New tab…, inside Add to
    let inside: Vec<_> = c
        .entries
        .iter()
        .filter(|e| e.zone == "children:folder-tab-add" && !e.is_separator())
        .map(|e| (e.id.clone(), e.label.clone(), e.enabled))
        .collect();
    assert_eq!(
        inside,
        [
            (format!("folder-tab-add:{wardrobe}"), "Wardrobe".to_owned(), true),
            (format!("folder-tab-add:{looks}"), "Looks".to_owned(), true),
            (
                "folder-tab-add-new".to_owned(),
                aurora_i18n::tr!("inventory-folder-tab-new").to_owned(),
                true
            ),
        ]
    );
    assert!(c.help.is_empty(), "nothing greyed, nothing to explain");

    // a tab that has every selected folder: a check mark, and a tooltip
    st.selection = [id(CLOTHING)].into();
    let c = catalog(&inv, &st, &prefs, CLOTHING);
    let has = entry(&c, &format!("folder-tab-add:{wardrobe}")).expect("listed");
    assert_eq!((has.enabled, has.checked), (true, Some(true)));
    assert_eq!(c.help[&has.id], aurora_i18n::tr!("inventory-folder-tab-has-tip"));
    assert_eq!(entry(&c, &format!("folder-tab-add:{looks}")).expect("listed").checked, None);

    // not on an item; greyed, with the reason, when an item is selected too
    st.selection = [id(SHIRT)].into();
    let c = catalog(&inv, &st, &prefs, SHIRT);
    assert!(
        entry(&c, "folder-tab").is_none() && entry(&c, "folder-tab-add").is_none(),
        "not on an item"
    );
    st.selection = [id(CLOTHING), id(SHIRT)].into();
    let c = catalog(&inv, &st, &prefs, CLOTHING);
    let why = aurora_i18n::tr!("inventory-folder-tab-why-item");
    assert!(!entry(&c, "folder-tab").expect("on a folder").enabled);
    assert!(!entry(&c, "folder-tab-add").expect("on a folder").enabled);
    assert_eq!((c.help["folder-tab"].as_str(), c.help["folder-tab-add"].as_str()), (why, why));

    // the Trash, the Library, the root: hidden (the Trash's own menu is
    // short) or greyed with the reason
    let mut greyed = 0;
    for refused in [OLD, TRASH, LIBRARY_CLOTHES, 1] {
        st.selection = [id(refused)].into();
        let c = catalog(&inv, &st, &prefs, refused);
        let Some(add_to) = entry(&c, "folder-tab-add") else {
            continue;
        };
        greyed += 1;
        assert!(!add_to.enabled, "{refused}");
        assert_eq!(
            c.help["folder-tab-add"],
            aurora_i18n::tr!("inventory-folder-tab-why-refused"),
            "{refused}"
        );
    }
    assert!(greyed >= 2, "the Library and the root show it greyed");

    // a full tab is greyed; no room for another tab greys Create tab only
    let full: Vec<Uuid> = (0..MAX_FOLDERS as u128).map(|n| id(1000 + n)).collect();
    folders::set(&mut prefs.custom_tabs[1], account(), full);
    while prefs.custom_tabs.len() < MAX_TABS {
        add(&mut prefs.custom_tabs, "More", Filters::default(), String::new());
    }
    st.selection = [id(BODY)].into();
    let c = catalog(&inv, &st, &prefs, BODY);
    assert!(!entry(&c, &format!("folder-tab-add:{looks}")).expect("listed").enabled);
    assert!(entry(&c, &format!("folder-tab-add:{wardrobe}")).expect("listed").enabled);
    assert!(!entry(&c, "folder-tab").expect("on a folder").enabled);
    assert!(!entry(&c, "folder-tab-add-new").expect("listed").enabled);
    let no_room = aurora_i18n::tr!("inventory-custom-tab-limit", count = MAX_TABS);
    assert_eq!(c.help["folder-tab"], no_room);
    assert_eq!(
        c.help[&format!("folder-tab-add:{looks}")],
        aurora_i18n::tr!("inventory-folder-tab-limit", count = MAX_FOLDERS)
    );
}

#[test]
fn add_to_tab_adds_once_and_says_so() {
    let inv = inventory();
    let mut prefs = scoped_prefs(&[CLOTHING]);
    let wardrobe = prefs.custom_tabs[0].id;
    let mut st = InventoryUi::default();
    add_to_tab(&inv, &mut st, &mut prefs, account(), wardrobe, &[id(BODY)]);
    assert_eq!(saved(&prefs), [id(CLOTHING), id(BODY)]);
    assert_eq!(
        st.message,
        aurora_i18n::tr!("inventory-folder-tab-added-one", folder = "Body", tab = "Wardrobe")
    );
    assert_eq!(st.tab, 0, "the tab shown stays");
    add_to_tab(&inv, &mut st, &mut prefs, account(), wardrobe, &[id(BODY)]);
    assert_eq!(saved(&prefs), [id(CLOTHING), id(BODY)], "not repeated");
    assert_eq!(
        st.message,
        aurora_i18n::tr!("inventory-folder-tab-already-one", folder = "Body", tab = "Wardrobe")
    );
    add_to_tab(&inv, &mut st, &mut prefs, account(), wardrobe, &[id(BODY), id(OTHER)]);
    assert_eq!(saved(&prefs), [id(CLOTHING), id(BODY), id(OTHER)]);
    let added = aurora_i18n::tr!("inventory-folder-tab-added-many", count = 1, tab = "Wardrobe");
    let already = aurora_i18n::tr!("inventory-folder-tab-added-already", count = 1);
    assert_eq!(st.message, format!("{added} {already}"));
    add_to_tab(&inv, &mut st, &mut prefs, account(), wardrobe, &[id(BODY), id(OTHER)]);
    assert_eq!(st.message, aurora_i18n::tr!("inventory-folder-tab-already-many", tab = "Wardrobe"));
    add_to_tab(&inv, &mut st, &mut prefs, account(), wardrobe, &[id(OLD)]);
    assert_eq!(st.message, aurora_i18n::tr!("inventory-folder-tab-why-refused"));
    assert_eq!(saved(&prefs).len(), 3);
}

// ---- Through the window: the folder menu, multi-select, drag ----

fn modifiers(ctrl: bool, shift: bool) -> egui::Modifiers {
    egui::Modifiers {
        ctrl,
        command: ctrl,
        shift,
        ..Default::default()
    }
}

/// A click with modifiers held (they reach `ui.input` through the event).
fn click_with(pos: egui::Pos2, button: egui::PointerButton, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    let mut events = vec![egui::Event::ModifiersChanged(modifiers), egui::Event::PointerMoved(pos)];
    for pressed in [true, false] {
        events.push(egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        });
    }
    events
}

/// The middle of a folder's row, as the list recorded it (`drag::record_row`).
fn row_at(frames: &Frames, st: &InventoryUi, n: u128) -> egui::Pos2 {
    let rect = frames
        .ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::unique(("inventory_drag_row", st.ui_id, id(n)))))
        .unwrap_or_else(|| panic!("row {n} is not drawn"));
    rect.min + Vec2::new(30.0, rect.height() * 0.5)
}

/// The last text drawn with this source: in an open menu, the menu's (the
/// tab strip shows the tabs' names too).
fn last_at(texts: &[Text], source: &str) -> egui::Pos2 {
    texts
        .iter()
        .rev()
        .find(|t| t.source == source)
        .unwrap_or_else(|| panic!("{source} is not drawn"))
        .rect
        .center()
}

struct Window {
    frames: Frames,
    classic: bool,
    inv: Inventory,
    st: InventoryUi,
    prefs: InventoryPreferences,
}

impl Window {
    fn new(classic: bool, prefs: InventoryPreferences) -> Self {
        let frames = Frames::new();
        crate::theme::Theme::default().apply(&frames.ctx, 1.0);
        let mut w = Self {
            frames,
            classic,
            inv: inventory(),
            st: InventoryUi::default(),
            prefs,
        };
        w.settle();
        w
    }

    fn run(&mut self, events: Vec<egui::Event>) -> Vec<Text> {
        run(&mut self.frames, self.classic, &mut self.inv, &mut self.st, &mut self.prefs, events)
    }

    fn settle(&mut self) -> Vec<Text> {
        for _ in 0..3 {
            self.run(vec![]);
        }
        self.run(vec![])
    }

    fn click_row(&mut self, n: u128, modifiers: egui::Modifiers) {
        let at = row_at(&self.frames, &self.st, n);
        self.run(click_with(at, egui::PointerButton::Primary, modifiers));
        self.run(vec![egui::Event::ModifiersChanged(Default::default())]);
        self.settle();
    }

    /// Right click on the row of `n`; the texts once its menu is open.
    fn menu(&mut self, n: u128) -> Vec<Text> {
        let at = row_at(&self.frames, &self.st, n);
        self.run(click_with(at, egui::PointerButton::Secondary, Default::default()));
        self.settle()
    }

    /// The pointer rests on `label` of the open menu: its submenu opens.
    fn hover(&mut self, texts: &[Text], label: &str) -> Vec<Text> {
        let at = last_at(texts, label);
        self.run(vec![egui::Event::PointerMoved(at)]);
        for _ in 0..10 {
            self.run(vec![]);
        }
        self.run(vec![])
    }

    fn choose(&mut self, texts: &[Text], label: &str) {
        let at = last_at(texts, label);
        self.run(vec![egui::Event::PointerMoved(at)]);
        self.run(click_with(at, egui::PointerButton::Primary, Default::default()));
        self.settle();
    }
}

fn two_tabs() -> InventoryPreferences {
    let mut prefs = scoped_prefs(&[CLOTHING]);
    add(&mut prefs.custom_tabs, "Looks", Filters::default(), String::new()).expect("room");
    prefs
}

fn saved_in(prefs: &InventoryPreferences, n: usize) -> Vec<Uuid> {
    folders::of(&prefs.custom_tabs[n], account()).to_vec()
}

#[test]
fn a_folders_menu_adds_the_ctrl_selection_to_a_tab_picked_under_add_to() {
    for classic in [true, false] {
        let mut w = Window::new(classic, two_tabs());
        // Ctrl+click: two folders
        w.click_row(BODY, Default::default());
        w.click_row(OTHER, modifiers(true, false));
        assert_eq!(
            w.st.selection,
            std::collections::HashSet::from([id(BODY), id(OTHER)]),
            "classic={classic}"
        );
        let texts = w.menu(OTHER);
        assert_eq!(w.st.selection.len(), 2, "classic={classic}: the right click keeps the selection");
        assert!(has(&texts, aurora_i18n::tr!("inventory-folder-tab-create")), "classic={classic}");
        let add_to = aurora_i18n::tr!("inventory-folder-tab-add-to");
        assert!(has(&texts, add_to), "classic={classic}");
        let texts = w.hover(&texts, add_to);
        for label in ["Wardrobe", "Looks", aurora_i18n::tr!("inventory-folder-tab-new")] {
            assert!(has(&texts, label), "classic={classic}: {label} under Add to");
        }
        w.choose(&texts, "Wardrobe");
        assert_eq!(saved_in(&w.prefs, 0), [id(CLOTHING), id(BODY), id(OTHER)], "classic={classic}");
        assert_eq!(
            w.st.message,
            aurora_i18n::tr!("inventory-folder-tab-added-many", count = 2, tab = "Wardrobe"),
            "classic={classic}"
        );
        assert_eq!(w.st.tab, 0, "classic={classic}: the tab shown stays");

        // again: already there, nothing repeated
        let texts = w.menu(BODY);
        let texts = w.hover(&texts, add_to);
        w.choose(&texts, "Wardrobe");
        assert_eq!(saved_in(&w.prefs, 0).len(), 3, "classic={classic}");
        assert_eq!(
            w.st.message,
            aurora_i18n::tr!("inventory-folder-tab-already-many", tab = "Wardrobe"),
            "classic={classic}"
        );

        // the other tab takes them too
        let texts = w.menu(BODY);
        let texts = w.hover(&texts, add_to);
        w.choose(&texts, "Looks");
        assert_eq!(saved_in(&w.prefs, 1), [id(BODY), id(OTHER)], "classic={classic}");
    }
}

#[test]
fn create_tab_takes_a_shift_range_of_folders() {
    for classic in [true, false] {
        let mut w = Window::new(classic, InventoryPreferences::default());
        w.click_row(BODY, Default::default());
        w.click_row(CLOTHING, modifiers(false, true));
        assert_eq!(
            w.st.selection,
            std::collections::HashSet::from([id(BODY), id(CLOTHING)]),
            "classic={classic}: Body and Clothing are next to each other"
        );
        let texts = w.menu(CLOTHING);
        w.choose(&texts, aurora_i18n::tr!("inventory-folder-tab-create"));
        assert_eq!(w.prefs.custom_tabs.len(), 1, "classic={classic}");
        let tab = &w.prefs.custom_tabs[0];
        assert_eq!(tab.name, "Body", "classic={classic}");
        assert_eq!(folders::of(tab, account()), [id(BODY), id(CLOTHING)], "classic={classic}");
        assert_eq!(
            w.st.custom_tabs.rename.as_ref().map(|r| r.id),
            Some(tab.id),
            "classic={classic}: Rename opens"
        );
    }
}

#[test]
fn folders_dragged_onto_a_custom_tab_join_it() {
    for classic in [true, false] {
        let mut w = Window::new(classic, two_tabs());
        let texts = w.settle();
        let tab = at(&texts, "Looks");
        w.click_row(BODY, Default::default());
        w.click_row(OTHER, modifiers(true, false));
        let from = row_at(&w.frames, &w.st, OTHER);
        w.run(vec![
            egui::Event::PointerMoved(from),
            egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        w.run(vec![egui::Event::PointerMoved(from + Vec2::new(20.0, 0.0))]);
        w.run(vec![egui::Event::PointerMoved(from + Vec2::new(40.0, 0.0))]);
        for _ in 0..3 {
            w.run(vec![egui::Event::PointerMoved(tab)]);
        }
        w.run(vec![egui::Event::PointerButton {
            pos: tab,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        w.settle();
        assert_eq!(saved_in(&w.prefs, 1), [id(BODY), id(OTHER)], "classic={classic}");
        assert_eq!(
            w.st.message,
            aurora_i18n::tr!("inventory-folder-tab-added-many", count = 2, tab = "Looks"),
            "classic={classic}"
        );

        // a folder in the Trash is refused, and the status line says why
        egui::DragAndDrop::set_payload(&w.frames.ctx, super::super::super::FolderDrag(id(OLD)));
        let away = egui::pos2(1200.0, 880.0);
        w.run(vec![
            egui::Event::PointerMoved(away),
            egui::Event::PointerButton {
                pos: away,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        w.run(vec![egui::Event::PointerMoved(tab)]);
        w.run(vec![egui::Event::PointerButton {
            pos: tab,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        w.settle();
        assert_eq!(saved_in(&w.prefs, 1).len(), 2, "classic={classic}");
        assert_eq!(
            w.st.message,
            aurora_i18n::tr!("inventory-folder-tab-why-refused"),
            "classic={classic}"
        );
    }
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
