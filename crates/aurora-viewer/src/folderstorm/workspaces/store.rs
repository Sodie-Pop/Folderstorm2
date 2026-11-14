//! The layouts as `settings.json` keeps them (field
//! `folderstorm_workspaces`), their limits and the list's operations. Pure:
//! nothing here touches egui or the viewer.
//!
//! Reading never fails and never drops an entry. Each entry of `layouts` is
//! read on its own: one made by a newer version, or one that breaks a limit
//! (a hand-edited file, a damaged one), is kept exactly as it was read
//! ([`Kept`]), shown as such, never applied and written back unchanged.
//! Fields this version does not know are kept on a layout it can use
//! ([`Layout::other`]), so a later version's parts survive an Update here.

use crate::folderstorm::viewport::ViewportSettings;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The schema this version writes.
pub const SCHEMA: u32 = 1;
/// At most this many layouts.
pub const MAX_LAYOUTS: usize = 32;
/// At most this many windows in a layout.
pub const MAX_WINDOWS: usize = 64;
/// A name has 1 to this many characters.
pub const MAX_NAME: usize = 40;
/// A window id has at most this many characters.
const MAX_ID: usize = 64;
/// Sizes are above zero and at most this, in points.
pub const MAX_POINTS: f32 = 16_384.0;
/// Positions and monitors stay within this distance of the origin.
const MAX_DISTANCE: f32 = 32_768.0;
/// A margin of the world viewport is at most this, in pixels (the sliders'
/// range).
pub(super) const MAX_MARGIN: u32 = 16_384;
/// At most this many monitors remembered with a layout.
pub(super) const MAX_MONITORS: usize = 16;
/// At most this many favorites (the layouts' limit, and as many again for
/// what a later version adds).
pub const MAX_FAVORITES: usize = 64;
/// Pixels per point, and a monitor's scale, are within this range.
const SCALES: std::ops::RangeInclusive<f32> = 0.25..=16.0;

/// The interface a layout's windows were saved in: its size in points and
/// how many pixels a point was.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub width: f32,
    pub height: f32,
    pub pixels_per_point: f32,
}

/// One window's place: its top-left corner from the interface's, its size
/// (the expanded one when it is minimized), in points; and whether it was
/// open.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub minimized: bool,
    /// The window was open when the layout was saved: applying the layout
    /// opens it if it is closed. Absent (a layout saved before the field
    /// existed) reads as closed: such a layout opens nothing.
    #[serde(default)]
    pub open: bool,
}

/// The *Window positions* part of a layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placements {
    pub frame: Frame,
    /// By window id (`super::windows`). Ids this version does not know are
    /// kept and left alone.
    pub windows: BTreeMap<String, Placement>,
}

/// A monitor of the setup a layout was saved on, in desktop pixels. Names
/// are never saved: place, size and scale are enough to recognise a setup.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default = "one")]
    pub scale: f32,
}

fn one() -> f32 {
    1.0
}

/// The *Viewer window* part of a layout, in desktop pixels
/// (folderstorm/plans/workspaces.md, checkpoint 4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum WindowFrame {
    /// A window with its border. `x`, `y`: its outer corner as the system
    /// gives it (absent where it does not: Wayland); `width`, `height`: its
    /// inside. Maximized, they are the size and place it goes back to.
    Normal {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y: Option<i32>,
        width: u32,
        height: u32,
        #[serde(default)]
        maximized: bool,
        /// *Lock the window size*: the window cannot be resized by hand.
        #[serde(default)]
        locked: bool,
    },
    /// Aurora's full screen, on the monitor at this place of the layout's
    /// `monitors` (or where the window is, when the layout has none).
    Fullscreen {
        #[serde(default)]
        monitor: u8,
    },
    /// No border, exactly over the monitors at these places of the
    /// layout's `monitors`. `locked`: not resizable either.
    Borderless {
        monitors: Vec<u8>,
        #[serde(default)]
        locked: bool,
    },
}

impl WindowFrame {
    /// Whether the layout asks for the window to be locked.
    pub fn locked(&self) -> bool {
        match self {
            WindowFrame::Normal { locked, .. } | WindowFrame::Borderless { locked, .. } => *locked,
            WindowFrame::Fullscreen { .. } => false,
        }
    }

    /// The same frame with its lock box as `on` (full screen has none).
    pub fn with_lock(mut self, on: bool) -> Self {
        if let WindowFrame::Normal { locked, .. } | WindowFrame::Borderless { locked, .. } = &mut self {
            *locked = on;
        }
        self
    }

    /// Whether the frame holds the window while it is in force: locked, or
    /// without a border.
    pub fn holds(&self) -> bool {
        self.locked() || matches!(self, WindowFrame::Borderless { .. })
    }

    fn ok(&self, monitors: usize) -> bool {
        let far = MAX_DISTANCE as i64;
        match self {
            WindowFrame::Normal { x, y, width, height, .. } => {
                x.zip(*y)
                    .is_none_or(|(x, y)| i64::from(x).abs() <= far && i64::from(y).abs() <= far)
                    && (1..=far).contains(&i64::from(*width))
                    && (1..=far).contains(&i64::from(*height))
            }
            // (a layout saved where the system lists no monitor: the
            // monitor the window is on)
            WindowFrame::Fullscreen { monitor } => usize::from(*monitor) < monitors.max(1),
            WindowFrame::Borderless { monitors: chosen, .. } => {
                !chosen.is_empty()
                    && chosen.iter().all(|n| usize::from(*n) < monitors)
                    && chosen.iter().collect::<BTreeSet<_>>().len() == chosen.len()
            }
        }
    }
}

/// What to do once the monitor setup has changed (section 8.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MonitorPolicy {
    /// Offer the layout saved on the new setup.
    #[default]
    Ask,
    /// Apply it without asking.
    Apply,
    Nothing,
}

impl MonitorPolicy {
    /// A word this version does not know reads as the default.
    fn read(word: &str) -> Self {
        match word {
            "apply" => Self::Apply,
            "nothing" => Self::Nothing,
            _ => Self::Ask,
        }
    }

    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// An entry of the favorites: what it names, by its kind, so that a later
/// version's kinds (workspaces) share the list and its order
/// (folderstorm/plans/workspaces.md, checkpoint 5).
#[derive(Clone, Debug, PartialEq)]
pub enum Favorite {
    /// The layout of this name.
    Layout(String),
    /// The workspace of this name (checkpoint 6).
    Workspace(String),
    /// An entry of a kind this version does not know, kept as it was read
    /// and never shown.
    Kept(serde_json::Value),
}

impl Serialize for Favorite {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Favorite::Layout(name) | Favorite::Workspace(name) => {
                use serde::ser::SerializeMap;
                let kind = if matches!(self, Favorite::Layout(_)) {
                    "layout"
                } else {
                    "workspace"
                };
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(kind, name)?;
                map.end()
            }
            Favorite::Kept(raw) => raw.serialize(serializer),
        }
    }
}

impl Favorite {
    /// Reads one entry of the file: `{ "layout": "name" }` or
    /// `{ "workspace": "name" }`, or something else to keep. None for what
    /// is not an entry at all.
    fn read(raw: serde_json::Value) -> Option<Self> {
        let entry = raw.as_object()?;
        let named = |name: &serde_json::Value| {
            name.as_str()
                .filter(|name| name_error(name).is_none())
                .map(|name| tidy(name).to_owned())
        };
        match (entry.get("layout"), entry.get("workspace")) {
            (Some(name), None) if entry.len() == 1 => named(name).map(Favorite::Layout),
            (None, Some(name)) if entry.len() == 1 => named(name).map(Favorite::Workspace),
            _ => Some(Favorite::Kept(raw)),
        }
    }

    /// The layout it names, if it names one.
    pub fn layout(&self) -> Option<&str> {
        match self {
            Favorite::Layout(name) => Some(name),
            Favorite::Workspace(_) | Favorite::Kept(_) => None,
        }
    }

    /// The workspace it names, if it names one.
    pub fn workspace(&self) -> Option<&str> {
        match self {
            Favorite::Workspace(name) => Some(name),
            Favorite::Layout(_) | Favorite::Kept(_) => None,
        }
    }
}

/// The two layouts of the toolbar's toggle; a place is None until chosen.
pub type Toggle = [Option<String>; 2];

fn toggle_unset(toggle: &Toggle) -> bool {
    toggle.iter().all(Option::is_none)
}

/// A named layout: where things go. Each part is optional; an absent part
/// is left alone when the layout is applied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub schema: u32,
    pub name: String,
    /// The viewer window: where it is, how large, whether it is locked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowFrame>,
    /// The world viewport and where the bars go.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<ViewportSettings>,
    /// Where each window is, and how large.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Placements>,
    /// The monitors it was saved on (none where the system does not tell).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub monitors: Vec<Monitor>,
    /// What this version does not know of the entry, kept as read.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

/// Why a layout cannot be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unusable {
    /// Made by a newer version of the viewer.
    Newer,
    /// Not a layout, or one that breaks a limit.
    Invalid,
}

/// An entry of the file this version does not use, kept as it was read.
#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    /// Its name if it has a usable one, for the list.
    pub name: Option<String>,
    pub why: Unusable,
    pub(super) raw: serde_json::Value,
}

/// One entry of the list.
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    Layout(Layout),
    Kept(Kept),
}

impl Serialize for Record {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Record::Layout(layout) => layout.serialize(serializer),
            Record::Kept(kept) => kept.raw.serialize(serializer),
        }
    }
}

impl Record {
    /// Reads one entry of the file. Never fails: what cannot be used is
    /// kept.
    fn read(raw: serde_json::Value) -> Self {
        let name = raw
            .get("name")
            .and_then(|name| name.as_str())
            .filter(|name| name_error(name).is_none())
            .map(str::to_owned);
        if raw
            .get("schema")
            .and_then(|schema| schema.as_u64())
            .is_some_and(|schema| schema > u64::from(SCHEMA))
        {
            return Record::Kept(Kept {
                name,
                why: Unusable::Newer,
                raw,
            });
        }
        match serde_json::from_value::<Layout>(raw.clone()) {
            Ok(layout) if layout.check().is_ok() => Record::Layout(layout),
            _ => Record::Kept(Kept {
                name,
                why: Unusable::Invalid,
                raw,
            }),
        }
    }

    pub fn layout(&self) -> Option<&Layout> {
        match self {
            Record::Layout(layout) => Some(layout),
            Record::Kept(_) => None,
        }
    }

    /// The name to show, if the entry has one.
    pub fn name(&self) -> Option<&str> {
        match self {
            Record::Layout(layout) => Some(&layout.name),
            Record::Kept(kept) => kept.name.as_deref(),
        }
    }
}

/// What is wrong with a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    /// A line break, a tab or another control character.
    Control,
    /// Another layout has it (ignoring case).
    Taken,
}

/// A name as it is stored and compared: without the spaces around it.
pub fn tidy(name: &str) -> &str {
    name.trim()
}

/// What is wrong with `name` on its own (not whether it is taken).
pub fn name_error(name: &str) -> Option<NameError> {
    let name = tidy(name);
    if name.is_empty() {
        Some(NameError::Empty)
    } else if name.chars().count() > MAX_NAME {
        Some(NameError::TooLong)
    } else if name.chars().any(char::is_control) {
        Some(NameError::Control)
    } else {
        None
    }
}

/// Two names are the same layout's: equal ignoring case and the spaces
/// around them.
pub fn same_name(a: &str, b: &str) -> bool {
    // (asked every frame while a layout holds the window: no allocation)
    tidy(a)
        .chars()
        .flat_map(char::to_lowercase)
        .eq(tidy(b).chars().flat_map(char::to_lowercase))
}

fn finite(values: &[f32]) -> bool {
    values.iter().all(|value| value.is_finite())
}

fn size_ok(width: f32, height: f32) -> bool {
    finite(&[width, height]) && width > 0.0 && height > 0.0 && width <= MAX_POINTS && height <= MAX_POINTS
}

impl Frame {
    pub(super) fn ok(&self) -> bool {
        size_ok(self.width, self.height) && self.pixels_per_point.is_finite() && SCALES.contains(&self.pixels_per_point)
    }
}

impl Placement {
    pub(super) fn ok(&self) -> bool {
        finite(&[self.x, self.y]) && self.x.abs() <= MAX_DISTANCE && self.y.abs() <= MAX_DISTANCE && size_ok(self.width, self.height)
    }
}

impl Placements {
    pub(super) fn ok(&self) -> bool {
        self.frame.ok()
            && self.windows.len() <= MAX_WINDOWS
            && self
                .windows
                .iter()
                .all(|(id, place)| !id.is_empty() && id.chars().count() <= MAX_ID && !id.chars().any(char::is_control) && place.ok())
    }
}

impl Monitor {
    pub(super) fn ok(&self) -> bool {
        let far = MAX_DISTANCE as i64;
        let (x, y, width, height) = (i64::from(self.x), i64::from(self.y), i64::from(self.width), i64::from(self.height));
        x.abs() <= far
            && y.abs() <= far
            && (1..=far).contains(&width)
            && (1..=far).contains(&height)
            && self.scale.is_finite()
            && SCALES.contains(&self.scale)
    }
}

fn world_ok(world: &ViewportSettings) -> bool {
    [world.left, world.right, world.top, world.bottom]
        .iter()
        .all(|margin| *margin <= MAX_MARGIN)
}

impl Layout {
    /// A layout of this version's schema.
    pub fn new(name: &str, world: Option<ViewportSettings>, windows: Option<Placements>, monitors: Vec<Monitor>) -> Self {
        Layout {
            schema: SCHEMA,
            name: tidy(name).to_owned(),
            window: None,
            world,
            windows,
            monitors,
            other: BTreeMap::new(),
        }
    }

    /// The same layout with a *Viewer window* part.
    pub fn with_window(mut self, window: Option<WindowFrame>) -> Self {
        self.window = window;
        self
    }

    /// Whether every value is within the limits. A layout that fails is
    /// never applied.
    pub fn check(&self) -> Result<(), Unusable> {
        if self.schema > SCHEMA {
            return Err(Unusable::Newer);
        }
        let ok = self.schema >= 1
            && name_error(&self.name).is_none()
            && self.world.as_ref().is_none_or(world_ok)
            && self.windows.as_ref().is_none_or(Placements::ok)
            && self.monitors.len() <= MAX_MONITORS
            && self.monitors.iter().all(Monitor::ok)
            && self.window.as_ref().is_none_or(|window| window.ok(self.monitors.len()));
        if ok { Ok(()) } else { Err(Unusable::Invalid) }
    }
}

/// Why the list refused a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    Name(NameError),
    /// [`MAX_LAYOUTS`] entries already.
    Full,
    /// The layout breaks a limit.
    Invalid,
    /// No usable layout there.
    Missing,
}

/// Everything about layouts that is saved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "Raw")]
pub struct Store {
    /// In the user's order.
    pub layouts: Vec<Record>,
    /// The layout last applied, saved or updated.
    pub active_layout: Option<String>,
    /// A window of the active layout was moved since (`super::apply`).
    pub modified: bool,
    /// Windows a layout placed while they were closed and that have not
    /// been shown since: their place is asked again at the next start.
    pub pending: BTreeSet<String>,
    /// The active layout's lock or borderless mode is in force
    /// (`super::viewer`): what a restart puts back.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub window_held: bool,
    /// What to do when the monitors change.
    #[serde(skip_serializing_if = "MonitorPolicy::is_default")]
    pub on_monitor_change: MonitorPolicy,
    /// The favorites, in the user's order: first in the switcher, in World
    /// › Layouts and in the favorites' round of the keyboard.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub favorites: Vec<Favorite>,
    /// The two layouts the toolbar's toggle switches between.
    #[serde(skip_serializing_if = "toggle_unset")]
    pub toggle: Toggle,
    /// The workspaces, in the user's order (`super::workspace`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<super::workspace::Record>,
    /// The workspace last applied, saved or updated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_workspace: Option<String>,
}

/// The store as the file has it, before each entry is read on its own.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Raw {
    layouts: serde_json::Value,
    active_layout: Option<String>,
    modified: bool,
    pending: BTreeSet<String>,
    window_held: bool,
    on_monitor_change: serde_json::Value,
    favorites: serde_json::Value,
    toggle: serde_json::Value,
    workspaces: serde_json::Value,
    active_workspace: Option<String>,
}

impl From<Raw> for Store {
    fn from(raw: Raw) -> Self {
        let entries = match raw.layouts {
            serde_json::Value::Array(entries) => entries,
            _ => Vec::new(),
        };
        if entries.len() > MAX_LAYOUTS {
            log::warn!("settings.json: more than {MAX_LAYOUTS} layouts, the rest is left out");
        }
        let mut layouts: Vec<Record> = Vec::new();
        for raw in entries.into_iter().take(MAX_LAYOUTS) {
            let mut record = Record::read(raw);
            // a second layout of the same name could not be told from the first
            if let Record::Layout(layout) = &record
                && layouts
                    .iter()
                    .filter_map(Record::layout)
                    .any(|other| same_name(&other.name, &layout.name))
                && let Ok(raw) = serde_json::to_value(layout)
            {
                record = Record::Kept(Kept {
                    name: Some(layout.name.clone()),
                    why: Unusable::Invalid,
                    raw,
                });
            }
            layouts.push(record);
        }
        let mut store = Store {
            layouts,
            active_layout: raw.active_layout,
            modified: raw.modified,
            pending: raw
                .pending
                .into_iter()
                .filter(|id| super::windows::find(id).is_some())
                .take(MAX_WINDOWS)
                .collect(),
            window_held: raw.window_held,
            on_monitor_change: raw.on_monitor_change.as_str().map(MonitorPolicy::read).unwrap_or_default(),
            favorites: Vec::new(),
            toggle: Toggle::default(),
            workspaces: super::workspace::read_list(raw.workspaces),
            active_workspace: raw.active_workspace,
        };
        if store.active_workspace().is_none() {
            store.active_workspace = None;
        }
        // a workspace names a layout of the list, or none
        let named: Vec<String> = store
            .workspaces
            .iter()
            .filter_map(|record| record.workspace()?.layout.clone())
            .filter(|name| !store.listed(name))
            .collect();
        for name in named {
            store.workspaces_follow_layout(&name, None);
        }
        // a favorite, a layout of the toggle: only of an entry of the list
        // (one this version cannot use keeps its star and its place)
        if let serde_json::Value::Array(entries) = raw.favorites {
            for favorite in entries.into_iter().filter_map(Favorite::read).take(MAX_FAVORITES) {
                let keep = match &favorite {
                    Favorite::Layout(name) => store.listed(name) && !store.is_favorite(name),
                    Favorite::Workspace(name) => store.workspace_listed(name) && !store.is_favorite_workspace(name),
                    Favorite::Kept(_) => true,
                };
                if keep {
                    store.favorites.push(favorite);
                }
            }
        }
        if let serde_json::Value::Array(places) = raw.toggle {
            for (place, name) in store.toggle.iter_mut().zip(places) {
                *place = name.as_str().map(|name| tidy(name).to_owned());
            }
            let listed = store.toggle.clone().map(|name| name.filter(|name| store.listed(name)));
            store.toggle = listed;
        }
        if store.active().is_none() {
            store.active_layout = None;
            store.modified = false;
            store.pending.clear();
        }
        // nothing holds the window without an active layout that asks for it
        store.window_held &= store
            .active()
            .and_then(|layout| layout.window.as_ref())
            .is_some_and(WindowFrame::holds);
        store
    }
}

impl Store {
    /// The usable layout of that name, and where it is in the list.
    pub fn find(&self, name: &str) -> Option<(usize, &Layout)> {
        self.layouts.iter().enumerate().find_map(|(index, record)| {
            record
                .layout()
                .filter(|layout| same_name(&layout.name, name))
                .map(|layout| (index, layout))
        })
    }

    /// The usable layout at `index`.
    pub fn layout(&self, index: usize) -> Option<&Layout> {
        self.layouts.get(index).and_then(Record::layout)
    }

    /// The active layout, while it is still in the list.
    pub fn active(&self) -> Option<&Layout> {
        self.active_layout
            .as_deref()
            .and_then(|name| self.find(name))
            .map(|(_, layout)| layout)
    }

    pub fn is_active(&self, index: usize) -> bool {
        self.layout(index)
            .zip(self.active_layout.as_deref())
            .is_some_and(|(layout, active)| same_name(&layout.name, active))
    }

    /// Whether an entry of the list, usable or not, has this name.
    fn listed(&self, name: &str) -> bool {
        self.layouts
            .iter()
            .any(|record| record.name().is_some_and(|other| same_name(other, name)))
    }

    /// Whether the layout of this name is a favorite.
    pub fn is_favorite(&self, name: &str) -> bool {
        self.favorites
            .iter()
            .any(|favorite| favorite.layout().is_some_and(|other| same_name(other, name)))
    }

    /// Makes the layout of this name a favorite (the last one) or takes its
    /// star away. True when the favorites changed.
    pub fn set_favorite(&mut self, name: &str, on: bool) -> bool {
        if on == self.is_favorite(name) {
            return false;
        }
        if !on {
            self.favorites
                .retain(|favorite| !favorite.layout().is_some_and(|other| same_name(other, name)));
            return true;
        }
        let Some((_, layout)) = self.find(name) else { return false };
        if self.favorites.len() >= MAX_FAVORITES {
            return false;
        }
        self.favorites.push(Favorite::Layout(layout.name.clone()));
        true
    }

    /// The favorites that can be used, in their order: where each is in
    /// the list, and the layout.
    pub fn favorite_layouts(&self) -> impl Iterator<Item = (usize, &Layout)> {
        self.favorites
            .iter()
            .filter_map(Favorite::layout)
            .filter_map(|name| self.find(name))
    }

    /// Chooses the layout at `place` (0 or 1) of the toolbar's toggle, or
    /// none. True when the choice changed.
    pub fn set_toggle(&mut self, place: usize, name: Option<&str>) -> bool {
        let name = name.and_then(|name| self.find(name)).map(|(_, layout)| layout.name.clone());
        match self.toggle.get_mut(place) {
            Some(chosen) if *chosen != name => {
                *chosen = name;
                true
            }
            _ => false,
        }
    }

    /// What is wrong with `name` for a new layout, or for the one at
    /// `renamed` (which may keep its own name, in another case).
    pub fn name_error(&self, name: &str, renamed: Option<usize>) -> Option<NameError> {
        if let Some(error) = name_error(name) {
            return Some(error);
        }
        let taken = self
            .layouts
            .iter()
            .enumerate()
            .any(|(index, record)| Some(index) != renamed && record.name().is_some_and(|other| same_name(other, name)));
        taken.then_some(NameError::Taken)
    }

    /// Adds a layout at the end and makes it the active one.
    pub fn add(&mut self, layout: Layout) -> Result<usize, StoreError> {
        if self.layouts.len() >= MAX_LAYOUTS {
            return Err(StoreError::Full);
        }
        if let Some(error) = self.name_error(&layout.name, None) {
            return Err(StoreError::Name(error));
        }
        layout.check().map_err(|_| StoreError::Invalid)?;
        self.set_active(Some(layout.name.clone()));
        self.layouts.push(Record::Layout(layout));
        Ok(self.layouts.len() - 1)
    }

    /// Puts `world`, `windows` and `window` in the layout at `index` for
    /// the parts it has (a part it does not have stays absent), with the
    /// monitors of now, and makes it the active one. Its name, its place in
    /// the list and what this version does not know of it are kept.
    pub fn update(
        &mut self,
        index: usize,
        world: ViewportSettings,
        windows: Option<Placements>,
        window: Option<WindowFrame>,
        monitors: Vec<Monitor>,
    ) -> Result<(), StoreError> {
        let old = self.layout(index).ok_or(StoreError::Missing)?;
        // a viewer window that cannot be read now keeps the saved one,
        // with the monitors it counts in
        let unread = old.window.is_some() && window.is_none();
        let new = Layout {
            schema: old.schema,
            name: old.name.clone(),
            window: old.window.as_ref().map(|saved| window.unwrap_or_else(|| saved.clone())),
            world: old.world.as_ref().map(|_| world),
            // a windows part that cannot be read now keeps the saved one
            windows: old.windows.as_ref().map(|saved| windows.unwrap_or_else(|| saved.clone())),
            monitors: if unread { old.monitors.clone() } else { monitors },
            other: old.other.clone(),
        };
        new.check().map_err(|_| StoreError::Invalid)?;
        self.set_active(Some(new.name.clone()));
        self.layouts[index] = Record::Layout(new);
        Ok(())
    }

    /// Puts another *Viewer window* in the layout at `index` (the tab's
    /// editor). The layout in use is then modified: its window is not the
    /// viewer's until it is applied.
    pub fn set_window(&mut self, index: usize, window: WindowFrame) -> Result<(), StoreError> {
        let active = self.is_active(index);
        let Some(Record::Layout(layout)) = self.layouts.get_mut(index) else {
            return Err(StoreError::Missing);
        };
        if !window.ok(layout.monitors.len()) {
            return Err(StoreError::Invalid);
        }
        if layout.window.as_ref() != Some(&window) {
            layout.window = Some(window);
            self.modified |= active;
        }
        Ok(())
    }

    /// Renames the layout at `index`; the active mark follows.
    pub fn rename(&mut self, index: usize, name: &str) -> Result<(), StoreError> {
        if self.layout(index).is_none() {
            return Err(StoreError::Missing);
        }
        if let Some(error) = self.name_error(name, Some(index)) {
            return Err(StoreError::Name(error));
        }
        let active = self.is_active(index);
        let new = tidy(name).to_owned();
        if let Some(Record::Layout(layout)) = self.layouts.get_mut(index) {
            let old = std::mem::replace(&mut layout.name, new.clone());
            if active {
                self.active_layout = Some(new.clone());
            }
            // its star and its place in the toggle follow the name
            let renamed = self
                .favorites
                .iter_mut()
                .filter_map(|favorite| match favorite {
                    Favorite::Layout(name) => Some(name),
                    Favorite::Workspace(_) | Favorite::Kept(_) => None,
                })
                .chain(self.toggle.iter_mut().flatten())
                .filter(|name| same_name(name, &old));
            for name in renamed {
                name.clone_from(&new);
            }
            // and the workspaces that name it
            self.workspaces_follow_layout(&old, Some(&new));
        }
        Ok(())
    }

    /// Removes the entry at `index`, usable or not.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.layouts.len() {
            return false;
        }
        if self.is_active(index) {
            self.set_active(None);
        }
        let removed = self.layouts.remove(index);
        // (unless another entry, one that cannot be used, has the same name)
        if let Some(name) = removed.name().filter(|name| !self.listed(name)) {
            self.favorites
                .retain(|favorite| !favorite.layout().is_some_and(|other| same_name(other, name)));
            for place in &mut self.toggle {
                place.take_if(|chosen| same_name(chosen, name));
            }
            // a workspace that named it keeps the layout in use from now on
            self.workspaces_follow_layout(name, None);
        }
        true
    }

    /// Moves the entry at `index` one place up (`up`) or down; where it is
    /// afterwards.
    pub fn shift(&mut self, index: usize, up: bool) -> Option<usize> {
        let to = if up { index.checked_sub(1)? } else { index + 1 };
        if index >= self.layouts.len() || to >= self.layouts.len() {
            return None;
        }
        self.layouts.swap(index, to);
        Some(to)
    }

    /// Marks `name` as the layout in use, as just applied: not modified,
    /// no window waiting from the one before, the viewer window not held
    /// (the caller says when it is).
    pub fn set_active(&mut self, name: Option<String>) {
        self.active_layout = name;
        self.modified = false;
        self.pending.clear();
        self.window_held = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame() -> Frame {
        Frame {
            width: 3840.0,
            height: 1080.0,
            pixels_per_point: 1.0,
        }
    }

    fn place(x: f32, y: f32, width: f32, height: f32) -> Placement {
        Placement {
            x,
            y,
            width,
            height,
            minimized: false,
            open: false,
        }
    }

    fn wide() -> Layout {
        let world = ViewportSettings {
            enabled: true,
            left: 1920,
            bars_around: true,
            ..Default::default()
        };
        let windows = Placements {
            frame: frame(),
            windows: BTreeMap::from([
                ("inventory".to_owned(), place(8.0, 48.0, 900.0, 700.0)),
                (
                    "worldmap".to_owned(),
                    Placement {
                        minimized: true,
                        ..place(920.0, 48.0, 700.0, 600.0)
                    },
                ),
            ]),
        };
        let monitors = vec![
            Monitor {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                scale: 1.0,
            },
            Monitor {
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
                scale: 1.25,
            },
        ];
        Layout::new("Wide", Some(world), Some(windows), monitors)
    }

    fn store_of(layouts: Vec<Layout>) -> Store {
        let mut store = Store::default();
        for layout in layouts {
            store.add(layout).expect("added");
        }
        store
    }

    fn read(value: serde_json::Value) -> Store {
        serde_json::from_value(value).expect("the store never fails to read")
    }

    #[test]
    fn a_store_survives_a_save_and_a_read() {
        let mut store = store_of(vec![
            wide(),
            Layout::new("Single", Some(ViewportSettings::default()), None, Vec::new()),
        ]);
        store.set_active(Some("Wide".into()));
        store.modified = true;
        store.pending.insert("inventory".into());
        let text = serde_json::to_string(&store).expect("saved");
        let back: Store = serde_json::from_str(&text).expect("read");
        assert_eq!(back, store);
        // an absent part is absent from the file, not null
        let single = &serde_json::to_value(&store).expect("saved")["layouts"][1];
        assert!(single.get("windows").is_none() && single.get("monitors").is_none(), "{single}");
    }

    #[test]
    fn settings_without_the_field_have_an_empty_store() {
        assert_eq!(read(json!({})), Store::default());
        // and odd values where the list should be do not fail the read
        for odd in [json!(null), json!("layouts"), json!(3), json!({"a": 1})] {
            assert_eq!(read(json!({ "layouts": odd })), Store::default());
        }
    }

    /// What a layout holds in the file: these fields and no other. Nothing
    /// of what a window shows (messages, conversations, Inventory folders
    /// or items, filters, selections) has a field to go in.
    #[test]
    fn a_saved_layout_holds_places_and_nothing_a_window_contains() {
        let value = serde_json::to_value(wide()).expect("saved");
        let keys = |value: &serde_json::Value| value.as_object().expect("an object").keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys(&value), ["monitors", "name", "schema", "windows", "world"]);
        assert_eq!(keys(&value["windows"]), ["frame", "windows"]);
        assert_eq!(keys(&value["windows"]["frame"]), ["height", "pixels_per_point", "width"]);
        for (id, window) in value["windows"]["windows"].as_object().expect("windows") {
            assert_eq!(keys(window), ["height", "minimized", "open", "width", "x", "y"], "{id}");
            assert!(
                window
                    .as_object()
                    .expect("a window")
                    .values()
                    .all(|v| v.is_number() || v.is_boolean())
            );
        }
        assert_eq!(
            keys(&value["world"]),
            [
                "bars_around",
                "bars_in_viewport",
                "bottom",
                "enabled",
                "in_mouselook",
                "left",
                "photo_whole_window",
                "right",
                "top"
            ]
        );
        assert_eq!(keys(&value["monitors"][0]), ["height", "scale", "width", "x", "y"]);
        // the only texts of a layout are its name and its window ids
        fn texts(value: &serde_json::Value, out: &mut Vec<String>) {
            match value {
                serde_json::Value::String(text) => out.push(text.clone()),
                serde_json::Value::Array(list) => list.iter().for_each(|v| texts(v, out)),
                serde_json::Value::Object(map) => map.values().for_each(|v| texts(v, out)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        texts(&value, &mut found);
        assert_eq!(found, ["Wide"]);
    }

    #[test]
    fn unknown_fields_of_a_layout_are_kept_and_unknown_windows_too() {
        let mut value = serde_json::to_value(wide()).expect("saved");
        value["background"] = json!({ "image": "margins.png", "dim": 0.5 });
        value["windows"]["windows"]["a_window_of_later"] = json!({ "x": 1.0, "y": 2.0, "width": 300.0, "height": 200.0 });
        let store = read(json!({ "layouts": [value.clone()], "active_layout": "Wide" }));
        let layout = store.layout(0).expect("usable");
        assert_eq!(layout.other["background"]["dim"], 0.5);
        assert!(layout.windows.as_ref().expect("windows").windows.contains_key("a_window_of_later"));
        // written back with them
        let mut expected = value;
        expected["windows"]["windows"]["a_window_of_later"]["minimized"] = json!(false);
        expected["windows"]["windows"]["a_window_of_later"]["open"] = json!(false);
        assert_eq!(serde_json::to_value(&store).expect("saved")["layouts"][0], expected);
    }

    #[test]
    fn a_newer_layout_is_kept_as_read_and_not_usable() {
        let newer = json!({ "schema": 2, "name": "From later", "world": "a new shape", "more": [1, 2, 3] });
        let store =
            read(json!({ "layouts": [newer.clone(), serde_json::to_value(wide()).expect("saved")], "active_layout": "From later" }));
        let Record::Kept(kept) = &store.layouts[0] else { panic!("kept") };
        assert_eq!((kept.why, kept.name.as_deref()), (Unusable::Newer, Some("From later")));
        assert!(store.layout(0).is_none());
        assert_eq!(store.active_layout, None, "a layout that cannot be used is not the active one");
        assert!(store.layout(1).is_some(), "the others are read");
        assert_eq!(
            serde_json::to_value(&store).expect("saved")["layouts"][0],
            newer,
            "written back unchanged"
        );
    }

    #[test]
    fn a_layout_that_breaks_a_limit_is_kept_as_read_and_not_usable() {
        let good = serde_json::to_value(wide()).expect("saved");
        let broken = |change: &dyn Fn(&mut serde_json::Value)| {
            let mut value = good.clone();
            change(&mut value);
            value
        };
        let many: serde_json::Map<String, serde_json::Value> = (0..=MAX_WINDOWS)
            .map(|n| (format!("w{n}"), json!({ "x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0 })))
            .collect();
        let cases: Vec<(&str, serde_json::Value)> = vec![
            ("no name", broken(&|v| v["name"] = json!(""))),
            ("a long name", broken(&|v| v["name"] = json!("x".repeat(MAX_NAME + 1)))),
            ("a control character", broken(&|v| v["name"] = json!("two\nlines"))),
            ("schema zero", broken(&|v| v["schema"] = json!(0))),
            ("no schema", json!({ "name": "Bare" })),
            ("not an object", json!("Wide")),
            (
                "a zero width",
                broken(&|v| v["windows"]["windows"]["inventory"]["width"] = json!(0.0)),
            ),
            (
                "a negative height",
                broken(&|v| v["windows"]["windows"]["inventory"]["height"] = json!(-5.0)),
            ),
            (
                "a huge size",
                broken(&|v| v["windows"]["windows"]["inventory"]["width"] = json!(20000.0)),
            ),
            (
                "a number too large for the type",
                broken(&|v| v["windows"]["windows"]["inventory"]["x"] = json!(1e39)),
            ),
            (
                "a far position",
                broken(&|v| v["windows"]["windows"]["inventory"]["y"] = json!(-40000.0)),
            ),
            (
                "not a number",
                broken(&|v| v["windows"]["windows"]["inventory"]["x"] = json!("left")),
            ),
            ("null where NaN was", broken(&|v| v["windows"]["frame"]["width"] = json!(null))),
            ("an empty frame", broken(&|v| v["windows"]["frame"]["height"] = json!(0.0))),
            ("an odd scale", broken(&|v| v["windows"]["frame"]["pixels_per_point"] = json!(0.0))),
            (
                "too many windows",
                broken(&|v| v["windows"]["windows"] = serde_json::Value::Object(many.clone())),
            ),
            (
                "an empty window id",
                broken(&|v| v["windows"]["windows"][""] = json!({ "x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0 })),
            ),
            ("a huge margin", broken(&|v| v["world"]["left"] = json!(20000))),
            ("a negative margin", broken(&|v| v["world"]["left"] = json!(-1))),
            ("a far monitor", broken(&|v| v["monitors"][0]["x"] = json!(-40000))),
            ("an empty monitor", broken(&|v| v["monitors"][0]["width"] = json!(0))),
            ("a monitor scale of zero", broken(&|v| v["monitors"][0]["scale"] = json!(0.0))),
            (
                "too many monitors",
                broken(&|v| v["monitors"] = json!(vec![json!({ "x": 0, "y": 0, "width": 800, "height": 600 }); MAX_MONITORS + 1])),
            ),
        ];
        for (what, value) in cases {
            let store = read(json!({ "layouts": [value.clone()] }));
            assert_eq!(store.layouts.len(), 1, "{what}");
            let Record::Kept(kept) = &store.layouts[0] else {
                panic!("{what}: still usable")
            };
            assert_eq!(kept.why, Unusable::Invalid, "{what}");
            assert_eq!(
                serde_json::to_value(&store).expect("saved")["layouts"][0],
                value,
                "{what}: written back unchanged"
            );
        }
    }

    #[test]
    fn a_second_layout_of_the_same_name_is_not_usable() {
        let mut again = wide();
        again.name = "  WIDE ".into();
        let store = read(json!({ "layouts": [wide(), again] }));
        assert!(store.layout(0).is_some());
        let Record::Kept(kept) = &store.layouts[1] else { panic!("kept") };
        assert_eq!(kept.why, Unusable::Invalid);
    }

    #[test]
    fn the_list_is_cut_at_its_limit_and_the_store_cleaned() {
        let many: Vec<_> = (0..MAX_LAYOUTS + 3)
            .map(|n| Layout::new(&format!("L{n}"), None, None, Vec::new()))
            .collect();
        let store = read(json!({
            "layouts": many,
            "active_layout": "l1",
            "modified": true,
            "pending": ["inventory", "preferences", "not a window"],
        }));
        assert_eq!(store.layouts.len(), MAX_LAYOUTS);
        assert_eq!(store.active().map(|l| l.name.as_str()), Some("L1"));
        assert!(store.modified);
        // only windows of the allowlist wait for a place
        assert_eq!(store.pending.iter().map(String::as_str).collect::<Vec<_>>(), ["inventory"]);
        // an active layout that is gone takes its marks with it
        let gone = read(json!({ "layouts": [], "active_layout": "Wide", "modified": true, "pending": ["inventory"] }));
        assert_eq!(gone, Store::default());
    }

    #[test]
    fn names_are_checked_and_unique_ignoring_case() {
        let store = store_of(vec![wide()]);
        assert_eq!(store.name_error("", None), Some(NameError::Empty));
        assert_eq!(store.name_error("   ", None), Some(NameError::Empty));
        assert_eq!(store.name_error(&"é".repeat(MAX_NAME), None), None, "characters, not bytes");
        assert_eq!(store.name_error(&"é".repeat(MAX_NAME + 1), None), Some(NameError::TooLong));
        assert_eq!(store.name_error("a\tb", None), Some(NameError::Control));
        assert_eq!(store.name_error(" wide ", None), Some(NameError::Taken));
        assert_eq!(store.name_error("WIDE", Some(0)), None, "its own name in another case");
        assert_eq!(store.name_error("Narrow", None), None);
    }

    #[test]
    fn adding_checks_the_name_the_limits_and_the_room() {
        let mut store = store_of(vec![wide()]);
        assert_eq!(store.active_layout.as_deref(), Some("Wide"));
        assert_eq!(store.add(wide()), Err(StoreError::Name(NameError::Taken)));
        let mut bad = Layout::new("Bad", None, None, Vec::new());
        bad.windows = Some(Placements {
            frame: Frame {
                width: f32::NAN,
                ..frame()
            },
            windows: BTreeMap::new(),
        });
        assert_eq!(store.add(bad), Err(StoreError::Invalid));
        assert_eq!(store.add(Layout::new("  Narrow  ", None, None, Vec::new())), Ok(1));
        assert_eq!(
            store.layout(1).map(|l| l.name.as_str()),
            Some("Narrow"),
            "stored without the spaces around"
        );
        assert_eq!(store.active_layout.as_deref(), Some("Narrow"));
        for n in store.layouts.len()..MAX_LAYOUTS {
            store.add(Layout::new(&format!("L{n}"), None, None, Vec::new())).expect("room");
        }
        assert_eq!(
            store.add(Layout::new("One too many", None, None, Vec::new())),
            Err(StoreError::Full)
        );
    }

    #[test]
    fn an_update_keeps_the_name_the_parts_and_what_is_not_known() {
        let mut layout = wide();
        layout.other.insert("window".into(), json!({ "locked": true }));
        let viewport_only = Layout::new("Viewport", Some(ViewportSettings::default()), None, Vec::new());
        let mut store = store_of(vec![layout, viewport_only]);
        let world = ViewportSettings {
            enabled: true,
            left: 100,
            ..Default::default()
        };
        let windows = Placements {
            frame: frame(),
            windows: BTreeMap::from([("people".to_owned(), place(1.0, 2.0, 300.0, 400.0))]),
        };
        store.modified = true;
        store
            .update(0, world.clone(), Some(windows.clone()), None, Vec::new())
            .expect("updated");
        let updated = store.layout(0).expect("still there");
        assert_eq!(updated.name, "Wide");
        assert_eq!(updated.world.as_ref(), Some(&world));
        assert_eq!(updated.windows.as_ref(), Some(&windows));
        assert!(updated.monitors.is_empty(), "the monitors of now");
        assert_eq!(updated.other["window"]["locked"], true);
        assert_eq!((store.active_layout.as_deref(), store.modified), (Some("Wide"), false));
        // a layout without a windows part does not gain one
        store.update(1, world.clone(), Some(windows), None, Vec::new()).expect("updated");
        let viewport = store.layout(1).expect("still there");
        assert_eq!((viewport.world.as_ref(), viewport.windows.as_ref()), (Some(&world), None));
        // windows that cannot be read now leave the saved ones
        let saved = store.layout(0).and_then(|l| l.windows.clone());
        store.update(0, world, None, None, Vec::new()).expect("updated");
        assert_eq!(store.layout(0).and_then(|l| l.windows.clone()), saved);
        assert_eq!(
            store.update(7, ViewportSettings::default(), None, None, Vec::new()),
            Err(StoreError::Missing)
        );
    }

    #[test]
    fn renaming_removing_and_moving_keep_the_active_mark_right() {
        let mut store = store_of(vec![
            wide(),
            Layout::new("Single", None, None, Vec::new()),
            Layout::new("Photos", None, None, Vec::new()),
        ]);
        store.set_active(Some("wide".into()));
        assert!(store.is_active(0));
        assert_eq!(store.rename(0, "Single"), Err(StoreError::Name(NameError::Taken)));
        store.rename(0, " Two monitors ").expect("renamed");
        assert_eq!(store.active_layout.as_deref(), Some("Two monitors"));
        assert!(store.is_active(0));
        store.rename(1, "One monitor").expect("renamed");
        assert_eq!(store.active_layout.as_deref(), Some("Two monitors"), "another layout's rename");
        // moving
        assert_eq!(store.shift(0, true), None, "already first");
        assert_eq!(store.shift(2, false), None, "already last");
        assert_eq!(store.shift(0, false), Some(1));
        assert!(store.is_active(1));
        assert_eq!(store.shift(9, true), None);
        // removing another layout, then the active one
        store.modified = true;
        store.pending.insert("inventory".into());
        assert!(store.remove(0));
        assert_eq!((store.active_layout.as_deref(), store.modified), (Some("Two monitors"), true));
        assert!(store.remove(0));
        assert_eq!(
            (store.active_layout.as_deref(), store.modified, store.pending.len()),
            (None, false, 0)
        );
        assert!(!store.remove(5));
    }

    #[test]
    fn an_unusable_entry_can_be_removed_and_blocks_its_name() {
        let newer = json!({ "schema": 9, "name": "Later" });
        let mut store = read(json!({ "layouts": [newer, wide()] }));
        assert_eq!(store.name_error("later", None), Some(NameError::Taken));
        assert_eq!(store.rename(0, "Other"), Err(StoreError::Missing));
        assert_eq!(
            store.update(0, ViewportSettings::default(), None, None, Vec::new()),
            Err(StoreError::Missing)
        );
        assert!(store.remove(0));
        assert_eq!(store.name_error("later", None), None);
    }

    fn locked_window() -> WindowFrame {
        WindowFrame::Normal {
            x: Some(-8),
            y: Some(0),
            width: 3840,
            height: 1040,
            maximized: false,
            locked: true,
        }
    }

    #[test]
    fn the_viewer_window_is_saved_in_its_three_modes() {
        let frames = [
            (
                locked_window(),
                json!({ "mode": "normal", "x": -8, "y": 0, "width": 3840, "height": 1040, "maximized": false, "locked": true }),
            ),
            (
                WindowFrame::Fullscreen { monitor: 1 },
                json!({ "mode": "fullscreen", "monitor": 1 }),
            ),
            (
                WindowFrame::Borderless {
                    monitors: vec![0, 1],
                    locked: true,
                },
                json!({ "mode": "borderless", "monitors": [0, 1], "locked": true }),
            ),
            // where the system does not tell the window's place
            (
                WindowFrame::Normal {
                    x: None,
                    y: None,
                    width: 1600,
                    height: 900,
                    maximized: true,
                    locked: false,
                },
                json!({ "mode": "normal", "width": 1600, "height": 900, "maximized": true, "locked": false }),
            ),
        ];
        for (frame, expected) in frames {
            let layout = wide().with_window(Some(frame.clone()));
            assert_eq!(layout.check(), Ok(()), "{frame:?}");
            let value = serde_json::to_value(&layout).expect("saved");
            assert_eq!(value["window"], expected);
            let back = read(json!({ "layouts": [value] }));
            assert_eq!(back.layout(0).and_then(|layout| layout.window.clone()), Some(frame));
        }
    }

    #[test]
    fn a_layout_from_before_the_viewer_window_reads_and_writes_as_it_was() {
        // what checkpoint 3 wrote: no `window`, no `window_held`, no policy
        let value = serde_json::to_value(store_of(vec![wide()])).expect("saved");
        let text = value.to_string();
        assert!(!text.contains("window_held") && !text.contains("on_monitor_change") && !text.contains("\"window\""));
        let store = read(value.clone());
        assert_eq!(store.layout(0).and_then(|layout| layout.window.clone()), None);
        assert!(!store.window_held);
        assert_eq!(store.on_monitor_change, MonitorPolicy::Ask);
        assert_eq!(serde_json::to_value(&store).expect("saved"), value);
    }

    #[test]
    fn a_viewer_window_out_of_its_limits_makes_the_layout_unusable() {
        let two = wide().monitors.len();
        assert_eq!(two, 2);
        let bad = [
            WindowFrame::Normal {
                x: Some(0),
                y: Some(0),
                width: 0,
                height: 1040,
                maximized: false,
                locked: false,
            },
            WindowFrame::Normal {
                x: Some(40_000),
                y: Some(0),
                width: 1280,
                height: 720,
                maximized: false,
                locked: false,
            },
            WindowFrame::Normal {
                x: Some(0),
                y: Some(0),
                width: 1280,
                height: 40_000,
                maximized: false,
                locked: false,
            },
            WindowFrame::Fullscreen { monitor: 2 },
            WindowFrame::Borderless {
                monitors: Vec::new(),
                locked: false,
            },
            WindowFrame::Borderless {
                monitors: vec![0, 2],
                locked: false,
            },
            WindowFrame::Borderless {
                monitors: vec![1, 1],
                locked: false,
            },
        ];
        for frame in bad {
            let layout = wide().with_window(Some(frame.clone()));
            assert_eq!(layout.check(), Err(Unusable::Invalid), "{frame:?}");
            // read from a file: kept as it is, never usable
            let value = serde_json::to_value(&layout).expect("saved");
            let store = read(json!({ "layouts": [value.clone()], "active_layout": "Wide", "window_held": true }));
            assert_eq!(store.layout(0), None);
            assert!(!store.window_held);
            assert_eq!(serde_json::to_value(&store).expect("saved")["layouts"][0], value);
        }
        // a mode of a later version: the entry is kept, not guessed at
        let mut value = serde_json::to_value(wide()).expect("saved");
        value["window"] = json!({ "mode": "tiled", "column": 2 });
        let store = read(json!({ "layouts": [value.clone()] }));
        assert!(matches!(&store.layouts[0], Record::Kept(kept) if kept.why == Unusable::Invalid));
        assert_eq!(serde_json::to_value(&store).expect("saved")["layouts"][0], value);
        // full screen without a monitor list (Wayland) is the monitor the window is on
        let nowhere = Layout::new("Full", None, None, Vec::new()).with_window(Some(WindowFrame::Fullscreen { monitor: 0 }));
        assert_eq!(nowhere.check(), Ok(()));
    }

    #[test]
    fn the_hold_and_the_monitors_choice_survive_a_restart() {
        let mut store = store_of(vec![wide().with_window(Some(locked_window()))]);
        store.window_held = true;
        store.on_monitor_change = MonitorPolicy::Apply;
        let value = serde_json::to_value(&store).expect("saved");
        assert_eq!(value["window_held"], true);
        assert_eq!(value["on_monitor_change"], "apply");
        assert_eq!(read(value.clone()), store);
        // nothing holds the window without a layout in use that asks for it
        let mut loose = value.clone();
        loose["active_layout"] = json!(null);
        assert!(!read(loose).window_held);
        let mut unlocked = value.clone();
        unlocked["layouts"][0]["window"]["locked"] = json!(false);
        assert!(!read(unlocked).window_held);
        // a word of a later version reads as the default, and so does nonsense
        for word in [json!("mirror"), json!(7), json!(null)] {
            let mut later = value.clone();
            later["on_monitor_change"] = word;
            assert_eq!(read(later).on_monitor_change, MonitorPolicy::Ask);
        }
        let mut nothing = value;
        nothing["on_monitor_change"] = json!("nothing");
        assert_eq!(read(nothing).on_monitor_change, MonitorPolicy::Nothing);
        // applying another layout lets the window go until it says otherwise
        store.set_active(Some("Wide".into()));
        assert!(!store.window_held);
    }

    #[test]
    fn update_and_the_editor_keep_the_viewer_window_within_its_part() {
        let mut store = store_of(vec![
            wide().with_window(Some(locked_window())),
            Layout::new("No window", Some(ViewportSettings::default()), None, Vec::new()),
        ]);
        let one = vec![wide().monitors[0]];
        let moved = WindowFrame::Normal {
            x: Some(100),
            y: Some(50),
            width: 1600,
            height: 900,
            maximized: false,
            locked: true,
        };
        store
            .update(0, ViewportSettings::default(), None, Some(moved.clone()), one.clone())
            .expect("updated");
        assert_eq!(store.layout(0).and_then(|l| l.window.clone()), Some(moved.clone()));
        assert_eq!(store.layout(0).map(|l| l.monitors.len()), Some(1));
        // a window that cannot be read now: the saved one stays, with its monitors
        store
            .update(0, ViewportSettings::default(), None, None, wide().monitors)
            .expect("updated");
        assert_eq!(store.layout(0).and_then(|l| l.window.clone()), Some(moved));
        assert_eq!(store.layout(0).map(|l| l.monitors.len()), Some(1));
        // a layout without the part does not gain it
        store
            .update(1, ViewportSettings::default(), None, Some(locked_window()), one)
            .expect("updated");
        assert_eq!(store.layout(1).and_then(|l| l.window.clone()), None);
        // the editor: the layout in use is then modified
        store.set_active(Some("Wide".into()));
        assert_eq!(store.set_window(0, WindowFrame::Fullscreen { monitor: 0 }), Ok(()));
        assert!(store.modified);
        assert_eq!(
            store.set_window(0, WindowFrame::Fullscreen { monitor: 1 }),
            Err(StoreError::Invalid)
        );
        assert_eq!(store.set_window(9, locked_window()), Err(StoreError::Missing));
        store.modified = false;
        assert_eq!(store.set_window(0, WindowFrame::Fullscreen { monitor: 0 }), Ok(()));
        assert!(!store.modified, "the same window is no change");
    }

    fn single() -> Layout {
        Layout::new("Single", Some(ViewportSettings::default()), None, Vec::new())
    }

    #[test]
    fn favorites_and_the_toggle_survive_a_save_and_a_read() {
        let mut store = store_of(vec![wide(), single()]);
        assert!(store.set_favorite("single", true), "by its name, whatever the case");
        assert!(store.set_favorite("Wide", true));
        assert!(!store.set_favorite("Wide", true), "already one");
        assert!(!store.set_favorite("Nowhere", true), "not a layout");
        assert!(store.set_toggle(0, Some("wide")));
        assert!(store.set_toggle(1, Some("Single")));
        assert!(!store.set_toggle(1, Some("Single")));
        assert!(!store.set_toggle(2, Some("Single")), "two places");
        let saved = serde_json::to_value(&store).expect("saved");
        assert_eq!(saved["favorites"], json!([{ "layout": "Single" }, { "layout": "Wide" }]));
        assert_eq!(saved["toggle"], json!(["Wide", "Single"]));
        let back = read(saved);
        assert_eq!(back, store);
        let order: Vec<_> = back
            .favorite_layouts()
            .map(|(index, layout)| (index, layout.name.as_str()))
            .collect();
        assert_eq!(order, [(1, "Single"), (0, "Wide")], "in the order they were starred");
        assert!(store.set_favorite("Single", false));
        assert!(!store.is_favorite("Single") && store.is_favorite("Wide"));
        assert!(store.set_toggle(0, None));
        assert_eq!(store.toggle, [None, Some("Single".to_owned())]);
    }

    #[test]
    fn a_store_without_favorites_is_written_as_before() {
        let store = store_of(vec![wide()]);
        let saved = serde_json::to_value(&store).expect("saved");
        let fields: BTreeSet<_> = saved.as_object().expect("an object").keys().map(String::as_str).collect();
        assert_eq!(fields, BTreeSet::from(["active_layout", "layouts", "modified", "pending"]));
        // and a file from before reads with neither
        let old = read(json!({ "layouts": [wide()], "active_layout": "Wide", "window_held": false }));
        assert!(old.favorites.is_empty());
        assert_eq!(old.toggle, [None, None]);
        assert_eq!(serde_json::to_value(&old).expect("saved"), saved);
    }

    #[test]
    fn favorites_of_unknown_kinds_are_kept_and_stale_ones_dropped() {
        let store = read(json!({
            "layouts": [wide(), single(), { "schema": 7, "name": "Later" }],
            "favorites": [
                { "board": "Sorting" },
                { "layout": "Wide" },
                { "layout": "wide" },
                { "layout": "Gone" },
                { "layout": "Later" },
                { "layout": "Single", "pinned": true },
                "junk",
                { "layout": 3 },
            ],
            "toggle": ["Gone", "single", "more"],
        }));
        assert_eq!(
            store.favorites,
            [
                Favorite::Kept(json!({ "board": "Sorting" })),
                Favorite::Layout("Wide".to_owned()),
                // one this version cannot use keeps its star for the version that can
                Favorite::Layout("Later".to_owned()),
                // a favorite with more than this version knows is kept whole
                Favorite::Kept(json!({ "layout": "Single", "pinned": true })),
            ]
        );
        let usable: Vec<_> = store.favorite_layouts().map(|(_, layout)| layout.name.as_str()).collect();
        assert_eq!(usable, ["Wide"]);
        assert!(!store.is_favorite("Single") && !store.is_favorite("Sorting"));
        assert_eq!(store.toggle, [None, Some("single".to_owned())]);
        let saved = serde_json::to_value(&store).expect("saved");
        assert_eq!(saved["favorites"][0], json!({ "board": "Sorting" }));
        assert_eq!(saved["favorites"][3], json!({ "layout": "Single", "pinned": true }));
        // not a list at all: nothing, and nothing lost of the rest
        let odd = read(json!({ "layouts": [wide()], "favorites": "Wide", "toggle": { "a": 1 } }));
        assert!(odd.favorites.is_empty() && odd.toggle == [None, None] && odd.layouts.len() == 1);
    }

    #[test]
    fn the_favorites_are_cut_at_their_limit() {
        let many: Vec<_> = (0..MAX_FAVORITES + 5).map(|n| json!({ "later": n })).collect();
        let mut store = read(json!({ "layouts": [wide()], "favorites": many }));
        assert_eq!(store.favorites.len(), MAX_FAVORITES);
        assert!(!store.set_favorite("Wide", true), "no room");
    }

    #[test]
    fn a_rename_and_a_delete_are_followed_by_the_star_and_the_toggle() {
        let mut store = store_of(vec![wide(), single()]);
        store.set_favorite("Wide", true);
        store.set_favorite("Single", true);
        store.set_toggle(0, Some("Wide"));
        store.set_toggle(1, Some("Single"));
        store.rename(0, "Two monitors").expect("renamed");
        assert!(store.is_favorite("Two monitors") && !store.is_favorite("Wide"));
        assert_eq!(store.toggle, [Some("Two monitors".to_owned()), Some("Single".to_owned())]);
        assert!(store.remove(0));
        assert_eq!(store.favorites, [Favorite::Layout("Single".to_owned())]);
        assert_eq!(store.toggle, [None, Some("Single".to_owned())]);
    }
}
