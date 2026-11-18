//! Folderstorm: Preferences › Workspaces (`crate::folderstorm::workspaces`,
//! FEATURES.md rows 1.3, 1.4, 1.8 and 1.10), a page of the Interface
//! section after Layout. Folderstorm (C++) had its layouts under Move &
//! View › Layout and its workspaces under Move & View › Workspaces.
//!
//! Four tabs, in the order of use: *Saved* ([`saved`]: the workspaces and
//! the layouts in a list, the selected entry's editor beside it, *New from
//! current…*),
//! *World viewport* and *Margins* (`super::folderstorm_viewport`: the live
//! arrangement a layout saves, and what the margins show), *Switching*
//! ([`switching`]: the standing preferences, and the ways back). Like the
//! rest of Preferences, everything applies at once; Update and Delete ask
//! first, in the page. The saved entries are not settings of a row:
//! "Reset Workspaces" never deletes one.

mod saved;
mod switching;
mod workspace_editor;

use super::super::cx::Cx;
use crate::folderstorm::workspaces::monitors;
use crate::folderstorm::workspaces::store::{Layout, Monitor, WindowFrame};
use crate::theme::Palette;
use crate::ui::widgets;
use aurora_i18n::tr;
use egui::{Sense, Vec2, pos2, vec2};

/// The page's tabs, for the other windows that open it on one.
pub mod tab {
    pub const SAVED: usize = 0;
    pub const VIEWPORT: usize = 1;
    pub const MARGINS: usize = 2;
    pub const SWITCHING: usize = 3;
}

/// The embedded Tabler icons of the page.
const ICON_CHECK: &str = "check";
const ICON_SAVE: &str = "device-floppy-filled";
const ICON_RENAME: &str = "pencil-filled";
const ICON_DELETE: &str = "trash-filled";
const ICON_CANCEL: &str = "x";
const ICON_BACK: &str = "arrow-back-up";
const ICON_RESET: &str = "restore";
const ICON_UP: &str = "arrow-up";
const ICON_DOWN: &str = "arrow-down";
const ICON_LOCK: &str = "lock-filled";
const ICON_UNLOCK: &str = "lock-open";
const ICON_STAR: &str = "star-filled";
/// A layout's and a workspace's, as World › Layouts and the switcher have them.
const ICON_LAYOUT: &str = "layout-filled";
const ICON_WORKSPACE: &str = "app-window-filled";

const ROW_H: f32 = 28.0;
const BUTTON_H: f32 = 26.0;
/// The width of a list of layouts to pick from.
const PICKER_W: f32 = 220.0;

/// The names of the tabs.
pub(in crate::ui::options) fn tabs() -> Vec<&'static str> {
    vec![
        tr!("fs-workspaces-tab-saved"),
        tr!("folderstorm-viewport"),
        tr!("fs-workspaces-tab-margins"),
        tr!("fs-workspaces-tab-switching"),
    ]
}

/// The rows of a tab.
pub(in crate::ui::options) fn page(cx: &mut Cx, tab: usize) {
    match tab {
        tab::SAVED => saved::page(cx),
        tab::VIEWPORT => super::folderstorm_viewport::group(cx),
        tab::MARGINS => super::folderstorm_viewport::margins(cx),
        _ => switching::page(cx),
    }
}

/// A question asked in the page before a change that loses something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Update,
    Delete,
}

fn muted(ui: &mut egui::Ui, p: &Palette, text: impl Into<String>) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(p.muted)).wrap());
}

fn warning(ui: &mut egui::Ui, p: &Palette, text: impl Into<String>) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(p.danger)).wrap());
}

/// What a line of the list shows.
struct Line<'a> {
    /// The kind's icon.
    icon: &'static str,
    name: &'a str,
    selected: bool,
    active: bool,
    usable: bool,
    locked: bool,
    favorite: bool,
    /// At the right: "In use", "Modified", why it cannot be used.
    note: &'a str,
}

fn paint_icon(ui: &egui::Ui, name: &str, center: egui::Pos2, side: f32, tint: egui::Color32) {
    if let Some(icon) = crate::ui::icons::global(name) {
        ui.painter().image(
            icon.id(),
            egui::Rect::from_center_size(center, Vec2::splat(side)),
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            tint,
        );
    }
}

/// One line of the list: the kind's icon (teal for the one in use), the
/// name, its star and lock, a note at the right.
fn entry(ui: &mut egui::Ui, p: &Palette, line: &Line) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let fill = if line.selected {
        p.violet.gamma_multiply(0.35)
    } else if response.hovered() {
        p.raised
    } else {
        p.field
    };
    ui.painter().rect_filled(rect, widgets::inner_radius(ui.visuals()), fill);
    let icon_ink = if line.active { p.teal } else { p.muted };
    paint_icon(ui, line.icon, pos2(rect.left() + 16.0, rect.center().y), 14.0, icon_ink);
    let note_ink = match (line.usable, line.active) {
        (false, _) => p.danger,
        (true, true) => p.teal,
        (true, false) => p.muted,
    };
    let note_galley = ui
        .painter()
        .layout_no_wrap(line.note.to_owned(), egui::FontId::proportional(11.5), note_ink);
    let mut note_w = if line.note.is_empty() { 0.0 } else { note_galley.size().x + 12.0 };
    // a layout that locks the viewer window: a lock before its note
    if line.locked {
        note_w += 20.0;
        paint_icon(ui, ICON_LOCK, pos2(rect.right() - note_w, rect.center().y), 13.0, p.muted);
        note_w += 8.0;
    }
    // a favorite: its star, before the lock and the note
    if line.favorite {
        note_w += 20.0;
        paint_icon(ui, ICON_STAR, pos2(rect.right() - note_w, rect.center().y), 13.0, p.warn);
        note_w += 8.0;
    }
    let name_left = rect.left() + 30.0;
    let ink = if line.usable { p.ink } else { p.muted };
    widgets::paint_elided(
        ui,
        line.name,
        pos2(name_left, rect.center().y),
        (rect.right() - 10.0 - note_w - name_left).max(1.0),
        egui::FontId::proportional(13.0),
        ink,
    );
    if !line.note.is_empty() {
        let at = pos2(
            rect.right() - 10.0 - note_galley.size().x,
            rect.center().y - note_galley.size().y / 2.0,
        );
        ui.painter().galley(at, note_galley, note_ink);
    }
    let lock = if line.locked { tr!("fs-layouts-locked") } else { "" };
    let star = if line.favorite { tr!("fs-layouts-favorite") } else { "" };
    let (name, note, selected) = (line.name, line.note, line.selected);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, format!("{name} {star} {lock} {note}")));
    response
}

/// The three modes of a viewer window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    Fullscreen,
    Borderless,
}

fn mode_of(frame: &WindowFrame) -> Mode {
    match frame {
        WindowFrame::Normal { .. } => Mode::Normal,
        WindowFrame::Fullscreen { .. } => Mode::Fullscreen,
        WindowFrame::Borderless { .. } => Mode::Borderless,
    }
}

fn monitor_rect(m: &Monitor) -> [i32; 4] {
    [
        m.x,
        m.y,
        i32::try_from(m.width).unwrap_or(i32::MAX),
        i32::try_from(m.height).unwrap_or(i32::MAX),
    ]
}

/// Where `frame` puts the window on the monitors it was saved on, and the
/// places of the monitors it names.
fn frame_on(frame: &WindowFrame, saved: &[Monitor]) -> (Option<[i32; 4]>, Vec<usize>) {
    match frame {
        WindowFrame::Normal { x, y, width, height, .. } => (
            x.zip(*y).map(|(x, y)| {
                [
                    x,
                    y,
                    i32::try_from(*width).unwrap_or(i32::MAX),
                    i32::try_from(*height).unwrap_or(i32::MAX),
                ]
            }),
            Vec::new(),
        ),
        WindowFrame::Fullscreen { monitor } => {
            let n = usize::from(*monitor);
            (saved.get(n).map(monitor_rect), vec![n])
        }
        WindowFrame::Borderless { monitors: chosen, .. } => {
            let places: Vec<usize> = chosen.iter().map(|n| usize::from(*n)).collect();
            (monitors::cover(places.iter().filter_map(|n| saved.get(*n))), places)
        }
    }
}

/// `frame` in another mode, on the monitors `saved`: the monitor a normal
/// window is on for full screen and borderless; the rectangle of the
/// monitors named, a little inside it, for a normal window. `now` is the
/// viewer window as it is, when it can be read as a normal one.
fn in_mode(frame: &WindowFrame, mode: Mode, saved: &[Monitor], now: Option<WindowFrame>) -> WindowFrame {
    let (rect, places) = frame_on(frame, saved);
    let centre = rect.map(|[x, y, w, h]| (i64::from(x) + i64::from(w) / 2, i64::from(y) + i64::from(h) / 2));
    let on = places
        .first()
        .copied()
        .or_else(|| centre.and_then(|(x, y)| monitors::monitor_at(x, y, saved)))
        .unwrap_or(0);
    let on = u8::try_from(on).unwrap_or(0);
    let locked = frame.locked();
    match mode {
        Mode::Fullscreen => WindowFrame::Fullscreen { monitor: on },
        Mode::Borderless => WindowFrame::Borderless {
            monitors: if places.is_empty() {
                vec![on]
            } else {
                places.iter().filter_map(|n| u8::try_from(*n).ok()).collect()
            },
            locked,
        },
        Mode::Normal => match (now, rect) {
            (Some(now @ WindowFrame::Normal { .. }), _) => now.with_lock(locked),
            (_, Some([x, y, w, h])) => WindowFrame::Normal {
                x: Some(x.saturating_add(w / 10)),
                y: Some(y.saturating_add(h / 10)),
                width: (w - w / 5).max(1) as u32,
                height: (h - h / 5).max(1) as u32,
                maximized: false,
                locked,
            },
            _ => WindowFrame::Normal {
                x: None,
                y: None,
                width: 1280,
                height: 720,
                maximized: false,
                locked,
            },
        },
    }
}

/// `frame` after a click on the monitor at `place`: full screen goes
/// there; borderless takes it in or lets it go (never its last one).
fn with_monitor(frame: &WindowFrame, place: usize) -> Option<WindowFrame> {
    let place = u8::try_from(place).ok()?;
    match frame {
        WindowFrame::Normal { .. } => None,
        WindowFrame::Fullscreen { .. } => Some(WindowFrame::Fullscreen { monitor: place }),
        WindowFrame::Borderless { monitors: chosen, locked } => {
            let mut chosen = chosen.clone();
            match chosen.iter().position(|n| *n == place) {
                Some(_) if chosen.len() == 1 => return None,
                Some(at) => {
                    chosen.remove(at);
                }
                None => chosen.push(place),
            }
            chosen.sort_unstable();
            Some(WindowFrame::Borderless {
                monitors: chosen,
                locked: *locked,
            })
        }
    }
}

/// The selected layout's viewer window: its mode, its monitors on a small
/// picture of the layout's own, its lock box. The frame as edited, when it
/// was.
fn window_editor(ui: &mut egui::Ui, p: &Palette, layout: &Layout, frame: &WindowFrame, now: Option<WindowFrame>) -> Option<WindowFrame> {
    let saved = &layout.monitors;
    let mut edited = None;
    ui.add_space(4.0);
    let mut mode = mode_of(frame);
    // full screen and borderless name monitors of the layout's own list
    let modes = [
        (Mode::Normal, tr!("fs-layouts-mode-normal"), false),
        (Mode::Fullscreen, tr!("fs-layouts-mode-fullscreen"), false),
        (Mode::Borderless, tr!("fs-layouts-mode-borderless"), false),
    ];
    ui.add_enabled_ui(!saved.is_empty(), |ui| {
        if super::super::segmented(ui, p, &mut mode, &modes) {
            edited = Some(in_mode(frame, mode, saved, now));
        }
    });
    if saved.is_empty() {
        muted(ui, p, tr!("fs-layouts-mode-no-monitors"));
    } else {
        let rects: Vec<[i32; 4]> = saved.iter().map(monitor_rect).collect();
        let (window, chosen) = frame_on(frame, saved);
        let pick = mode != Mode::Normal;
        if let Some(place) = crate::folderstorm::diagram::widget::monitors(ui, p, &rects, window, &chosen, pick) {
            edited = with_monitor(frame, place).or(edited);
        }
        match mode {
            Mode::Normal => {}
            Mode::Fullscreen => muted(ui, p, tr!("fs-layouts-mode-pick-fullscreen")),
            Mode::Borderless => muted(ui, p, tr!("fs-layouts-mode-pick-borderless")),
        }
    }
    if mode != Mode::Fullscreen {
        let mut locked = frame.locked();
        if widgets::checkbox(ui, p, &mut locked, tr!("fs-layouts-lock")).changed() {
            edited = Some(frame.clone().with_lock(locked));
        }
    }
    edited
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folderstorm::workspaces::apply::{self, Live, Report};
    use crate::folderstorm::workspaces::store::{MonitorPolicy, Store};
    use crate::folderstorm::workspaces::window::{Command, Pretend};
    use crate::folderstorm::workspaces::words::report_text;
    use crate::settings::Settings;
    use crate::ui::options::{OptionsUi, Page, show};
    use crate::ui::widgets::Floater;
    use crate::ui::widgets::elide_tests::{Frames, Text};
    use egui::Rect;

    mod background;
    mod workspaces;

    /// Preferences on Workspaces › Saved, over a viewer with two windows open.
    struct Window {
        frames: Frames,
        p: Palette,
        icons: crate::ui::icons::Icons,
        s: Settings,
        st: OptionsUi,
        changed: bool,
        /// A pretend viewer window over two pretend monitors, when the
        /// test has one (`Window::with_viewer`).
        viewer: Option<Pretend>,
        /// The account and its HUDs, as the app tells the workspaces.
        shell: HudShell,
    }

    /// The viewer as far as a workspace's HUDs go (row 1.7): the account
    /// logged in, the HUDs worn, the names of the Inventory's items. No
    /// window is open or opened.
    #[derive(Default)]
    struct HudShell {
        account: Option<uuid::Uuid>,
        huds: Vec<crate::folderstorm::workspaces::context::Hud>,
        names: std::collections::BTreeMap<uuid::Uuid, String>,
    }

    impl apply::Shell for HudShell {
        fn is_open(&self, _: &crate::folderstorm::workspaces::windows::Window) -> bool {
            false
        }

        fn open(&mut self, _: &egui::Context, _: &crate::folderstorm::workspaces::windows::Window) -> bool {
            false
        }

        fn close(&mut self, _: &egui::Context, _: &crate::folderstorm::workspaces::windows::Window) {}

        fn account(&self) -> Option<uuid::Uuid> {
            self.account
        }

        fn worn_huds(&self) -> Vec<crate::folderstorm::workspaces::context::Hud> {
            self.huds.clone()
        }

        fn item_name(&self, item: uuid::Uuid) -> Option<String> {
            self.names.get(&item).cloned()
        }
    }

    const MONITORS: [[i32; 4]; 2] = [[0, 0, 1920, 1080], [1920, 0, 1920, 1080]];

    impl Window {
        /// The same, with a viewer window the layouts can read and drive,
        /// on the left one of two monitors.
        fn with_viewer() -> Self {
            let mut window = Window::new();
            window.viewer = Some(Pretend::new(100, 60, 1280, 900));
            window.settle();
            window
        }

        fn seen(&self) -> crate::folderstorm::workspaces::window::Seen {
            self.viewer.as_ref().expect("a pretend window").seen
        }

        fn new() -> Self {
            let frames = Frames::new();
            let theme = crate::theme::Theme::default();
            theme.apply(&frames.ctx, 1.0);
            frames.ctx.all_styles_mut(|s| s.animation_time = 0.0);
            let icons = crate::ui::icons::Icons::load(&frames.ctx, None);
            let mut st = OptionsUi::default();
            st.open(Page::Workspaces, tab::SAVED);
            let mut window = Window {
                frames,
                p: theme.palette(),
                icons,
                s: Settings::default(),
                st,
                changed: false,
                viewer: None,
                shell: HudShell::default(),
            };
            window.run(Vec::new());
            // the two windows out of the way of Preferences, at known places
            widgets::place_floater(&window.frames.ctx, "inventory", pos2(20.0, 600.0), vec2(300.0, 280.0));
            widgets::place_floater(&window.frames.ctx, "people", pos2(340.0, 600.0), vec2(260.0, 280.0));
            window.settle();
            window
        }

        /// One frame: the app's call before the pass, two of the viewer's
        /// windows, then Preferences.
        fn run(&mut self, events: Vec<egui::Event>) -> Vec<Text> {
            let ctx = self.frames.ctx.clone();
            if let Some(viewer) = &self.viewer {
                crate::folderstorm::diagram::set_desktop(
                    &ctx,
                    crate::folderstorm::diagram::Desktop {
                        window: viewer.seen.inner,
                        position: viewer.seen.outer,
                        monitors: MONITORS
                            .iter()
                            .map(|rect| crate::folderstorm::diagram::Monitor { rect: *rect, scale: 1.0 })
                            .collect(),
                    },
                );
            }
            let outcome = apply::frame(
                &ctx,
                &mut Live::of(&mut self.s),
                self.viewer.as_ref().map(|viewer| viewer.seen),
                &mut self.shell,
            );
            self.changed |= outcome.save;
            if let Some(viewer) = &mut self.viewer {
                viewer.send(&outcome.commands);
                viewer.frame();
            }
            let (p, icons, s, st, changed) = (&self.p, &self.icons, &mut self.s, &mut self.st, &mut self.changed);
            self.frames.run(events, |ui| {
                let ctx = ui.ctx();
                let screen = ctx.content_rect();
                crate::ui::layout::set_work_area(ctx, screen);
                crate::ui::layout::set_move_area(ctx, screen);
                for id in ["inventory", "people"] {
                    Floater::new(id, id, pos2(20.0, 600.0), vec2(300.0, 280.0)).show(ctx, p, &mut true, |ui| ui.label("…"));
                }
                *changed |= show(ctx, p, icons, s, st, &mut true).changed;
            })
        }

        fn settle(&mut self) -> Vec<Text> {
            let mut texts = Vec::new();
            for _ in 0..50 {
                texts = self.run(Vec::new());
            }
            texts
        }

        fn rect_of(&mut self, text: &str) -> Option<Rect> {
            let texts = self.run(Vec::new());
            texts.iter().find(|t| t.source == text).map(|t| t.rect)
        }

        fn shows(&mut self, text: &str) -> bool {
            self.rect_of(text).is_some()
        }

        fn click_at(&mut self, at: egui::Pos2) {
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            for events in [
                vec![egui::Event::PointerMoved(at)],
                vec![button(true)],
                vec![button(false)],
                Vec::new(),
                Vec::new(),
            ] {
                self.run(events);
            }
        }

        /// Scrolls the page down, with the wheel over it, until `text` is
        /// shown well above the window's footer (at most a few turns).
        fn reveal(&mut self, text: &str) {
            for _ in 0..16 {
                if self.rect_of(text).is_some_and(|rect| rect.bottom() < 620.0) {
                    return;
                }
                let at = pos2(700.0, 500.0);
                self.run(vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: vec2(0.0, -80.0),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
                for _ in 0..10 {
                    self.run(Vec::new());
                }
            }
        }

        /// Scrolls the page back to its top.
        fn scroll_top(&mut self) {
            let at = pos2(700.0, 500.0);
            for _ in 0..6 {
                self.run(vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: vec2(0.0, 2000.0),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
            self.settle();
        }

        /// Clicks the text `label` (a button's, a line's of the list).
        fn click(&mut self, label: &str) {
            let rect = self.rect_of(label).unwrap_or_else(|| panic!("“{label}” is shown"));
            self.click_at(rect.center());
        }

        /// Clicks the last text `label` drawn (the page's, after the
        /// sidebar's).
        fn click_last(&mut self, label: &str) {
            let texts = self.run(Vec::new());
            let rect = texts
                .iter()
                .filter(|t| t.source == label)
                .map(|t| t.rect)
                .next_back()
                .unwrap_or_else(|| panic!("“{label}” is shown"));
            self.click_at(rect.center());
        }

        /// *New from current…*, on "Layout".
        fn layout_form(&mut self) {
            self.click(tr!("fs-saved-new"));
            self.click_last(tr!("fs-workspaces-save-layout"));
        }

        /// Another tab of the page.
        fn tab(&mut self, tab: usize) {
            self.st.open(Page::Workspaces, tab);
            self.settle();
        }

        fn type_text(&mut self, text: &str) {
            self.run(vec![egui::Event::Text(text.to_owned())]);
            self.run(Vec::new());
        }

        fn save_as(&mut self, name: &str) {
            self.layout_form();
            self.click(tr!("fs-layouts-name-hint"));
            self.type_text(name);
            self.click(tr!("fs-layouts-save"));
            self.settle();
        }

        fn store(&self) -> &Store {
            &self.s.folderstorm_workspaces
        }
    }

    fn wide() -> crate::folderstorm::viewport::ViewportSettings {
        crate::folderstorm::viewport::ViewportSettings {
            enabled: true,
            left: 400,
            bars_around: true,
            ..Default::default()
        }
    }

    #[test]
    fn the_tab_saves_the_arrangement_under_a_name_and_marks_it_in_use() {
        let mut w = Window::new();
        assert!(w.shows(tr!("fs-saved-empty")));
        assert!(!w.shows(tr!("fs-layouts-apply")), "nothing selected, no actions");
        w.s.folderstorm_viewport = wide();
        w.changed = false;
        w.save_as("Wide");
        assert!(w.changed, "the settings are saved");
        let layout = w.store().layout(0).expect("saved");
        assert_eq!(layout.name, "Wide");
        assert_eq!(layout.world.as_ref(), Some(&wide()));
        let windows = layout.windows.as_ref().expect("the windows part");
        assert_eq!(
            windows.windows.keys().map(String::as_str).collect::<Vec<_>>(),
            ["inventory", "people"]
        );
        assert_eq!((windows.windows["inventory"].x, windows.windows["inventory"].y), (20.0, 600.0));
        // listed, selected, in use; the field is ready for another name
        assert!(w.shows("Wide") && w.shows(tr!("fs-layouts-active")));
        assert!(!w.shows(tr!("fs-saved-empty")));
        assert!(!w.shows(tr!("fs-layouts-name-hint")), "the form gives way to the new entry");
        assert!(w.shows(tr!("fs-layouts-apply")));
        let holds = format!("{} · {}", tr!("fs-layouts-part-world"), tr!("fs-layouts-holds-windows", count = 2));
        assert!(w.shows(&holds), "what it holds");
    }

    #[test]
    fn a_name_that_cannot_be_used_is_refused_with_the_reason() {
        let mut w = Window::new();
        w.layout_form();
        w.click(tr!("fs-layouts-save"));
        assert!(w.shows(tr!("fs-layouts-error-empty")));
        assert!(w.store().layouts.is_empty());
        w.save_as("Wide");
        assert!(!w.shows(tr!("fs-layouts-error-empty")));
        w.save_as("  wide ");
        assert!(w.shows(tr!("fs-layouts-error-taken")));
        assert_eq!(w.store().layouts.len(), 1);
        // without a part there is nothing to save
        w.click(tr!("fs-layouts-part-world"));
        w.click(tr!("fs-layouts-part-windows"));
        assert!(w.shows(tr!("fs-layouts-error-parts")));
        w.click(tr!("fs-layouts-save"));
        assert_eq!(w.store().layouts.len(), 1);
        // one part is enough: the world alone
        w.click(tr!("fs-layouts-part-world"));
        w.type_text("");
        w.click(tr!("fs-layouts-save"));
        w.settle();
        assert_eq!(w.store().layouts.len(), 1, "still named like the first");
    }

    #[test]
    fn apply_puts_the_layout_back_and_says_what_it_did() {
        let mut w = Window::new();
        w.s.folderstorm_viewport = wide();
        w.save_as("Wide");
        w.s.folderstorm_viewport = Default::default();
        widgets::move_floater(&w.frames.ctx, "people", pos2(700.0, 620.0));
        w.settle();
        w.save_as("Single");
        assert!(w.store().is_active(1));
        assert!(!w.shows(tr!("fs-layouts-previous-done")));
        // select "Wide", apply
        w.click("Wide");
        w.changed = false;
        w.click(tr!("fs-layouts-apply"));
        w.settle();
        assert!(w.changed);
        assert_eq!(w.s.folderstorm_viewport, wide());
        assert!(w.store().is_active(0));
        let people = w.frames.ctx.memory(|m| m.area_rect(egui::Id::unique("people"))).expect("shown");
        assert_eq!(people.min, pos2(340.0, 600.0));
        assert!(w.shows(&tr!("fs-layouts-applied-windows", name = "Wide", count = 2)));
        // moving a window marks it
        widgets::move_floater(&w.frames.ctx, "people", pos2(700.0, 620.0));
        w.settle();
        assert!(w.shows(tr!("fs-layouts-modified")));
        // Previous arrangement (Switching): "Single" again, and its line
        // selected
        w.tab(tab::SWITCHING);
        w.click(tr!("fs-layouts-previous"));
        w.settle();
        assert_eq!(w.s.folderstorm_viewport, Default::default());
        assert!(w.store().is_active(1));
        assert!(w.shows(tr!("fs-layouts-previous-done")));
        w.tab(tab::SAVED);
        // a double click on a line applies it
        let line = w.rect_of("Wide").expect("listed").center();
        w.click_at(line);
        w.click_at(line);
        w.settle();
        assert!(w.store().is_active(0), "double-clicked");
    }

    #[test]
    fn update_and_delete_ask_first() {
        let mut w = Window::new();
        w.save_as("Wide");
        let saved = w.store().clone();
        w.s.folderstorm_viewport = wide();
        w.settle();
        assert!(w.shows(tr!("fs-layouts-modified")), "the world is no longer the layout's");
        // Update: asked, cancelled, nothing changes
        w.click(tr!("fs-layouts-update"));
        let question = tr!("fs-layouts-ask-update", name = "Wide");
        assert!(w.shows(&question));
        assert_eq!(w.store(), &saved);
        w.click(tr!("common-cancel"));
        assert!(!w.shows(&question));
        assert_eq!(w.store(), &saved);
        // Update: asked, confirmed
        w.click(tr!("fs-layouts-update"));
        w.click(tr!("fs-layouts-update"));
        w.settle();
        assert_eq!(w.store().layout(0).and_then(|l| l.world.clone()), Some(wide()));
        assert!(w.shows(tr!("fs-layouts-active")) && !w.shows(tr!("fs-layouts-modified")));
        // Delete: asked, cancelled, then confirmed
        w.click(tr!("common-delete"));
        let question = tr!("fs-layouts-ask-delete", name = "Wide");
        assert!(w.shows(&question));
        w.click(tr!("common-cancel"));
        assert_eq!(w.store().layouts.len(), 1);
        w.click(tr!("common-delete"));
        w.click(tr!("common-delete"));
        w.settle();
        assert!(w.store().layouts.is_empty());
        assert_eq!(w.store().active_layout, None);
        assert!(w.shows(tr!("fs-saved-empty")));
        // deleting a layout changes nothing of the arrangement
        assert_eq!(w.s.folderstorm_viewport, wide());
    }

    #[test]
    fn a_layout_is_renamed_and_moved_in_the_list() {
        let mut w = Window::new();
        w.save_as("Wide");
        w.save_as("Single");
        w.click("Wide");
        w.click(tr!("fs-layouts-rename"));
        // the field has the keyboard, its name selected: typing replaces it
        w.type_text("Wide screen");
        w.click(tr!("fs-layouts-rename"));
        w.settle();
        assert_eq!(w.store().layout(0).map(|l| l.name.as_str()), Some("Wide screen"));
        // a name already taken is refused and the field stays
        w.click(tr!("fs-layouts-rename"));
        w.type_text("single");
        w.click(tr!("fs-layouts-rename"));
        assert!(w.shows(tr!("fs-layouts-error-taken")));
        assert_eq!(w.store().layout(0).map(|l| l.name.as_str()), Some("Wide screen"));
        w.click(tr!("common-cancel"));
        assert!(!w.shows(tr!("fs-layouts-error-taken")));
        // "Single" is in use; moved up, it still is
        assert!(w.store().is_active(1));
        w.click("Single");
        let up = w.rect_of(tr!("common-delete")).expect("the actions").right_center() + vec2(20.0, 0.0);
        w.click_at(up);
        w.settle();
        assert_eq!(w.store().layout(0).map(|l| l.name.as_str()), Some("Single"));
        assert!(w.store().is_active(0));
    }

    #[test]
    fn an_entry_that_cannot_be_used_is_listed_and_can_only_be_deleted() {
        let mut w = Window::new();
        w.s.folderstorm_workspaces = serde_json::from_value(serde_json::json!({
            "layouts": [
                { "schema": 7, "name": "From later", "world": "another shape" },
                Layout::new("Wide", Some(wide()), None, Vec::new()),
            ],
            "active_layout": "Wide",
        }))
        .expect("read");
        let saved = w.store().clone();
        w.settle();
        assert!(w.shows("From later") && w.shows(tr!("fs-layouts-newer")));
        w.click("From later");
        assert!(w.shows(tr!("fs-layouts-newer-help")));
        // Apply, Update and Rename are greyed: clicking them does nothing
        for label in [tr!("fs-layouts-apply"), tr!("fs-layouts-update"), tr!("fs-layouts-rename")] {
            w.click(label);
        }
        w.settle();
        assert_eq!(w.store(), &saved);
        assert_eq!(w.s.folderstorm_viewport, Default::default());
        assert!(!w.shows(&tr!("fs-layouts-ask-update", name = "From later")));
        // it is written back as it was read
        assert_eq!(
            serde_json::to_value(w.store()).expect("saved")["layouts"][0],
            serde_json::json!({ "schema": 7, "name": "From later", "world": "another shape" })
        );
        // and can be deleted
        w.click(tr!("common-delete"));
        w.click(tr!("common-delete"));
        w.settle();
        assert_eq!(w.store().layouts.len(), 1);
        assert!(w.store().is_active(0));
    }

    #[test]
    fn reset_bars_and_world_and_previous_are_greyed_until_they_have_something_to_do() {
        let mut w = Window::new();
        w.tab(tab::SWITCHING);
        // nothing to reset, nothing to go back to: clicks do nothing
        w.click(tr!("fs-layouts-reset"));
        w.click(tr!("fs-layouts-previous"));
        assert!(!w.changed);
        w.s.folderstorm_viewport = wide();
        w.click(tr!("fs-layouts-reset"));
        w.settle();
        assert_eq!(w.s.folderstorm_viewport, Default::default());
        assert!(w.changed);
        // and Previous brings the viewport back
        w.click(tr!("fs-layouts-previous"));
        w.settle();
        assert_eq!(w.s.folderstorm_viewport, wide());
    }

    #[test]
    fn the_search_finds_the_tab_by_dual_monitor_and_layout() {
        for query in ["dual monitor", "layout"] {
            let mut w = Window::new();
            w.st.query = query.to_owned();
            w.settle();
            assert!(w.shows(tr!("fs-saved-list")), "{query}");
        }
    }

    #[test]
    fn resetting_the_page_keeps_the_saved_entries_and_resets_the_standing_preferences() {
        let mut w = Window::new();
        w.save_as("Wide");
        let saved = w.store().clone();
        // the four standing preferences of Switching, changed
        let store = &mut w.s.folderstorm_workspaces;
        store.strip = true;
        store.keep_huds = true;
        store.on_monitor_change = MonitorPolicy::Nothing;
        assert!(store.set_toggle(0, Some("Wide")));
        let d = Settings::default();
        let mut r = crate::ui::options::OptionsResult::default();
        let (p, s, st) = (w.p, &mut w.s, &mut w.st);
        w.frames.run(Vec::new(), |ui| {
            let mut cx = Cx::new(ui, p, s, &d, st, &mut r, super::super::super::cx::Mode::Reset, "");
            for tab in 0..tabs().len() {
                cx.enter(Page::Workspaces, tab);
                page(&mut cx, tab);
            }
        });
        assert_eq!(w.store(), &saved, "the layouts kept, the four back to their defaults");
    }

    /// The four standing preferences are tracked: their modified dots, the
    /// Modified settings, the page's reset.
    #[test]
    fn the_standing_preferences_count_as_modified() {
        let mut s = Settings::default();
        let modified = |s: &mut Settings| {
            let ctx = egui::Context::default();
            let p = crate::theme::Theme::default().palette();
            let d = Settings::default();
            let mut st = OptionsUi::default();
            let mut r = crate::ui::options::OptionsResult::default();
            let mut n = 0;
            crate::ui::options::run_ui(&ctx, egui::RawInput::default(), |ui| {
                let mut cx = Cx::new(ui, p, s, &d, &mut st, &mut r, super::super::super::cx::Mode::Count, "");
                for tab in 0..tabs().len() {
                    cx.enter(Page::Workspaces, tab);
                    page(&mut cx, tab);
                }
                n = cx.counts.modified(Page::Workspaces);
            });
            n
        };
        assert_eq!(modified(&mut s), 0);
        s.folderstorm_workspaces.strip = true;
        s.folderstorm_workspaces.keep_huds = true;
        s.folderstorm_workspaces.on_monitor_change = MonitorPolicy::Apply;
        s.folderstorm_workspaces.toggle = [None, Some("Single".into())];
        assert_eq!(modified(&mut s), 4);
        // a saved layout is not a modified setting
        let mut s = Settings::default();
        s.folderstorm_workspaces
            .add(Layout::new("Wide", Some(wide()), None, Vec::new()))
            .expect("added");
        assert_eq!(modified(&mut s), 0);
    }

    fn two_monitors() -> Vec<Monitor> {
        MONITORS
            .iter()
            .map(|[x, y, width, height]| Monitor {
                x: *x,
                y: *y,
                width: *width as u32,
                height: *height as u32,
                scale: 1.0,
            })
            .collect()
    }

    #[test]
    fn a_layout_is_saved_with_the_viewer_window_and_its_lock() {
        // no viewer window to read: the part is greyed and left out
        let mut w = Window::new();
        w.save_as("No window");
        assert_eq!(w.store().layout(0).and_then(|layout| layout.window.clone()), None);
        // with one: saved by default, locked when the box is ticked
        let mut w = Window::with_viewer();
        w.layout_form();
        assert!(w.shows(tr!("fs-layouts-part-window")) && w.shows(tr!("fs-layouts-lock")));
        w.click(tr!("fs-layouts-lock"));
        w.changed = false;
        w.save_as("Locked");
        assert!(w.changed);
        assert_eq!(
            w.store().layout(0).and_then(|layout| layout.window.clone()),
            Some(WindowFrame::Normal {
                x: Some(100),
                y: Some(60),
                width: 1280,
                height: 900,
                maximized: false,
                locked: true,
            })
        );
        assert!(w.store().window_held);
        assert!(!w.seen().resizable, "locked at once");
        let words = format!(
            "{}, {}",
            tr!("fs-layouts-holds-window", width = 1280, height = 900),
            tr!("fs-layouts-holds-locked")
        );
        let texts = w.run(Vec::new());
        assert!(
            texts.iter().any(|t| t.source.starts_with(&words)),
            "what it holds starts with its window"
        );
    }

    #[test]
    fn unlock_is_greyed_until_a_layout_holds_the_window() {
        let mut w = Window::with_viewer();
        w.tab(tab::SWITCHING);
        w.click(tr!("fs-layouts-unlock"));
        assert!(w.seen().resizable);
        w.tab(tab::SAVED);
        w.layout_form();
        w.click(tr!("fs-layouts-lock"));
        w.save_as("Locked");
        assert!(!w.seen().resizable);
        // (found by the search, from any tab)
        w.st.query = "unlock".to_owned();
        w.settle();
        w.changed = false;
        w.click(tr!("fs-layouts-unlock"));
        w.run(Vec::new());
        w.st.query.clear();
        w.settle();
        assert!(w.changed);
        assert!(!w.store().window_held);
        assert!(w.seen().resizable);
        assert_eq!(
            w.viewer.as_ref().and_then(|viewer| viewer.log.last()),
            Some(&Command::Resizable(true))
        );
        // the layout keeps its lock box: applying it locks again
        w.click(tr!("fs-layouts-apply"));
        w.settle();
        assert!(w.store().window_held && !w.seen().resizable);
    }

    #[test]
    fn the_selected_layout_s_window_is_edited_in_the_tab() {
        let mut w = Window::with_viewer();
        w.save_as("Desk");
        assert_eq!(w.store().layout(0).map(|layout| layout.monitors.clone()), Some(two_monitors()));
        let window = |w: &Window| w.store().layout(0).and_then(|layout| layout.window.clone());
        // its lock box (the first of the two on the page): at once, for the layout in use
        w.click(tr!("fs-layouts-lock"));
        w.run(Vec::new());
        assert!(window(&w).is_some_and(|frame| frame.locked()));
        assert!(w.store().window_held && !w.seen().resizable);
        assert!(!w.shows(tr!("fs-layouts-modified")));
        // borderless: over the monitor the window is on, then over both
        w.click(tr!("fs-layouts-mode-borderless"));
        assert_eq!(
            window(&w),
            Some(WindowFrame::Borderless {
                monitors: vec![0],
                locked: true,
            })
        );
        assert!(w.shows(tr!("fs-layouts-modified")), "the viewer's window is not that yet");
        assert!(w.shows(tr!("fs-layouts-mode-pick-borderless")));
        w.click("2");
        assert_eq!(
            window(&w),
            Some(WindowFrame::Borderless {
                monitors: vec![0, 1],
                locked: true,
            })
        );
        // never without a monitor
        w.click("2");
        w.click("1");
        assert_eq!(
            window(&w),
            Some(WindowFrame::Borderless {
                monitors: vec![0],
                locked: true,
            })
        );
        w.click("2");
        // Apply does it
        w.click(tr!("fs-layouts-apply"));
        w.settle();
        assert!(!w.seen().decorated && !w.seen().resizable);
        assert_eq!((w.seen().outer, w.seen().inner), (Some((0, 0)), (3840, 1080)));
        assert!(!w.shows(tr!("fs-layouts-modified")));
        // full screen on the second monitor
        w.click(tr!("fs-layouts-mode-fullscreen"));
        assert_eq!(window(&w), Some(WindowFrame::Fullscreen { monitor: 0 }));
        assert!(w.shows(tr!("fs-layouts-mode-pick-fullscreen")));
        w.click("2");
        assert_eq!(window(&w), Some(WindowFrame::Fullscreen { monitor: 1 }));
        // and a normal window again: the viewer's own, as it is
        w.click(tr!("fs-layouts-mode-normal"));
        assert!(matches!(window(&w), Some(WindowFrame::Normal { .. })));
    }

    #[test]
    fn a_layout_saved_without_monitors_only_has_a_normal_window() {
        let mut w = Window::new();
        w.s.folderstorm_workspaces
            .add(
                Layout::new("Wayland", None, None, Vec::new()).with_window(Some(WindowFrame::Normal {
                    x: None,
                    y: None,
                    width: 1600,
                    height: 900,
                    maximized: false,
                    locked: false,
                })),
            )
            .expect("added");
        assert!(w.shows(tr!("fs-layouts-mode-no-monitors")));
        w.click(tr!("fs-layouts-mode-borderless"));
        assert!(matches!(
            w.store().layout(0).and_then(|layout| layout.window.clone()),
            Some(WindowFrame::Normal { .. })
        ));
    }

    #[test]
    fn what_happens_when_the_monitors_change_is_a_choice_of_three() {
        let mut w = Window::new();
        w.tab(tab::SWITCHING);
        assert_eq!(w.store().on_monitor_change, MonitorPolicy::Ask);
        w.changed = false;
        w.click(tr!("fs-layouts-monitors-nothing"));
        assert_eq!(w.store().on_monitor_change, MonitorPolicy::Nothing);
        assert!(w.changed);
        w.click(tr!("fs-layouts-monitors-apply"));
        assert_eq!(w.store().on_monitor_change, MonitorPolicy::Apply);
        w.click(tr!("fs-layouts-monitors-ask"));
        assert_eq!(w.store().on_monitor_change, MonitorPolicy::Ask);
    }

    #[test]
    fn the_report_says_what_became_of_the_viewer_window() {
        let report = Report {
            name: Some("2 monitors".into()),
            windows: 3,
            fitted: 2,
            missing: 1,
            left: true,
            unlocked: true,
            ..Default::default()
        };
        let text = report_text(&report, 0);
        for part in [
            tr!("fs-layouts-applied-missing", count = 1),
            tr!("fs-layouts-applied-left").to_owned(),
            tr!("fs-layouts-applied-unlocked").to_owned(),
            tr!("fs-layouts-applied-fitted", count = 2),
        ] {
            assert!(text.contains(&part), "{text}");
        }
        let skipped = Report {
            no_window: true,
            ..Default::default()
        };
        assert!(report_text(&skipped, 0).contains(tr!("fs-layouts-applied-no-window")));
    }

    #[test]
    fn a_window_in_another_mode_keeps_what_it_can() {
        let saved = two_monitors();
        // a normal window on the right monitor
        let normal = WindowFrame::Normal {
            x: Some(2000),
            y: Some(100),
            width: 1600,
            height: 900,
            maximized: false,
            locked: true,
        };
        assert_eq!(
            in_mode(&normal, Mode::Fullscreen, &saved, None),
            WindowFrame::Fullscreen { monitor: 1 }
        );
        let borderless = in_mode(&normal, Mode::Borderless, &saved, None);
        assert_eq!(
            borderless,
            WindowFrame::Borderless {
                monitors: vec![1],
                locked: true,
            }
        );
        // back to a normal window: inside the monitors it covered
        assert_eq!(
            in_mode(&borderless, Mode::Normal, &saved, None),
            WindowFrame::Normal {
                x: Some(2112),
                y: Some(108),
                width: 1536,
                height: 864,
                maximized: false,
                locked: true,
            }
        );
        // or the viewer's own window when it can be read, with the layout's lock box
        let now = WindowFrame::Normal {
            x: Some(5),
            y: Some(6),
            width: 1000,
            height: 700,
            maximized: false,
            locked: false,
        };
        assert_eq!(in_mode(&borderless, Mode::Normal, &saved, Some(now.clone())), now.with_lock(true));
        // every result is a window the store takes
        for mode in [Mode::Normal, Mode::Fullscreen, Mode::Borderless] {
            for frame in [&normal, &borderless, &WindowFrame::Fullscreen { monitor: 0 }] {
                let layout = Layout::new("L", None, None, saved.clone()).with_window(Some(in_mode(frame, mode, &saved, None)));
                assert_eq!(layout.check(), Ok(()), "{frame:?} as {mode:?}");
            }
        }
        // a click on a monitor
        assert_eq!(with_monitor(&normal, 0), None);
        assert_eq!(with_monitor(&borderless, 1), None, "its last monitor stays");
        assert_eq!(
            with_monitor(&borderless, 0),
            Some(WindowFrame::Borderless {
                monitors: vec![0, 1],
                locked: true,
            })
        );
    }

    #[test]
    fn the_tab_gives_a_layout_its_star() {
        let mut w = Window::new();
        w.save_as("Wide");
        w.changed = false;
        // the star is the last of the selected layout's buttons, after the
        // two arrows
        let star = w.rect_of(tr!("common-delete")).expect("the actions").right_center() + vec2(85.0, 0.0);
        w.click_at(star);
        assert!(w.store().is_favorite("Wide"));
        assert!(w.changed, "the settings are saved");
        w.click_at(star);
        assert!(!w.store().is_favorite("Wide"));
    }

    #[test]
    fn the_two_layouts_of_the_toggle_are_chosen_in_the_tab() {
        let mut w = Window::new();
        w.save_as("Wide");
        w.save_as("Single");
        // on Switching, also found by the window's search, which then
        // shows its two rows
        w.st.query = "switcher".into();
        w.settle();
        assert!(w.shows(tr!("fs-layouts-toggle-first")) && w.shows(tr!("fs-layouts-toggle-second")));
        let all =
            |w: &mut Window, label: &str| -> Vec<Rect> { w.run(Vec::new()).iter().filter(|t| t.source == label).map(|t| t.rect).collect() };
        let unset = all(&mut w, tr!("fs-layouts-toggle-none"));
        assert_eq!(unset.len(), 2, "neither is chosen at first");
        // the first picker: its list opens over the page, "Wide" is in it
        w.click_at(unset[0].center());
        assert_eq!(all(&mut w, tr!("fs-layouts-toggle-none")).len(), 3, "the list is open");
        let wide = *all(&mut w, "Wide").last().expect("offered");
        w.changed = false;
        w.click_at(wide.center());
        assert_eq!(w.store().toggle, [Some("Wide".to_owned()), None]);
        assert!(w.changed, "the settings are saved");
        // the second one
        let unset = all(&mut w, tr!("fs-layouts-toggle-none"));
        assert_eq!(unset.len(), 1);
        w.click_at(unset[0].center());
        let single = *all(&mut w, "Single").last().expect("offered");
        w.click_at(single.center());
        assert_eq!(w.store().toggle, [Some("Wide".to_owned()), Some("Single".to_owned())]);
        // and back to none
        let first = *all(&mut w, "Wide").last().expect("the first picker shows it");
        w.click_at(first.center());
        let none = *all(&mut w, tr!("fs-layouts-toggle-none")).last().expect("offered");
        w.click_at(none.center());
        assert_eq!(w.store().toggle, [None, Some("Single".to_owned())]);
    }

    /// Draws Preferences on `tab` of the Workspaces page for a few frames with
    /// the software painter; the texts of the last frame.
    /// `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps the picture as
    /// `<folder>/<name>.png`.
    fn drawn(name: &str, tab: usize, s: Settings, cogs: bool) -> Vec<String> {
        drawn_found(name, tab, s, cogs, "")
    }

    /// [`drawn`], with `query` typed in the window's search.
    fn drawn_found(name: &str, tab: usize, s: Settings, cogs: bool, query: &str) -> Vec<String> {
        drawn_scrolled(name, tab, s, cogs, query, None, 0.0)
    }

    /// [`drawn_found`], with what the app says of HUDs (`huds`) and the
    /// page scrolled down by `scroll` points with the wheel.
    fn drawn_scrolled(
        name: &str,
        tab: usize,
        mut s: Settings,
        cogs: bool,
        query: &str,
        huds: Option<crate::folderstorm::workspaces::context::HudView>,
        scroll: f32,
    ) -> Vec<String> {
        let theme = crate::theme::Theme::default();
        let p = theme.skin_palette();
        let ctx = egui::Context::default();
        theme.apply(&ctx, 1.0);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let icons = crate::ui::icons::Icons::load(&ctx, None);
        let mut st = OptionsUi::default();
        st.open(Page::Workspaces, tab);
        st.demo_open_cogs = cogs;
        st.query = query.to_owned();
        let (width, height) = (1280u32, 900u32);
        let window = Rect::from_min_size(egui::Pos2::ZERO, vec2(width as f32, height as f32));
        let mut pixels = vec![0u32; (width * height) as usize];
        let mut painter = crate::crash::software::Painter::default();
        let mut texts = Vec::new();
        if let Some(huds) = huds {
            crate::folderstorm::workspaces::context::set_view(&ctx, huds);
        }
        let frames = if scroll > 0.0 { 60 } else { 8 };
        for frame in 0..frames {
            let at = egui::pos2(700.0, 500.0);
            let events = match frame {
                2 if scroll > 0.0 => vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: vec2(0.0, -scroll),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                _ => Vec::new(),
            };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(window),
                    time: Some(frame as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |ui| {
                    show(ui.ctx(), &p, &icons, &mut s, &mut st, &mut true);
                },
            );
            texts.clear();
            fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
                match shape {
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                    egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
                    _ => {}
                }
            }
            for clipped in &output.shapes {
                collect(&clipped.shape, &mut texts);
            }
            let primitives = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
            pixels.fill(0);
            painter.paint(
                &mut pixels,
                [width as usize, height as usize],
                &primitives,
                &output.textures_delta,
                output.pixels_per_point,
                p.panel,
            );
            output.textures_delta.clear();
        }
        if let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") {
            let rgba: Vec<_> = pixels
                .iter()
                .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
                .collect();
            let path = std::path::Path::new(&dir).join(format!("{name}.png"));
            image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8).expect("capture written");
        }
        texts
    }

    /// The two pickers of the *Switch layout* button, as the window's
    /// search shows them (`preferences-layouts-quick.png`).
    #[test]
    fn the_quick_switch_pickers_are_drawn() {
        let s = Settings {
            folderstorm_workspaces: serde_json::from_value(serde_json::json!({
                "layouts": [
                    Layout::new("2 monitors", Some(wide()), None, Vec::new()),
                    Layout::new("1 monitor", Some(Default::default()), None, Vec::new()),
                ],
                "favorites": [{ "layout": "1 monitor" }],
                "toggle": ["2 monitors", null],
            }))
            .expect("read"),
            ..Default::default()
        };
        let texts = drawn_found("preferences-layouts-quick", tab::SWITCHING, s, false, "switcher");
        for expected in [
            tr!("fs-layouts-toggle-first"),
            tr!("fs-layouts-toggle-second"),
            "2 monitors",
            tr!("fs-layouts-toggle-none"),
        ] {
            assert!(texts.iter().any(|text| text == expected), "“{expected}” is drawn");
        }
    }

    /// The picture of the tab with a layout in use and modified, another
    /// one, and an entry from a newer version (`preferences-layouts.png`).
    #[test]
    fn the_tab_is_drawn_with_its_list_and_actions() {
        let s = Settings {
            folderstorm_workspaces: serde_json::from_value(serde_json::json!({
                "layouts": [
                    Layout::new("2 monitors", Some(wide()), None, Vec::new()),
                    Layout::new("1 monitor", Some(Default::default()), None, Vec::new()),
                    { "schema": 7, "name": "From a later version" },
                ],
                "active_layout": "2 monitors",
                "modified": true,
            }))
            .expect("read"),
            folderstorm_viewport: wide(),
            ..Default::default()
        };
        let texts = drawn("preferences-layouts", tab::SAVED, s, false);
        for icon in [
            ICON_CHECK,
            ICON_SAVE,
            ICON_RENAME,
            ICON_DELETE,
            ICON_CANCEL,
            ICON_BACK,
            ICON_RESET,
            ICON_UP,
            ICON_DOWN,
            ICON_STAR,
        ] {
            assert!(crate::ui::icons::global(icon).is_some(), "the icon “{icon}” is embedded");
        }
        for expected in [
            "2 monitors",
            "1 monitor",
            "From a later version",
            tr!("fs-layouts-modified"),
            tr!("fs-layouts-newer"),
            tr!("fs-layouts-apply"),
            tr!("fs-saved-new"),
        ] {
            assert!(texts.iter().any(|t| t == expected), "“{expected}” is drawn");
        }
    }

    /// The picture of the tab with a locked borderless layout selected:
    /// its window editor, the lock on its line, Unlock and the monitors'
    /// choice (`preferences-layouts-window.png`).
    #[test]
    fn the_tab_is_drawn_with_the_viewer_window_s_editor() {
        let s = Settings {
            folderstorm_workspaces: serde_json::from_value(serde_json::json!({
                "layouts": [
                    Layout::new("2 monitors", Some(wide()), None, two_monitors()).with_window(Some(WindowFrame::Borderless {
                        monitors: vec![0, 1],
                        locked: true,
                    })),
                    Layout::new("1 monitor", Some(Default::default()), None, two_monitors()[..1].to_vec()).with_window(Some(
                        WindowFrame::Normal {
                            x: Some(0),
                            y: Some(0),
                            width: 1920,
                            height: 1040,
                            maximized: false,
                            locked: false,
                        }
                    )),
                ],
                "active_layout": "2 monitors",
                "window_held": true,
            }))
            .expect("read"),
            folderstorm_viewport: wide(),
            ..Default::default()
        };
        assert!(s.folderstorm_workspaces.window_held);
        let texts = drawn("preferences-layouts-window", tab::SAVED, s.clone(), false);
        let window = tr!("fs-layouts-part-window").to_uppercase();
        for icon in [ICON_LOCK, ICON_UNLOCK] {
            assert!(crate::ui::icons::global(icon).is_some(), "the icon “{icon}” is embedded");
        }
        for expected in [
            window.as_str(),
            tr!("fs-layouts-mode-normal"),
            tr!("fs-layouts-mode-fullscreen"),
            tr!("fs-layouts-mode-borderless"),
            tr!("fs-layouts-mode-pick-borderless"),
            tr!("fs-layouts-lock"),
        ] {
            assert!(texts.iter().any(|t| t == expected), "“{expected}” is drawn");
        }
        // Unlock and the monitors' rule, on Switching
        let texts = drawn("preferences-workspaces-switching", tab::SWITCHING, s, false);
        for expected in [
            tr!("fs-strip-show"),
            tr!("fs-layouts-toggle-first"),
            tr!("fs-switching-keys-button"),
            tr!("fs-layouts-previous"),
            tr!("fs-layouts-unlock"),
            tr!("fs-layouts-reset"),
            tr!("fs-layouts-monitors-ask"),
            tr!("fs-layouts-monitors-apply"),
            tr!("fs-layouts-monitors-nothing"),
            tr!("fs-context-take-off"),
        ] {
            assert!(texts.iter().any(|t| t == expected), "“{expected}” is drawn");
        }
    }

    /// The bars' three places are one choice of the World viewport group,
    /// out of its cog (`preferences-bars-place.png`).
    #[test]
    fn the_bars_place_is_one_choice_of_three() {
        let s = Settings {
            folderstorm_viewport: wide(),
            ..Default::default()
        };
        let texts = drawn("preferences-bars-place", tab::VIEWPORT, s, false);
        for expected in [
            tr!("folderstorm-viewport-bars"),
            tr!("folderstorm-viewport-bars-inside"),
            tr!("folderstorm-viewport-bars-around"),
            tr!("folderstorm-viewport-bars-window"),
        ] {
            assert!(texts.iter().any(|t| t == expected), "“{expected}” is drawn");
        }
    }

    /// The Margins tab with the viewport off: one notice with its button,
    /// the rows greyed; a click on it turns the viewport on.
    #[test]
    fn the_margins_wait_for_the_viewport_behind_one_notice() {
        let mut w = Window::new();
        w.tab(tab::MARGINS);
        assert!(w.shows(tr!("fs-margins-off")));
        w.changed = false;
        w.click(tr!("fs-margins-turn-on"));
        w.settle();
        assert!(w.s.folderstorm_viewport.enabled && w.changed);
        assert!(!w.shows(tr!("fs-margins-off")), "no notice once it is on");
    }

    /// The Saved tab's list: the workspaces, then the layouts, each under
    /// its title, in the user's order (a favorite keeps its place).
    #[test]
    fn the_list_puts_the_workspaces_first_then_the_layouts() {
        let mut w = Window::new();
        w.s.folderstorm_workspaces = serde_json::from_value(serde_json::json!({
            "layouts": [
                Layout::new("Wide", Some(wide()), None, Vec::new()),
                Layout::new("Single", Some(Default::default()), None, Vec::new()),
            ],
            "workspaces": [
                { "schema": 1, "name": "Sorting", "open": ["inventory"] },
                { "schema": 1, "name": "Exploring", "open": ["worldmap"] },
            ],
            "favorites": [{ "layout": "Single" }, { "workspace": "Exploring" }],
        }))
        .expect("read");
        let texts = w.settle();
        let top = |name: &str| {
            texts
                .iter()
                .find(|t| t.source == name)
                .map(|t| t.rect.top())
                .unwrap_or_else(|| panic!("{name}"))
        };
        let workspaces = top(&tr!("fs-workspaces-list").to_uppercase());
        let layouts = top(&tr!("fs-layouts-list").to_uppercase());
        assert!(workspaces < top("Sorting") && top("Sorting") < top("Exploring"));
        assert!(top("Exploring") < layouts && layouts < top("Wide") && top("Wide") < top("Single"));
        // nothing in use, nothing selected: the right pane says what to do
        assert!(w.shows(tr!("fs-saved-pick")));
    }

    /// The texts of the Saved tab drawn in a window `width` wide, with where
    /// each is.
    fn placed(s: Settings, width: f32) -> Vec<(String, Rect)> {
        let theme = crate::theme::Theme::default();
        let p = theme.skin_palette();
        let ctx = egui::Context::default();
        theme.apply(&ctx, 1.0);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let icons = crate::ui::icons::Icons::load(&ctx, None);
        let mut st = OptionsUi::default();
        st.open(Page::Workspaces, tab::SAVED);
        let mut s = s;
        let mut texts = Vec::new();
        for frame in 0..8 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 900.0))),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ui| {
                    show(ui.ctx(), &p, &icons, &mut s, &mut st, &mut true);
                },
            );
            texts.clear();
            fn collect(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
                match shape {
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                    egui::Shape::Text(t) => out.push((t.galley.text().to_owned(), Rect::from_min_size(t.pos, t.galley.size()))),
                    _ => {}
                }
            }
            for clipped in &output.shapes {
                collect(&clipped.shape, &mut texts);
            }
            output.textures_delta.clear();
        }
        texts
    }

    fn rect_in(texts: &[(String, Rect)], text: &str) -> Rect {
        texts
            .iter()
            .find(|(t, _)| t == text)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("“{text}” is drawn"))
    }

    /// Two panes on a wide page, the editor right of the list; under the
    /// window's narrow width, the list above the editor.
    #[test]
    fn the_editor_is_beside_the_list_or_under_it_when_narrow() {
        let seeded = || {
            let mut s = Settings::default();
            s.folderstorm_workspaces
                .add(Layout::new("Wide", Some(wide()), None, Vec::new()))
                .expect("added");
            s.folderstorm_workspaces.set_active(Some("Wide".into()));
            s
        };
        let texts = placed(seeded(), 1280.0);
        let (new, apply) = (rect_in(&texts, tr!("fs-saved-new")), rect_in(&texts, tr!("fs-layouts-apply")));
        assert!(apply.left() > new.right(), "side by side: {new:?} {apply:?}");
        let texts = placed(seeded(), 700.0);
        let (new, apply) = (rect_in(&texts, tr!("fs-saved-new")), rect_in(&texts, tr!("fs-layouts-apply")));
        assert!(apply.top() > new.bottom(), "the editor under the list: {new:?} {apply:?}");
    }

    /// A workspace shows its layout as a card, which opens it; a layout
    /// shows the workspaces and the button that use it, which lead back.
    #[test]
    fn a_workspace_shows_its_layout_and_a_layout_what_uses_it() {
        let mut w = Window::new();
        w.s.folderstorm_workspaces = serde_json::from_value(serde_json::json!({
            "layouts": [Layout::new("Single", Some(Default::default()), None, Vec::new())],
            "workspaces": [{ "schema": 1, "name": "Sorting", "layout": "Single", "open": ["inventory"] }],
            "active_workspace": "Sorting",
            "toggle": ["Single", null],
        }))
        .expect("read");
        w.settle();
        let selected = |w: &Window| saved::load(&w.frames.ctx).selected;
        assert_eq!(selected(&w).map(|pick| pick.kind), Some(saved::Kind::Workspace));
        w.reveal(tr!("fs-saved-open-layout"));
        w.click(tr!("fs-saved-open-layout"));
        w.settle();
        assert_eq!(
            selected(&w),
            Some(saved::Pick {
                kind: saved::Kind::Layout,
                index: 0
            })
        );
        // the layout's users: the workspace, the first layout of the toggle
        w.reveal(tr!("fs-saved-used-by-some"));
        assert!(w.shows(tr!("fs-saved-used-by-some")) && w.shows(tr!("fs-saved-toggle-first")));
        assert!(!w.shows(tr!("fs-saved-toggle-second")));
        // the workspace's name leads back to it
        w.click_last("Sorting");
        w.settle();
        assert_eq!(selected(&w).map(|pick| pick.kind), Some(saved::Kind::Workspace));
    }

    /// A layout's *Edit the current viewport* opens the World viewport
    /// tab; *Switching* opens that tab.
    #[test]
    fn a_layout_leads_to_the_viewport_and_switching_tabs() {
        let mut w = Window::new();
        w.save_as("Wide");
        w.reveal(tr!("fs-saved-edit-viewport"));
        w.click(tr!("fs-saved-edit-viewport"));
        w.settle();
        assert_eq!((w.st.page, w.st.tab), (Page::Workspaces, tab::VIEWPORT));
        w.tab(tab::SAVED);
        w.reveal(tr!("fs-saved-switching"));
        w.click_last(tr!("fs-saved-switching"));
        w.settle();
        assert_eq!((w.st.page, w.st.tab), (Page::Workspaces, tab::SWITCHING));
        assert!(w.shows(tr!("fs-saved-used-by-none")) || w.shows(tr!("fs-layouts-toggle-first")));
    }

    /// Each list's own report: what the last apply did, in one line under
    /// the buttons.
    #[test]
    fn the_report_is_one_line_with_the_rest_on_hover() {
        let report = Report {
            name: Some("Wide".into()),
            windows: 2,
            fitted: 1,
            ..Default::default()
        };
        let text = report_text(&report, 0);
        let first = text.split_inclusive(". ").next().unwrap_or(&text).trim_end().to_owned();
        assert_eq!(first, tr!("fs-layouts-applied-windows", name = "Wide", count = 2));
        assert_ne!(first, text, "the rest is on hover");
    }

    /// The two Folderstorm pages' sidebar icons are embedded ones.
    #[test]
    fn the_two_pages_have_embedded_icons() {
        for page in [Page::Workspaces, Page::LocalAssistant] {
            assert!(crate::ui::icons::embedded(page.icon()), "{page:?}");
        }
    }
}
