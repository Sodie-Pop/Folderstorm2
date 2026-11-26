//! The taskbar of the margins (folderstorm/plans/margin-desktop.md, "The
//! taskbar"): an island of Aurora's bars along the bottom of one margin
//! monitor, with a button for each collapsed window, or, by a choice of
//! Preferences, for each open window.
//!
//! Every `ui::widgets::Floater` tells the taskbar what it is from one hook
//! in `Floater::show` ([`holds`]): its id, its title, its icon, whether it
//! is collapsed, whether something waits inside. The same call answers
//! "am I held?": a collapsed window is **held** while the taskbar stands,
//! which means it is not drawn (the hook returns before the window is laid
//! out) and its button stands for its title bar. Its place, its size and
//! its collapsed state stay where Aurora keeps them, so a window restored
//! is where it was, and with the taskbar off, or no margin to put it in,
//! [`holds`] answers false and the title bars are Aurora's again.
//!
//! The taskbar reads the windows as the hook saw them: those drawn before
//! it in the frame, and those of the last frame for the ones drawn after.
//! It keeps nothing in the settings but its three choices
//! (`store::Taskbar`): what it lists is the windows that are open.
//!
//! It is an island like Aurora's dock (`ui::bars::islands::area`), on the
//! windows' layer: a window dragged over it covers it, a click on it brings
//! it back, and the windows keep their title bar clear of it.

use super::store::Desktop;
use crate::theme::Palette;
use crate::ui::bars::islands;
use crate::ui::icons::Icons;
use crate::ui::menu;
use crate::ui::widgets::{self, Floater};
use aurora_i18n::tr;
use egui::{FontId, Id, LayerId, Order, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

/// A button's height (a slot of Aurora's islands).
const BUTTON_H: f32 = islands::SLOT;
/// The widest a button with its title gets.
const MAX_W: f32 = 180.0;
/// Under this width a title says nothing: the buttons are their icon alone.
const MIN_TEXT_W: f32 = 96.0;
/// A button that is its icon alone.
const ICON_W: f32 = 32.0;
/// The *More* button, with its count.
const MORE_W: f32 = 52.0;
const GAP: f32 = islands::GAP;
const PAD: f32 = islands::PAD;
const ICON: f32 = islands::ICON;
const TEXT: f32 = 12.0;

const ICON_MORE: &str = "chevrons-right";
const ICON_RESTORE: &str = "arrows-maximize";
const ICON_COLLAPSE: &str = "minus";
const ICON_FRONT: &str = "stack-2";
const ICON_CLOSE: &str = "x";

/// The icons named here, for the test that they are embedded.
#[cfg(test)]
pub(super) const ICONS: [&str; 5] = [ICON_MORE, ICON_RESTORE, ICON_COLLAPSE, ICON_FRONT, ICON_CLOSE];

/// The name of the taskbar's island (`islands::area`).
const ISLAND: &str = "folderstorm_taskbar";

/// A window as its `Floater::show` told it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Window {
    pub id: String,
    pub title: String,
    pub icon: String,
    /// Collapsed and held: not drawn, its button stands for it.
    pub held: bool,
    /// Collapsed, held or not (the taskbar may not stand now).
    down: bool,
    /// Something waits inside (unread messages).
    pub attention: bool,
    /// The strength of the attention mark's violet, and whether it moves.
    flash: (f32, bool),
    /// The frame it last told.
    seen: u64,
    /// When it was first seen, and when it was last collapsed: the orders
    /// of the two lists.
    opened: u64,
    collapsed: u64,
}

/// What the taskbar knows between frames.
#[derive(Clone, Default)]
struct Registry {
    /// The last frame the taskbar was on: the windows are recorded up to
    /// the frame after it.
    live: Option<u64>,
    /// It stood then (or was kept in mouselook): collapsed windows are held.
    holding: bool,
    windows: Vec<Window>,
    /// The next place in the two orders.
    next: u64,
    /// Windows that were held and are drawn collapsed for one frame on
    /// their way out (being restored or closed), with that frame.
    leaving: Vec<(LayerId, u64)>,
}

fn registry_id() -> Id {
    Id::unique("folderstorm_taskbar_registry")
}

/// The hook of `Floater::show`: the open window `floater` tells the
/// taskbar what it is, `collapsed` being true when it is reduced to its
/// title bar and stays so this frame (not closing, not expanding, and not
/// one that keeps a compact body). True when the taskbar holds it: the
/// window is then not drawn.
///
/// With the taskbar off this is one read of egui's memory and false.
pub fn holds(ctx: &egui::Context, floater: &Floater<'_>, icon: &str, collapsed: bool) -> bool {
    let frame = ctx.cumulative_frame_nr_for(egui::ViewportId::ROOT);
    ctx.data_mut(|d| {
        let registry = d.get_temp_mut_or_default::<Registry>(registry_id());
        if registry.live.is_none_or(|at| at + 1 < frame) {
            return false;
        }
        // a window of another layer (the poser's, over the photo
        // viewfinder) is a tool of what is under it: never held
        let held = collapsed && registry.holding && floater.order == Order::Middle;
        let icon = floater.framed_header.unwrap_or(icon);
        let flash = floater.attention_flash.unwrap_or((0.0, false));
        let next = registry.next;
        match registry.windows.iter_mut().find(|w| w.id == floater.id) {
            Some(window) => {
                // gone for a while (closed, or hidden with the interface)
                // and back: a new place among the open windows
                if window.seen + 1 < frame {
                    window.opened = next;
                    registry.next += 1;
                }
                if collapsed && !window.down {
                    window.collapsed = registry.next;
                    registry.next += 1;
                } else if window.held && !held && registry.holding && window.seen + 1 >= frame {
                    registry.leaving.push((LayerId::new(Order::Middle, Id::unique(floater.id)), frame));
                }
                if window.title != floater.title {
                    window.title.clone_from(&floater.title);
                }
                if window.icon != icon {
                    icon.clone_into(&mut window.icon);
                }
                window.held = held;
                window.down = collapsed;
                window.attention = floater.attention;
                window.flash = flash;
                window.seen = frame;
            }
            None => {
                registry.windows.push(Window {
                    id: floater.id.to_owned(),
                    title: floater.title.clone(),
                    icon: icon.to_owned(),
                    held,
                    down: collapsed,
                    attention: floater.attention,
                    flash,
                    seen: frame,
                    opened: next,
                    collapsed: next + 1,
                });
                registry.next += 2;
            }
        }
        held
    })
}

/// Whether the taskbar holds the window `id` now (it is collapsed and its
/// button stands for it), for the tests.
#[cfg(test)]
pub(super) fn held(ctx: &egui::Context, id: &str) -> bool {
    let frame = ctx.cumulative_frame_nr_for(egui::ViewportId::ROOT);
    ctx.data(|d| d.get_temp::<Registry>(registry_id()))
        .is_some_and(|registry| registry.windows.iter().any(|w| w.id == id && w.held && w.seen + 1 >= frame))
}

/// What a click on a window's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Act {
    /// Expand it where it was and bring it to the front.
    Restore,
    /// Collapse it (into the taskbar).
    Collapse,
    /// Bring it to the front.
    Front,
    /// Close it as its cross does (its own questions run).
    Close,
}

/// The click rule: a collapsed window is restored; of the open ones (*All
/// open windows*), the one in front is collapsed and another comes to the
/// front.
pub(super) fn click(held: bool, front: bool) -> Act {
    match (held, front) {
        (true, _) => Act::Restore,
        (false, true) => Act::Collapse,
        (false, false) => Act::Front,
    }
}

/// How `count` buttons fit in `room` points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Fit {
    /// The buttons on the island; the rest are in *More*.
    pub shown: usize,
    /// The width of each.
    pub width: f32,
    /// With their title, or their icon alone.
    pub text: bool,
    pub more: bool,
}

/// The buttons share the room, each [`MAX_W`] wide at most; when a title
/// would no longer read they are their icon alone; when even those do not
/// fit, the last ones go in *More*, at the right end.
pub(super) fn fit(room: f32, count: usize) -> Fit {
    if count == 0 {
        return Fit {
            shown: 0,
            width: 0.0,
            text: true,
            more: false,
        };
    }
    let n = count as f32;
    let each = ((room - GAP * (n - 1.0)) / n).min(MAX_W);
    if each >= MIN_TEXT_W {
        return Fit {
            shown: count,
            width: each.floor(),
            text: true,
            more: false,
        };
    }
    let icons = |room: f32| ((room + GAP) / (ICON_W + GAP)).floor().max(0.0) as usize;
    if icons(room) >= count {
        return Fit {
            shown: count,
            width: ICON_W,
            text: false,
            more: false,
        };
    }
    Fit {
        shown: icons(room - MORE_W - GAP).min(count),
        width: ICON_W,
        text: false,
        more: true,
    }
}

/// The windows of the taskbar, in its order: the held ones in the order
/// they were collapsed, or (`all`) every open one in the order they were
/// opened.
pub(super) fn listed(windows: &[Window], all: bool) -> Vec<Window> {
    let mut list: Vec<Window> = windows.iter().filter(|w| all || w.held).cloned().collect();
    list.sort_by_key(|w| if all { w.opened } else { w.collapsed });
    list
}

/// What the app gives the taskbar each frame.
pub struct Env<'a> {
    pub p: &'a Palette,
    pub icons: &'a Icons,
    /// The interface is hidden (its shortcut, immersive mode, mouselook
    /// without the interface): no taskbar.
    pub hidden: bool,
    /// In mouselook: the windows held stay held, even while the viewport
    /// is dropped there and no margin is left.
    pub mouselook: bool,
    /// The keyboard goes to the taskbar now (the key action *Focus the
    /// taskbar*): to its first button.
    pub focus: bool,
}

/// The windows the taskbar knows, for the tests.
#[cfg(test)]
pub(super) fn windows(ctx: &egui::Context) -> Vec<Window> {
    ctx.data(|d| d.get_temp::<Registry>(registry_id())).unwrap_or_default().windows
}

/// What the last frame drew, for the tests.
#[cfg(test)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Drawn {
    pub frame: u64,
    pub island: Option<Rect>,
    /// The buttons: the window's id, its rectangle, with its title.
    pub buttons: Vec<(String, Rect, bool)>,
    pub more: Option<Rect>,
}

#[cfg(test)]
pub(super) fn drawn(ctx: &egui::Context) -> Drawn {
    let drawn: Drawn = ctx
        .data(|d| d.get_temp(Id::unique("folderstorm_taskbar_drawn")))
        .unwrap_or_default();
    if drawn.frame + 1 >= ctx.cumulative_frame_nr_for(egui::ViewportId::ROOT) {
        drawn
    } else {
        Drawn::default()
    }
}

/// The id of the button of the window `id`.
pub(super) fn button_id(id: &str) -> Id {
    Id::unique(("folderstorm_taskbar_button", id))
}

fn paint_icon(ui: &egui::Ui, icons: &Icons, name: &str, center: Pos2, tint: egui::Color32) {
    if let Some(icon) = icons.get(name) {
        ui.painter().image(
            icon.id(),
            Rect::from_center_size(center, egui::Vec2::splat(ICON)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            tint,
        );
    }
}

/// The window in front among `windows`: the topmost of their layers that
/// is not a held one's. (Not `widgets::floater_is_front`: a click on the
/// taskbar brings the taskbar's own island to the top.)
fn front(ctx: &egui::Context, windows: &[Window]) -> Option<String> {
    ctx.memory(|m| {
        m.layer_ids()
            .filter(|layer| layer.order == Order::Middle && m.areas().is_visible(layer))
            .filter_map(|layer| windows.iter().find(|w| !w.held && Id::unique(w.id.as_str()) == layer.id))
            .last()
            .map(|w| w.id.clone())
    })
}

/// A window just held was the one on top: the next one comes to the front,
/// as when a window is minimized on a computer's desktop. Aurora's Ctrl+W
/// closes the topmost window, collapsed or not: without this it would
/// close a window that is not on screen.
fn hand_the_front_over(ctx: &egui::Context, windows: &[Window]) {
    let top = ctx.memory(|m| {
        m.layer_ids()
            .filter(|layer| layer.order == Order::Middle)
            .filter_map(|layer| windows.iter().find(|w| Id::unique(w.id.as_str()) == layer.id))
            .last()
            .cloned()
    });
    let Some(top) = top.filter(|w| w.held) else {
        return;
    };
    // (being restored: it is on top on purpose)
    let expanding = ctx
        .data(|d| d.get_temp::<bool>(Id::unique(top.id.as_str()).with(widgets::EXPAND)))
        .unwrap_or(false);
    if expanding {
        return;
    }
    if let Some(next) = front(ctx, windows) {
        ctx.move_to_top(LayerId::new(Order::Middle, Id::unique(next.as_str())));
    }
}

fn run(ctx: &egui::Context, id: &str, act: Act) {
    match act {
        // (expanded on its next frame, where it was, and brought forward)
        Act::Restore | Act::Front => widgets::bring_floater_front(ctx, id),
        Act::Collapse => widgets::minimize_floater(ctx, id),
        Act::Close => widgets::close_floater(ctx, id),
    }
}

/// The right-click menu of a window's button.
fn button_menu(ui: &mut egui::Ui, p: &Palette, window: &Window, front: bool) -> Option<Act> {
    let mut act = None;
    if window.held {
        if menu::item(ui, p, ICON_RESTORE, tr!("fs-taskbar-restore")) {
            act = Some(Act::Restore);
        }
    } else {
        if menu::item_if(ui, p, ICON_FRONT, tr!("fs-taskbar-front"), !front) {
            act = Some(Act::Front);
        }
        if menu::item(ui, p, ICON_COLLAPSE, tr!("common-collapse")) {
            act = Some(Act::Collapse);
        }
    }
    menu::separator(ui, p);
    if menu::item(ui, p, ICON_CLOSE, tr!("common-close")) {
        act = Some(Act::Close);
    }
    act
}

/// What the screen reader hears for a window's button.
fn spoken(window: &Window, front: bool) -> String {
    if window.held {
        tr!("fs-taskbar-spoken-collapsed", title = window.title.as_str())
    } else if front {
        tr!("fs-taskbar-spoken-front", title = window.title.as_str())
    } else {
        window.title.clone()
    }
}

/// The taskbar of this frame. To be called in the interface's pass, once
/// the bars are laid out, every frame (also while the interface is
/// hidden: the windows held stay held).
pub fn show(ctx: &egui::Context, desktop: &Desktop, env: &Env) {
    if !desktop.taskbar.on {
        // (the registry goes stale: `holds` answers false from the next frame)
        return;
    }
    let strip = super::taskbar_strip(ctx, desktop).map(|(_, rect)| rect);
    let frame = ctx.cumulative_frame_nr_for(egui::ViewportId::ROOT);
    let (windows, leaving) = ctx.data_mut(|d| {
        let registry = d.get_temp_mut_or_default::<Registry>(registry_id());
        let was = registry.holding && registry.live.is_some_and(|at| at + 1 >= frame);
        registry.live = Some(frame);
        registry.holding = strip.is_some() || (env.mouselook && was);
        registry.windows.retain(|w| w.seen + 1 >= frame);
        (registry.windows.clone(), std::mem::take(&mut registry.leaving))
    });
    // a held window being restored or closed is drawn collapsed for one
    // frame by `Floater::show` (its title bar, where it stood): that frame
    // of it is taken off the screen
    for (layer, at) in leaving {
        if at == frame {
            ctx.transform_layer_shapes(layer, egui::emath::TSTransform::from_translation(vec2(0.0, -1.0e6)));
        }
    }
    hand_the_front_over(ctx, &windows);
    let Some(rect) = strip.filter(|_| !env.hidden) else {
        return;
    };
    let p = env.p;
    let list = listed(&windows, desktop.taskbar.all_windows);
    let front = front(ctx, &windows);
    let fit = fit(rect.width() - PAD * 2.0, list.len());
    let mut acts: Vec<(String, Act)> = Vec::new();
    #[cfg(test)]
    let mut drawn = Drawn {
        frame,
        island: Some(rect),
        ..Default::default()
    };
    islands::area(ctx, p, ISLAND, rect, |ui| {
        // (the island takes the pointer between its buttons, as the dock does)
        ui.interact(rect, Id::unique("folderstorm_taskbar_back"), Sense::click());
        let cy = rect.center().y;
        if list.is_empty() {
            let text = if desktop.taskbar.all_windows {
                tr!("fs-taskbar-empty-all")
            } else {
                tr!("fs-taskbar-empty")
            };
            widgets::paint_elided(
                ui,
                text,
                pos2(rect.left() + PAD + 6.0, cy),
                rect.width() - PAD * 2.0 - 12.0,
                FontId::proportional(TEXT),
                p.muted,
            );
            return;
        }
        let mut x = rect.left() + PAD;
        for (place, window) in list.iter().take(fit.shown).enumerate() {
            let button = Rect::from_min_size(pos2(x, cy - BUTTON_H / 2.0), vec2(fit.width, BUTTON_H));
            x = button.right() + GAP;
            let is_front = front.as_deref() == Some(window.id.as_str());
            let response = ui.interact(button, button_id(&window.id), Sense::click());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, spoken(window, is_front)));
            if env.focus && place == 0 {
                response.request_focus();
            }
            let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
            islands::slot_face(ui, p, button, response.hovered() || open, is_front);
            // with every open window listed, a collapsed one is muted
            let ink = if window.held && desktop.taskbar.all_windows {
                p.muted
            } else {
                p.ink
            };
            if fit.text {
                paint_icon(ui, env.icons, &window.icon, pos2(button.left() + 6.0 + ICON / 2.0, cy), ink);
                widgets::paint_elided(
                    ui,
                    &window.title,
                    pos2(button.left() + 6.0 + ICON + 6.0, cy),
                    button.width() - ICON - 18.0,
                    FontId::proportional(TEXT),
                    ink,
                );
            } else {
                paint_icon(ui, env.icons, &window.icon, button.center(), ink);
            }
            let radius = ui.visuals().window_corner_radius;
            if window.attention {
                let (wash, moving) = window.flash;
                if moving {
                    ui.ctx().request_repaint();
                }
                widgets::attention_mark(ui.painter(), p, button, radius, wash);
            }
            if response.has_focus() {
                ui.painter()
                    .rect_stroke(button, radius, Stroke::new(1.5, p.violet_light), StrokeKind::Inside);
            }
            if response.clicked() {
                acts.push((window.id.clone(), click(window.held, is_front)));
            } else if response.clicked_by(egui::PointerButton::Middle) {
                acts.push((window.id.clone(), Act::Close));
            }
            menu::context_menu(&response, p, |ui| {
                if let Some(act) = button_menu(ui, p, window, is_front) {
                    acts.push((window.id.clone(), act));
                }
            });
            if !open {
                response.on_hover_text(&window.title);
            }
            #[cfg(test)]
            drawn.buttons.push((window.id.clone(), button, fit.text));
        }
        if fit.more {
            let more = Rect::from_min_size(pos2(rect.right() - PAD - MORE_W, cy - BUTTON_H / 2.0), vec2(MORE_W, BUTTON_H));
            let response = ui.interact(more, Id::unique("folderstorm_taskbar_more"), Sense::click());
            let left = list.len() - fit.shown;
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tr!("fs-strip-more", n = left as i64)));
            let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
            let hot = response.hovered() || open;
            islands::slot_face(ui, p, more, hot, false);
            let ink = if hot { p.ink } else { p.muted };
            let icon = pos2(more.left() + 6.0 + ICON / 2.0, cy);
            paint_icon(ui, env.icons, ICON_MORE, icon, ink);
            let count = ui.painter().layout_no_wrap(left.to_string(), FontId::proportional(TEXT), ink);
            widgets::paint_ink_left(ui.painter(), count, pos2(icon.x + ICON / 2.0 + 5.0, cy), ink);
            if response.has_focus() {
                let radius = ui.visuals().window_corner_radius;
                ui.painter()
                    .rect_stroke(more, radius, Stroke::new(1.5, p.violet_light), StrokeKind::Inside);
            }
            // the windows that did not fit, by their title: a click does
            // what the button would
            menu::dropdown_placed(
                &response,
                p,
                egui::PopupCloseBehavior::CloseOnClick,
                Some((egui::RectAlign::TOP_END, 10.0)),
                |ui| {
                    for window in list.iter().skip(fit.shown) {
                        let is_front = front.as_deref() == Some(window.id.as_str());
                        if menu::item(ui, p, &window.icon, &window.title) {
                            acts.push((window.id.clone(), click(window.held, is_front)));
                        }
                    }
                },
            );
            if !open {
                response.on_hover_text(tr!("fs-strip-more", n = left as i64));
            }
            #[cfg(test)]
            {
                drawn.more = Some(more);
            }
        }
    });
    #[cfg(test)]
    ctx.data_mut(|d| d.insert_temp(Id::unique("folderstorm_taskbar_drawn"), drawn));
    for (id, act) in acts {
        run(ctx, &id, act);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: &str, held: bool, opened: u64, collapsed: u64) -> Window {
        Window {
            id: id.to_owned(),
            title: id.to_owned(),
            icon: "x".to_owned(),
            held,
            down: held,
            attention: false,
            flash: (0.0, false),
            seen: 0,
            opened,
            collapsed,
        }
    }

    #[test]
    fn a_click_restores_collapses_or_brings_to_the_front() {
        assert_eq!(click(true, false), Act::Restore);
        assert_eq!(click(true, true), Act::Restore);
        assert_eq!(click(false, true), Act::Collapse);
        assert_eq!(click(false, false), Act::Front);
    }

    #[test]
    fn the_buttons_narrow_to_their_icon_then_go_in_more() {
        let room = 600.0;
        // a few: their title, never wider than the widest
        assert_eq!(
            fit(room, 2),
            Fit {
                shown: 2,
                width: MAX_W,
                text: true,
                more: false
            }
        );
        // they share the room while a title still reads
        let six = fit(room, 6);
        assert!(six.text && !six.more && six.shown == 6);
        assert!(six.width >= MIN_TEXT_W && six.width * 6.0 + GAP * 5.0 <= room);
        // then their icon alone
        let ten = fit(room, 10);
        assert_eq!((ten.shown, ten.text, ten.more, ten.width), (10, false, false, ICON_W));
        let all = ((room + GAP) / (ICON_W + GAP)).floor() as usize;
        assert_eq!(fit(room, all).shown, all);
        assert!(!fit(room, all).more);
        // then the rest in More, which keeps its place at the right end
        let many = fit(room, all + 1);
        assert!(many.more && !many.text);
        assert!(many.shown < all + 1 && many.shown > 0);
        assert!(many.shown as f32 * (ICON_W + GAP) + MORE_W <= room);
        // no room at all: More alone
        assert_eq!((fit(40.0, 5).shown, fit(40.0, 5).more), (0, true));
        assert_eq!(fit(room, 0).shown, 0);
        assert!(!fit(room, 0).more);
    }

    #[test]
    fn the_held_windows_are_listed_as_collapsed_and_all_as_opened() {
        let windows = [window("a", true, 0, 9), window("b", false, 2, 3), window("c", true, 4, 5)];
        let ids = |all: bool| listed(&windows, all).iter().map(|w| w.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(false), ["c", "a"], "the collapsed ones, in the order they were collapsed");
        assert_eq!(ids(true), ["a", "b", "c"], "every open one, in the order they were opened");
    }
}
