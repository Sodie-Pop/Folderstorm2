//! Folderstorm: the build tools in a custom world viewport
//! (`crate::folderstorm::viewport::build_tools`). Declared in `crate::build`
//! (as `folderstorm_viewport_tests`) to reach its private edge panning.

use super::edge_pan::{EdgePan, Motion, drag_cursor};
use super::geom::Cam;
use super::manip::Manip;
use super::{BuildSettings, EditMode, Mods};
use crate::folderstorm::viewport::build_tools::world_cam;
use crate::world::World;
use aurora_net::build::BuildCmd;
use glam::{Quat, Vec3};
use std::time::Instant;

/// A window over two 1920 × 1080 monitors, the world on the right one.
const WINDOW: (u32, u32) = (3840, 1080);
const RECT: [u32; 4] = [1920, 0, 1920, 1080];
/// The world on the middle third, with a strip above and below it.
const BOXED: [u32; 4] = [1280, 120, 1280, 840];

/// A camera on `target` with the proportions of a `w` × `h` view, made for
/// a view of that size at the window's corner.
fn camera(target: Vec3, w: u32, h: u32) -> Cam {
    let eye = target + Vec3::new(4.0, -6.0, 3.0);
    let fov = 60f32.to_radians();
    let mut camera = crate::camera::Camera::default();
    camera.position = eye;
    camera.target = target;
    camera.fov_y = fov;
    let vp = camera.proj(w as f32 / h as f32) * camera.view();
    Cam::new(eye, target, vp, fov, w as f32, h as f32, 1.25)
}

/// The same camera as the app makes it (the whole window's size, the
/// world's proportions), then placed in `rect`.
fn in_rect(target: Vec3, rect: [u32; 4]) -> Cam {
    let window = camera(target, rect[2], rect[3]);
    let window = Cam {
        width: WINDOW.0 as f32,
        height: WINDOW.1 as f32,
        ..window
    };
    world_cam(window, Some(rect))
}

fn close(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01
}

#[test]
fn the_view_centre_lands_on_the_world_centre() {
    for rect in [RECT, BOXED] {
        let cam = in_rect(Vec3::ZERO, rect);
        let centre = cam.project_px(Vec3::ZERO).unwrap();
        let expected = (rect[0] as f32 + rect[2] as f32 / 2.0, rect[1] as f32 + rect[3] as f32 / 2.0);
        assert!(close(centre, expected), "{centre:?} {expected:?}");
        // egui points too (what the outlines and handles are drawn with)
        let points = cam.project(Vec3::ZERO).unwrap();
        assert!((points.x * 1.25 - expected.0).abs() < 0.01 && (points.y * 1.25 - expected.1).abs() < 0.01);
    }
}

#[test]
fn points_project_into_the_rect_as_in_a_view_of_its_size() {
    for rect in [RECT, BOXED] {
        let world = in_rect(Vec3::ZERO, rect);
        let alone = camera(Vec3::ZERO, rect[2], rect[3]);
        for p in [Vec3::new(1.0, 0.5, -0.3), Vec3::new(-2.0, 1.0, 1.5), Vec3::new(0.2, -1.0, 0.0)] {
            let (x, y) = alone.project_px(p).unwrap();
            let expected = (x + rect[0] as f32, y + rect[1] as f32);
            let got = world.project_px(p).unwrap();
            assert!(close(got, expected), "{p}: {got:?} {expected:?}");
            assert!(got.0 >= rect[0] as f32 && got.0 < (rect[0] + rect[2]) as f32);
            assert!(got.1 >= rect[1] as f32 && got.1 < (rect[1] + rect[3]) as f32);
        }
    }
}

#[test]
fn a_click_in_the_rect_gives_the_ray_of_a_view_of_its_size() {
    for rect in [RECT, BOXED] {
        let world = in_rect(Vec3::ZERO, rect);
        let alone = camera(Vec3::ZERO, rect[2], rect[3]);
        for (x, y) in [(0.0, 0.0), (640.0, 360.0), (1279.0, 839.0), (100.0, 700.0)] {
            let (o, d) = world.ray(x + rect[0] as f32, y + rect[1] as f32);
            let (eo, ed) = alone.ray(x, y);
            assert!(o.abs_diff_eq(eo, 1e-4) && d.abs_diff_eq(ed, 1e-4), "{x} {y}: {d} {ed}");
        }
        // and the ray through a projected point goes through it
        let p = Vec3::new(0.7, -0.4, 0.9);
        let (x, y) = world.project_px(p).unwrap();
        let (o, d) = world.ray(x, y);
        let off = (p - o) - d * (p - o).dot(d);
        assert!(off.length() < 1e-3, "{off}");
    }
}

#[test]
fn handles_keep_the_size_of_a_view_of_the_rect() {
    let world = in_rect(Vec3::ZERO, BOXED);
    let alone = camera(Vec3::ZERO, BOXED[2], BOXED[3]);
    let p = Vec3::new(0.5, 0.5, 0.5);
    assert!((world.meters_for_pixels(p, 50.0) - alone.meters_for_pixels(p, 50.0)).abs() < 1e-6);
    assert!((world.pixel_meter_ratio() - alone.pixel_meter_ratio()).abs() < 1e-3);
}

#[test]
fn the_whole_window_and_the_huds_are_left_alone() {
    let cam = camera(Vec3::ZERO, WINDOW.0, WINDOW.1);
    let same = world_cam(cam, None);
    assert_eq!((same.origin, same.width, same.height), (cam.origin, cam.width, cam.height));
    let hud = Cam::for_hud(aurora_render::HudView::new([WINDOW.0, WINDOW.1], 1.0, -1.0, 1.0), 1.0);
    let same = world_cam(hud, Some(RECT));
    assert_eq!((same.origin, same.width, same.height), (hud.origin, hud.width, hud.height));
}

#[test]
fn edge_panning_follows_the_edges_of_the_world() {
    let world = in_rect(Vec3::ZERO, RECT);
    let alone = camera(Vec3::ZERO, RECT[2], RECT[3]);
    let shift = |(x, y): (f32, f32)| (x + RECT[0] as f32, y + RECT[1] as f32);
    for cursor in [(2.0, 540.0), (1917.0, 540.0), (960.0, 3.0), (960.0, 540.0)] {
        let (mut a, mut b) = (EdgePan::default(), EdgePan::default());
        let motion = Motion::Plane(Vec3::Z);
        let got = a.update(&world, shift(cursor), Vec3::ZERO, motion, 0.05, true);
        let expected = b.update(&alone, cursor, Vec3::ZERO, motion, 0.05, true);
        assert!(got.abs_diff_eq(expected, 1e-5), "{cursor:?}: {got} {expected}");
    }
    // at the world's left edge it pans, though the window goes on
    let pan = EdgePan::default().update(
        &world,
        (RECT[0] as f32 + 2.0, 540.0),
        Vec3::ZERO,
        Motion::Plane(Vec3::Z),
        0.05,
        true,
    );
    assert!(pan.length() > 0.0);
    for cursor in [(-50.0, 540.0), (2000.0, 1200.0), (900.0, 500.0)] {
        let got = drag_cursor(&world, shift(cursor), shift((960.0, 540.0)), (4.0, -3.0));
        let expected = shift(drag_cursor(&alone, cursor, (960.0, 540.0), (4.0, -3.0)));
        assert!(close(got, expected), "{cursor:?}: {got:?} {expected:?}");
    }
}

/// A one-metre cube of the demo region, selected, and its key.
fn cube() -> (World, crate::world::objects::ObjKey, usize) {
    let mut world = World::new(std::sync::Arc::new(crate::scene::avatar::AvatarLibrary::load()));
    for ev in crate::demo::events().into_iter().chain(crate::demo::hud::events()) {
        world.apply(ev);
    }
    let mut update = crate::demo::hud::object(31, false);
    update.parent_id = 0;
    update.state = 0;
    update.position = Vec3::new(128.0, 128.0, 32.0);
    update.rotation = Quat::IDENTITY;
    update.scale = Vec3::ONE;
    let handle = world.main_region.unwrap();
    world.apply(aurora_net::NetEvent::ObjectUpdates {
        handle,
        objects: vec![update.clone()],
    });
    let idx = world.objects.index_of_uuid(&update.full_id).unwrap();
    let key = world.objects.get(idx).unwrap().key;
    (world, key, idx)
}

/// Grabs a handle at `grab` and drags it to `to`; the cube's position and
/// size after the drag, or None when nothing was grabbed.
fn drag(cam: &Cam, mode: EditMode, grab: (f32, f32), to: (f32, f32)) -> Option<(Vec3, Vec3)> {
    let (mut world, key, idx) = cube();
    let now = Instant::now();
    let settings = BuildSettings {
        snap: false,
        follow_edit_object: true,
        ..Default::default()
    };
    let mut manip = Manip::default();
    if !manip.try_grab(&mut world, &settings, cam, &[key], false, mode, grab, now) {
        return None;
    }
    let mut out: Vec<BuildCmd> = Vec::new();
    manip.update(
        &mut world,
        &settings,
        cam,
        &[key],
        false,
        mode,
        to,
        false,
        Mods::default(),
        now,
        &mut out,
        None,
    );
    let o = world.objects.get(idx).unwrap();
    Some((o.position, o.scale))
}

#[test]
fn handles_are_grabbed_and_dragged_in_the_world_rect() {
    let (world, key, _) = cube();
    let bounds = super::bounds_of(&world, &[key], false, Instant::now()).unwrap();
    let (centre, corner) = (bounds.center, bounds.center + bounds.rotation * bounds.half);
    for rect in [RECT, BOXED] {
        let world = in_rect(centre, rect);
        let alone = camera(centre, rect[2], rect[3]);
        let shift = |(x, y): (f32, f32)| (x + rect[0] as f32, y + rect[1] as f32);
        // a stretch box at the cube's corner, pulled outwards
        let start = alone.project_px(corner).unwrap();
        let mid = alone.project_px(centre).unwrap();
        let to = (start.0 + (start.0 - mid.0) * 0.5, start.1 + (start.1 - mid.1) * 0.5);
        let expected = drag(&alone, EditMode::Stretch, start, to).expect("grabbed in a view of the rect's size");
        assert!(expected.1.min_element() > 1.0, "{expected:?}");
        let got = drag(&world, EditMode::Stretch, shift(start), shift(to)).expect("grabbed in the world rect");
        assert!(
            got.0.abs_diff_eq(expected.0, 1e-4) && got.1.abs_diff_eq(expected.1, 1e-4),
            "{got:?} {expected:?}"
        );
        // the same place in the window, but not in the world, grabs nothing
        assert_eq!(drag(&world, EditMode::Stretch, start, to), None);
        // a move arrow (found along the cube's x axis), dragged sideways
        let (grab, expected) = (1..40)
            .filter_map(|i| alone.project_px(centre + Vec3::X * (0.1 * i as f32)))
            .find_map(|at| {
                let moved = drag(&alone, EditMode::Move, at, (at.0 + 80.0, at.1 + 30.0))?;
                (!moved.0.abs_diff_eq(centre, 1e-3)).then_some((at, moved))
            })
            .expect("an arrow in a view of the rect's size");
        let got = drag(&world, EditMode::Move, shift(grab), shift((grab.0 + 80.0, grab.1 + 30.0))).expect("grabbed in the world rect");
        assert!(got.0.abs_diff_eq(expected.0, 1e-4), "{got:?} {expected:?}");
    }
}

#[test]
fn the_overlay_is_cut_at_the_world_edges() {
    let ctx = egui::Context::default();
    let mut clip = None;
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(3840.0, 1080.0))),
            ..Default::default()
        },
        |ui| {
            crate::folderstorm::viewport::set_world_area(ui.ctx(), Some(RECT));
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            clip = Some(crate::folderstorm::viewport::build_tools::clip(ui.ctx(), painter).clip_rect());
        },
    );
    crate::ui::login::test_output(output);
    let clip = clip.unwrap();
    assert_eq!(clip, egui::Rect::from_min_size(egui::pos2(1920.0, 0.0), egui::vec2(1920.0, 1080.0)));
}
