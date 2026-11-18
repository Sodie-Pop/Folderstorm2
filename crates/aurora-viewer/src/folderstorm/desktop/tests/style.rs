//! Frame tests of a shortcut's icon style: the colour and the picture
//! given from its menu (*Icon ›*) and the window *Shortcut icon*, drawn on
//! its face; a picture copied from a file, and its copy deleted once no
//! shortcut wears it; the Inventory textures offered, and drawn, only
//! when the Legacy Viewer would show the user their UUID.

use super::*;
use crate::folderstorm::backgrounds::import::ImportError;
use crate::folderstorm::desktop::style::{self, Picture};
use crate::folderstorm::desktop::style_ui;
use crate::world::inventory::actions as rules;
use aurora_net::inventory::InvItem;

/// A texture of the bench's inventory, full permissions.
const SUNSET: u128 = 9201;
/// A texture without the right to transfer it: never offered.
const BOUGHT: u128 = 9202;
/// Their assets: never drawn, never written.
const SUNSET_ASSET: u128 = 0x5005_E700_0000_9201;
const BOUGHT_ASSET: u128 = 0x5005_E700_0000_9202;
/// The colour the bench gives the fetched texture.
const GREEN: Color32 = Color32::from_rgb(10, 200, 30);

impl Bench {
    /// The bench with two textures in the folder [`MINE`].
    fn with_textures() -> Self {
        let mut b = Bench::new();
        // taller monitors: the window *Shortcut icon* shows whole
        b.size = [1920, 900];
        b.world = Some([640, 0, 640, 900]);
        b.monitors = Some(MonitorLayout {
            window: [0, 0, 1920, 900],
            monitors: vec![[0, 0, 640, 900], [640, 0, 640, 900], [1280, 0, 640, 900]],
        });
        b.pixels = vec![0; (b.size[0] * b.size[1]) as usize];
        b.pointer = pos2(960.0, 450.0);
        let texture = |id: u128, name: &str, owner_mask: u32, asset: u128| InvItem {
            id: Uuid::from_u128(id),
            parent: Uuid::from_u128(MINE),
            name: name.into(),
            asset_type: style::AT_TEXTURE,
            inv_type: 0,
            asset_id: Uuid::from_u128(asset),
            owner: ME,
            base_mask: 0x7fff_ffff,
            owner_mask,
            ..Default::default()
        };
        b.inv.add_items(vec![
            // ("Aaa": first in the list, whatever the demo's inventory holds)
            texture(SUNSET, "Aaa sunset", 0x7fff_ffff, SUNSET_ASSET),
            texture(BOUGHT, "Aaa bought", rules::FULL & !rules::TRANSFER | rules::MOVE, BOUGHT_ASSET),
        ]);
        b
    }

    /// Opens the shortcut's *Icon ›* and clicks its entry `label`.
    fn icon_menu(&mut self, id: u32, label: &str) {
        self.menu(self.cell_of(id).center(), aurora_i18n::tr!("fs-desktop-icon"));
        let entry = self
            .drawn(label)
            .unwrap_or_else(|| panic!("“{label}” is in Icon ›; drawn: {:?}", self.texts));
        self.click(entry.center());
        self.frames(2);
    }

    /// Where the icon of the shortcut `id` is drawn.
    fn icon_of(&self, id: u32) -> Rect {
        let cell = self.cell_of(id);
        let side = self.desktop.size.icon();
        Rect::from_min_size(pos2(cell.center().x - side / 2.0, cell.top() + 8.0), vec2(side, side))
    }

    /// How many pixels of `rect` are `colour`, give or take a little.
    fn pixels_of(&self, rect: Rect, colour: Color32) -> usize {
        let near = |a: u8, b: u8| a.abs_diff(b) <= 3;
        let mut n = 0;
        for y in rect.top() as u32..rect.bottom() as u32 {
            for x in rect.left() as u32..rect.right() as u32 {
                let c = self.at(x, y);
                n += usize::from(near(c.r(), colour.r()) && near(c.g(), colour.g()) && near(c.b(), colour.b()));
            }
        }
        n
    }

    fn fetched(&self) -> Vec<Uuid> {
        self.out
            .iter()
            .filter_map(|r| match r {
                Request::Fetch(asset) => Some(*asset),
                _ => None,
            })
            .collect()
    }

    /// Nothing drawn says an asset id.
    fn says_no_asset(&self) -> bool {
        self.texts.iter().all(|(text, _)| {
            [SUNSET_ASSET, BOUGHT_ASSET]
                .iter()
                .all(|n| !text.contains(&Uuid::from_u128(*n).to_string()))
        })
    }
}

#[test]
fn the_icon_menu_offers_a_colour_a_picture_and_a_reset() {
    let mut b = Bench::new();
    let people = b.add("people", 0, (0, 0));
    b.frames(3);
    b.menu(b.cell_of(people).center(), aurora_i18n::tr!("fs-desktop-icon"));
    for label in [
        aurora_i18n::tr!("fs-desktop-icon-colour"),
        aurora_i18n::tr!("fs-desktop-icon-file"),
        aurora_i18n::tr!("fs-desktop-icon-texture"),
        aurora_i18n::tr!("fs-desktop-icon-reset"),
    ] {
        assert!(b.drawn(label).is_some(), "“{label}” is in Icon ›");
    }
    b.picture("desktop-icon-style-menu");
    b.key(egui::Key::Escape);
    b.frames(2);
    // Colour…: the window, its colours
    b.icon_menu(people, aurora_i18n::tr!("fs-desktop-icon-colour"));
    assert_eq!(style_ui::editing(&b.ctx), Some(people));
    assert!(b.drawn(aurora_i18n::tr!("fs-desktop-style-title")).is_some());
    let green = style::PRESETS[4];
    let swatch: Rect = b
        .ctx
        .data(|d| d.get_temp(style_ui::swatch_id(green)))
        .expect("the colours are drawn");
    b.click(swatch.center());
    b.frames(2);
    let style = |b: &Bench| b.desktop.shortcut(people).expect("a shortcut").style.clone();
    assert_eq!(style(&b).colour, Some(green));
    assert!(b.out.contains(&Request::Changed));
    // drawn in that colour
    let [r, g, bl] = green;
    assert!(
        b.pixels_of(b.icon_of(people), Color32::from_rgb(r, g, bl)) > 20,
        "the icon is drawn in its colour"
    );
    b.picture("desktop-icon-style-colour");
    // Default colour, in the window
    let default = b
        .drawn(aurora_i18n::tr!("fs-desktop-style-colour-default"))
        .expect("Default colour is in the window");
    b.click(default.center());
    b.frames(2);
    assert_eq!(style(&b).colour, None);
    // Reset icon: an icon and a colour go, from the menu
    b.desktop.set_icon(people, Some("star-filled"));
    b.desktop.set_colour(people, Some(green));
    style_ui::close(&b.ctx);
    b.frames(2);
    b.icon_menu(people, aurora_i18n::tr!("fs-desktop-icon-reset"));
    let shortcut = b.desktop.shortcut(people).expect("a shortcut");
    assert!(shortcut.icon.is_none() && shortcut.style.is_default());
}

#[test]
fn a_picture_from_a_file_takes_the_icon_s_place_and_its_copy_goes_with_it() {
    let mut b = Bench::new();
    // an account of this test's own: its folder of copies is pruned
    let account = Uuid::from_u128(0xF11E_0001);
    b.desktop.sign_in(account);
    let people = b.add("people", 0, (0, 0));
    let map = b.add("worldmap", 0, (0, 1));
    b.frames(3);
    let folder = style::folder(account);
    let _ = std::fs::remove_dir_all(&folder);
    // what the dialog's thread does with the file chosen
    let picture = style::store(&crate::folderstorm::backgrounds::import::tests::png(40, 20), "cat.png", &folder).expect("copied");
    let Picture::File { file, .. } = picture.clone() else {
        panic!("a file picture");
    };
    style_ui::answer_dialog(&b.ctx, account, people, Some(Ok(picture.clone())));
    b.frames(2);
    assert_eq!(
        b.desktop.shortcut(people).and_then(|s| s.style.picture.clone()),
        Some(picture.clone())
    );
    assert!(b.out.contains(&Request::Changed));
    // decoded off the event thread, then drawn in its own colours (red on
    // the left, blue on the right)
    let icon = b.icon_of(people);
    for _ in 0..300 {
        if b.pixels_of(icon, Color32::RED) > 20 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        b.frames(1);
    }
    assert!(b.pixels_of(icon, Color32::RED) > 20, "the picture is drawn");
    assert!(b.pixels_of(icon, Color32::BLUE) > 20, "whole");
    b.picture("desktop-icon-style-file");
    // a refused file says why; a cancelled dialog says nothing
    b.out.clear();
    style_ui::answer_dialog(&b.ctx, account, map, Some(Err(ImportError::TooLarge)));
    b.frames(1);
    assert_eq!(b.notices(), vec![aurora_i18n::tr!("fs-background-error-too-large")]);
    style_ui::answer_dialog(&b.ctx, account, map, None);
    b.frames(1);
    assert_eq!(b.notices().len(), 1);
    assert!(!style_ui::dialog_open(&b.ctx));
    // an answer for an account no longer logged in is dropped
    style_ui::answer_dialog(&b.ctx, ME, map, Some(Ok(picture.clone())));
    b.frames(1);
    assert_eq!(b.desktop.shortcut(map).and_then(|s| s.style.picture.clone()), None);
    // the copy is shared while two shortcuts wear it; removed from both, it goes
    b.desktop.set_picture(map, Some(picture));
    b.icon_menu(people, aurora_i18n::tr!("fs-desktop-icon-colour"));
    let remove = b
        .drawn(aurora_i18n::tr!("fs-desktop-style-picture-remove"))
        .expect("Remove the picture is in the window");
    b.click(remove.center());
    b.frames(2);
    assert_eq!(b.desktop.shortcut(people).and_then(|s| s.style.picture.clone()), None);
    assert!(folder.join(&file).is_file(), "the other shortcut wears it still");
    // the other shortcut removed: no shortcut wears it, its copy is deleted
    style_ui::close(&b.ctx);
    b.frames(1);
    b.menu(b.cell_of(map).center(), aurora_i18n::tr!("fs-desktop-remove"));
    assert!(b.desktop.shortcut(map).is_none());
    assert!(!folder.join(&file).exists(), "no shortcut wears it: its copy is deleted");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn only_textures_whose_uuid_the_user_may_see_are_offered_and_drawn() {
    let mut b = Bench::with_textures();
    let people = b.add("people", 0, (0, 0));
    b.frames(3);
    b.icon_menu(people, aurora_i18n::tr!("fs-desktop-icon-texture"));
    assert_eq!(style_ui::editing(&b.ctx), Some(people));
    b.frames(1);
    let row = |b: &Bench, n: u128| b.ctx.data(|d| d.get_temp::<Rect>(style_ui::texture_row_id(Uuid::from_u128(n))));
    assert!(b.drawn("Aaa sunset").is_some(), "a full-permission texture is listed");
    assert!(
        b.drawn("Aaa bought").is_none() && row(&b, BOUGHT).is_none(),
        "a no-transfer one is not"
    );
    assert!(b.says_no_asset());
    b.picture("desktop-icon-style-textures");
    // chosen: its item id is kept, its asset fetched, never shown
    let sunset = row(&b, SUNSET).expect("its row");
    b.click(sunset.center());
    b.frames(2);
    assert_eq!(
        b.desktop.shortcut(people).and_then(|s| s.style.picture.clone()),
        Some(Picture::Texture {
            texture: Uuid::from_u128(SUNSET)
        })
    );
    assert!(b.fetched().contains(&Uuid::from_u128(SUNSET_ASSET)));
    assert!(!b.fetched().contains(&Uuid::from_u128(BOUGHT_ASSET)));
    let written = serde_json::to_string(&b.desktop).expect("written");
    assert!(written.contains(&Uuid::from_u128(SUNSET).to_string()));
    assert!(!written.contains(&Uuid::from_u128(SUNSET_ASSET).to_string()), "never its asset id");
    // fetched: drawn
    let texture = b.ctx.load_texture(
        "test-sunset",
        egui::ColorImage::new([4, 4], vec![GREEN; 16]),
        egui::TextureOptions::LINEAR,
    );
    b.images.insert(Uuid::from_u128(SUNSET_ASSET), texture);
    style_ui::close(&b.ctx);
    b.frames(3);
    let icon = b.icon_of(people);
    assert!(b.pixels_of(icon, GREEN) > 200, "the texture is drawn in the icon's place");
    assert!(b.says_no_asset());
    b.picture("desktop-icon-style-texture");
    // its permissions changed (no copy any more): the icon again, no fetch
    let mask = |b: &mut Bench, owner_mask: u32| {
        let mut item = b.inv.items[&Uuid::from_u128(SUNSET)].clone();
        item.owner_mask = owner_mask;
        b.inv.add_items(vec![item]);
    };
    mask(&mut b, rules::FULL & !rules::COPY);
    b.out.clear();
    b.frames(2);
    assert_eq!(b.pixels_of(icon, GREEN), 0, "not drawn without the rights");
    assert!(b.fetched().is_empty());
    // RLVa hides the Inventory: not drawn, not offered
    mask(&mut b, 0x7fff_ffff);
    b.frames(2);
    assert!(b.pixels_of(icon, GREEN) > 200);
    b.no_inventory = true;
    b.frames(2);
    assert_eq!(b.pixels_of(icon, GREEN), 0, "nothing of the Inventory under @showinv");
    b.menu(b.cell_of(people).center(), aurora_i18n::tr!("fs-desktop-icon"));
    let entry = b.drawn(aurora_i18n::tr!("fs-desktop-icon-texture")).expect("listed, greyed");
    b.click(entry.center());
    b.frames(2);
    assert_eq!(style_ui::editing(&b.ctx), None, "a greyed entry opens nothing");
}

#[test]
fn a_shortcut_whose_face_rlva_hides_shows_no_style_of_its_own() {
    let mut b = Bench::new();
    let folder = b.add_folder(MINE, 0, (0, 0));
    let green = style::PRESETS[4];
    b.desktop.set_colour(folder, Some(green));
    b.frames(3);
    let [r, g, bl] = green;
    let colour = Color32::from_rgb(r, g, bl);
    assert!(b.pixels_of(b.icon_of(folder), colour) > 20);
    b.no_inventory = true;
    b.frames(2);
    assert_eq!(b.pixels_of(b.icon_of(folder), colour), 0, "the lock, in the theme's colour");
}
