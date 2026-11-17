use super::super::folders::tests::inventory;
use super::*;
use crate::ui::widgets::elide_tests::{Frames, Text};

fn tab(icon: Option<&str>) -> CustomTab {
    CustomTab {
        id: 1,
        name: "Pets".into(),
        icon: icon.map(str::to_owned),
        ..Default::default()
    }
}

// ---- the list and the store ----

#[test]
fn the_choices_are_embedded_named_once_and_translated() {
    assert!((30..=48).contains(&CHOICES.len()), "{}", CHOICES.len());
    let mut seen = std::collections::HashSet::new();
    for (icon, key) in CHOICES {
        assert!(crate::ui::icons::embedded(icon), "{icon} is not an embedded Tabler icon");
        assert!(seen.insert(*icon), "{icon} twice");
        assert_ne!(*icon, DEFAULT, "the default is offered as Default");
        assert_ne!(aurora_i18n::text(*key), *key, "{key} has no text");
    }
    assert!(crate::ui::icons::embedded(DEFAULT));
}

#[test]
fn the_icon_is_one_of_the_choices_or_the_default() {
    assert_eq!(of(&tab(None)), DEFAULT);
    assert_eq!(of(&tab(Some("paw-filled"))), "paw-filled");
    assert_eq!(chosen(&tab(Some("paw-filled"))), Some("paw-filled"));
    // a name this version does not know is kept, drawn as the default
    let later = tab(Some("rocket-filled"));
    assert_eq!((of(&later), chosen(&later)), (DEFAULT, None));

    let mut t = tab(None);
    set(&mut t, Some("cube"));
    assert_eq!(t.icon.as_deref(), Some("cube"));
    set(&mut t, Some(DEFAULT));
    assert_eq!(t.icon, None, "the default is not kept");
    set(&mut t, Some("cube"));
    set(&mut t, Some("not-an-icon"));
    assert_eq!(t.icon, None);
    set(&mut t, Some("cube"));
    set(&mut t, None);
    assert_eq!(t.icon, None);
}

#[test]
fn the_slot_of_a_custom_tab_gives_its_icon() {
    let mut prefs = InventoryPreferences::default();
    add(&mut prefs.custom_tabs, "Plain", Filters::default(), String::new()).expect("room");
    add(&mut prefs.custom_tabs, "Pets", Filters::default(), String::new()).expect("room");
    set(&mut prefs.custom_tabs[1], Some("paw-filled"));
    assert_eq!(of_slot(&prefs, SLOT_BASE), DEFAULT);
    assert_eq!(of_slot(&prefs, SLOT_BASE + 1), "paw-filled");
    assert_eq!(of_slot(&prefs, SLOT_BASE + 2), DEFAULT, "no such tab");
    assert_eq!(of_slot(&prefs, 0), DEFAULT, "a built-in tab");
}

#[test]
fn the_icon_round_trips_and_is_left_out_while_default() {
    let mut prefs = InventoryPreferences::default();
    add(&mut prefs.custom_tabs, "Plain", Filters::default(), String::new()).expect("room");
    add(&mut prefs.custom_tabs, "Pets", Filters::default(), String::new()).expect("room");
    set(&mut prefs.custom_tabs[1], Some("paw-filled"));
    let saved = serde_json::to_value(&prefs).expect("serialize");
    let tabs = saved["custom_tabs"].as_array().expect("tabs");
    assert!(tabs[0].get("icon").is_none(), "left out while default: {}", tabs[0]);
    assert_eq!(tabs[1]["icon"], "paw-filled");
    let loaded: InventoryPreferences = serde_json::from_value(saved).expect("deserialize");
    assert_eq!(loaded, prefs);
    // Clone copies it
    let id = prefs.custom_tabs[1].id;
    clone_tab(&mut prefs.custom_tabs, id, "Pets copy").expect("room");
    assert_eq!(prefs.custom_tabs[2].icon.as_deref(), Some("paw-filled"));
}

#[test]
fn a_hand_edited_icon_is_read_leniently() {
    let loaded: InventoryPreferences = serde_json::from_str(
        r#"{"custom_tabs":[
            {"id":1,"name":"A","icon":5},
            {"id":2,"name":"B","icon":["paw-filled"]},
            {"id":3,"name":"C","icon":""},
            {"id":4,"name":"D","icon":"  paw-filled\n"},
            {"id":5,"name":"E","icon":null},
            {"id":6,"name":"F","icon":"rocket-filled"},
            {"id":7,"name":"G"}
        ],"show_recent":false}"#,
    )
    .expect("the rest of the settings load");
    let icons: Vec<Option<&str>> = loaded.custom_tabs.iter().map(|t| t.icon.as_deref()).collect();
    assert_eq!(icons, [None, None, None, Some("paw-filled"), None, Some("rocket-filled"), None]);
    assert!(!loaded.show_recent);
    assert_eq!(of(&loaded.custom_tabs[5]), DEFAULT);

    // normalize: the default's name and blanks go
    let mut tabs = vec![tab(Some(DEFAULT)), tab(Some(" ")), tab(Some("paw-filled"))];
    tabs[1].id = 2;
    tabs[1].name = "Other".into();
    tabs[2].id = 3;
    tabs[2].name = "Third".into();
    assert!(super::super::normalize(&mut tabs));
    assert_eq!(tabs[0].icon, None);
    assert_eq!(tabs[1].icon, None);
    assert_eq!(tabs[2].icon.as_deref(), Some("paw-filled"));
    assert!(!super::super::normalize(&mut tabs), "a clean list is left alone");
}

// ---- drawn in both styles ----

fn draw(ui: &mut egui::Ui, classic: bool, inv: &mut Inventory, st: &mut InventoryUi, prefs: &mut InventoryPreferences, width: f32) {
    let p = crate::theme::Theme::default().palette();
    ui.set_max_size(Vec2::new(width, 700.0));
    if classic {
        super::super::super::inventory_contents(ui, &p, &Icons::default(), inv, st, prefs, &Facts::default(), &mut vec![]);
    } else {
        super::super::super::aurora::contents(
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
    super::super::super::controls::dialogs(ui.ctx(), &p, st, prefs);
}

struct Bench {
    frames: Frames,
    classic: bool,
    width: f32,
    inv: Inventory,
    st: InventoryUi,
    prefs: InventoryPreferences,
}

impl Bench {
    /// Two custom tabs, "Plain" (the default icon) and "Pets" (the paw).
    fn new(classic: bool, width: f32) -> Self {
        let frames = Frames::new();
        crate::theme::Theme::default().apply(&frames.ctx, 1.0);
        let _icons = crate::ui::icons::Icons::load(&frames.ctx, None);
        let mut prefs = InventoryPreferences::default();
        add(&mut prefs.custom_tabs, "Plain", Filters::default(), String::new()).expect("room");
        add(&mut prefs.custom_tabs, "Pets", Filters::default(), String::new()).expect("room");
        set(&mut prefs.custom_tabs[1], Some("paw-filled"));
        let mut b = Self {
            frames,
            classic,
            width,
            inv: inventory(),
            st: InventoryUi::default(),
            prefs,
        };
        b.settle();
        b
    }

    fn run(&mut self, events: Vec<egui::Event>) -> Vec<Text> {
        let (classic, width) = (self.classic, self.width);
        let (inv, st, prefs) = (&mut self.inv, &mut self.st, &mut self.prefs);
        self.frames.run(events, |ui| draw(ui, classic, inv, st, prefs, width))
    }

    fn settle(&mut self) -> Vec<Text> {
        for _ in 0..3 {
            self.run(vec![]);
        }
        self.run(vec![])
    }

    /// Where the icon `name` is painted this frame.
    fn painted(&mut self, name: &str) -> Vec<egui::Rect> {
        let (classic, width) = (self.classic, self.width);
        let (inv, st, prefs) = (&mut self.inv, &mut self.st, &mut self.prefs);
        let shapes = self.frames.shapes(vec![], |ui| draw(ui, classic, inv, st, prefs, width));
        let texture = crate::ui::icons::global(name).unwrap_or_else(|| panic!("{name} is loaded")).id();
        shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.calc_bounds()),
                egui::Shape::Rect(rect) if rect.brush.as_ref().is_some_and(|brush| brush.fill_texture_id == texture) => Some(rect.rect),
                _ => None,
            })
            .collect()
    }
}

/// The text of a tab's name in its strip (the Legacy strip leaves room
/// before a name with an icon).
fn label<'a>(texts: &'a [Text], name: &str) -> &'a Text {
    texts
        .iter()
        .find(|t| t.source.trim_start() == name)
        .unwrap_or_else(|| panic!("{name} is not drawn"))
}

#[test]
fn the_chosen_icon_is_drawn_beside_the_tab_in_both_styles() {
    // Legacy strip; Aurora's narrow tab icons; Aurora's wide shortcuts column
    for (classic, width) in [(true, 420.0), (false, 420.0), (false, 1200.0)] {
        let mut b = Bench::new(classic, width);
        let texts = b.settle();
        let pets = label(&texts, "Pets").rect;
        let paws = b.painted("paw-filled");
        assert!(
            paws.iter().any(|r| pets.expand(40.0).contains(r.center())),
            "classic={classic} width={width}: the paw beside Pets ({pets:?}, {paws:?})"
        );
        let plain = label(&texts, "Plain").rect;
        if classic {
            // the Legacy strip is text only, but for a chosen icon: drawn in
            // the room left before the name
            assert!(texts.iter().any(|t| t.source == "Plain"), "the default adds nothing");
            assert!(label(&texts, "Pets").source.starts_with(' '), "room before Pets");
            assert!(
                paws.iter()
                    .any(|r| r.center().x < pets.left() + 20.0 && r.center().x > pets.left() - 4.0),
                "classic: the paw before the name ({pets:?}, {paws:?})"
            );
        } else {
            let bookmarks = b.painted(DEFAULT);
            assert!(
                bookmarks.iter().any(|r| plain.expand(40.0).contains(r.center())),
                "width={width}: Plain keeps the bookmark ({plain:?}, {bookmarks:?})"
            );
            assert!(
                !paws.iter().any(|r| plain.expand(10.0).contains(r.center())),
                "width={width}: no paw on Plain"
            );
        }

        // back to the default: the paw goes
        set(&mut b.prefs.custom_tabs[1], None);
        b.settle();
        assert!(b.painted("paw-filled").is_empty(), "classic={classic} width={width}");
    }
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

/// The last text drawn with this source: in an open menu, the menu's.
fn last_at(texts: &[Text], source: &str) -> egui::Pos2 {
    texts
        .iter()
        .rev()
        .find(|t| t.source == source)
        .unwrap_or_else(|| panic!("{source} is not drawn"))
        .rect
        .center()
}

#[test]
fn the_tab_menu_offers_icon_and_default_resets_it() {
    for classic in [true, false] {
        let mut b = Bench::new(classic, 420.0);
        let texts = b.settle();
        let pets = label(&texts, "Pets").rect.center();
        b.run(click(pets, egui::PointerButton::Secondary));
        let texts = b.settle();
        let icon = aurora_i18n::tr!("inventory-tab-icon");
        let at = last_at(&texts, icon);
        b.run(vec![egui::Event::PointerMoved(at)]);
        for _ in 0..10 {
            b.run(vec![]);
        }
        let texts = b.run(vec![]);
        let default = aurora_i18n::tr!("inventory-tab-icon-default");
        let at = last_at(&texts, default);
        b.run(vec![egui::Event::PointerMoved(at)]);
        b.run(click(at, egui::PointerButton::Primary));
        b.settle();
        assert_eq!(b.prefs.custom_tabs[1].icon, None, "classic={classic}");
        assert_eq!(b.prefs.custom_tabs[1].name, "Pets", "classic={classic}");
    }
}

#[test]
fn customize_tab_shows_the_icon_row() {
    for classic in [true, false] {
        let mut b = Bench::new(classic, 420.0);
        select(&mut b.st, &b.prefs, SLOT_BASE + 1);
        customize(&mut b.st);
        let texts = b.settle();
        assert!(
            texts.iter().any(|t| t.source == aurora_i18n::tr!("inventory-tab-icon")),
            "classic={classic}: the Icon row"
        );
        let default = aurora_i18n::tr!("inventory-tab-icon-default");
        let at = last_at(&texts, default);
        b.run(click(at, egui::PointerButton::Primary));
        b.settle();
        assert_eq!(b.prefs.custom_tabs[1].icon, None, "classic={classic}: Default");
    }
}
