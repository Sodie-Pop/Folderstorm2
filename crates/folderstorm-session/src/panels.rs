//! Checkpoint 6 of the session host, pop-out panels (session-host plan,
//! section 7): the host's chat, its characters' controls and its monitors
//! can each leave the host's window for a window of their own, and come
//! back.
//!
//! The rules, all pure:
//! - which panels are out, and where their windows were ([`Config`]),
//!   checked when the settings file is read ([`Config::cleaned`]);
//! - a window is never opened where no screen shows it ([`on_screen`]);
//! - closing a panel's window docks the panel back and nothing else
//!   ([`Config::closed`]): only the host's own window closes the host;
//! - a monitor is seen while any window that draws it is visible
//!   ([`seen`]): a minimised, hidden or covered window sees none.

use crate::ipc::MAX_SLOTS;
use serde::{Deserialize, Serialize};

/// A part of the host's window that can have a window of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Panel {
    /// The shared chat.
    Chat,
    /// The characters and what can be done with each.
    Controls,
    /// The background characters' pictures.
    Monitors,
    /// Checkpoint 7: what waits for an answer, across all characters.
    Inbox,
}

/// How many panels there are.
pub const PANELS: usize = 4;

impl Panel {
    pub const ALL: [Panel; PANELS] = [Panel::Chat, Panel::Controls, Panel::Monitors, Panel::Inbox];

    pub fn index(self) -> usize {
        match self {
            Panel::Chat => 0,
            Panel::Controls => 1,
            Panel::Monitors => 2,
            Panel::Inbox => 3,
        }
    }

    /// The panel's name in the demo's variable.
    pub fn key(self) -> &'static str {
        match self {
            Panel::Chat => "chat",
            Panel::Controls => "controls",
            Panel::Monitors => "monitors",
            Panel::Inbox => "inbox",
        }
    }

    /// The size of the panel's window the first time it opens, and the
    /// smallest it takes, in the screen's points.
    pub fn default_size(self) -> [u32; 2] {
        match self {
            Panel::Chat => [620, 640],
            Panel::Controls => [540, 720],
            Panel::Monitors => [700, 520],
            Panel::Inbox => [520, 380],
        }
    }
}

/// The smallest window of a panel, in the screen's points.
pub const MIN_SIZE: [u32; 2] = [360, 280];

/// The panels named in a list such as `chat,monitors` (the demo's
/// variable), each once, in the panels' own order. Unknown names are
/// dropped; `all` names them all.
pub fn parse_list(list: &str) -> Vec<Panel> {
    let names: Vec<String> = list.split(',').map(|name| name.trim().to_ascii_lowercase()).collect();
    Panel::ALL
        .into_iter()
        .filter(|panel| {
            names
                .iter()
                .any(|name| name == panel.key() || name == "all" || (*panel == Panel::Controls && name == "characters"))
        })
        .collect()
}

/// Where a panel's window is: the place of its outer corner on the desktop
/// (None where the system does not tell, as on Wayland) and the size of
/// its inside, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    #[serde(default)]
    pub position: Option<[i32; 2]>,
    pub size: [u32; 2],
}

/// The largest side of a window, and how far from the desktop's origin
/// one may be: numbers a window can have, not numbers to overflow with.
const MAX_SIDE: u32 = 16_384;
const FAR: i32 = 1 << 16;

impl Place {
    /// A place a window can have had. Whether a screen shows it today is
    /// [`on_screen`]'s matter.
    pub fn valid(&self) -> bool {
        let side = |side: u32, least: u32| (least / 2..=MAX_SIDE).contains(&side);
        side(self.size[0], MIN_SIZE[0])
            && side(self.size[1], MIN_SIZE[1])
            && self.position.is_none_or(|at| at.iter().all(|c| (-FAR..=FAR).contains(c)))
    }
}

/// One panel in the host's settings file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelConfig {
    /// The panel has its own window.
    pub out: bool,
    /// Where that window was last: kept while the panel is docked, for
    /// the next time it leaves.
    pub place: Option<Place>,
}

/// The panels in the host's settings file (`session-host.json`,
/// `panels`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub chat: PanelConfig,
    pub controls: PanelConfig,
    pub monitors: PanelConfig,
    /// (absent from a file of before checkpoint 7: docked)
    pub inbox: PanelConfig,
}

impl Config {
    pub fn get(&self, panel: Panel) -> PanelConfig {
        match panel {
            Panel::Chat => self.chat,
            Panel::Controls => self.controls,
            Panel::Monitors => self.monitors,
            Panel::Inbox => self.inbox,
        }
    }

    fn get_mut(&mut self, panel: Panel) -> &mut PanelConfig {
        match panel {
            Panel::Chat => &mut self.chat,
            Panel::Controls => &mut self.controls,
            Panel::Monitors => &mut self.monitors,
            Panel::Inbox => &mut self.inbox,
        }
    }

    pub fn out(&self, panel: Panel) -> bool {
        self.get(panel).out
    }

    /// The file as the host can use it: a place no window can have had is
    /// dropped (the panel then opens at its first size, where the system
    /// puts it).
    pub fn cleaned(mut self) -> Self {
        for panel in Panel::ALL {
            let config = self.get_mut(panel);
            config.place = config.place.filter(Place::valid);
        }
        self
    }

    /// The panel leaves the host's window, or comes back. Its place is
    /// kept either way. True when that changed something.
    pub fn set_out(&mut self, panel: Panel, out: bool) -> bool {
        std::mem::replace(&mut self.get_mut(panel).out, out) != out
    }

    /// The panel's window was closed: the panel is docked back. Never
    /// more than that: closing a panel closes neither the host nor a
    /// character. True when that changed something.
    pub fn closed(&mut self, panel: Panel) -> bool {
        self.set_out(panel, false)
    }

    /// The panel's window was moved or resized. A place no window can
    /// have is not kept. True when that changed something.
    pub fn placed(&mut self, panel: Panel, place: Place) -> bool {
        let config = self.get_mut(panel);
        if !place.valid() || config.place == Some(place) {
            return false;
        }
        config.place = Some(place);
        true
    }
}

/// A connected screen: its rectangle on the desktop, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The part of a window's top edge that must show for the window to be
/// caught and moved: this wide and this high, in physical pixels.
const GRIP: [i64; 2] = [120, 24];

/// Where a window saved at `place` opens today, given the connected
/// screens: where it was when a screen still shows enough of its top
/// edge to move it; otherwise whole on the nearest screen, no larger
/// than it. Without a known screen or a saved position, the system
/// places the window (position None).
pub fn on_screen(place: Place, screens: &[Screen]) -> Place {
    let screens: Vec<Screen> = screens.iter().copied().filter(|s| s.width > 0 && s.height > 0).collect();
    let (Some([x, y]), false) = (place.position, screens.is_empty()) else {
        return Place { position: None, ..place };
    };
    let (x, y) = (i64::from(x), i64::from(y));
    let (width, height) = (i64::from(place.size[0]), i64::from(place.size[1]));
    let edges = |s: &Screen| {
        let (left, top) = (i64::from(s.x), i64::from(s.y));
        (left, top, left + i64::from(s.width), top + i64::from(s.height))
    };
    // the top edge's strip, as wide as the window
    let grip_shows = |s: &Screen| {
        let (left, top, right, bottom) = edges(s);
        let wide = (x + width).min(right) - x.max(left);
        let high = (y + GRIP[1]).min(bottom) - y.max(top);
        // (a title bar above the screen's top cannot be caught)
        y >= top && wide >= GRIP[0].min(width) && high >= GRIP[1]
    };
    if screens.iter().any(grip_shows) {
        return place;
    }
    // nowhere to be caught: whole on the screen nearest to its centre
    let (cx, cy) = (x + width / 2, y + height / 2);
    let distance = |s: &Screen| {
        let (left, top, right, bottom) = edges(s);
        let dx = (left - cx).max(cx - (right - 1)).max(0);
        let dy = (top - cy).max(cy - (bottom - 1)).max(0);
        // (squares of two desktop distances: wider than their sum can be)
        i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy)
    };
    let Some(nearest) = screens.iter().min_by_key(|s| distance(s)) else {
        return Place { position: None, ..place };
    };
    let (left, top, right, bottom) = edges(nearest);
    let width = width.min(right - left);
    let height = height.min(bottom - top);
    let x = x.clamp(left, right - width);
    let y = y.clamp(top, bottom - height);
    Place {
        // (all four are within a screen's rectangle: they fit)
        position: Some([x as i32, y as i32]),
        size: [width as u32, height as u32],
    }
}

/// What one of the host's windows shows of the monitors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sight {
    /// The monitors on screen in the window's last drawing.
    pub seen: [bool; MAX_SLOTS],
    /// The window is minimised or hidden.
    pub minimised: bool,
    /// The window is covered whole by others, as the system says.
    pub occluded: bool,
}

impl Sight {
    /// The monitors someone can see in that window now.
    pub fn visible(&self) -> [bool; MAX_SLOTS] {
        if self.minimised || self.occluded {
            [false; MAX_SLOTS]
        } else {
            self.seen
        }
    }
}

/// The monitors on screen now in any of the host's windows: checkpoint
/// 5's rule, window by window. A monitor drawn only in a window that is
/// minimised, hidden or covered asks nothing of its viewer, whatever the
/// host's other windows do.
pub fn seen(windows: &[Sight]) -> [bool; MAX_SLOTS] {
    let mut seen = [false; MAX_SLOTS];
    for window in windows {
        for (slot, visible) in window.visible().into_iter().enumerate() {
            seen[slot] |= visible;
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    const HD: Screen = Screen {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    /// A second screen to the left of the first, lower.
    const LEFT: Screen = Screen {
        x: -1280,
        y: 200,
        width: 1280,
        height: 720,
    };

    fn place(x: i32, y: i32, width: u32, height: u32) -> Place {
        Place {
            position: Some([x, y]),
            size: [width, height],
        }
    }

    #[test]
    fn the_demo_s_list_names_panels() {
        assert_eq!(parse_list("chat,monitors"), [Panel::Chat, Panel::Monitors]);
        assert_eq!(parse_list(" Monitors , chat ,chat"), [Panel::Chat, Panel::Monitors]);
        assert_eq!(parse_list("characters"), [Panel::Controls]);
        assert_eq!(parse_list("controls"), [Panel::Controls]);
        assert_eq!(parse_list("all"), Panel::ALL);
        assert!(parse_list("").is_empty());
        assert!(parse_list("stage,1,chatty").is_empty());
        for (index, panel) in Panel::ALL.into_iter().enumerate() {
            assert_eq!(panel.index(), index);
            assert!(panel.default_size()[0] >= MIN_SIZE[0] && panel.default_size()[1] >= MIN_SIZE[1]);
        }
    }

    #[test]
    fn panels_start_docked_and_remember_where_their_window_was() {
        let mut panels = Config::default();
        assert!(Panel::ALL.into_iter().all(|panel| !panels.out(panel)));
        assert!(panels.set_out(Panel::Chat, true));
        assert!(!panels.set_out(Panel::Chat, true), "already out");
        assert!(panels.out(Panel::Chat) && !panels.out(Panel::Controls) && !panels.out(Panel::Monitors));
        let at = place(100, 80, 600, 500);
        assert!(panels.placed(Panel::Chat, at));
        assert!(!panels.placed(Panel::Chat, at), "the same place changes nothing");
        // docked back: the place is kept for the next time
        assert!(panels.set_out(Panel::Chat, false));
        assert_eq!(
            panels.get(Panel::Chat),
            PanelConfig {
                out: false,
                place: Some(at)
            }
        );
        // a place no window has is not kept
        assert!(!panels.placed(Panel::Chat, place(100, 80, 0, 500)));
        assert!(!panels.placed(Panel::Chat, place(i32::MIN, 80, 600, 500)));
        assert_eq!(panels.get(Panel::Chat).place, Some(at));
    }

    #[test]
    fn closing_a_panel_s_window_docks_it_and_nothing_else() {
        let mut panels = Config::default();
        panels.set_out(Panel::Chat, true);
        panels.set_out(Panel::Monitors, true);
        panels.placed(Panel::Monitors, place(10, 10, 700, 500));
        assert!(panels.closed(Panel::Monitors));
        assert!(!panels.out(Panel::Monitors));
        assert!(panels.out(Panel::Chat), "the other panels stay where they are");
        assert_eq!(panels.get(Panel::Monitors).place, Some(place(10, 10, 700, 500)));
        assert!(!panels.closed(Panel::Monitors), "closed twice: nothing more");
    }

    #[test]
    fn the_file_is_checked_when_read() {
        // an old file has no panels: all docked
        let old: Config = serde_json::from_str("{}").expect("reads");
        assert_eq!(old, Config::default());
        let read: Config = serde_json::from_str(
            r#"{
                "chat": {"out": true, "place": {"position": [100, 50], "size": [640, 480]}},
                "controls": {"out": true, "place": {"position": [0, 0], "size": [4000000000, 480]}},
                "monitors": {"place": {"position": [2000000000, 0], "size": [640, 480]}}
            }"#,
        )
        .expect("reads");
        let cleaned = read.cleaned();
        assert_eq!(cleaned.chat.place, Some(place(100, 50, 640, 480)));
        // the size or the position of no window: the panel is still out,
        // at its first size
        assert_eq!(cleaned.controls, PanelConfig { out: true, place: None });
        assert_eq!(cleaned.monitors, PanelConfig::default());
        // a place without a position (Wayland) is a place
        let wayland: PanelConfig = serde_json::from_str(r#"{"out": true, "place": {"size": [640, 480]}}"#).expect("reads");
        assert!(wayland.place.is_some_and(|p| p.valid() && p.position.is_none()));
        // written and read back the same
        let text = serde_json::to_string(&cleaned).expect("writes");
        assert_eq!(serde_json::from_str::<Config>(&text).expect("reads"), cleaned);
        // not the right shapes at all: the file does not read (the host
        // then starts from its defaults, as for the rest of the file)
        assert!(serde_json::from_str::<Config>(r#"{"chat": {"out": "yes"}}"#).is_err());
        assert!(serde_json::from_str::<Config>(r#"{"chat": {"place": {"size": [-1, 2]}}}"#).is_err());
    }

    #[test]
    fn a_window_a_screen_shows_stays_where_it_was() {
        let screens = [HD, LEFT];
        for at in [
            place(100, 80, 600, 500),
            // partly off the right and the bottom, its top edge showing
            place(1700, 900, 600, 500),
            // across both screens
            place(-300, 300, 600, 400),
            // on the second screen
            place(-1200, 250, 600, 400),
        ] {
            assert_eq!(on_screen(at, &screens), at, "{at:?}");
        }
    }

    #[test]
    fn a_window_off_every_screen_comes_back_whole_on_the_nearest() {
        // the second screen was unplugged: back on the first, at its edge
        assert_eq!(on_screen(place(-1200, 250, 600, 400), &[HD]), place(0, 250, 600, 400));
        // only a sliver of its top edge shows: not enough to catch
        assert_eq!(on_screen(place(1880, 100, 600, 400), &[HD]), place(1320, 100, 600, 400));
        // its title bar is above the screen
        assert_eq!(on_screen(place(300, -40, 600, 400), &[HD]), place(300, 0, 600, 400));
        // below the screen's bottom
        assert_eq!(on_screen(place(300, 1070, 600, 400), &[HD]), place(300, 680, 600, 400));
        // a minimised window's place, as Windows tells it
        assert_eq!(on_screen(place(-32000, -32000, 600, 400), &[HD]), place(0, 0, 600, 400));
        // nearest of two screens
        assert_eq!(on_screen(place(-1400, 1200, 600, 400), &[HD, LEFT]), place(-1280, 520, 600, 400));
        assert_eq!(on_screen(place(2500, 1200, 600, 400), &[HD, LEFT]), place(1320, 680, 600, 400));
        // larger than the screen it lands on: no larger than it
        let small = Screen {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        assert_eq!(on_screen(place(5000, 5000, 1600, 1200), &[small]), place(0, 0, 800, 600));
        // the far corners overflow nothing
        let far = place(i32::MAX, i32::MIN, u32::MAX, u32::MAX);
        assert_eq!(on_screen(far, &[HD]), place(0, 0, 1920, 1080));
    }

    #[test]
    fn without_a_screen_or_a_position_the_system_places_the_window() {
        let at = place(100, 80, 600, 500);
        assert_eq!(
            on_screen(at, &[]),
            Place {
                position: None,
                size: [600, 500]
            }
        );
        let empty = Screen {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        assert_eq!(on_screen(at, &[empty]).position, None);
        let unplaced = Place {
            position: None,
            size: [600, 500],
        };
        assert_eq!(on_screen(unplaced, &[HD]), unplaced);
    }

    #[test]
    fn a_monitor_is_seen_while_a_visible_window_draws_it() {
        let main = Sight {
            seen: [false, true, false, false, false],
            ..Default::default()
        };
        let monitors = Sight {
            seen: [false, false, true, true, false],
            ..Default::default()
        };
        assert_eq!(seen(&[main, monitors]), [false, true, true, true, false]);
        // the monitors' window is minimised: its monitors pause, the
        // host's own window's do not
        let minimised = Sight {
            minimised: true,
            ..monitors
        };
        assert_eq!(seen(&[main, minimised]), [false, true, false, false, false]);
        // the host's window is covered: the monitors' window goes on
        let covered = Sight { occluded: true, ..main };
        assert_eq!(seen(&[covered, monitors]), [false, false, true, true, false]);
        assert_eq!(seen(&[covered, minimised]), [false; MAX_SLOTS]);
        // drawn in two windows: seen while either shows
        let both = Sight {
            seen: [false, true, true, false, false],
            ..Default::default()
        };
        assert_eq!(seen(&[covered, both]), [false, true, true, false, false]);
        assert_eq!(seen(&[]), [false; MAX_SLOTS]);
    }
}
