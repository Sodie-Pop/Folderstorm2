use super::*;
use egui::{Id, Pos2, Rect, pos2, vec2};

/// A window over two 1920 x 1080 monitors, the world on the right one.
const WINDOW: (u32, u32) = (3840, 1080);
const WORLD: [u32; 4] = [1920, 0, 1920, 1080];

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(pos2(x, y), vec2(w, h))
}

#[test]
fn photo_mode_takes_the_world_viewport_unless_given_the_whole_window() {
    assert_eq!(super::rect(Some(WORLD), false), Some(WORLD));
    assert_eq!(super::rect(Some(WORLD), true), None, "the whole window asked");
    assert_eq!(super::rect(None, false), None, "viewport off");
    assert_eq!(super::rect(Some([10, 0, 0, 1080]), false), None, "an empty world");
    // the "Window" photo size and proportions: the world's
    assert_eq!(window_size(Some(WORLD), WINDOW), (1920, 1080));
    assert_eq!(window_size(None, WINDOW), WINDOW);
}

#[test]
fn the_af_point_is_kept_in_the_frame_of_the_world() {
    let px = px_rect(Some(WORLD), WINDOW);
    assert_eq!(px, [1920.0, 0.0, 1920.0, 1080.0]);
    // the middle of the frame is the middle of the right monitor, not the
    // window's (the left edge of the world) nor offset twice by the margin
    assert_eq!(px_at(px, pos2(0.5, 0.5)), (2880.0, 540.0));
    assert_eq!(px_at(px, pos2(0.0, 0.0)), (1920.0, 0.0));
    assert_eq!(unit_at(px, (2880.0, 540.0)), pos2(0.5, 0.5));
    assert_eq!(unit_at(px, (1920.0 + 480.0, 270.0)), pos2(0.25, 0.25));
    let back = px_at(px, unit_at(px, (3000.0, 900.0)));
    assert!((back.0 - 3000.0).abs() < 1e-3 && (back.1 - 900.0).abs() < 1e-3);
    // viewport off: the window's own fractions, as in Aurora
    let whole = px_rect(None, WINDOW);
    assert_eq!(px_at(whole, pos2(0.5, 0.5)), (1920.0, 540.0));
    assert_eq!(unit_at(whole, (960.0, 270.0)), pos2(0.25, 0.25));
}

#[test]
fn clicks_and_the_wheel_act_in_the_world_and_never_in_a_margin() {
    let world = Some(WORLD);
    for inside in [(2880.0, 540.0), (1920.0, 0.0), (3839.0, 1079.0)] {
        assert!(takes_pointer(world, inside), "{inside:?}");
    }
    for outside in [(960.0, 540.0), (1919.9, 540.0), (3840.0, 540.0), (2880.0, 1080.0), (-1.0, 0.0)] {
        assert!(!takes_pointer(world, outside), "{outside:?}");
    }
    // the world on the left monitor
    let left = Some([0, 0, 1920, 1080]);
    assert!(takes_pointer(left, (960.0, 540.0)));
    assert!(!takes_pointer(left, (2880.0, 540.0)));
    // viewport off, or photo mode given the whole window: everywhere
    assert!(takes_pointer(None, (960.0, 540.0)));
    assert!(takes_pointer(super::rect(world, true), (960.0, 540.0)));
}

#[test]
fn the_overlay_rectangle_is_in_points_and_inside_the_window() {
    assert_eq!(points(Some(WORLD), 2.0), Some(rect(960.0, 0.0, 960.0, 540.0)));
    assert_eq!(points(Some(WORLD), f32::NAN), Some(rect(1920.0, 0.0, 1920.0, 1080.0)));
    assert_eq!(points(None, 1.0), None);
    let ctx = egui::Context::default();
    let screen = rect(0.0, 0.0, 3840.0, 1080.0);
    let run = |set: Option<Option<Rect>>| {
        let mut got = Rect::NOTHING;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                if let Some(area) = set {
                    set_area(ui.ctx(), area);
                }
                got = super::screen(ui.ctx());
            },
        );
        output.textures_delta.clear();
        got
    };
    assert_eq!(run(None), screen, "nothing set: the whole window");
    assert_eq!(
        run(Some(Some(rect(1920.0, 0.0, 1920.0, 1080.0)))),
        rect(1920.0, 0.0, 1920.0, 1080.0)
    );
    assert_eq!(run(Some(None)), screen, "viewport off");
    // a world rectangle of a larger window than this one is cut to it
    assert_eq!(
        run(Some(Some(rect(1920.0, 0.0, 3840.0, 1080.0)))),
        rect(1920.0, 0.0, 1920.0, 1080.0)
    );
    assert_eq!(run(Some(Some(rect(4000.0, 0.0, 100.0, 100.0)))), screen);
}

/// The viewfinder laid out for a few frames in a 3840 x 1080 window, with
/// photo mode's rectangle `area` (points) and the pointer at `pointer`:
/// the rectangles of its areas by name, the bounds of what it painted, and
/// whether the pointer is over the interface (where the window's wheel and
/// clicks never reach photo mode's handlers).
fn viewfinder_layout(area: Option<Rect>, pointer: Pos2) -> (Vec<(&'static str, Rect)>, Rect, bool) {
    use crate::snapshot::viewfinder::{Display, OnScreen};
    use crate::ui::viewfinder::{Frame, Keys, Viewfinder};
    let ctx = egui::Context::default();
    ctx.all_styles_mut(|s| s.animation_time = 0.0);
    let theme = crate::theme::Theme::default();
    theme.apply(&ctx, 1.0);
    let p = theme.palette();
    let screen = rect(0.0, 0.0, WINDOW.0 as f32, WINDOW.1 as f32);
    let mut settings = crate::snapshot::Settings::default();
    settings.viewfinder.display = Display::Full;
    settings.viewfinder.controls = OnScreen::Always;
    // the camera pad, the tool on the left
    settings.viewfinder.pad = true;
    let mut panel = crate::ui::snapshot::SnapshotUi::default();
    let mut vf = Viewfinder::default();
    vf.hinted = true;
    let mut fov = crate::camera::DEFAULT_FOV;
    let isolation = crate::snapshot::isolate::Isolation::default();
    let catalog = crate::world::env_select::SettingsCatalog::default();
    let photos = Vec::new();
    let thumbs = std::collections::HashMap::new();
    let full = std::collections::HashMap::new();
    let time = |_: i64| String::new();
    let reel = crate::ui::photo_reel::ReelView {
        photos: &photos,
        thumbs: &thumbs,
        full: &full,
        can_send: &[],
        time: &time,
        arriving: false,
    };
    let histogram = [10u32; crate::snapshot::viewfinder::HISTOGRAM_BINS];
    let photo_screen = area.unwrap_or(screen);
    let window = (photo_screen.width() as u32, photo_screen.height() as u32);
    let mut painted = Rect::NOTHING;
    let mut over = false;
    for n in 0..4 {
        let frame = Frame {
            now: 1.0 + f64::from(n) * 0.1,
            photo: window,
            window,
            fps: 60.0,
            focal_mm: 35.0,
            ev: 0.0,
            base_exposure: 1.0,
            roll: 0.0,
            sharp: (2.0, 4.0),
            point: photo_screen.center(),
            face: None,
            group: Vec::new(),
            points: Vec::new(),
            zone: None,
            pitch: 0.0,
            histogram: Some(&histogram),
            still: true,
            review: None,
            first_person: false,
            first_person_allowed: true,
            free_camera: false,
            keys: Keys {
                menu: "Tab",
                af_mode: "N",
                first_person: Some("M"),
            },
            isolation: &isolation,
            isolate_rows: &[],
            isolate_hover: None,
            presets: &[],
            preset_object: None,
            posing: false,
            reel: &reel,
        };
        let view = crate::ui::snapshot::View {
            window,
            photo: window,
            tiles: 1,
            preview: None,
            busy: None,
            message: None,
            last_saved: None,
            gallery: &[],
            reel: None,
            folder: std::path::PathBuf::new(),
            inventory: None,
            item_saved: false,
            profile: None,
            profile_posted: false,
            email: None,
            exposure: 1.0,
            hdr: true,
            env: crate::ui::snapshot::EnvView {
                catalog: &catalog,
                own: crate::world::eep_env::LocalView::SHARED,
                sun: (0.0, 45.0),
                locked: false,
                loading: false,
            },
            back_key: None,
            quality_reduced: false,
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame.now),
                events: vec![egui::Event::PointerMoved(pointer)],
                ..Default::default()
            },
            |ui| {
                set_area(ui.ctx(), area);
                vf.show(
                    ui.ctx(),
                    &p,
                    &mut settings,
                    &mut panel,
                    &mut fov,
                    crate::camera::DEFAULT_FOV,
                    &frame,
                    &view,
                );
                over = crate::ui::widgets::pointer_over_ui(ui.ctx(), pointer);
            },
        );
        output.textures_delta.clear();
        painted = painted_bounds(&ctx, output.shapes.clone());
    }
    let names = [
        "vf_top_left",
        "vf_top_right",
        "vf_dials",
        "vf_dock",
        "vf_ring",
        "vf_af_choices",
        "vf_pad",
    ];
    let areas = names
        .iter()
        .filter_map(|&name| Some((name, ctx.memory(|m| m.area_rect(Id::unique(name)))?)))
        .collect();
    (areas, painted, over)
}

/// The bounds of what the shapes really paint: their triangles' visible
/// corners, inside their clip rectangles (an empty text has no triangle).
fn painted_bounds(ctx: &egui::Context, shapes: Vec<egui::epaint::ClippedShape>) -> Rect {
    let mut bounds = Rect::NOTHING;
    for prim in ctx.tessellate(shapes, 1.0) {
        let egui::epaint::Primitive::Mesh(mesh) = &prim.primitive else {
            continue;
        };
        for v in mesh.vertices.iter().filter(|v| v.color.a() > 0) {
            bounds.extend_with(v.pos.clamp(prim.clip_rect.min, prim.clip_rect.max));
        }
    }
    bounds
}

fn area(areas: &[(&'static str, Rect)], name: &str) -> Rect {
    areas
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, r)| *r)
        .unwrap_or_else(|| panic!("{name} not shown: {areas:?}"))
}

/// The overlay and its tools at their places in the world on the right
/// monitor: the left column at the world's left edge (not centred, not at
/// the window's left, not pushed twice by the margin), the bottom dials at
/// the world's centre, the tool column at its right edge; nothing painted
/// in the margin.
#[test]
fn the_viewfinder_is_laid_out_in_the_world_on_the_right_monitor() {
    let world = rect(1920.0, 0.0, 1920.0, 1080.0);
    let (areas, painted, _) = viewfinder_layout(Some(world), pos2(2880.0, 300.0));
    for (name, r) in &areas {
        assert!(world.expand(0.5).contains_rect(*r), "{name} at {r:?} leaves the world {world:?}");
    }
    assert!(
        painted.left() >= world.left() - 0.5 && painted.right() <= world.right() + 0.5,
        "painted over {painted:?}, the world is {world:?}"
    );
    // the focus and shortcuts column, and the camera pad: at the world's left
    let top_left = area(&areas, "vf_top_left");
    assert!((top_left.left() - (world.left() + 24.0)).abs() < 1.0, "{top_left:?}");
    let pad = area(&areas, "vf_pad");
    assert!((pad.left() - (world.left() + 24.0)).abs() < 1.0, "{pad:?}");
    // the size strip at the world's right
    let top_right = area(&areas, "vf_top_right");
    assert!((top_right.right() - (world.right() - 24.0)).abs() < 1.0, "{top_right:?}");
    // the dials and the lens ring: the world's centre, not the window's
    for name in ["vf_dials", "vf_ring"] {
        let r = area(&areas, name);
        assert!((r.center().x - world.center().x).abs() < 1.0, "{name} at {r:?}");
    }
    // the tool column and the shutter: at the world's right edge
    let dock = area(&areas, "vf_dock");
    assert!(dock.right() <= world.right() && dock.right() > world.right() - 120.0, "{dock:?}");
}

#[test]
fn the_viewfinder_is_laid_out_in_the_world_on_the_left_monitor() {
    let world = rect(0.0, 0.0, 1920.0, 1080.0);
    let (areas, painted, _) = viewfinder_layout(Some(world), pos2(960.0, 300.0));
    for (name, r) in &areas {
        assert!(world.expand(0.5).contains_rect(*r), "{name} at {r:?}");
    }
    assert!(painted.right() <= world.right() + 0.5, "{painted:?}");
    assert!((area(&areas, "vf_top_left").left() - 24.0).abs() < 1.0);
    assert!((area(&areas, "vf_dials").center().x - 960.0).abs() < 1.0);
}

#[test]
fn without_the_viewport_the_viewfinder_keeps_the_whole_window() {
    let window = rect(0.0, 0.0, 3840.0, 1080.0);
    let (areas, painted, _) = viewfinder_layout(None, pos2(1920.0, 300.0));
    assert!((area(&areas, "vf_top_left").left() - 24.0).abs() < 1.0);
    assert!((area(&areas, "vf_pad").left() - 24.0).abs() < 1.0);
    assert!((area(&areas, "vf_dials").center().x - 1920.0).abs() < 1.0);
    assert!(area(&areas, "vf_dock").right() > 3700.0);
    assert!(painted.width() > 3000.0, "the overlay spans the window: {painted:?} of {window:?}");
}

/// The window's wheel reaches photo mode (`App::viewfinder_wheel`) only
/// when egui does not hold the pointer: over the world, no area of the
/// overlay may cover it; in a margin nothing covers it either, and photo
/// mode's own check turns the notch away there.
#[test]
fn the_wheel_over_the_world_is_left_to_photo_mode() {
    let world = rect(1920.0, 0.0, 1920.0, 1080.0);
    let world_px = Some(WORLD);
    for at in [pos2(2880.0, 400.0), pos2(2200.0, 200.0), pos2(3500.0, 300.0)] {
        let (_, _, over) = viewfinder_layout(Some(world), at);
        assert!(!over, "an area of the overlay takes the wheel at {at:?}");
        assert!(takes_pointer(world_px, (at.x, at.y)), "photo mode turns away {at:?}");
    }
    let margin = pos2(960.0, 400.0);
    let (_, _, over) = viewfinder_layout(Some(world), margin);
    assert!(!over, "nothing of the overlay in the margin");
    assert!(!takes_pointer(world_px, (margin.x, margin.y)), "no photo-mode zoom in the margin");
    // over a control (the dials) egui keeps it, as without the viewport
    let (areas, _, _) = viewfinder_layout(Some(world), pos2(2880.0, 400.0));
    let dials = area(&areas, "vf_dials").center();
    let (_, _, over) = viewfinder_layout(Some(world), dials);
    assert!(over, "the dials take their own wheel");
}
