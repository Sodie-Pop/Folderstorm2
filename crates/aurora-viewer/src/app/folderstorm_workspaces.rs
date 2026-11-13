//! Folderstorm: layouts in the app (the rules are in
//! `crate::folderstorm::viewport::bars`; folderstorm/plans/workspaces.md,
//! checkpoint 3).

use super::*;
use crate::folderstorm::viewport;

impl App {
    /// The bars' area this frame, in points: inside the custom world
    /// viewport `world` (the renderer's, in window pixels, already under
    /// the session host's bar), or around it, each side's bars in the
    /// margin when it has room (`viewport::bars`). None leaves the bars on
    /// the whole window of `size` pixels.
    pub(super) fn folderstorm_bars_rect(&self, ctx: &egui::Context, world: Option<[u32; 4]>, size: (u32, u32)) -> Option<egui::Rect> {
        let s = &self.settings.folderstorm_viewport;
        let inside = viewport::bars_rect(s, world, ctx.pixels_per_point())?;
        if !s.bars_around {
            return Some(inside);
        }
        let mut bars = viewport::bars::thickness(ctx, self.interface.last_room.map(|(room, _)| room));
        // the session host's bar is docked over the world's top
        // (`app/folderstorm_characters/bar.rs`): the top bars stay under it
        if self.whole_world_viewport(size) != world {
            bars.top = 0.0;
        }
        Some(viewport::bars::around(inside, ctx.content_rect(), bars))
    }
}
