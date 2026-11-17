use super::*;
use crate::ui::inventory::custom_tabs::folders::tests::{CLOTHING, account, id, inventory};
use crate::ui::inventory::custom_tabs::{self as tabs, CUSTOM_TAB, SLOT_BASE};
use crate::ui::widgets::elide_tests::{Frames, Text};

// ---- the settings ----

#[test]
fn modes_are_kept_per_window_only_when_changed() {
    let mut modes = Modes::default();
    assert_eq!(modes.get(Key::Main), Mode::default());
    let pinned = Mode {
        pinned: true,
        quiet: false,
    };
    modes.set(Key::Main, pinned);
    modes.set(Key::Tab(3), Mode { pinned: true, quiet: true });
    modes.set(
        Key::Folder(id(CLOTHING)),
        Mode {
            pinned: false,
            quiet: true,
        },
    );
    modes.set(Key::Additional(1), Mode::default());
    assert_eq!(modes.get(Key::Main), pinned);
    assert_eq!(modes.get(Key::Additional(1)), Mode::default());
    assert!(modes.get(Key::Tab(3)).quiet && modes.get(Key::Folder(id(CLOTHING))).quiet);
    assert!(!modes.get(Key::Tab(4)).quiet, "another tab's window");
    let saved = serde_json::to_string(&modes).expect("serialize");
    assert!(!saved.contains("additional"), "the default mode is not written: {saved}");
    let loaded: Modes = serde_json::from_str(&saved).expect("deserialize");
    assert_eq!(loaded, modes);
    // back to the default: forgotten
    modes.set(Key::Main, Mode::default());
    assert!(!serde_json::to_string(&modes).expect("serialize").contains("main"));
    // a deleted tab forgets its window
    assert!(modes.retain_tabs(&[5]));
    assert_eq!(modes.get(Key::Tab(3)), Mode::default());
    assert!(modes.get(Key::Folder(id(CLOTHING))).quiet, "folder windows stay");
}

/// Each additional window has its own mode; the first keeps the key it had
/// when there was one (`"additional"`), so a mode saved then still applies.
#[test]
fn each_additional_window_has_its_own_mode() {
    let quiet = Mode {
        pinned: false,
        quiet: true,
    };
    let pinned = Mode {
        pinned: true,
        quiet: false,
    };
    let old: Modes = serde_json::from_str(r#"{"additional":{"pinned":true,"quiet":false}}"#).expect("saved before");
    assert_eq!(old.get(Key::Additional(1)), pinned, "slot 1 reads the old key");
    assert_eq!(old.get(Key::Additional(2)), Mode::default());
    let mut modes = Modes::default();
    modes.set(Key::Additional(1), pinned);
    for slot in 2..=crate::ui::inventory::extra_windows::MAX_SLOTS {
        modes.set(Key::Additional(slot), if slot % 2 == 0 { quiet } else { Mode::default() });
    }
    let saved = serde_json::to_string(&modes).expect("serialize");
    assert!(
        saved.contains(r#""additional":"#) && saved.contains(r#""additional:2":"#),
        "{saved}"
    );
    assert!(!saved.contains("additional:3"), "the default mode is not written: {saved}");
    let loaded: Modes = serde_json::from_str(&saved).expect("deserialize");
    assert_eq!(loaded, modes);
    assert_eq!(loaded.get(Key::Additional(1)), pinned);
    assert_eq!(loaded.get(Key::Additional(16)), quiet);
    assert_eq!(loaded.get(Key::Additional(15)), Mode::default());
}

#[test]
fn only_the_last_folder_windows_are_remembered() {
    let mut modes = Modes::default();
    let quiet = Mode {
        pinned: false,
        quiet: true,
    };
    for n in 0..(MAX_FOLDER_WINDOWS as u128 + 10) {
        modes.set(Key::Folder(id(9000 + n)), quiet);
    }
    modes.set(Key::Main, quiet);
    let folders = modes.0.keys().filter(|k| k.starts_with("folder:")).count();
    assert_eq!(folders, MAX_FOLDER_WINDOWS);
    let last = Key::Folder(id(9000 + MAX_FOLDER_WINDOWS as u128 + 9));
    assert!(modes.get(last).quiet, "the one just set stays");
    assert!(modes.get(Key::Main).quiet);
    let old: InventoryPreferences = serde_json::from_str(r#"{"custom_tabs":[]}"#).expect("old settings");
    assert_eq!(old.window_modes, Modes::default(), "settings saved before: no modes");
}

// ---- the windows, drawn by `show` ----

struct Viewer {
    frames: Frames,
    world: World,
    st: InventoryUi,
    prefs: InventoryPreferences,
    icons: Icons,
    classic: bool,
}

impl Viewer {
    fn new(classic: bool, prefs: InventoryPreferences) -> Self {
        let frames = Frames::new();
        crate::theme::Theme::default().apply(&frames.ctx, 1.0);
        let icons = Icons::load(&frames.ctx, None);
        let mut world = World::new(std::sync::Arc::new(crate::scene::avatar::AvatarLibrary::load()));
        world.inventory = inventory();
        world.agent_id = account();
        Self {
            frames,
            world,
            st: InventoryUi::default(),
            prefs,
            icons,
            classic,
        }
    }

    /// One frame of every Inventory window; the main one `open` or not.
    fn frame(&mut self, open: bool, events: Vec<egui::Event>) -> Vec<Text> {
        let p = crate::theme::Theme::default().palette();
        let (world, st, prefs, icons, classic) = (&mut self.world, &mut self.st, &mut self.prefs, &self.icons, self.classic);
        let images = Default::default();
        let scripts = Default::default();
        self.frames.run(events, |ui| {
            let mut open = open;
            show(
                ui.ctx(),
                &p,
                icons,
                world,
                st,
                prefs,
                &mut open,
                false,
                &images,
                &Default::default(),
                &Default::default(),
                &scripts,
                classic,
            );
        })
    }

    fn settle(&mut self, open: bool) -> Vec<Text> {
        for _ in 0..3 {
            self.frame(open, Vec::new());
        }
        self.frame(open, Vec::new())
    }

    /// Click the title bar button `icon` of the window `floater`.
    fn click_action(&mut self, open: bool, floater: &str, icon: &str) {
        let key = egui::Id::unique(("floater_action", floater, icon));
        let pos = self
            .frames
            .ctx
            .data(|d| d.get_temp::<egui::Rect>(key))
            .unwrap_or_else(|| panic!("{floater} has no {icon} button"))
            .center();
        for pressed in [true, false] {
            self.frame(
                open,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        self.frames.ctx.data_mut(|d| d.remove::<egui::Rect>(key));
        self.settle(open);
    }

    fn layer_shown(&self, order: egui::Order, floater: &str) -> bool {
        let layer = egui::LayerId::new(order, egui::Id::unique(floater));
        self.frames.ctx.memory(|m| m.areas().is_visible(&layer))
    }

    /// The window `floater` is in front of the other windows.
    fn in_front(&self, floater: &str) -> bool {
        let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::unique(floater));
        self.frames.ctx.memory(|m| m.areas().top_layer_id(egui::Order::Middle)) == Some(layer)
    }
}

fn has(texts: &[Text], source: &str) -> bool {
    texts.iter().any(|t| t.source == source)
}

fn count(texts: &[Text], source: &str) -> usize {
    texts.iter().filter(|t| t.source == source).count()
}

fn row(texts: &[Text], name: &str) -> bool {
    texts.iter().any(|t| t.source == name || t.source.starts_with(&format!("{name} ")))
}

#[test]
fn the_title_bar_pins_and_quiets_the_main_window() {
    for classic in [true, false] {
        let mut v = Viewer::new(classic, InventoryPreferences::default());
        let texts = v.settle(true);
        assert!(row(&texts, "Clothing"), "classic={classic}");
        assert!(v.layer_shown(egui::Order::Middle, "inventory"), "classic={classic}");
        // a folder window opened after it covers it
        v.st.open_folder_window(id(CLOTHING));
        let folder = floater_id(v.st.windows[0].id);
        let texts = v.settle(true);
        assert!(v.in_front(&folder), "classic={classic}: the newer window in front");
        let tabs_label = if classic {
            aurora_i18n::tr!("common-inventory")
        } else {
            aurora_i18n::tr!("inventory-tab-all")
        };
        let normal = count(&texts, tabs_label);
        let hint = aurora_i18n::tr!("ui-inventory-filter-inventory");
        let status = aurora_i18n::tr!("workspace-inventory-selected", count = 0);
        let (hints, statuses) = (count(&texts, hint), count(&texts, &status));

        // pin: above the other windows, remembered
        v.click_action(true, "inventory", "pin-filled");
        assert!(v.prefs.window_modes.get(Key::Main).pinned, "classic={classic}");
        assert!(v.in_front("inventory"), "classic={classic}: on top");
        // the folder window brought to the front goes back behind it
        widgets::bring_floater_front(&v.frames.ctx, &folder);
        v.settle(true);
        assert!(v.in_front("inventory"), "classic={classic}: still on top");

        // quiet: the list alone, the title bar kept
        v.click_action(true, "inventory", "arrows-minimize");
        assert!(v.prefs.window_modes.get(Key::Main).quiet, "classic={classic}");
        let texts = v.settle(true);
        assert!(row(&texts, "Clothing"), "classic={classic}: the list stays");
        assert!(count(&texts, tabs_label) < normal, "classic={classic}: no tab strip");
        if classic {
            assert!(count(&texts, hint) < hints, "classic={classic}: no search box");
            assert!(count(&texts, &status) < statuses, "classic={classic}: no status line");
        }

        // and back, still pinned
        v.click_action(true, "inventory", "arrows-maximize");
        v.click_action(true, "inventory", "pinned-filled");
        assert_eq!(v.prefs.window_modes.get(Key::Main), Mode::default(), "classic={classic}");
        widgets::bring_floater_front(&v.frames.ctx, &folder);
        let texts = v.settle(true);
        assert_eq!(count(&texts, tabs_label), normal, "classic={classic}: all back");
        assert!(v.in_front(&folder), "classic={classic}: unpinned, covered again");
    }
}

#[test]
fn a_folder_window_has_its_own_mode() {
    for classic in [true, false] {
        let mut prefs = InventoryPreferences::default();
        prefs
            .window_modes
            .set(Key::Folder(id(CLOTHING)), Mode { pinned: true, quiet: true });
        let mut v = Viewer::new(classic, prefs);
        v.st.open_folder_window(id(CLOTHING));
        let window = floater_id(v.st.windows[0].id);
        let texts = v.settle(false);
        assert!(row(&texts, "Shirt") && row(&texts, "Shoes"), "classic={classic}: its folder's list");
        assert!(v.st.windows[0].state.window_mode.quiet, "classic={classic}");
        assert!(!v.st.window_mode.quiet, "classic={classic}: the main window keeps its own");
        if classic {
            let hint = aurora_i18n::tr!("ui-inventory-filter-inventory-folder");
            assert!(!has(&texts, hint), "no search box");
        }
        assert!(v.in_front(&window), "classic={classic}: pinned");
        v.click_action(false, &window, "arrows-maximize");
        assert!(!v.prefs.window_modes.get(Key::Folder(id(CLOTHING))).quiet, "classic={classic}");
        if classic {
            let texts = v.settle(false);
            assert!(has(&texts, aurora_i18n::tr!("ui-inventory-filter-inventory-folder")));
        }
    }
}

// ---- tab windows (custom_tabs/window.rs) ----

fn wardrobe() -> InventoryPreferences {
    let mut prefs = InventoryPreferences::default();
    tabs::add(&mut prefs.custom_tabs, "Wardrobe", view::Filters::default(), String::new()).expect("room");
    tabs::folders::set(&mut prefs.custom_tabs[0], account(), vec![id(CLOTHING)]);
    prefs
}

#[test]
fn a_tab_opens_in_a_window_of_its_own() {
    for classic in [true, false] {
        let mut v = Viewer::new(classic, wardrobe());
        let tab = v.prefs.custom_tabs[0].id;
        v.settle(true);
        // "Open in a window" asks the main window, on its next frame
        tabs::window::request_open(&v.frames.ctx, tab);
        let texts = v.settle(false);
        let [w] = v.st.windows.as_slice() else {
            panic!("classic={classic}: one window");
        };
        assert_eq!(w.id, tabs::window::window_id(tab));
        assert!(w.open && !w.additional, "classic={classic}");
        assert_eq!((w.state.tab, tabs::name_of(&w.state)), (CUSTOM_TAB, Some("Wardrobe")));
        assert!(has(&texts, "Wardrobe"), "classic={classic}: named after the tab");
        assert!(
            row(&texts, "Shirt") && !row(&texts, "Unrelated"),
            "classic={classic}: the tab's list"
        );
        let strip = if classic {
            aurora_i18n::tr!("common-inventory")
        } else {
            aurora_i18n::tr!("inventory-tab-all")
        };
        assert!(!has(&texts, strip), "classic={classic}: no tab strip");

        // again: the same window
        tabs::window::request_open(&v.frames.ctx, tab);
        v.settle(false);
        assert_eq!(v.st.windows.len(), 1, "classic={classic}");

        // its own pin, kept by its tab
        let floater = floater_id(tabs::window::window_id(tab));
        v.click_action(false, &floater, "pin-filled");
        assert!(v.prefs.window_modes.get(Key::Tab(tab)).pinned, "classic={classic}");
        assert!(!v.prefs.window_modes.get(Key::Main).pinned, "classic={classic}");
        assert!(v.in_front(&floater), "classic={classic}");

        // RLVa's @showinv closes it
        v.st.close_additional_window();
        v.settle(false);
        assert!(v.st.windows.is_empty(), "classic={classic}: closed by @showinv");

        // deleting the tab closes its window and forgets its mode
        tabs::window::request_open(&v.frames.ctx, tab);
        v.settle(false);
        assert_eq!(v.st.windows.len(), 1, "classic={classic}");
        v.prefs.custom_tabs.clear();
        v.settle(false);
        assert!(v.st.windows.is_empty(), "classic={classic}: gone with its tab");
        v.settle(true);
        assert_eq!(v.prefs.window_modes.get(Key::Tab(tab)), Mode::default(), "classic={classic}");
    }
}

#[test]
fn a_quiet_tab_window_shows_its_list_alone() {
    for classic in [true, false] {
        let mut v = Viewer::new(classic, wardrobe());
        let tab = v.prefs.custom_tabs[0].id;
        v.prefs.window_modes.set(
            Key::Tab(tab),
            Mode {
                pinned: false,
                quiet: true,
            },
        );
        tabs::window::request_open(&v.frames.ctx, tab);
        let texts = v.settle(false);
        assert!(row(&texts, "Shirt") && row(&texts, "Boot"), "classic={classic}");
        assert!(!row(&texts, "Unrelated"), "classic={classic}");
        assert!(v.st.windows[0].state.window_mode.quiet, "classic={classic}");
    }
}

#[test]
fn two_windows_on_one_tab_share_its_filters_and_search() {
    let mut prefs = wardrobe();
    let mut main = InventoryUi::default();
    let mut window = InventoryUi::default();
    tabs::select(&mut main, &prefs, SLOT_BASE);
    tabs::select(&mut window, &prefs, SLOT_BASE);
    main.search = "boot".into();
    tabs::sync(&mut main, &mut prefs);
    tabs::sync(&mut window, &mut prefs);
    assert_eq!(window.search, "boot", "taken up by the other window");
    assert_eq!(prefs.custom_tabs[0].search, "boot");
    window.filters.types = 1 << 0;
    tabs::sync(&mut window, &mut prefs);
    tabs::sync(&mut main, &mut prefs);
    assert_eq!(main.filters.types, 1 << 0);
    assert_eq!(main.search, "boot");
    // nothing changes while both agree
    tabs::sync(&mut main, &mut prefs);
    tabs::sync(&mut window, &mut prefs);
    assert_eq!((main.search.as_str(), window.search.as_str()), ("boot", "boot"));
}
