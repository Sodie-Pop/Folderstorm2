//! The window's side of checkpoint 6, pop-out panels (session-host plan,
//! section 7): the chat, the characters' controls and the inbox can each
//! leave the host's window for a window of their own, and come back.
//! (Until checkpoint 9 the monitors were such a panel; each monitor now
//! has its own window: `monitor_windows.rs`.)
//! Which are out and where is the `folderstorm-session` crate's
//! (`panels`); this module opens, places and closes their windows.
//!
//! - **Each window is drawn on the processor**, as the host's own: a
//!   [`View`] is a window with its own interface context, painter and
//!   pixels. The host still opens no graphics device.
//! - **Closing a panel's window docks the panel.** Only the host's own
//!   window closes the host.
//! - **No window takes the keyboard by itself.** A panel the user pops
//!   out opens in front, as any window they ask for; the panels found out
//!   when the host starts open without taking the keyboard, and nothing
//!   here ever asks for the foreground afterwards. No character's window
//!   is touched: popping a panel out or back changes nothing for the
//!   viewers.
//! - **A window opens where a screen shows it** (`panels::on_screen`): a
//!   place saved on a screen that is gone comes back on one that is there.

use super::monitor_windows::Seat;
use super::{Act, BUTTON, Window_};
use crate::theme::{Palette, Theme};
use crate::ui::{
    fonts,
    icons::Icons,
    widgets::{self, ActionKind},
};
use ::folderstorm_session::panels::{self, MIN_SIZE, PANELS, Panel, Place, Sight};
use aurora_i18n::tr;
use egui::RichText;
use std::sync::Arc;
use winit::{
    event::WindowEvent,
    event_loop::{ActiveEventLoop, OwnedDisplayHandle},
    window::{Window, WindowId},
};

/// One of the host's windows: its own, or a panel's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Which {
    Main,
    Pane(Panel),
    /// Checkpoint 8: the bar's compact window (`ui/bar.rs`).
    Bar,
    /// Checkpoint 9: a character's monitor, and the cycling monitor
    /// (`ui/monitor_windows.rs`).
    Monitor(usize),
    Cycling,
}

/// What every window of the host is drawn with.
pub(super) struct Look {
    pub theme: Theme,
    pub font_scale: f32,
    /// The user's font; None in the tests (egui's own).
    pub font: Option<String>,
}

impl Look {
    /// A window's interface, before its window: a context of its own
    /// with the theme, the fonts and the icons.
    pub fn view(&self) -> View {
        let context = egui::Context::default();
        self.theme.apply(&context, self.font_scale);
        if let Some(font) = &self.font {
            fonts::apply(&context, font);
        }
        let icons = Icons::load(&context, None);
        View {
            context,
            _icons: icons,
            painter: Default::default(),
            surface: None,
            window: None,
            input: None,
            redraw: true,
            focused: false,
            occluded: false,
            hidden: false,
            monitors: Default::default(),
        }
    }
}

/// A window of the host and what draws into it.
pub(super) struct View {
    pub context: egui::Context,
    /// (keeps the icons' textures alive)
    _icons: Icons,
    pub painter: crate::crash::software::Painter,
    pub surface: Option<softbuffer::Surface<OwnedDisplayHandle, Arc<Window>>>,
    pub window: Option<Arc<Window>>,
    pub input: Option<egui_winit::State>,
    pub redraw: bool,
    /// The window has the keyboard.
    pub focused: bool,
    /// The window is covered whole by others, as the system says.
    pub occluded: bool,
    /// Checkpoint 8: the window is hidden (the host's own, while a bar is
    /// on screen): nothing in it is seen.
    pub hidden: bool,
    /// The monitors' textures in this window, and which are on screen.
    pub monitors: super::monitors::Panel,
}

impl View {
    /// Gives the view its window: the pixels and the keyboard's and
    /// mouse's way into the interface.
    pub fn attach(&mut self, event_loop: &ActiveEventLoop, window: Window) -> anyhow::Result<()> {
        let window = Arc::new(window);
        let display = softbuffer::Context::new(event_loop.owned_display_handle()).map_err(|e| anyhow::anyhow!("{e}"))?;
        let surface = softbuffer::Surface::new(&display, window.clone()).map_err(|e| anyhow::anyhow!("{e}"))?;
        let input = egui_winit::State::new(
            self.context.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        window.request_redraw();
        self.window = Some(window);
        self.surface = Some(surface);
        self.input = Some(input);
        Ok(())
    }

    /// As the crash window does: egui-winit's state and the window go
    /// while the event loop still lives (Wayland).
    pub fn detach(&mut self) {
        self.input = None;
        self.surface = None;
        self.window = None;
    }

    fn minimised(&self) -> bool {
        self.window.as_ref().and_then(|w| w.is_minimized()).unwrap_or(false)
    }

    /// What this window shows of the monitors now.
    pub fn sight(&self) -> Sight {
        Sight {
            seen: self.monitors.seen,
            minimised: self.minimised() || self.hidden,
            occluded: self.occluded,
        }
    }

    /// Where the window is, when that is a place to open it again: not
    /// while it is minimised or maximised (the place before is kept).
    fn place(&self) -> Option<Place> {
        let window = self.window.as_ref()?;
        if self.minimised() || window.is_maximized() {
            return None;
        }
        let size = window.inner_size();
        Some(Place {
            // (not told on Wayland: the system places the window)
            position: window.outer_position().ok().map(|at| [at.x, at.y]),
            size: [size.width, size.height],
        })
    }
}

/// The connected screens, for a saved place to be checked against.
pub(super) fn screens(event_loop: &ActiveEventLoop) -> Vec<panels::Screen> {
    event_loop
        .available_monitors()
        .map(|monitor| {
            let (at, size) = (monitor.position(), monitor.size());
            panels::Screen {
                x: at.x,
                y: at.y,
                width: size.width,
                height: size.height,
            }
        })
        .collect()
}

/// A panel's name.
pub(super) fn name(panel: Panel) -> &'static str {
    match panel {
        Panel::Chat => tr!("folderstorm-chat-heading"),
        Panel::Controls => tr!("folderstorm-host-heading"),
        Panel::Inbox => tr!("folderstorm-inbox"),
    }
}

impl Window_ {
    pub(super) fn view(&self, which: Which) -> Option<&View> {
        match which {
            Which::Main => Some(&self.main),
            Which::Pane(panel) => self.panes[panel.index()].as_ref(),
            Which::Bar => self.bar.view.as_ref(),
            Which::Monitor(_) | Which::Cycling => Seat::of(which).and_then(|seat| self.monitors.view(seat)),
        }
    }

    pub(super) fn view_mut(&mut self, which: Which) -> Option<&mut View> {
        match which {
            Which::Main => Some(&mut self.main),
            Which::Pane(panel) => self.panes[panel.index()].as_mut(),
            Which::Bar => self.bar.view.as_mut(),
            Which::Monitor(_) | Which::Cycling => Seat::of(which).and_then(|seat| self.monitors.view_mut(seat)),
        }
    }

    /// Whether a panel has a window of its own.
    pub(super) fn out(&self, panel: Panel) -> bool {
        self.host.desk.panels().out(panel)
    }

    /// The window a panel is drawn in.
    pub(super) fn home(&self, panel: Panel) -> Which {
        if self.out(panel) { Which::Pane(panel) } else { Which::Main }
    }

    /// The window an event is for.
    pub(super) fn which(&self, id: WindowId) -> Option<Which> {
        let is = |view: &View| view.window.as_ref().is_some_and(|w| w.id() == id);
        if is(&self.main) {
            return Some(Which::Main);
        }
        if self.bar.view.as_ref().is_some_and(is) {
            return Some(Which::Bar);
        }
        if let Some((seat, _)) = self.monitors.views().find(|(_, view)| is(view)) {
            return Some(seat.which());
        }
        Panel::ALL
            .into_iter()
            .find(|panel| self.panes[panel.index()].as_ref().is_some_and(is))
            .map(Which::Pane)
    }

    /// Every window is drawn again.
    pub(super) fn redraw_all(&mut self) {
        self.main.redraw = true;
        for pane in self.panes.iter_mut().flatten().chain(&mut self.bar.view) {
            pane.redraw = true;
        }
        for view in self.monitors.views_mut() {
            view.redraw = true;
        }
    }

    /// The panels' windows follow the desk: one opens for a panel that is
    /// out and has none, and the window of a panel that is docked closes.
    /// The windows opened by the first call are those found out when the
    /// host starts: they do not take the keyboard.
    pub(super) fn sync_panes(&mut self, event_loop: &ActiveEventLoop) {
        let restoring = !std::mem::replace(&mut self.restored, true);
        for panel in Panel::ALL {
            let i = panel.index();
            match (self.out(panel), self.panes[i].is_some()) {
                (true, false) => match self.open_pane(event_loop, panel, !restoring) {
                    Ok(view) => {
                        log::info!("session host: {} in its own window", panel.key());
                        self.panes[i] = Some(view);
                        self.redraw_all();
                    }
                    Err(e) => {
                        // no window: the panel stays in the host's
                        log::warn!("session host: no window for {} ({e}): docked", panel.key());
                        self.host.desk.panel_closed(panel);
                        self.redraw_all();
                    }
                },
                (false, true) => {
                    log::info!("session host: {} docked back", panel.key());
                    if let Some(mut view) = self.panes[i].take() {
                        view.detach();
                    }
                    self.redraw_all();
                }
                _ => {}
            }
        }
    }

    /// The title of a panel's window; the inbox's says how many wait.
    pub(super) fn pane_title(&self, panel: Panel) -> String {
        match (panel, self.host.inbox.count()) {
            (Panel::Inbox, count @ 1..) => format!("{} ({}) — {}", name(panel), super::inbox::count_text(count), self.title),
            _ => format!("{} — {}", name(panel), self.title),
        }
    }

    fn open_pane(&self, event_loop: &ActiveEventLoop, panel: Panel, active: bool) -> anyhow::Result<View> {
        let title = self.pane_title(panel);
        let [width, height] = panel.default_size();
        let mut attributes = crate::cli::with_desktop_id(Window::default_attributes())
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(width, height))
            .with_min_inner_size(winit::dpi::LogicalSize::new(MIN_SIZE[0], MIN_SIZE[1]))
            .with_active(active);
        if let Some(saved) = self.host.desk.panels().get(panel).place {
            let place = panels::on_screen(saved, &screens(event_loop));
            if place != saved {
                log::info!("session host: {}'s window was off the screens: brought back", panel.key());
            }
            attributes = attributes.with_inner_size(winit::dpi::PhysicalSize::new(place.size[0], place.size[1]));
            if let Some([x, y]) = place.position {
                attributes = attributes.with_position(winit::dpi::PhysicalPosition::new(x, y));
            }
        }
        let mut view = self.look.view();
        view.attach(event_loop, event_loop.create_window(attributes)?)?;
        Ok(view)
    }

    /// What a panel's window is told, before the interface hears it. True
    /// when the window is gone (closed: its panel is docked).
    pub(super) fn pane_event(&mut self, panel: Panel, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CloseRequested => {
                // docked back; never the host's or a character's end
                self.host.desk.panel_closed(panel);
                if let Some(mut view) = self.panes[panel.index()].take() {
                    view.detach();
                }
                log::info!("session host: {}'s window closed: docked back", panel.key());
                self.redraw_all();
                true
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if let Some(place) = self.panes[panel.index()].as_ref().and_then(View::place) {
                    self.host.desk.panel_placed(panel, place);
                }
                false
            }
            _ => false,
        }
    }

    /// At the top of the host's window: which parts have a window of
    /// their own.
    pub(super) fn panel_strip(&self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<Act>) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr!("folderstorm-panel-own-window")).color(p.muted));
            for panel in Panel::ALL {
                let mut out = self.out(panel);
                // the inbox's badge: how many wait, wherever the inbox is
                let label = match (panel, self.host.inbox.count()) {
                    (Panel::Inbox, count @ 1..) => RichText::new(format!("{} ({})", name(panel), super::inbox::count_text(count)))
                        .strong()
                        .color(p.amber),
                    _ => RichText::new(name(panel)),
                };
                if widgets::checkbox(ui, p, &mut out, label)
                    .on_hover_text(tr!("folderstorm-panel-own-window-help"))
                    .changed()
                {
                    acts.push(Act::Panel(panel, out));
                }
            }
        });
    }

    /// In the host's window, in the place of the panels that are out.
    pub(super) fn panels_elsewhere(&self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<Act>) {
        ui.label(
            RichText::new(tr!("folderstorm-host-title", product = folderstorm_brand::PRODUCT))
                .size(18.0)
                .strong()
                .color(p.ink),
        );
        ui.add_space(8.0);
        for panel in Panel::ALL.into_iter().filter(|panel| self.out(*panel)) {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tr!("folderstorm-panel-out", panel = name(panel))).color(p.muted));
                self.dock_button(ui, p, panel, acts);
            });
            ui.add_space(4.0);
        }
    }

    /// "Dock back", in a panel's own window and beside its name in the
    /// host's.
    pub(super) fn dock_button(&self, ui: &mut egui::Ui, p: &Palette, panel: Panel, acts: &mut Vec<Act>) {
        let dock = tr!("folderstorm-panel-dock");
        if widgets::action_button(ui, p, "arrow-back-up", dock, ActionKind::Flat, BUTTON)
            .on_hover_text(tr!("folderstorm-panel-dock-help"))
            .clicked()
        {
            acts.push(Act::Panel(panel, false));
        }
    }
}

/// The panels' windows, none open.
pub(super) fn no_panes() -> [Option<View>; PANELS] {
    Default::default()
}

#[cfg(test)]
mod tests {
    use super::super::super::chat_ui::tests::{heard, host};
    use super::super::tests::{draw, draw_in, window};
    use super::*;
    use ::folderstorm_session::ipc::{Conversation, LineKind};
    use std::time::Instant;

    fn shows(drawn: &[String], text: &str) -> bool {
        drawn.iter().any(|t| t.contains(text))
    }

    fn painted(pixels: &[u32]) -> usize {
        let background = pixels[0];
        pixels.iter().filter(|p| **p != background).count()
    }

    /// The panel's window as `sync_panes` makes it, without a window.
    fn pop_out(app: &mut Window_, panel: Panel) {
        app.host.desk.set_panel_out(panel, true);
        app.panes[panel.index()] = Some(app.look.view());
    }

    fn running(app: &mut Window_, now: Instant) {
        for slot in 0..2 {
            app.host.desk.launch(slot, slot == 0);
            app.host.desk.connected(slot, 1, now);
        }
        app.host.desk.take_effects();
    }

    #[test]
    fn every_panel_starts_in_the_host_s_window() {
        let app = window(host());
        assert!(app.panes.iter().all(Option::is_none));
        for panel in Panel::ALL {
            assert_eq!(app.home(panel), Which::Main);
            assert!(!name(panel).is_empty());
        }
    }

    #[test]
    fn the_chat_is_drawn_in_its_own_window_on_the_processor() {
        let mut host = host();
        heard(
            &mut host,
            0,
            Conversation::Nearby,
            1,
            LineKind::Said,
            "Jane Doe",
            "hello neighbours",
        );
        let mut app = window(host);
        // docked: the host's window offers the panels and shows the chat
        draw(&mut app, 1120, 760);
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, tr!("folderstorm-panel-own-window")));
        assert!(shows(&drawn, "hello neighbours"));
        pop_out(&mut app, Panel::Chat);
        assert_eq!(app.home(Panel::Chat), Which::Pane(Panel::Chat));
        // the host's window: the characters, no chat
        draw(&mut app, 1120, 760);
        let (drawn, pixels) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, "demo-1") && !shows(&drawn, "hello neighbours"), "{drawn:?}");
        assert!(painted(&pixels) > 10_000);
        // a narrow host window has no chat page to turn to
        let (drawn, _) = draw(&mut app, 460, 760);
        assert!(shows(&drawn, "demo-1"));
        // the chat's window: the chat, painted by the processor, and the
        // way back
        draw_in(&mut app, Which::Pane(Panel::Chat), 620, 640);
        let (drawn, pixels) = draw_in(&mut app, Which::Pane(Panel::Chat), 620, 640);
        assert!(shows(&drawn, "hello neighbours") && shows(&drawn, tr!("folderstorm-panel-dock")));
        assert!(!shows(&drawn, tr!("folderstorm-host-intro")), "{drawn:?}");
        assert!(painted(&pixels) > 5_000, "something was painted");
    }

    #[test]
    fn what_the_chat_s_window_shows_is_read_only_while_that_window_has_the_keyboard() {
        let mut host = host();
        heard(&mut host, 0, Conversation::Nearby, 1, LineKind::Said, "Jane Doe", "hello");
        let mut app = window(host);
        app.main.focused = true;
        pop_out(&mut app, Panel::Chat);
        // the host's window has the keyboard, the chat's has not: the
        // line it shows is not read
        draw(&mut app, 1120, 760);
        draw_in(&mut app, Which::Pane(Panel::Chat), 620, 640);
        assert_eq!(app.host.chat.total_unread(), 1);
        // drawing the host's window does not change what the chat looks at
        if let Some(pane) = &mut app.panes[Panel::Chat.index()] {
            pane.focused = true;
        }
        draw_in(&mut app, Which::Pane(Panel::Chat), 620, 640);
        assert_eq!(app.host.chat.total_unread(), 0);
        heard(&mut app.host, 0, Conversation::Nearby, 2, LineKind::Said, "Jane Doe", "again");
        draw(&mut app, 1120, 760);
        draw_in(&mut app, Which::Pane(Panel::Chat), 620, 640);
        assert_eq!(app.host.chat.total_unread(), 0);
    }

    #[test]
    fn the_controls_are_drawn_in_their_own_window_and_the_host_s_says_where_they_are() {
        let mut app = window(host());
        pop_out(&mut app, Panel::Controls);
        draw_in(&mut app, Which::Pane(Panel::Controls), 540, 720);
        let (drawn, pixels) = draw_in(&mut app, Which::Pane(Panel::Controls), 540, 720);
        assert!(shows(&drawn, "demo-1") && shows(&drawn, "demo-2"));
        assert!(shows(&drawn, tr!("folderstorm-host-intro")) && shows(&drawn, tr!("folderstorm-panel-dock")));
        assert!(painted(&pixels) > 5_000);
        // the host's window keeps the chat, whatever its width
        let (drawn, _) = draw(&mut app, 600, 760);
        assert!(shows(&drawn, tr!("folderstorm-chat-all")), "{drawn:?}");
        assert!(!shows(&drawn, tr!("folderstorm-host-intro")));
        // both out: the host's window says where they are and offers the
        // way back
        pop_out(&mut app, Panel::Chat);
        draw(&mut app, 600, 400);
        let (drawn, pixels) = draw(&mut app, 600, 400);
        let out = |panel| tr!("folderstorm-panel-out", panel = name(panel));
        assert!(
            shows(&drawn, &out(Panel::Chat)) && shows(&drawn, &out(Panel::Controls)),
            "{drawn:?}"
        );
        assert!(!shows(&drawn, &out(Panel::Inbox)));
        assert!(shows(&drawn, tr!("folderstorm-panel-dock")));
        assert!(painted(&pixels) > 1_000);
    }

    #[test]
    fn the_host_s_close_question_stays_in_the_host_s_window() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        pop_out(&mut app, Panel::Controls);
        app.closing = true;
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, tr!("folderstorm-host-close-question")), "{drawn:?}");
        let (drawn, _) = draw_in(&mut app, Which::Pane(Panel::Controls), 540, 720);
        assert!(!shows(&drawn, tr!("folderstorm-host-close-question")));
        assert!(shows(&drawn, tr!("folderstorm-host-quit-all")));
    }

    #[test]
    fn closing_a_panel_s_window_docks_it_and_keeps_the_host_open() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        pop_out(&mut app, Panel::Inbox);
        pop_out(&mut app, Panel::Chat);
        draw_in(&mut app, Which::Pane(Panel::Inbox), 520, 380);
        app.host.desk.take_effects();
        // the window's close button
        assert!(app.pane_event(Panel::Inbox, &WindowEvent::CloseRequested));
        assert!(app.panes[Panel::Inbox.index()].is_none());
        assert!(!app.out(Panel::Inbox) && app.out(Panel::Chat));
        assert!(app.panes[Panel::Chat.index()].is_some(), "the other panel's window stays");
        assert!(!app.host.exit && !app.closing);
        assert_eq!(app.host.desk.live(), 2);
        let effects = app.host.desk.take_effects();
        assert!(
            matches!(effects.as_slice(), [::folderstorm_session::host::Effect::Save]),
            "{effects:?}"
        );
        // "Dock back" does the same from inside the interface
        let acts = vec![Act::Panel(Panel::Chat, false)];
        app.apply(acts, now);
        assert!(!app.out(Panel::Chat));
    }

    /// Every icon this file names is one the viewer embeds.
    #[test]
    fn the_panels_icons_exist() {
        assert!(crate::ui::icons::embedded("arrow-back-up"));
        assert!(crate::ui::icons::embedded("arrows-exchange"));
    }
}
