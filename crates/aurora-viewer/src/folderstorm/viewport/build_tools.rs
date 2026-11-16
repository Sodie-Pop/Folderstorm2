//! Folderstorm: the build tools in a custom world viewport.
//!
//! The build tools' camera (`crate::build::geom::Cam`) turns world points
//! into window points for the selection outlines, the manipulators (move
//! arrows, stretch boxes, rotation rings), their grid and guides, and turns
//! the cursor back into a ray to grab and drag them. Made for the whole
//! window, it spread the world's projection over all of it, so with the
//! world drawn in a part of the window everything it drew was centred on
//! the window and every drag missed. [`world_cam`] gives it the world's
//! part instead: its size (what the projection and the handles' sizes
//! follow) and its corner in the window (`Cam::origin`), like the
//! renderer's own projection and picks
//! (`aurora_render::Renderer::set_world_rect`).

use crate::build::geom::Cam;

/// `cam`, made for the whole window, in the world's part of it, `rect`
/// (x, y, width, height in window pixels; None when the world fills the
/// window). A HUD camera is left as it is: the HUDs' own view already lays
/// them out in the world's part (`aurora_render::HudView::area`).
pub fn world_cam(cam: Cam, rect: Option<[u32; 4]>) -> Cam {
    match rect {
        Some([x, y, w, h]) if cam.hud.is_none() && w > 0 && h > 0 => Cam {
            origin: glam::Vec2::new(x as f32, y as f32),
            width: w as f32,
            height: h as f32,
            ..cam
        },
        _ => cam,
    }
}

/// The build overlay's painter kept in the world's part of the window this
/// frame (`super::dialog_area`, the window when the world fills it): a
/// handle half out of the world is cut at its edge, as the Legacy Viewer's
/// 3D viewport cuts it, rather than drawn over the margins.
pub fn clip(ctx: &egui::Context, painter: egui::Painter) -> egui::Painter {
    let area = painter.clip_rect().intersect(super::dialog_area(ctx));
    painter.with_clip_rect(area)
}
