//! Frame tests: an `egui::Context` run as the app runs it, the background
//! put under the frame's shapes after the pass, the frame drawn with the
//! software painter (no graphics device).

use super::import::tests::scratch;
use super::record::{Background, Fit, Kind};
use super::{demo, import, paint};
use crate::folderstorm::viewport::{self, MonitorLayout};
use crate::folderstorm::workspaces::Store;
use crate::folderstorm::workspaces::store::Layout;
use egui::{Color32, Pos2, Rect, vec2};
use std::path::PathBuf;

/// A viewer window of `size` pixels at 100 %, its world viewport, and what
/// the last frame painted.
struct Viewer {
    ctx: egui::Context,
    size: [u32; 2],
    /// The world's part of the window in pixels; None: all of it.
    world: Option<[u32; 4]>,
    /// The monitors published with the window: (x, y, width, height).
    monitors: Option<MonitorLayout>,
    store: Store,
    folder: PathBuf,
    painter: crate::crash::software::Painter,
    pixels: Vec<u32>,
    /// The shapes the background put in the last frame.
    added: usize,
    /// Whether the pass marks where the background goes, as the app does
    /// after the world's labels (`super::mark`); false: as before that
    /// hook, the background first.
    marked: bool,
    /// A HUD attachment the renderer draws, in pixels (x, y, width,
    /// height): over the whole window, between the world's labels with
    /// what goes under the HUDs and the rest of the interface.
    hud: Option<[u32; 4]>,
    time: f64,
}

/// What the renderer leaves where egui draws nothing: the world's
/// pixels in these tests, and black margins.
const WORLD: Color32 = Color32::from_rgb(1, 2, 3);

/// The colour of the HUD attachment of these tests.
const HUD: Color32 = Color32::from_rgb(250, 200, 0);

impl Viewer {
    fn new(name: &str, width: u32, height: u32) -> Self {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        Viewer {
            ctx,
            size: [width, height],
            world: None,
            monitors: None,
            store: Store::default(),
            folder: scratch(name),
            painter: Default::default(),
            pixels: vec![0; (width * height) as usize],
            added: 0,
            marked: true,
            hud: None,
            time: 0.0,
        }
    }

    /// One frame: the world's part of the window published before the
    /// pass (as the app does); the end of the world's labels and the
    /// background's mark, as the app paints them in the world; a window; the background put in after the pass; the frame
    /// painted over the world as the renderer does it (what precedes the
    /// end of the world's labels cut to the world viewport).
    fn frame(&mut self) {
        self.time += 1.0 / 60.0;
        viewport::set_world_area(&self.ctx, self.world);
        if let Some(layout) = &self.monitors {
            viewport::set_monitors(&self.ctx, layout.clone());
        }
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(self.size[0] as f32, self.size[1] as f32));
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(self.time),
                ..Default::default()
            },
            |ui| {
                // the end of the world's labels (none painted here) and the
                // background's mark, as app.rs paints them in the world
                let ctx = ui.ctx().clone();
                aurora_render::end_world_labels(&ctx);
                if self.marked {
                    super::mark(&ctx);
                }
                // a window over a margin: drawn over the background
                egui::Area::new(egui::Id::unique("a window"))
                    .fixed_pos(egui::pos2(10.0, 10.0))
                    .show(ui.ctx(), |ui| {
                        ui.painter()
                            .rect_filled(Rect::from_min_size(egui::pos2(10.0, 10.0), vec2(40.0, 30.0)), 0.0, Color32::WHITE);
                    });
            },
        );
        let callback = |s: &egui::epaint::ClippedShape| matches!(s.shape, egui::Shape::Callback(_));
        let before = output.shapes.iter().filter(|s| !callback(s)).count();
        let huds_over = !self.store.background_over_huds;
        paint::underlay_in(
            &self.ctx,
            super::in_effect(&self.store),
            huds_over,
            &self.folder,
            &mut output.shapes,
        );
        self.added = output.shapes.iter().filter(|s| !callback(s)).count() - before;
        // the mark taken out again: the end of the world's labels is left
        // among the callbacks, and the end of what goes under the HUDs
        // after a background under them
        let callbacks = output.shapes.iter().filter(|s| callback(s)).count();
        let under = huds_over && self.added > 0;
        assert_eq!(
            callbacks,
            1 + usize::from(under),
            "the background's mark is never left for the renderer"
        );
        let mut primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let (labels, under_huds) = renderer_parts(&mut primitives, self.world, output.pixels_per_point);
        self.painter.update_textures(&output.textures_delta);
        let size = [self.size[0] as usize, self.size[1] as usize];
        // the world drawn by the renderer, black around it
        self.pixels.fill(0);
        let [wx, wy, ww, wh] = self.world.unwrap_or([0, 0, self.size[0], self.size[1]]);
        self.fill([wx, wy, ww, wh], WORLD);
        // the renderer's order: the world's labels, what goes under the
        // HUDs, the HUD attachments, the rest of the interface
        let ppp = output.pixels_per_point;
        self.painter.draw(&mut self.pixels, size, &primitives[..labels], ppp);
        self.painter.draw(&mut self.pixels, size, &primitives[labels..under_huds], ppp);
        if let Some(hud) = self.hud {
            self.fill(hud, HUD);
        }
        self.painter.draw(&mut self.pixels, size, &primitives[under_huds..], ppp);
        self.painter.free_textures(&output.textures_delta);
        output.textures_delta.clear();
    }

    /// Frames until the picture is decoded and drawn (or two seconds).
    fn settle(&mut self) {
        let started = std::time::Instant::now();
        loop {
            self.frame();
            let file = super::in_effect(&self.store).picture.as_ref().map(|p| p.file.clone());
            let waiting = file.is_some_and(|file| paint::status(&self.ctx, &file) == paint::Status::Loading);
            if !waiting || started.elapsed().as_secs() > 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // the frame after the upload draws it
        self.frame();
        self.frame();
    }

    /// `rect` (x, y, width, height pixels) filled with `colour`, as the
    /// renderer draws the world or a HUD.
    fn fill(&mut self, rect: [u32; 4], colour: Color32) {
        let [x0, y0, w, h] = rect;
        for y in y0..(y0 + h).min(self.size[1]) {
            for x in x0..(x0 + w).min(self.size[0]) {
                self.pixels[(y * self.size[0] + x) as usize] = rgb(colour);
            }
        }
    }

    fn at(&self, x: u32, y: u32) -> Color32 {
        let p = self.pixels[(y * self.size[0] + x) as usize];
        Color32::from_rgb((p >> 16) as u8, (p >> 8) as u8, p as u8)
    }

    /// The world's pixels are all the world's.
    fn world_untouched(&self) -> bool {
        let [wx, wy, ww, wh] = self.world.expect("a world viewport");
        (wy..wy + wh).all(|y| (wx..wx + ww).all(|x| self.at(x, y) == WORLD))
    }

    /// The last frame kept as `<folder>/<name>.png` when
    /// `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>`.
    fn picture(&self, name: &str) {
        let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") else {
            return;
        };
        let rgba: Vec<_> = self
            .pixels
            .iter()
            .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
            .collect();
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        image::save_buffer(path, &rgba, self.size[0], self.size[1], image::ColorType::Rgba8).expect("picture written");
    }

    /// The demo's picture copied in, as the default background.
    fn with_picture(&mut self, fit: Fit) {
        let picture = import::store(&demo::png(), "dusk.png", &self.folder).expect("copied");
        self.store.background = Background {
            kind: Kind::Picture,
            colour: [20, 30, 60, 255],
            picture: Some(picture),
            fit,
            ..Default::default()
        };
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// What the renderer does with the frame (`aurora_render::Renderer::render`):
/// the end of the world's labels taken out (`ui_world::take_end`) and what
/// precedes it cut to the world viewport, `world` in pixels; then the end
/// of what goes under the HUD attachments looked for after it
/// (`aurora_render::take_hud_underlay_end`, the renderer's own). The
/// labels' end and the end of what goes under the HUDs.
fn renderer_parts(primitives: &mut Vec<egui::ClippedPrimitive>, world: Option<[u32; 4]>, pixels_per_point: f32) -> (usize, usize) {
    // the end of the world's labels: the callback left once the ends of
    // what goes under the HUDs are emptied (which moves nothing)
    let mut emptied = primitives.clone();
    aurora_render::take_hud_underlay_end(&mut emptied, 0);
    let Some(end) = emptied
        .iter()
        .position(|p| matches!(p.primitive, egui::epaint::Primitive::Callback(_)))
    else {
        let under = aurora_render::take_hud_underlay_end(primitives, 0);
        return (0, under);
    };
    primitives.remove(end);
    let under = aurora_render::take_hud_underlay_end(primitives, end);
    let Some([x, y, w, h]) = world else {
        return (end, under);
    };
    let world = Rect::from_min_size(
        egui::pos2(x as f32, y as f32) / pixels_per_point,
        vec2(w as f32, h as f32) / pixels_per_point,
    );
    for primitive in &mut primitives[..end] {
        primitive.clip_rect = primitive.clip_rect.intersect(world);
    }
    (end, under)
}

fn rgb(c: Color32) -> u32 {
    (u32::from(c.r()) << 16) | (u32::from(c.g()) << 8) | u32::from(c.b())
}

/// Two monitors of 640 × 360 side by side under a window that covers both.
fn two_monitors() -> MonitorLayout {
    MonitorLayout {
        window: [0, 0, 1280, 360],
        monitors: vec![[0, 0, 640, 360], [640, 0, 640, 360]],
    }
}

#[test]
fn nothing_is_painted_while_the_world_fills_the_window_or_nothing_is_chosen() {
    let mut v = Viewer::new("none", 640, 360);
    v.with_picture(Fit::Fill);
    v.settle();
    assert_eq!(v.added, 0, "no margin");
    assert_eq!(
        paint::status(&v.ctx, &v.store.background.picture.clone().expect("chosen").file),
        paint::Status::Unknown,
        "not even decoded"
    );
    v.world = Some([320, 0, 320, 360]);
    v.store.background = Background::default();
    v.settle();
    assert_eq!(v.added, 0, "nothing chosen");
    assert_eq!(v.at(10, 200), Color32::BLACK);
}

#[test]
fn a_colour_fills_the_margins_and_never_the_world() {
    let mut v = Viewer::new("colour", 640, 360);
    v.world = Some([160, 40, 320, 280]);
    v.store.background = Background {
        kind: Kind::Colour,
        colour: [200, 40, 40, 255],
        ..Default::default()
    };
    // (egui shows a new area from its second frame)
    v.frame();
    v.frame();
    assert_eq!(v.added, 4, "four margins");
    assert!(v.world_untouched());
    for (x, y) in [(5, 300), (630, 5), (300, 5), (300, 350)] {
        assert_eq!(v.at(x, y), Color32::from_rgb(200, 40, 40), "({x}, {y})");
    }
    // the window drawn over it
    assert_eq!(v.at(30, 30), Color32::WHITE);
}

/// The bug of the first build: the background was put first among the
/// frame's shapes, before the end of the world's labels, so the renderer
/// cut it to the world viewport with the labels and the margins stayed
/// black, colour and picture alike.
#[test]
fn the_background_goes_after_the_world_s_labels_and_is_not_cut_to_the_world() {
    let mut v = Viewer::new("after-labels", 1280, 360);
    // the world on the right monitor, the whole left one a margin
    v.world = Some([640, 0, 640, 360]);
    v.monitors = Some(two_monitors());
    v.store.background = Background {
        kind: Kind::Colour,
        colour: [40, 160, 90, 255],
        ..Default::default()
    };
    // without the mark, the colour is put first: cut with the labels
    v.marked = false;
    v.frame();
    v.frame();
    assert_eq!(v.added, 1, "the left margin's colour is in the frame");
    assert_eq!(v.at(320, 180), Color32::BLACK, "but cut to the world, as before the fix");
    // with the mark, as the app paints it: after the labels, shown
    v.marked = true;
    v.frame();
    assert_eq!(v.added, 1);
    assert_eq!(v.at(320, 180), Color32::from_rgb(40, 160, 90));
    assert!(v.world_untouched());
    // a picture too
    v.with_picture(Fit::Fill);
    v.settle();
    assert_eq!(v.added, 2, "the colour, then the picture");
    let left = v.at(320, 180);
    assert_ne!(left, Color32::BLACK);
    assert_ne!(left, Color32::from_rgb(20, 30, 60), "the picture, not only its colour");
    assert!(v.world_untouched());
}

/// A HUD moved into a margin: the renderer draws the HUD attachments over
/// the whole window, after the world's labels and before the rest of the
/// interface. By default the background goes under them (drawn before the
/// HUD pass) and the HUD shows; *HUDs over the background* unticked, the
/// background is drawn with the rest and covers it. Windows stay over both.
#[test]
fn a_hud_in_a_margin_shows_over_the_background_unless_the_user_chooses_otherwise() {
    let mut v = Viewer::new("huds-over", 1280, 360);
    v.world = Some([640, 0, 640, 360]);
    v.monitors = Some(two_monitors());
    // in the left margin, under the window's corner (10..50, 10..40)
    v.hud = Some([30, 20, 200, 100]);
    let colour = Color32::from_rgb(40, 160, 90);
    v.store.background = Background {
        kind: Kind::Colour,
        colour: [40, 160, 90, 255],
        ..Default::default()
    };
    assert!(!v.store.background_over_huds, "HUDs over the background by default");
    v.frame();
    v.frame();
    assert_eq!(v.at(150, 80), HUD, "the HUD over the background");
    assert_eq!(v.at(400, 200), colour, "the background around it");
    assert_eq!(v.at(40, 30), Color32::WHITE, "the window over the HUD");
    assert!(v.world_untouched());
    v.picture("background-huds-over");
    // unticked: the background over the HUD, the window still over both
    v.store.background_over_huds = true;
    v.frame();
    assert_eq!(v.at(150, 80), colour, "the background over the HUD");
    assert_eq!(v.at(40, 30), Color32::WHITE);
    assert!(v.world_untouched());
    v.picture("background-over-huds");
    // a picture follows the same order
    v.with_picture(Fit::Fill);
    v.store.background_over_huds = false;
    v.settle();
    assert_eq!(v.at(150, 80), HUD);
    let around = v.at(400, 200);
    assert!(around != HUD && around != Color32::BLACK, "{around:?}");
    v.store.background_over_huds = true;
    v.frame();
    assert_ne!(v.at(150, 80), HUD);
    // a HUD over the world is never under the background, whichever order
    v.hud = Some([700, 20, 100, 100]);
    v.frame();
    assert_eq!(v.at(750, 70), HUD);
}

/// Where the background hides a HUD, as the pointer and the build tools
/// read it: in a margin, while the background in effect is painted over the
/// HUD attachments (`super::hides_huds_at`), as `super::underlay` last
/// painted it.
#[test]
fn the_background_hides_the_huds_only_in_a_margin_and_only_when_over_them() {
    let ctx = egui::Context::default();
    let mut store = Store::default();
    let hides = |store: &Store, world: Option<[u32; 4]>| {
        let mut shapes = Vec::new();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 360.0))),
                ..Default::default()
            },
            |ui| {
                viewport::set_world_area(ui.ctx(), world);
                super::underlay(ui.ctx(), store, &mut shapes);
            },
        );
        crate::ui::login::test_output(output);
        let margin = egui::pos2(100.0, 100.0);
        let in_world = egui::pos2(900.0, 100.0);
        assert!(!super::hides_huds_at(&ctx, in_world), "never in the world");
        super::hides_huds_at(&ctx, margin)
    };
    let world = Some([640, 0, 640, 360]);
    assert!(!hides(&store, world), "no background: nothing hides them");
    store.background = Background {
        kind: Kind::Colour,
        colour: [40, 160, 90, 255],
        ..Default::default()
    };
    assert!(!hides(&store, world), "HUDs over the background, by default");
    store.background_over_huds = true;
    assert!(hides(&store, world), "the background over the HUDs");
    assert!(!hides(&store, None), "no margin while the world fills the window");
}

#[test]
fn the_store_writes_the_huds_order_only_when_the_background_goes_over_them() {
    let mut store = Store::default();
    let text = serde_json::to_value(&store).expect("written");
    assert!(text.get("background_over_huds").is_none());
    store.background_over_huds = true;
    let text = serde_json::to_value(&store).expect("written");
    assert_eq!(text["background_over_huds"], true);
    let read: Store = serde_json::from_value(text).expect("read");
    assert!(read.background_over_huds);
    // anything but `true` reads as HUDs over the background
    let read: Store = serde_json::from_value(serde_json::json!({ "background_over_huds": "yes" })).expect("read");
    assert!(!read.background_over_huds);
}

#[test]
fn the_picture_is_drawn_in_each_monitor_s_margin_and_the_world_kept() {
    let mut v = Viewer::new("picture", 1280, 360);
    // the world on the right monitor, the left one a margin
    v.world = Some([640, 0, 640, 360]);
    v.monitors = Some(two_monitors());
    v.with_picture(Fit::Fill);
    v.settle();
    let file = v.store.background.picture.clone().expect("chosen").file;
    assert_eq!(paint::status(&v.ctx, &file), paint::Status::Shown);
    assert!(v.world_untouched());
    // the arrow's white near the top-left of the picture, the sky under
    // it: the picture is the right way up and fills the left monitor
    // (480 × 270 → 640 × 360)
    let arrow = v.at(53, 60);
    assert!(arrow.r() > 230 && arrow.g() > 230 && arrow.b() > 230, "{arrow:?}");
    let sky = v.at(200, 30);
    assert!(sky.b() > sky.r(), "{sky:?}");
    v.picture("background-fill-left-monitor");
}

#[test]
fn each_fit_and_the_span_over_both_monitors() {
    for (fit, span, name) in [
        (Fit::Contain, false, "background-fit"),
        (Fit::Centre, false, "background-centre"),
        (Fit::Tile, false, "background-tile"),
        (Fit::Stretch, true, "background-span"),
    ] {
        let mut v = Viewer::new(name, 1280, 360);
        // the world in the middle: half of each monitor is a margin
        v.world = Some([320, 0, 640, 360]);
        v.monitors = Some(two_monitors());
        v.with_picture(fit);
        v.store.background.span = span;
        v.settle();
        assert!(v.world_untouched(), "{name}");
        let colour = Color32::from_rgb(20, 30, 60);
        match fit {
            // 480 × 270 in 640 × 360: the same proportions, no border
            Fit::Contain => assert_ne!(v.at(5, 180), colour),
            // at its own size in the middle of each monitor: the colour
            // at the monitor's corner, the picture from x = 80
            Fit::Centre => {
                assert_eq!(v.at(5, 5), colour);
                assert_ne!(v.at(100, 180), colour);
                assert_eq!(v.at(1275, 5), colour);
            }
            // from each monitor's corner: the frame at its top-left
            Fit::Tile => {
                assert_eq!(v.at(1, 1), Color32::from_rgb(235, 225, 200));
                assert_eq!(
                    v.at(641 + 480, 1),
                    Color32::from_rgb(235, 225, 200),
                    "the right monitor's second tile"
                );
            }
            // one picture over the window: its frame at both ends
            _ => {
                assert_eq!(v.at(1, 180), Color32::from_rgb(235, 225, 200));
                assert_eq!(v.at(1278, 180), Color32::from_rgb(235, 225, 200));
            }
        }
        v.picture(name);
    }
}

#[test]
fn a_missing_picture_gives_its_colour() {
    let mut v = Viewer::new("missing", 640, 360);
    v.world = Some([320, 0, 320, 360]);
    let mut store = Store::default();
    demo::seed("missing", &mut store, &mut Default::default(), 640, &v.folder);
    v.store = store;
    v.settle();
    assert_eq!(paint::status(&v.ctx, demo::MISSING), paint::Status::Missing);
    assert_eq!(v.at(100, 100), Color32::from_rgb(72, 24, 32));
    assert!(v.world_untouched());
}

#[test]
fn a_layout_s_own_background_is_in_effect_while_it_is_active() {
    let mut v = Viewer::new("layout", 640, 360);
    v.world = Some([320, 0, 320, 360]);
    v.store.background = Background {
        kind: Kind::Colour,
        colour: [10, 200, 10, 255],
        ..Default::default()
    };
    let mut own = Layout::new("Wall", None, None, Vec::new());
    own.background = Some(Background {
        kind: Kind::Colour,
        colour: [10, 10, 200, 255],
        ..Default::default()
    });
    v.store.add(own).expect("added");
    v.store.add(Layout::new("Plain", None, None, Vec::new())).expect("added");
    // "Plain" is active (the last added): the default
    v.frame();
    assert_eq!(v.at(100, 200), Color32::from_rgb(10, 200, 10));
    v.store.set_active(Some("Wall".into()));
    v.frame();
    assert_eq!(v.at(100, 200), Color32::from_rgb(10, 10, 200));
    // its own taken away: the default again
    assert!(v.store.set_layout_background(0, None));
    v.frame();
    assert_eq!(v.at(100, 200), Color32::from_rgb(10, 200, 10));
}

#[test]
fn the_store_keeps_the_backgrounds_and_writes_nothing_while_default() {
    let mut store = Store::default();
    let text = serde_json::to_value(&store).expect("written");
    assert!(text.get("background").is_none());
    let folder = scratch("store");
    demo::seed("layout", &mut store, &mut Default::default(), 1920, &folder);
    let text = serde_json::to_string(&store).expect("written");
    let read: Store = serde_json::from_str(&text).expect("read");
    assert_eq!(read, store);
    assert_eq!(read.layout(0).and_then(|l| l.background.as_ref()).map(|b| b.fit), Some(Fit::Tile));
    assert_eq!(super::in_effect(&read).fit, Fit::Tile);
    // both refer to the one copy
    assert_eq!(super::referenced(&read).len(), 1);
    // a layout whose background cannot be read keeps the layout
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("json");
    value["layouts"][0]["background"] = serde_json::json!({ "fit": 12 });
    value["background"] = serde_json::json!("plaid");
    let read: Store = serde_json::from_value(value).expect("read");
    let layout = read.layout(0).expect("still a layout");
    assert_eq!(layout.background, None);
    assert_eq!(read.background, Background::default());
    // Update keeps a layout's own background
    let world = store.layout(0).and_then(|l| l.world.clone()).unwrap_or_default();
    store.update(0, world, None, None, Vec::new()).expect("updated");
    assert!(store.layout(0).expect("there").background.is_some());
    let _ = std::fs::remove_dir_all(&folder);
}
