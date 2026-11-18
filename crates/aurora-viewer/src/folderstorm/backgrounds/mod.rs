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
//!   the pass, the shapes after it).
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
/// `frame`, else first.
pub fn underlay(ctx: &egui::Context, store: &Store, frame: &mut Vec<egui::epaint::ClippedShape>) {
    paint::underlay_in(ctx, in_effect(store), &import::folder(), frame);
}
