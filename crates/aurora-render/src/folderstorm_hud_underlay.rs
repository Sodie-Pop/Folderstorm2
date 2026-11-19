//! Folderstorm: interface shapes drawn under the HUD attachments, over the
//! whole window (folderstorm/plans/workspaces.md, "1.8 as built").
//!
//! The renderer draws the interface in two parts around the HUD
//! attachments: what precedes `end_world_labels` (the world's labels, cut
//! to the world viewport), then the HUD attachments over the whole window,
//! then the rest of the interface. A background painted in the margins the
//! world viewport leaves cannot go with the labels (it would be cut away)
//! and, with the rest, covers a HUD moved into a margin.
//!
//! [`end_hud_underlay`] marks a third part: the shapes of the rest that
//! precede it are drawn after the world's labels and before the HUD
//! attachments, not cut to the world. Without the marker the rest is drawn
//! after the HUD attachments, as upstream does.

use egui::ClippedPrimitive;
use egui::epaint::Primitive;
use std::sync::Arc;

struct End;

/// The marker that ends the shapes drawn under the HUD attachments: put
/// after them, after `end_world_labels`, among the frame's shapes. It
/// survives tessellation and is never drawn.
pub fn end_hud_underlay(rect: egui::Rect) -> egui::Shape {
    egui::Shape::Callback(egui::PaintCallback {
        rect,
        callback: Arc::new(End),
    })
}

fn is_end(primitive: &ClippedPrimitive) -> bool {
    matches!(&primitive.primitive, Primitive::Callback(c) if c.callback.is::<End>())
}

/// Where the shapes drawn under the HUD attachments end among
/// `primitives`, looked for from `from` (the end of the world's labels):
/// the first marker's place; `from` without one (nothing under the HUD
/// attachments). Every marker, wherever it is, becomes an empty mesh with
/// an empty clip, which egui-wgpu skips: no index of the frame moves (the
/// end of the world's labels stays right), and none of these callbacks,
/// which egui-wgpu does not know, reaches it.
pub fn take_end(primitives: &mut [ClippedPrimitive], from: usize) -> usize {
    let mut end = None;
    for (at, p) in primitives.iter_mut().enumerate() {
        if is_end(p) {
            if at >= from && end.is_none() {
                end = Some(at);
            }
            p.primitive = Primitive::Mesh(egui::Mesh::default());
            p.clip_rect = egui::Rect::NOTHING;
        }
    }
    end.unwrap_or(from.min(primitives.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Color32, Rect, pos2, vec2};

    fn colors(jobs: &[ClippedPrimitive]) -> Vec<Color32> {
        jobs.iter()
            .flat_map(|p| match &p.primitive {
                Primitive::Mesh(mesh) => mesh.vertices.iter().map(|v| v.color).collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect()
    }

    fn square(x: f32) -> Rect {
        Rect::from_min_size(pos2(x, 0.0), vec2(20.0, 20.0))
    }

    /// A frame: a label of the world (red), the end of the world's labels,
    /// a margin's background (green), with or without the end of what goes
    /// under the HUD attachments, a HUD's label (blue), a window (yellow).
    fn frame(under: bool) -> Vec<ClippedPrimitive> {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |_| {
            let background = ctx.layer_painter(egui::LayerId::background());
            background.rect_filled(square(0.0), 0.0, Color32::RED);
            crate::end_world_labels(&ctx);
            background.rect_filled(square(20.0), 0.0, Color32::GREEN);
            if under {
                background.add(end_hud_underlay(ctx.content_rect()));
            }
            background.rect_filled(square(40.0), 0.0, Color32::BLUE);
            ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::unique("window")))
                .rect_filled(square(60.0), 0.0, Color32::YELLOW);
        });
        output.textures_delta.clear();
        ctx.tessellate(output.shapes, output.pixels_per_point)
    }

    /// The three parts in the renderer's order: the world's labels, what
    /// goes under the HUD attachments, what goes over them.
    fn parts(primitives: &mut Vec<ClippedPrimitive>) -> [Vec<Color32>; 3] {
        let labels = crate::ui_world::take_end(primitives);
        let under = take_end(primitives, labels);
        [
            colors(&primitives[..labels]),
            colors(&primitives[labels..under]),
            colors(&primitives[under..]),
        ]
    }

    #[test]
    fn what_precedes_the_marker_goes_under_the_huds_and_the_rest_over_them() {
        let mut primitives = frame(true);
        let [labels, under, over] = parts(&mut primitives);
        assert!(labels.contains(&Color32::RED) && !labels.contains(&Color32::GREEN));
        assert!(under.contains(&Color32::GREEN) && !under.contains(&Color32::RED));
        assert!(!under.contains(&Color32::BLUE) && !under.contains(&Color32::YELLOW));
        assert!(over.contains(&Color32::BLUE) && over.contains(&Color32::YELLOW));
        assert!(!over.contains(&Color32::GREEN));
        assert!(
            primitives.iter().all(|p| matches!(p.primitive, Primitive::Mesh(_))),
            "no marker is left for egui-wgpu"
        );
    }

    #[test]
    fn without_the_marker_everything_after_the_labels_goes_over_the_huds() {
        let mut primitives = frame(false);
        let [labels, under, over] = parts(&mut primitives);
        assert!(labels.contains(&Color32::RED));
        assert!(under.is_empty());
        for color in [Color32::GREEN, Color32::BLUE, Color32::YELLOW] {
            assert!(over.contains(&color), "{color:?}");
        }
    }

    #[test]
    fn a_marker_among_the_labels_is_emptied_in_place_and_ends_nothing() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1.0, 1.0));
        let mesh = || ClippedPrimitive {
            clip_rect: rect,
            primitive: Primitive::Mesh(egui::Mesh::default()),
        };
        let marker = || ClippedPrimitive {
            clip_rect: rect,
            primitive: match end_hud_underlay(rect) {
                egui::Shape::Callback(c) => Primitive::Callback(c),
                _ => unreachable!("a callback"),
            },
        };
        let mut primitives = vec![mesh(), marker(), mesh(), marker(), mesh()];
        assert_eq!(take_end(&mut primitives, 2), 3, "looked for from the end of the labels");
        assert_eq!(primitives.len(), 5, "nothing moves");
        for at in [1, 3] {
            assert!(matches!(&primitives[at].primitive, Primitive::Mesh(m) if m.is_empty()));
            assert_eq!(primitives[at].clip_rect, Rect::NOTHING, "an empty mesh, an empty clip");
        }
        let mut primitives = vec![mesh(), marker(), mesh()];
        assert_eq!(take_end(&mut primitives, 2), 2, "one before the labels' end ends nothing");
        assert_eq!(take_end(&mut primitives, 9), 3, "a start past the end");
    }
}
