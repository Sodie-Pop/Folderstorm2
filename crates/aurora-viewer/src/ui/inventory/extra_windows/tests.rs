use super::*;
use crate::ui::inventory::custom_tabs::folders::tests::{CLOTHING, account, id, inventory};
use crate::ui::widgets::elide_tests::{Frames, Text};

// ---- slots, ids, titles, places ----

#[test]
fn each_slot_has_its_own_fixed_id_the_first_aurora_s() {
    assert_eq!(id_of(1), Some(ADDITIONAL_WINDOW), "slot 1 is Aurora's additional window");
    let ids: std::collections::BTreeSet<Uuid> = (1..=MAX_SLOTS).filter_map(id_of).collect();
    assert_eq!(ids.len(), usize::from(MAX_SLOTS), "16 distinct ids");
    for slot in 1..=MAX_SLOTS {
        let id = id_of(slot).expect("a slot");
        assert_eq!(slot_of(id), Some(slot));
        let floater = floater_of(slot).expect("a slot");
        assert_eq!(floater, floater_id(id));
        assert_eq!(slot_of_floater(&floater), Some(slot));
    }
    assert_eq!(id_of(0), None);
    assert_eq!(id_of(MAX_SLOTS + 1), None);
    // not slots: the main window, the windows after the last slot or
    // before the first, the folder and tab windows
    let after = Uuid::from_u128(ADDITIONAL_WINDOW.as_u128() + u128::from(MAX_SLOTS));
    let before = Uuid::from_u128(ADDITIONAL_WINDOW.as_u128() - 1);
    for other in [Uuid::nil(), after, before, custom_tabs::window::window_id(1), Uuid::new_v4()] {
        assert_eq!(slot_of(other), None, "{other}");
    }
    for other in [
        "inventory".to_owned(),
        format!("inventory_{}", ADDITIONAL_WINDOW.simple()),
        format!("inventory_{after}"),
        format!("{ADDITIONAL_WINDOW}"),
    ] {
        assert_eq!(slot_of_floater(&other), None, "{other}");
    }
}

#[test]
fn the_slots_are_numbered_after_the_main_window() {
    assert_eq!(aurora_i18n::tr_in!("en", "inventory-windows-title", number = 2), "Inventory 2");
    assert_eq!(aurora_i18n::tr_in!("fr", "inventory-windows-title", number = 17), "Inventaire 17");
    for slot in 1..=MAX_SLOTS {
        let expected = aurora_i18n::tr!("inventory-windows-title", number = usize::from(slot) + 1);
        assert_eq!(slot_title(slot), expected);
        assert_eq!(title(id_of(slot).expect("a slot")), expected);
    }
    assert_eq!(title(Uuid::nil()), aurora_i18n::tr!("common-inventory"));
}

#[test]
fn each_slot_first_opens_a_step_further_inside_the_screen() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 900.0));
    let first = egui::pos2(100.0, 60.0);
    let first_of = |slot: u8| first_pos(id_of(slot).expect("a slot"), first, screen);
    assert_eq!(first_of(1), first, "slot 1 where Aurora's window opens");
    assert_eq!(first_of(2), first + egui::vec2(CASCADE, CASCADE));
    assert_eq!(first_of(3), first + egui::vec2(2.0 * CASCADE, 2.0 * CASCADE));
    for slot in 2..=MAX_SLOTS {
        let rect = egui::Rect::from_min_size(first_of(slot), DEFAULT_SIZE);
        assert!(screen.contains_rect(rect), "slot {slot}: {rect:?}");
    }
    // a small screen: still from its top left corner
    let small = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 300.0));
    let pos = first_pos(id_of(16).expect("a slot"), first, small);
    assert_eq!(pos, egui::Pos2::ZERO);
    // a window that is not a slot opens where Aurora's does
    assert_eq!(first_pos(Uuid::nil(), first, screen), first);
}

// ---- opening and closing, without drawing ----

fn open_slots(st: &InventoryUi) -> Vec<u8> {
    (1..=MAX_SLOTS).filter(|slot| st.folderstorm_slot_open(*slot)).collect()
}

#[test]
fn the_next_window_is_the_lowest_free_slot_up_to_16() {
    let ctx = egui::Context::default();
    let mut st = InventoryUi::default();
    assert_eq!(st.folderstorm_new_window(&ctx), Some(1));
    assert_eq!(st.folderstorm_new_window(&ctx), Some(2));
    assert_eq!(st.folderstorm_new_window(&ctx), Some(3));
    assert!(st.additional_open(), "slot 1 is the additional window");
    let w = &st.windows[1];
    assert!(w.additional && w.open && w.root.is_nil() && w.id == id_of(2).expect("a slot"));

    // a closed slot is the next one
    st.folderstorm_close_slot(2);
    assert_eq!(open_slots(&st), [1, 3], "that slot alone");
    assert_eq!(st.folderstorm_new_window(&ctx), Some(2));
    assert_eq!(st.windows.len(), 3, "the closed entry, not dropped yet, is reused");

    // up to 16; then nothing opens and the notice says so
    for slot in 4..=MAX_SLOTS {
        assert_eq!(st.folderstorm_new_window(&ctx), Some(slot));
    }
    assert_eq!(st.folderstorm_open_slots(), 16);
    assert!(st.notice.is_none());
    assert_eq!(st.folderstorm_new_window(&ctx), None);
    assert_eq!(st.windows.len(), 16, "no 17th window");
    let (notice, _) = st.notice.clone().expect("the notice");
    assert_eq!(notice, aurora_i18n::tr!("inventory-windows-all-open", max = MAX_SLOTS));

    // a slot open again brings it to the front, nothing more
    st.folderstorm_open_slot(&ctx, 5);
    assert_eq!(st.windows.len(), 16);
}

#[test]
fn close_all_closes_the_slots_and_rlva_every_additional_window() {
    let ctx = egui::Context::default();
    let mut st = InventoryUi::default();
    for _ in 0..MAX_SLOTS {
        st.folderstorm_new_window(&ctx);
    }
    st.open_folder_window(id(CLOTHING));
    let mut prefs = InventoryPreferences::default();
    let tab = custom_tabs::add(&mut prefs.custom_tabs, "Wardrobe", view::Filters::default(), String::new()).expect("room");
    custom_tabs::window::open(&ctx, &mut st, &prefs, tab);
    assert_eq!(st.windows.len(), 18);

    // *Close all additional windows*: the slots, not the folder or tab window
    st.folderstorm_close_all_slots();
    assert!(open_slots(&st).is_empty());
    assert_eq!(st.windows.iter().filter(|w| w.open).count(), 2, "the folder and tab windows stay");
    assert!(st.windows.iter().any(|w| w.open && w.root == id(CLOTHING)));
    assert!(st.windows.iter().any(|w| w.open && custom_tabs::window::is(w)));

    // RLVa's @showinv: every slot and the tab window too
    for _ in 0..MAX_SLOTS {
        st.folderstorm_new_window(&ctx);
    }
    assert_eq!(st.folderstorm_open_slots(), 16);
    st.close_additional_window();
    assert!(open_slots(&st).is_empty());
    assert!(!st.windows.iter().any(|w| w.open && custom_tabs::window::is(w)));
}

/// Ctrl+Shift+I's name in Preferences › Shortcuts.
#[test]
fn the_shortcut_is_named_after_what_it_does() {
    assert_eq!(
        crate::keybinds::label(crate::keybinds::Action::NewInventoryWindow),
        aurora_i18n::tr!("inventory-windows-new")
    );
}

// ---- drawn by `show`, both styles ----

struct Viewer {
    frames: Frames,
    world: World,
    st: InventoryUi,
    prefs: InventoryPreferences,
    icons: Icons,
    classic: bool,
}

impl Viewer {
    fn new(classic: bool) -> Self {
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
            prefs: InventoryPreferences::default(),
            icons,
            classic,
        }
    }

    /// One frame of every Inventory window, the main one open or not.
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
                None,
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

    /// Where the button of the window `window` (nil: the main one) is.
    fn button(&self, window: Uuid) -> Option<egui::Rect> {
        self.frames.ctx.data(|d| d.get_temp::<egui::Rect>(button_rect_id(window)))
    }

    fn click_at(&mut self, open: bool, pos: egui::Pos2, button: egui::PointerButton) {
        for pressed in [true, false] {
            self.frame(
                open,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }

    /// Click the button of the window `window`, with `button`.
    fn click_button(&mut self, open: bool, window: Uuid, button: egui::PointerButton) -> Vec<Text> {
        let classic = self.classic;
        let rect = self
            .button(window)
            .unwrap_or_else(|| panic!("classic={classic}: {window} has the button"));
        // in front, so that the click is its own
        if window.is_nil() {
            widgets::bring_floater_front(&self.frames.ctx, "inventory");
        } else {
            widgets::bring_floater_front(&self.frames.ctx, &floater_id(window));
        }
        self.frame(open, Vec::new());
        self.click_at(open, rect.center(), button);
        self.settle(open)
    }
}

/// The state of the open window `id`.
fn window(v: &mut Viewer, id: Uuid) -> &mut InventoryUi {
    &mut v.st.windows.iter_mut().find(|w| w.id == id).expect("open").state
}

fn has(texts: &[Text], source: &str) -> bool {
    texts.iter().any(|t| t.source == source)
}

/// The button opens the next window, from the main window and from an
/// additional one, in both styles; each window has its numbered title, its
/// own search and tab; closed, a window forgets them and keeps its place.
#[test]
fn the_button_in_every_whole_inventory_window_opens_the_next_one() {
    for classic in [true, false] {
        let mut v = Viewer::new(classic);
        v.settle(true);
        assert!(v.button(Uuid::nil()).is_some(), "classic={classic}: the main window has it");
        v.click_button(true, Uuid::nil(), egui::PointerButton::Primary);
        assert_eq!(open_slots(&v.st), [1], "classic={classic}");

        // from slot 1's own button: slot 2
        let first = id_of(1).expect("a slot");
        assert!(v.button(first).is_some(), "classic={classic}: an additional window has it");
        let texts = v.click_button(true, first, egui::PointerButton::Primary);
        assert_eq!(open_slots(&v.st), [1, 2], "classic={classic}");
        assert!(has(&texts, &slot_title(1)), "classic={classic}: Inventory 2");
        assert!(has(&texts, &slot_title(2)), "classic={classic}: Inventory 3");

        // each its own search and tab
        let second = id_of(2).expect("a slot");
        window(&mut v, first).search = "shirt".into();
        set_tab(window(&mut v, second), &InventoryPreferences::default(), 3);
        v.settle(true);
        assert_eq!(window(&mut v, first).search, "shirt");
        assert!(window(&mut v, second).search.is_empty(), "classic={classic}");
        assert_eq!((window(&mut v, first).tab, window(&mut v, second).tab), (0, 3), "classic={classic}");
        assert!(v.st.search.is_empty() && v.st.tab == 0, "classic={classic}: the main window's own");

        // closed then reopened: afresh, at the same place
        let floater = floater_of(1).expect("a slot");
        let place = egui::AreaState::load(&v.frames.ctx, egui::Id::unique(&floater))
            .map(|state| state.left_top_pos())
            .expect("drawn");
        v.st.folderstorm_close_slot(1);
        v.settle(true);
        assert!(v.st.windows.iter().all(|w| w.id != first), "classic={classic}: dropped");
        v.click_button(true, Uuid::nil(), egui::PointerButton::Primary);
        assert_eq!(open_slots(&v.st), [1, 2], "classic={classic}: the free slot 1 again");
        assert!(window(&mut v, first).search.is_empty(), "classic={classic}: afresh");
        let again = egui::AreaState::load(&v.frames.ctx, egui::Id::unique(&floater)).map(|state| state.left_top_pos());
        assert_eq!(again, Some(place), "classic={classic}: its place kept");
    }
}

/// A folder window or a tab window, the two kinds that are not slots.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Folder,
    Tab,
}

/// A viewer with the main window closed and one window of `kind` open;
/// returns its id.
fn with_window(classic: bool, kind: Kind) -> (Viewer, Uuid) {
    let mut v = Viewer::new(classic);
    let ctx = v.frames.ctx.clone();
    match kind {
        Kind::Folder => v.st.open_folder_window(id(CLOTHING)),
        Kind::Tab => {
            let tab = custom_tabs::add(&mut v.prefs.custom_tabs, "Wardrobe", view::Filters::default(), String::new()).expect("room");
            custom_tabs::window::open(&ctx, &mut v.st, &v.prefs, tab);
        }
    }
    v.settle(false);
    assert_eq!(v.st.windows.len(), 1, "classic={classic} {kind:?}");
    let window = v.st.windows[0].id;
    assert_eq!(slot_of(window), None, "classic={classic} {kind:?}: not a slot");
    (v, window)
}

/// The folder and tab windows have the button too, in both styles: it opens
/// the lowest free slot, again and again, and stays lit while one is open;
/// the window itself stays open and is not a slot.
#[test]
fn folder_and_tab_windows_have_the_button_and_it_opens_the_next_slot() {
    for classic in [true, false] {
        for kind in [Kind::Folder, Kind::Tab] {
            let (mut v, window) = with_window(classic, kind);
            assert!(v.button(window).is_some(), "classic={classic} {kind:?}: the button is drawn");
            assert_eq!(v.st.folderstorm_open_slots(), 0, "classic={classic} {kind:?}: not slots");

            let texts = v.click_button(false, window, egui::PointerButton::Primary);
            assert_eq!(open_slots(&v.st), [1], "classic={classic} {kind:?}");
            assert!(has(&texts, &slot_title(1)), "classic={classic} {kind:?}: Inventory 2 shows");
            v.click_button(false, window, egui::PointerButton::Primary);
            assert_eq!(open_slots(&v.st), [1, 2], "classic={classic} {kind:?}: the next one");
            assert!(
                v.st.windows.iter().any(|w| w.id == window && w.open),
                "classic={classic} {kind:?}: the window stays open"
            );

            // a free slot is the next one, from this window too
            v.st.folderstorm_close_slot(1);
            v.settle(false);
            v.click_button(false, window, egui::PointerButton::Primary);
            assert_eq!(open_slots(&v.st), [1, 2], "classic={classic} {kind:?}: slot 1 again");
        }
    }
}

/// In quiet mode a folder or tab window shows its list alone, without the
/// button, as the other windows do.
#[test]
fn quiet_folder_and_tab_windows_hide_the_button() {
    use crate::ui::inventory::window_mode::{Key, Mode};
    for classic in [true, false] {
        for kind in [Kind::Folder, Kind::Tab] {
            let (mut v, window) = with_window(classic, kind);
            let key = Key::of_window(&v.st.windows[0]).expect("a key");
            v.prefs.window_modes.set(
                key,
                Mode {
                    pinned: false,
                    quiet: true,
                },
            );
            v.frames.ctx.data_mut(|d| d.remove::<egui::Rect>(button_rect_id(window)));
            v.settle(false);
            assert!(v.st.windows[0].state.window_mode.quiet, "classic={classic} {kind:?}: quiet");
            assert!(v.button(window).is_none(), "classic={classic} {kind:?}: no button in quiet mode");
        }
    }
}

/// At 16 the button of a folder or tab window is greyed and opens nothing;
/// its right-click menu still closes all the slots, and opens one.
#[test]
fn at_16_the_button_of_folder_and_tab_windows_is_greyed_its_menu_works() {
    for classic in [true, false] {
        for kind in [Kind::Folder, Kind::Tab] {
            let (mut v, window) = with_window(classic, kind);
            let ctx = v.frames.ctx.clone();
            for _ in 0..MAX_SLOTS {
                v.st.folderstorm_new_window(&ctx);
            }
            v.settle(false);
            v.click_button(false, window, egui::PointerButton::Primary);
            assert_eq!(v.st.windows.len(), 17, "classic={classic} {kind:?}: nothing opens");
            assert!(v.st.notice.is_none(), "classic={classic} {kind:?}: a greyed button is not clicked");

            let texts = v.click_button(false, window, egui::PointerButton::Secondary);
            let close_all = texts
                .iter()
                .find(|t| t.source == aurora_i18n::tr!("inventory-windows-close-all"))
                .unwrap_or_else(|| panic!("classic={classic} {kind:?}: the menu"))
                .rect;
            v.click_at(false, close_all.center(), egui::PointerButton::Primary);
            v.settle(false);
            assert_eq!(v.st.folderstorm_open_slots(), 0, "classic={classic} {kind:?}");
            assert_eq!(v.st.windows.len(), 1, "classic={classic} {kind:?}: this window stays");
            assert_eq!(v.st.windows[0].id, window);

            let texts = v.click_button(false, window, egui::PointerButton::Secondary);
            let new = texts
                .iter()
                .find(|t| t.source == aurora_i18n::tr!("inventory-windows-new"))
                .unwrap_or_else(|| panic!("classic={classic} {kind:?}: the menu"))
                .rect;
            v.click_at(false, new.center(), egui::PointerButton::Primary);
            v.settle(false);
            assert_eq!(open_slots(&v.st), [1], "classic={classic} {kind:?}");
        }
    }
}

/// With all 16 open the button is greyed and opens nothing; its menu's
/// *Close all additional windows* closes the slots, not a folder window.
#[test]
fn at_16_the_button_is_greyed_and_its_menu_closes_them_all() {
    for classic in [true, false] {
        let mut v = Viewer::new(classic);
        let ctx = v.frames.ctx.clone();
        for _ in 0..MAX_SLOTS {
            v.st.folderstorm_new_window(&ctx);
        }
        v.st.open_folder_window(id(CLOTHING));
        v.settle(true);
        let texts = v.click_button(true, Uuid::nil(), egui::PointerButton::Primary);
        assert_eq!(v.st.windows.len(), 17, "classic={classic}: nothing opens");
        assert!(v.st.notice.is_none(), "classic={classic}: a greyed button is not clicked");
        assert!(!has(&texts, aurora_i18n::tr!("inventory-windows-close-all")));

        // its right-click menu
        let texts = v.click_button(true, Uuid::nil(), egui::PointerButton::Secondary);
        assert!(has(&texts, aurora_i18n::tr!("inventory-windows-new")), "classic={classic}");
        let close_all = texts
            .iter()
            .find(|t| t.source == aurora_i18n::tr!("inventory-windows-close-all"))
            .unwrap_or_else(|| panic!("classic={classic}: the menu"))
            .rect;
        v.click_at(true, close_all.center(), egui::PointerButton::Primary);
        v.settle(true);
        assert_eq!(v.st.folderstorm_open_slots(), 0, "classic={classic}");
        assert_eq!(v.st.windows.len(), 1, "classic={classic}: the folder window stays");
        assert_eq!(v.st.windows[0].root, id(CLOTHING));

        // the menu's *New Inventory window*
        let texts = v.click_button(true, Uuid::nil(), egui::PointerButton::Secondary);
        let new = texts
            .iter()
            .find(|t| t.source == aurora_i18n::tr!("inventory-windows-new"))
            .unwrap_or_else(|| panic!("classic={classic}: the menu"))
            .rect;
        v.click_at(true, new.center(), egui::PointerButton::Primary);
        v.settle(true);
        assert_eq!(open_slots(&v.st), [1], "classic={classic}");
    }
}

/// The notice of a 17th window shows in the Inventory windows' status.
#[test]
fn a_17th_window_is_refused_with_a_notice() {
    let mut v = Viewer::new(true);
    let ctx = v.frames.ctx.clone();
    for _ in 0..=MAX_SLOTS {
        v.st.folderstorm_new_window(&ctx);
    }
    let texts = v.settle(true);
    let notice = aurora_i18n::tr!("inventory-windows-all-open", max = MAX_SLOTS);
    assert!(texts.iter().any(|t| t.source == notice), "the notice is shown");
    assert!(
        v.st.windows
            .iter()
            .all(|w| w.state.notice.as_ref().is_some_and(|(text, _)| *text == notice))
    );
}
