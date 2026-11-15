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
    /// The shapes the background put first in the last frame.
    added: usize,
    time: f64,
}

/// What the renderer leaves where egui draws nothing: the world's
/// pixels in these tests, and black margins.
const WORLD: Color32 = Color32::from_rgb(1, 2, 3);

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
            time: 0.0,
        }
    }

    /// One frame: the world's part of the window published before the
    /// pass (as the app does), an empty interface, the background put
    /// first, the frame painted over the world.
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
                // a window over a margin: drawn over the background
                egui::Area::new(egui::Id::unique("a window"))
                    .fixed_pos(egui::pos2(10.0, 10.0))
                    .show(ui.ctx(), |ui| {
                        ui.painter()
                            .rect_filled(Rect::from_min_size(egui::pos2(10.0, 10.0), vec2(40.0, 30.0)), 0.0, Color32::WHITE);
                    });
            },
        );
        let before = output.shapes.len();
        paint::underlay_in(&self.ctx, super::in_effect(&self.store), &self.folder, &mut output.shapes);
        self.added = output.shapes.len() - before;
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        self.painter.update_textures(&output.textures_delta);
        let size = [self.size[0] as usize, self.size[1] as usize];
        // the world drawn by the renderer, black around it
        self.pixels.fill(0);
        let [wx, wy, ww, wh] = self.world.unwrap_or([0, 0, self.size[0], self.size[1]]);
        for y in wy..wy + wh {
            for x in wx..wx + ww {
                self.pixels[(y * self.size[0] + x) as usize] = rgb(WORLD);
            }
        }
        self.painter.draw(&mut self.pixels, size, &primitives, output.pixels_per_point);
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
