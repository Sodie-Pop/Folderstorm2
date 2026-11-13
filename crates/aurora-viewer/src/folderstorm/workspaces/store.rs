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
/// (the expanded one when it is minimized), in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub minimized: bool,
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

/// A named layout: where things go. Each part is optional; an absent part
/// is left alone when the layout is applied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub schema: u32,
    pub name: String,
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
    raw: serde_json::Value,
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
    tidy(a).to_lowercase() == tidy(b).to_lowercase()
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
            world,
            windows,
            monitors,
            other: BTreeMap::new(),
        }
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
            && self.monitors.iter().all(Monitor::ok);
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
}

/// The store as the file has it, before each entry is read on its own.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Raw {
    layouts: serde_json::Value,
    active_layout: Option<String>,
    modified: bool,
    pending: BTreeSet<String>,
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
        };
        if store.active().is_none() {
            store.active_layout = None;
            store.modified = false;
            store.pending.clear();
        }
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

    /// Puts `world` and `windows` in the layout at `index` for the parts it
    /// has (a part it does not have stays absent), with the monitors of now,
    /// and makes it the active one. Its name, its place in the list and
    /// what this version does not know of it are kept.
    pub fn update(
        &mut self,
        index: usize,
        world: ViewportSettings,
        windows: Option<Placements>,
        monitors: Vec<Monitor>,
    ) -> Result<(), StoreError> {
        let old = self.layout(index).ok_or(StoreError::Missing)?;
        let new = Layout {
            schema: old.schema,
            name: old.name.clone(),
            world: old.world.as_ref().map(|_| world),
            // a windows part that cannot be read now keeps the saved one
            windows: old.windows.as_ref().map(|saved| windows.unwrap_or_else(|| saved.clone())),
            monitors,
            other: old.other.clone(),
        };
        new.check().map_err(|_| StoreError::Invalid)?;
        self.set_active(Some(new.name.clone()));
        self.layouts[index] = Record::Layout(new);
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
        if let Some(Record::Layout(layout)) = self.layouts.get_mut(index) {
            layout.name = tidy(name).to_owned();
            if active {
                self.active_layout = Some(layout.name.clone());
            }
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
        self.layouts.remove(index);
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
    /// no window waiting from the one before.
    pub fn set_active(&mut self, name: Option<String>) {
        self.active_layout = name;
        self.modified = false;
        self.pending.clear();
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
            assert_eq!(keys(window), ["height", "minimized", "width", "x", "y"], "{id}");
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
        value["window"] = json!({ "mode": "borderless", "monitors": [0, 1] });
        value["windows"]["windows"]["a_window_of_later"] = json!({ "x": 1.0, "y": 2.0, "width": 300.0, "height": 200.0 });
        let store = read(json!({ "layouts": [value.clone()], "active_layout": "Wide" }));
        let layout = store.layout(0).expect("usable");
        assert_eq!(layout.other["window"]["mode"], "borderless");
        assert!(layout.windows.as_ref().expect("windows").windows.contains_key("a_window_of_later"));
        // written back with them
        let mut expected = value;
        expected["windows"]["windows"]["a_window_of_later"]["minimized"] = json!(false);
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
        store.update(0, world.clone(), Some(windows.clone()), Vec::new()).expect("updated");
        let updated = store.layout(0).expect("still there");
        assert_eq!(updated.name, "Wide");
        assert_eq!(updated.world.as_ref(), Some(&world));
        assert_eq!(updated.windows.as_ref(), Some(&windows));
        assert!(updated.monitors.is_empty(), "the monitors of now");
        assert_eq!(updated.other["window"]["locked"], true);
        assert_eq!((store.active_layout.as_deref(), store.modified), (Some("Wide"), false));
        // a layout without a windows part does not gain one
        store.update(1, world.clone(), Some(windows), Vec::new()).expect("updated");
        let viewport = store.layout(1).expect("still there");
        assert_eq!((viewport.world.as_ref(), viewport.windows.as_ref()), (Some(&world), None));
        // windows that cannot be read now leave the saved ones
        let saved = store.layout(0).and_then(|l| l.windows.clone());
        store.update(0, world, None, Vec::new()).expect("updated");
        assert_eq!(store.layout(0).and_then(|l| l.windows.clone()), saved);
        assert_eq!(
            store.update(7, ViewportSettings::default(), None, Vec::new()),
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
            store.update(0, ViewportSettings::default(), None, Vec::new()),
            Err(StoreError::Missing)
        );
        assert!(store.remove(0));
        assert_eq!(store.name_error("later", None), None);
    }
}
