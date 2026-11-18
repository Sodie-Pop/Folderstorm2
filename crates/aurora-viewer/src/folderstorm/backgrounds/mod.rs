//! Folderstorm: workspace backgrounds (FEATURES.md row 1.8;
//! folderstorm/plans/workspaces.md, "1.8 in detail").
//!
//! Folderstorm (C++) painted a chosen wallpaper in the spare part of a
//! window stretched over several monitors, outside the world viewport
//! (Preferences › Move & View › Backgrounds): a colour or a picture, fitted
//! per display or spanning them, dimmed at will, a default and a choice per
//! workspace. Here a background is a default, and optionally a layout's own
//! (a layout says where things go); the one in effect is the active
//! layout's own, else the default.
//!
//! - [`record`]: what `settings.json` keeps.
//! - [`place`]: the margins, the monitors' areas, each fit (pure maths).
//! - [`import`]: the copy of a chosen picture, and its decoding.
//! - [`paint`]: the texture, and the shapes put under the interface,
//!   right after the world's labels (two hooks in `app.rs`: the mark in
//!   the pass, the shapes after it), and under the HUD attachments unless
//!   the user chose otherwise (`aurora_render::end_hud_underlay`).
//! - [`demo`]: the demo, `AURORA_DEMO_BACKGROUND`.
//!
//! The world is never covered: only the window minus the world viewport is
//! painted, and nothing at all while the world fills the window.

pub mod demo;
pub mod import;
pub mod paint;
pub mod place;
pub mod record;

#[cfg(test)]
mod tests;

pub use record::Background;

use crate::folderstorm::workspaces::Store;

/// The background in effect: the active layout's own, else the default.
pub fn in_effect(store: &Store) -> &Background {
    store
        .active()
        .and_then(|layout| layout.background.as_ref())
        .unwrap_or(&store.background)
}

/// The names of the copies some background refers to: the default, each
/// layout's own, and those of entries this version keeps unread.
pub fn referenced(store: &Store) -> std::collections::BTreeSet<String> {
    use crate::folderstorm::workspaces::store::Record;
    let mut out = std::collections::BTreeSet::new();
    let backgrounds = std::iter::once(&store.background).chain(store.layouts.iter().filter_map(|r| r.layout()?.background.as_ref()));
    out.extend(backgrounds.filter_map(|b| b.picture.as_ref()).map(|p| p.file.clone()));
    for record in &store.layouts {
        match record {
            Record::Layout(_) => {}
            Record::Kept(kept) => {
                if let Some(file) = kept.raw()["background"]["picture"]["file"].as_str()
                    && record::managed_name(file)
                {
                    out.insert(file.to_owned());
                }
            }
        }
    }
    out
}

/// Marks, in the pass, where the background goes among the frame's shapes:
/// right after the world's labels (the app's hook after
/// `aurora_render::end_world_labels`; [`paint::mark`]).
pub fn mark(ctx: &egui::Context) {
    paint::mark(ctx);
}

/// The background in effect painted under the interface this frame (the
/// app's hook, after the pass): its shapes go where [`mark`] was in
/// `frame`, else first; under the HUD attachments unless the user chose
/// the background over them.
pub fn underlay(ctx: &egui::Context, store: &Store, frame: &mut Vec<egui::epaint::ClippedShape>) {
    let background = in_effect(store);
    set_hides_huds(ctx, background.paints() && store.background_over_huds);
    paint::underlay_in(ctx, background, !store.background_over_huds, &import::folder(), frame);
}

fn hides_huds_id() -> egui::Id {
    egui::Id::unique("folderstorm_background_hides_huds")
}

/// Says whether the background in effect hides the HUD attachments in the
/// margins: painted over them ([`hides_huds`]).
pub fn set_hides_huds(ctx: &egui::Context, hides: bool) {
    ctx.data_mut(|d| d.insert_temp(hides_huds_id(), hides));
}

/// Whether the background hides the HUD attachments at `pos` (points): `pos`
/// is in a margin of the world viewport, and the background in effect is
/// painted over the HUD attachments (*HUDs over the background* off; as
/// [`underlay`] last painted it). A HUD there is not seen, so neither the
/// pointer (`crate::folderstorm::margin_pointer`) nor the build tools'
/// overlay reaches it.
pub fn hides_huds_at(ctx: &egui::Context, pos: egui::Pos2) -> bool {
    hides_huds(ctx) && crate::folderstorm::viewport::world_area(ctx).is_some_and(|world| !world.contains(pos))
}

/// Whether the background in effect, as [`underlay`] last painted it, hides
/// the HUD attachments in the margins (there being margins or not).
pub fn hides_huds(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(hides_huds_id())).unwrap_or(false)
}
