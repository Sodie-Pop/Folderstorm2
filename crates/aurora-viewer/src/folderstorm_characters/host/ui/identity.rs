//! The window's side of checkpoint 7's colours and aliases (session-host
//! plan, section 13): in a character's card, "Alias and colour" opens a
//! field for the alias and the palette's swatches. What an alias may be
//! and how a colour is fitted to the theme are the `folderstorm-session`
//! crate's (`identity`); how both are drawn elsewhere is `faces.rs`.
//!
//! Both are the user's own marks for this host's windows: kept in
//! `session-host.json`, told to no viewer and never sent to a grid.

use super::super::faces;
use super::{Act, BUTTON, Window_};
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use ::folderstorm_session::host::Slot;
use ::folderstorm_session::identity::{AliasError, Colour, MAX_ALIAS};
use aurora_i18n::tr;
use egui::{RichText, Vec2};

/// The character whose alias and colour are being chosen.
#[derive(Debug, Default)]
pub(super) struct Editor {
    slot: Option<usize>,
    /// The alias as typed, before it is set.
    alias: String,
    /// Why the last text was no alias.
    error: Option<String>,
}

impl Editor {
    /// That character left the desk: nothing typed for it stays for the
    /// next one of its slot.
    pub fn close_for(&mut self, slot: usize) {
        if self.slot == Some(slot) {
            *self = Self::default();
        }
    }

    /// The character whose alias is being typed.
    pub fn slot(&self) -> Option<usize> {
        self.slot
    }
}

pub(super) fn colour_name(colour: Colour) -> &'static str {
    match colour {
        Colour::None => tr!("folderstorm-colour-none"),
        Colour::Red => tr!("folderstorm-colour-red"),
        Colour::Orange => tr!("folderstorm-colour-orange"),
        Colour::Yellow => tr!("folderstorm-colour-yellow"),
        Colour::Green => tr!("folderstorm-colour-green"),
        Colour::Teal => tr!("folderstorm-colour-teal"),
        Colour::Blue => tr!("folderstorm-colour-blue"),
        Colour::Violet => tr!("folderstorm-colour-violet"),
        Colour::Pink => tr!("folderstorm-colour-pink"),
    }
}

fn alias_error(error: AliasError) -> String {
    match error {
        AliasError::TooLong => tr!("folderstorm-alias-too-long", max = MAX_ALIAS),
        AliasError::Character(_) => tr!("folderstorm-alias-character").to_owned(),
    }
}

/// One colour to pick: the colour as this theme draws it, ringed when it
/// is the character's.
fn swatch(ui: &mut egui::Ui, p: &Palette, colour: Colour, chosen: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::click());
    let centre = rect.center();
    let painter = ui.painter();
    match faces::tint(p, colour) {
        Some(tint) => {
            painter.circle_filled(centre, 7.0, tint);
        }
        None => {
            // no colour: an empty ring, struck through
            let stroke = egui::Stroke::new(1.2, p.muted);
            painter.circle_stroke(centre, 6.5, stroke);
            painter.line_segment([centre + Vec2::new(-4.5, 4.5), centre + Vec2::new(4.5, -4.5)], stroke);
        }
    }
    if chosen {
        painter.circle_stroke(centre, 10.5, egui::Stroke::new(1.6, p.ink));
    } else if response.hovered() {
        painter.circle_stroke(centre, 10.5, egui::Stroke::new(1.0, p.muted_dim));
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(colour_name(colour))
}

impl Window_ {
    /// Beside a character's name: opens or closes its alias and colour
    /// (lit while they are open).
    /// Checkpoint 10: the card's palette opens the settings window on
    /// that character, where its alias and colour are chosen.
    pub(super) fn identity_button(&self, ui: &mut egui::Ui, p: &Palette, slot: usize, acts: &mut Vec<Act>) {
        if widgets::icon_toggle_sized(ui, p, "palette-filled", tr!("folderstorm-identity-help"), false, 18.0) {
            let open = super::settings::Act::Open(::folderstorm_session::settings::Section::Characters, Some(slot));
            acts.push(Act::Settings(open));
        }
    }

    /// The character whose alias and colour are chosen (None: none).
    pub(super) fn identity_edit(&mut self, slot: Option<usize>) {
        let alias = slot.and_then(|slot| self.host.desk.slot(slot)).and_then(|s| s.alias.clone());
        self.identity = Editor {
            slot,
            alias: alias.unwrap_or_default(),
            error: None,
        };
    }

    /// The alias typed is set, or said not to be one.
    pub(super) fn identity_alias(&mut self, slot: usize, text: &str) {
        match self.host.desk.set_alias(slot, text) {
            Ok(()) => {
                self.identity.error = None;
                if self.identity.slot == Some(slot) {
                    // as it was kept: without the spaces around it
                    self.identity.alias = self.host.desk.slot(slot).and_then(|s| s.alias.clone()).unwrap_or_default();
                }
            }
            Err(error) => self.identity.error = Some(alias_error(error)),
        }
    }

    /// Under a character's row while "Alias and colour" is open.
    /// The character's alias and colour (checkpoint 10: in the settings
    /// window's Characters section).
    pub(super) fn identity_fields(&mut self, ui: &mut egui::Ui, p: &Palette, slot: usize, state: &Slot, acts: &mut Vec<Act>) {
        ui.label(RichText::new(tr!("folderstorm-identity")).strong().color(p.ink));
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr!("folderstorm-alias")).color(p.muted));
            let real = faces::real_name(state);
            let field = widgets::text_field(ui, true, &mut self.identity.alias, |b, _| {
                egui::TextEdit::singleline(b)
                    .id_salt(("folderstorm-identity-alias", slot))
                    .char_limit(MAX_ALIAS)
                    .hint_text(real)
                    .desired_width(190.0)
            });
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let changed = self.identity.alias.trim() != state.alias.as_deref().unwrap_or_default();
            let set = tr!("folderstorm-alias-set");
            if (widgets::action_button_if(ui, p, "check", set, ActionKind::Flat, BUTTON, changed).clicked() || entered) && changed {
                acts.push(Act::Alias(slot, self.identity.alias.clone()));
            }
        });
        if let Some(error) = &self.identity.error {
            ui.label(RichText::new(error).color(p.danger));
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr!("folderstorm-colour")).color(p.muted));
            for colour in std::iter::once(Colour::None).chain(Colour::PALETTE) {
                if swatch(ui, p, colour, state.colour == colour).clicked() {
                    acts.push(Act::Colour(slot, colour));
                }
            }
        });
        ui.label(RichText::new(tr!("folderstorm-identity-note")).color(p.muted));
        ui.add_space(2.0);
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::chat_ui::tests::host;
    use super::super::tests::{draw, window};
    use super::*;

    fn shows(drawn: &[String], text: &str) -> bool {
        drawn.iter().any(|t| t.contains(text))
    }

    #[test]
    fn a_card_shows_the_alias_the_real_name_and_the_colour() {
        let mut app = window(host());
        let before = draw(&mut app, 1120, 760).1;
        app.apply(
            vec![Act::Colour(1, Colour::Teal), Act::Alias(1, "  Shop keeper ".into())],
            std::time::Instant::now(),
        );
        assert_eq!(app.host.desk.slot(1).and_then(|s| s.alias.clone()), Some("Shop keeper".to_owned()));
        draw(&mut app, 1120, 760);
        let (drawn, pixels) = draw(&mut app, 1120, 760);
        // the alias on the card, in the chat's tab and on the quick action;
        // the real name still on the card
        assert!(drawn.iter().filter(|t| t.contains("Shop keeper")).count() >= 3, "{drawn:?}");
        assert!(shows(&drawn, "demo-2"), "{drawn:?}");
        // the colour is painted (the dot, the tab's and the button's lines)
        let p = app.palette;
        let tint = faces::tint(&p, Colour::Teal).expect("a colour");
        let teal = u32::from(tint.r()) << 16 | u32::from(tint.g()) << 8 | u32::from(tint.b());
        let count = |pixels: &[u32]| pixels.iter().filter(|pixel| **pixel & 0x00FF_FFFF == teal).count();
        assert_eq!(count(&before), 0);
        assert!(count(&pixels) > 60, "{} pixels of the colour", count(&pixels));
        // the settings are to be written, and nothing was told to a viewer
        assert!(app.host.unsaved.is_some());
    }

    #[test]
    fn the_settings_offer_the_palette_and_refuse_what_is_no_alias() {
        use super::super::panels::Which;
        use super::super::tests::draw_in;
        use ::folderstorm_session::settings::Section;
        let now = std::time::Instant::now();
        let mut app = window(host());
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(
            !shows(&drawn, tr!("folderstorm-identity-note")),
            "the cards have no editor any more"
        );
        // the card's palette opens the settings on that character
        app.apply(
            vec![Act::Settings(super::super::settings::Act::Open(Section::Characters, Some(0)))],
            now,
        );
        app.settings.view = Some(app.look.view());
        draw_in(&mut app, Which::Settings, 1000, 900);
        let (drawn, pixels) = draw_in(&mut app, Which::Settings, 1000, 900);
        assert!(
            shows(&drawn, tr!("folderstorm-alias")) && shows(&drawn, tr!("folderstorm-colour")),
            "{drawn:?}"
        );
        assert!(shows(&drawn, tr!("folderstorm-identity")));
        assert!(shows(&drawn, tr!("folderstorm-identity-note")));
        // every colour of the palette is painted as a swatch
        let p = app.palette;
        for colour in Colour::PALETTE {
            let tint = faces::tint(&p, colour).expect("a colour");
            let rgb = u32::from(tint.r()) << 16 | u32::from(tint.g()) << 8 | u32::from(tint.b());
            assert!(pixels.iter().any(|pixel| *pixel & 0x00FF_FFFF == rgb), "{colour:?} has no swatch");
            assert!(!colour_name(colour).is_empty());
        }
        // a text that is no alias: said, and nothing changes
        app.apply(vec![Act::Alias(0, "two\nlines".into())], now);
        assert_eq!(app.host.desk.slot(0).and_then(|s| s.alias.clone()), None);
        let (drawn, _) = draw_in(&mut app, Which::Settings, 1000, 900);
        assert!(shows(&drawn, tr!("folderstorm-alias-character")), "{drawn:?}");
        app.apply(vec![Act::Alias(0, "x".repeat(MAX_ALIAS + 1))], now);
        let (drawn, _) = draw_in(&mut app, Which::Settings, 1000, 900);
        assert!(shows(&drawn, &tr!("folderstorm-alias-too-long", max = MAX_ALIAS)), "{drawn:?}");
        // a good one clears the error
        app.apply(vec![Act::Alias(0, "Main".into())], now);
        let (drawn, _) = draw_in(&mut app, Which::Settings, 1000, 900);
        assert!(!shows(&drawn, tr!("folderstorm-alias-character")));
        assert!(shows(&drawn, "Main"));
        // a character removed takes its typed alias with it
        app.identity_edit(Some(1));
        app.apply(vec![Act::Remove(1)], now);
        assert_eq!(app.identity.slot, None);
    }

    /// Every icon this file names is one the viewer embeds.
    #[test]
    fn the_identity_s_icons_exist() {
        for name in ["palette-filled", "check"] {
            assert!(crate::ui::icons::embedded(name), "{name} is not an embedded icon");
        }
    }
}
