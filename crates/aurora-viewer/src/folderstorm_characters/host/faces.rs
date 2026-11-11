//! How the host's windows tell the characters apart (session-host plan,
//! checkpoint 7): a character's alias in place of its name, and its
//! colour as a small mark beside it. The rules (which colours, how one is
//! fitted to the theme, what an alias may be) are the
//! `folderstorm-session` crate's `identity`; this file draws them.
//!
//! Neither is ever told to a viewer: they are read from the desk and
//! painted here, nothing else.

use crate::theme::Palette;
use ::folderstorm_session::host::Slot;
use ::folderstorm_session::identity::{Colour, TEXT_CONTRAST};
use egui::{Color32, Vec2};

/// What a character is shown as.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Face {
    /// Its alias when it has one, else its real name.
    pub name: String,
    /// Its real name, when the alias stands in its place: for a hover,
    /// and for where a line is about to be said.
    pub real: Option<String>,
    /// Its colour as this theme draws it; None without a colour.
    pub tint: Option<Color32>,
}

impl Face {
    /// The name with the real one beside it, where the user must be sure
    /// of who is meant (who will say a line).
    pub fn full(&self) -> String {
        match &self.real {
            Some(real) => format!("{} ({real})", self.name),
            None => self.name.clone(),
        }
    }
}

/// The avatar's name once logged in, the profile's before.
pub(super) fn real_name(slot: &Slot) -> String {
    match slot.status.as_ref().filter(|s| !s.name.is_empty()) {
        Some(status) => status.name.clone(),
        None => slot.profile.shown().to_owned(),
    }
}

/// The name a character is shown by: its alias when it has one.
pub(super) fn shown_name(slot: &Slot) -> String {
    slot.alias.clone().unwrap_or_else(|| real_name(slot))
}

pub(super) fn face(p: &Palette, slot: &Slot) -> Face {
    Face {
        name: shown_name(slot),
        real: slot.alias.as_ref().map(|_| real_name(slot)),
        tint: tint(p, slot.colour),
    }
}

/// A colour as this theme draws it: readable as a text, and so as a
/// mark, on every background the host's windows have.
pub(super) fn tint(p: &Palette, colour: Colour) -> Option<Color32> {
    let rgb = |c: Color32| [c.r(), c.g(), c.b()];
    let [r, g, b] = colour.fitted(&[rgb(p.panel), rgb(p.field), rgb(p.raised)], TEXT_CONTRAST)?;
    Some(Color32::from_rgb(r, g, b))
}

/// A character's colour as a dot before its name; a ring when it has
/// none, where every row has its mark (the condensed rows).
pub(super) fn dot(ui: &mut egui::Ui, p: &Palette, tint: Option<Color32>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(12.0, 14.0), egui::Sense::hover());
    match tint {
        Some(tint) => ui.painter().circle_filled(rect.center(), 4.5, tint),
        None => ui.painter().circle_stroke(rect.center(), 4.0, egui::Stroke::new(1.0, p.muted_dim)),
    };
    response
}

/// A character's colour as a line under something of its own (its tab,
/// its quick action).
pub(super) fn underline(ui: &egui::Ui, rect: egui::Rect, tint: Color32) {
    let line = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 6.0, rect.bottom() - 3.0),
        egui::pos2(rect.right() - 6.0, rect.bottom() - 1.0),
    );
    ui.painter().rect_filled(line, 1.0, tint);
}

/// A small icon that only says something (hover), in that colour.
pub(super) fn sign(ui: &mut egui::Ui, icon: &str, colour: Color32, tip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
    if let Some(texture) = crate::ui::icons::global(icon) {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter().image(texture.id(), rect.shrink(1.0), uv, colour);
    }
    response.on_hover_text(tip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::folderstorm_session::host::{Config, Desk};
    use ::folderstorm_session::identity::contrast;

    fn rgb(c: Color32) -> [u8; 3] {
        [c.r(), c.g(), c.b()]
    }

    /// A light skin: the default one's backgrounds and inks exchanged.
    fn light(mut p: Palette) -> Palette {
        p.panel = Color32::from_rgb(0xF6, 0xF6, 0xFA);
        p.field = Color32::from_rgb(0xFF, 0xFF, 0xFF);
        p.raised = Color32::from_rgb(0xE6, 0xE6, 0xF0);
        p.ink = Color32::from_rgb(0x14, 0x12, 0x22);
        p
    }

    #[test]
    fn every_colour_is_readable_on_the_theme_s_backgrounds_dark_and_light() {
        let dark = crate::theme::Theme::default().skin_palette();
        let high = crate::high_contrast::palette();
        for p in [dark, light(dark), high] {
            for colour in Colour::PALETTE {
                let tint = tint(&p, colour).expect("a colour of the palette");
                for background in [p.panel, p.field, p.raised] {
                    let ratio = contrast(rgb(tint), rgb(background));
                    assert!(ratio >= TEXT_CONTRAST, "{colour:?} on {background:?}: {ratio:.2}");
                }
            }
            assert_eq!(tint(&p, Colour::None), None);
        }
    }

    #[test]
    fn an_alias_stands_for_the_name_and_the_real_one_stays_at_hand() {
        let p = crate::theme::Theme::default().skin_palette();
        let mut desk = Desk::new(Config::demo(2));
        desk.set_colour(1, Colour::Teal);
        assert_eq!(desk.set_alias(1, "Shop"), Ok(()));
        let plain = face(&p, desk.slot(0).expect("the first character"));
        assert_eq!((plain.name.as_str(), plain.real.as_deref(), plain.tint), ("demo-1", None, None));
        assert_eq!(plain.full(), "demo-1");
        let named = face(&p, desk.slot(1).expect("the second character"));
        assert_eq!((named.name.as_str(), named.real.as_deref()), ("Shop", Some("demo-2")));
        assert_eq!(named.full(), "Shop (demo-2)");
        assert!(named.tint.is_some());
    }
}
