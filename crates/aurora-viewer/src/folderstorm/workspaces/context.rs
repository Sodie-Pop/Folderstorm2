//! A workspace's context (FEATURES.md row 1.7; folderstorm/plans/
//! workspaces.md, "1.7 in detail"): three optional parts beside its
//! windows: a saved graphics profile by name, a camera view or preset by
//! name, and HUDs to add, by the user's own Inventory item ids.
//!
//! Folderstorm (C++) (`doc/plans/workspace-context-and-snapshot.md`): every
//! group "Do not change" by default; HUDs are only added, never an
//! unrelated attachment removed, and *Previous* does not detach them; a
//! missing, locked or restricted part is skipped and said.
//!
//! What is here is pure: the record's parts and their limits, what the app
//! is asked to do ([`Ask`]) and what it says it did ([`Done`]), and which
//! HUDs are worn or skipped ([`plan_huds`]). The app does each part with
//! the viewer's own code (`app/folderstorm_workspaces/context.rs`).

use super::store::Store;
use super::workspace::id_text;
use crate::camera::{CameraPreset, CameraSettings};
use crate::graphics_presets::saved::Saved;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// At most this many HUDs in a workspace.
pub const MAX_HUDS: usize = 16;
/// The HUD attachment points (avatar_lad.xml: Center 2 to Bottom Right).
pub const HUD_POINTS: std::ops::RangeInclusive<u8> = 31..=38;

/// A graphics profile's or a camera preset's name as a workspace may hold
/// it: the rule of Aurora's graphics profiles (1 to 64 characters, one
/// line).
pub fn name_ok(name: &str) -> bool {
    crate::graphics_presets::saved::valid_name(name)
}

/// The camera a workspace chooses: one of the Controls window's views, or
/// a saved camera preset by name. Never a position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Camera {
    /// Rear, Front, Side or Shoulder, as the user saved it or as its
    /// template. Never Custom (offsets edited by hand are not a view).
    View(CameraPreset),
    /// A camera preset (`Settings::camera_shots`) by name.
    Preset(String),
}

impl Camera {
    pub fn ok(&self) -> bool {
        match self {
            Camera::View(view) => *view != CameraPreset::Custom,
            Camera::Preset(name) => name_ok(name),
        }
    }
}

/// One HUD of a workspace: the user's own Inventory item (the original,
/// never a link or an asset) and the HUD point it was worn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hud {
    #[serde(with = "id_text")]
    pub item: Uuid,
    pub point: u8,
}

/// The HUDs a workspace adds. Item ids mean something for one account
/// only: another account skips them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Huds {
    /// The account they were chosen by.
    #[serde(with = "id_text")]
    pub account: Uuid,
    pub items: Vec<Hud>,
}

impl Huds {
    /// Within the limits: at most [`MAX_HUDS`] distinct items, each on a
    /// HUD point.
    pub fn ok(&self) -> bool {
        let mut seen = BTreeSet::new();
        self.items.len() <= MAX_HUDS
            && self
                .items
                .iter()
                .all(|hud| !hud.item.is_nil() && HUD_POINTS.contains(&hud.point) && seen.insert(hud.item))
    }

    pub fn holds(&self, item: Uuid) -> bool {
        self.items.iter().any(|hud| hud.item == item)
    }
}

/// A workspace's context: each part it has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    pub graphics: Option<String>,
    pub camera: Option<Camera>,
    pub huds: Option<Huds>,
}

impl Context {
    pub fn is_empty(&self) -> bool {
        self.graphics.is_none() && self.camera.is_none() && self.huds.as_ref().is_none_or(|huds| huds.items.is_empty())
    }
}

/// The graphics as they were before a workspace changed them: the values,
/// even unsaved, and the profile selected then.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphicsBefore {
    pub values: Saved,
    pub selected: Option<String>,
}

/// The camera view as it was: the view, its offsets, the field of view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    pub preset: CameraPreset,
    pub camera_offset: [f32; 3],
    pub focus_offset: [f32; 3],
    pub offset_scale: f32,
    pub fov: f32,
}

impl CameraView {
    pub fn of(camera: &CameraSettings) -> Self {
        CameraView {
            preset: camera.preset,
            camera_offset: camera.camera_offset,
            focus_offset: camera.focus_offset,
            offset_scale: camera.offset_scale,
            fov: camera.fov,
        }
    }

    pub fn put(&self, camera: &mut CameraSettings) {
        camera.preset = self.preset;
        camera.camera_offset = self.camera_offset;
        camera.focus_offset = self.focus_offset;
        camera.offset_scale = self.offset_scale;
        camera.fov = self.fov;
    }
}

/// What *Previous arrangement* puts back of a context: the parts the
/// workspace changed. HUDs are never taken off.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Before {
    pub graphics: Option<GraphicsBefore>,
    pub camera: Option<CameraView>,
}

impl Before {
    pub fn is_empty(&self) -> bool {
        self.graphics.is_none() && self.camera.is_none()
    }
}

/// What the app is asked to do, once, in the frame after the request.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// A workspace's context.
    Apply(Context),
    /// *Previous arrangement*: the graphics and the camera as they were.
    Restore(Before),
}

/// How a part of the context went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Done,
    /// The profile or the preset is not in the list any more.
    Missing,
    /// RLVa locks the camera.
    Locked,
    /// In mouselook: the camera is left as it is.
    Mouselook,
    /// The app's own message (a camera preset's part that could not be
    /// applied).
    Failed(String),
}

/// What became of a workspace's HUDs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HudCounts {
    /// Asked to be worn (the server answers later).
    pub asked: usize,
    /// Worn already: nothing to do.
    pub worn: usize,
    /// Not in the Inventory (deleted, or not loaded yet).
    pub missing: usize,
    /// Not an object, or in the Library or the Trash.
    pub not_wearable: usize,
    /// Refused by RLVa.
    pub blocked: usize,
    /// Chosen by another account: all skipped.
    pub other_account: bool,
    /// An outfit change was already under way: none was asked.
    pub busy: bool,
    /// The outfit change was refused as a whole: Aurora's message.
    pub refused: Option<String>,
}

/// What the app says it did, for the report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Told {
    pub graphics: Option<(String, Part)>,
    pub camera: Option<(Camera, Part)>,
    pub huds: Option<HudCounts>,
    /// *Previous arrangement* put the graphics or the camera back.
    pub restored: bool,
}

/// What the app did with an [`Ask`], and what it changed from (for
/// *Previous arrangement*).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Done {
    pub told: Told,
    pub before: Before,
}

/// An item of a workspace's HUDs as the app finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Missing,
    NotWearable,
    Worn,
    Blocked,
    Wearable,
}

/// Which of `huds` are to be worn, for the account logged in (`account`):
/// none for another account; each item as `item` finds it.
pub fn plan_huds(huds: &Huds, account: Option<Uuid>, item: impl Fn(&Hud) -> Item) -> (Vec<Hud>, HudCounts) {
    let mut counts = HudCounts::default();
    if account != Some(huds.account) {
        counts.other_account = true;
        return (Vec::new(), counts);
    }
    let mut wear = Vec::new();
    for hud in &huds.items {
        match item(hud) {
            Item::Missing => counts.missing += 1,
            Item::NotWearable => counts.not_wearable += 1,
            Item::Worn => counts.worn += 1,
            Item::Blocked => counts.blocked += 1,
            Item::Wearable => wear.push(*hud),
        }
    }
    (wear, counts)
}

/// What Preferences shows of HUDs: the account logged in, the HUDs worn
/// now and the names of the items the workspaces hold, as the app has
/// them (`apply::Shell`): said each frame by `apply::frame`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudView {
    pub account: Option<Uuid>,
    /// Worn now, each with its point and its item's name.
    pub worn: Vec<(Hud, String)>,
    /// The names of the workspaces' HUD items the Inventory has.
    pub names: BTreeMap<Uuid, String>,
}

fn view_id() -> egui::Id {
    egui::Id::unique("folderstorm_workspaces_huds")
}

/// Says what the app has of HUDs (`apply::frame`, each frame).
pub fn set_view(ctx: &egui::Context, view: HudView) {
    let same = ctx
        .data(|d| d.get_temp::<std::sync::Arc<HudView>>(view_id()))
        .is_some_and(|said| *said == view);
    if !same {
        ctx.data_mut(|d| d.insert_temp(view_id(), std::sync::Arc::new(view)));
    }
}

/// What the app last said of HUDs.
pub fn view(ctx: &egui::Context) -> std::sync::Arc<HudView> {
    ctx.data(|d| d.get_temp::<std::sync::Arc<HudView>>(view_id())).unwrap_or_default()
}

impl Store {
    /// The graphics profile applied with the workspace at `index`, or none.
    /// True when it changed.
    pub fn set_workspace_graphics(&mut self, index: usize, name: Option<&str>) -> bool {
        let name = name.map(|name| name.trim().to_owned());
        if name.as_deref().is_some_and(|name| !name_ok(name)) {
            return false;
        }
        match self.workspace_mut(index) {
            Some(workspace) if workspace.graphics != name => {
                workspace.graphics = name;
                true
            }
            _ => false,
        }
    }

    /// The camera chosen with the workspace at `index`, or none. True when
    /// it changed.
    pub fn set_workspace_camera(&mut self, index: usize, camera: Option<Camera>) -> bool {
        if camera.as_ref().is_some_and(|camera| !camera.ok()) {
            return false;
        }
        match self.workspace_mut(index) {
            Some(workspace) if workspace.camera != camera => {
                workspace.camera = camera;
                true
            }
            _ => false,
        }
    }

    /// The HUDs of the workspace at `index`, or none. True when changed.
    pub fn set_workspace_huds(&mut self, index: usize, huds: Option<Huds>) -> bool {
        if huds.as_ref().is_some_and(|huds| !huds.ok()) {
            return false;
        }
        // (an empty list is no list)
        let huds = huds.filter(|huds| !huds.items.is_empty());
        match self.workspace_mut(index) {
            Some(workspace) if workspace.huds != huds => {
                workspace.huds = huds;
                true
            }
            _ => false,
        }
    }

    /// Puts `hud` in the HUDs the workspace at `index` adds for `account`,
    /// or takes it out. HUDs chosen by another account are not mixed with
    /// these: they must be forgotten first. True when changed.
    pub fn set_workspace_hud(&mut self, index: usize, account: Uuid, hud: Hud, on: bool) -> bool {
        let Some(workspace) = self.workspace(index) else {
            return false;
        };
        let mut huds = workspace.huds.clone().unwrap_or(Huds {
            account,
            items: Vec::new(),
        });
        if huds.account != account {
            return false;
        }
        match on {
            true if huds.holds(hud.item) => return false,
            true => huds.items.push(hud),
            false => huds.items.retain(|other| other.item != hud.item),
        }
        self.set_workspace_huds(index, Some(huds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folderstorm::workspaces::workspace::Workspace;
    use serde_json::json;
    use std::collections::BTreeSet;

    const ME: Uuid = Uuid::from_u128(7);

    fn hud(n: u128, point: u8) -> Hud {
        Hud {
            item: Uuid::from_u128(9000 + n),
            point,
        }
    }

    fn huds(items: &[Hud]) -> Huds {
        Huds {
            account: ME,
            items: items.to_vec(),
        }
    }

    fn store() -> Store {
        let mut store = Store::default();
        store
            .add_workspace(Workspace::new("Photos", None, BTreeSet::from(["minimap".to_owned()])))
            .expect("added");
        store
    }

    fn saved(store: &Store) -> serde_json::Value {
        serde_json::to_value(store).expect("saved")
    }

    fn read(value: serde_json::Value) -> Store {
        serde_json::from_value(value).expect("read")
    }

    #[test]
    fn the_context_survives_a_save_and_a_read_and_is_absent_until_chosen() {
        let mut store = store();
        let before = saved(&store);
        assert_eq!(
            before["workspaces"][0],
            json!({ "schema": 1, "name": "Photos", "open": ["minimap"] })
        );
        assert!(store.set_workspace_graphics(0, Some(" My photo profile ")));
        assert!(store.set_workspace_camera(0, Some(Camera::View(CameraPreset::Front))));
        assert!(store.set_workspace_huds(0, Some(huds(&[hud(1, 35), hud(2, 31)]))));
        let file = saved(&store);
        assert_eq!(file["workspaces"][0]["graphics"], "My photo profile");
        assert_eq!(file["workspaces"][0]["camera"], json!({ "view": "Front" }));
        assert_eq!(
            file["workspaces"][0]["huds"],
            json!({
                "account": "00000000-0000-0000-0000-000000000007",
                "items": [
                    { "item": "00000000-0000-0000-0000-000000002329", "point": 35 },
                    { "item": "00000000-0000-0000-0000-00000000232a", "point": 31 },
                ],
            })
        );
        assert_eq!(read(file.clone()), store);
        // a preset by name
        assert!(store.set_workspace_camera(0, Some(Camera::Preset("Portrait".to_owned()))));
        assert_eq!(saved(&store)["workspaces"][0]["camera"], json!({ "preset": "Portrait" }));
        // an update of its windows keeps it
        store
            .update_workspace(0, &BTreeSet::from(["worldmap".to_owned()]))
            .expect("updated");
        assert_eq!(store.workspace(0).expect("usable").graphics.as_deref(), Some("My photo profile"));
        assert!(store.workspace(0).expect("usable").huds.is_some());
        // all taken away: the file is as before (but for its windows)
        assert!(store.set_workspace_graphics(0, None));
        assert!(store.set_workspace_camera(0, None));
        assert!(store.set_workspace_huds(0, None));
        let mut file = saved(&store);
        file["workspaces"][0]["open"] = json!(["minimap"]);
        assert_eq!(file, before);
    }

    #[test]
    fn a_context_out_of_its_limits_is_refused_or_kept_as_read() {
        let mut store = store();
        assert!(!store.set_workspace_graphics(0, Some("  ")));
        assert!(!store.set_workspace_graphics(0, Some(&"g".repeat(65))));
        assert!(
            !store.set_workspace_camera(0, Some(Camera::View(CameraPreset::Custom))),
            "never Custom"
        );
        assert!(!store.set_workspace_camera(0, Some(Camera::Preset("two\nlines".to_owned()))));
        assert!(!store.set_workspace_huds(0, Some(huds(&[hud(1, 2)]))), "a body point");
        assert!(
            !store.set_workspace_huds(0, Some(huds(&[hud(1, 31), hud(1, 32)]))),
            "twice the same item"
        );
        let many: Vec<Hud> = (0..=MAX_HUDS as u128).map(|n| hud(n, 33)).collect();
        assert!(!store.set_workspace_huds(0, Some(huds(&many))), "at most 16");
        assert!(store.set_workspace_huds(0, Some(huds(&many[..MAX_HUDS]))));
        assert!(store.set_workspace_huds(0, Some(huds(&[]))));
        assert!(store.workspace(0).expect("usable").huds.is_none(), "an empty list is no list");
        assert!(store.set_workspace_huds(0, Some(huds(&many[..2]))));
        // from the file: kept as read, never applied
        for (field, value) in [
            ("graphics", json!("")),
            ("camera", json!({ "view": "Custom" })),
            ("camera", json!({ "zoom": 3 })),
            ("huds", json!({ "account": "not an id", "items": [] })),
            (
                "huds",
                json!({ "account": "00000000-0000-0000-0000-000000000007", "items": [{ "item": "00000000-0000-0000-0000-000000002329", "point": 39 }] }),
            ),
        ] {
            let mut file = saved(&store);
            file["workspaces"][0][field] = value.clone();
            let read = read(file.clone());
            assert!(read.workspace(0).is_none(), "{field}: {value}");
            assert_eq!(saved(&read)["workspaces"][0], file["workspaces"][0]);
        }
    }

    #[test]
    fn huds_are_edited_one_by_one_for_one_account() {
        let mut store = store();
        assert!(store.set_workspace_hud(0, ME, hud(1, 35), true));
        assert!(!store.set_workspace_hud(0, ME, hud(1, 32), true), "already there");
        assert!(store.set_workspace_hud(0, ME, hud(2, 31), true));
        assert!(
            !store.set_workspace_hud(0, Uuid::from_u128(8), hud(3, 31), true),
            "another account's list"
        );
        assert!(store.set_workspace_hud(0, ME, hud(1, 35), false));
        assert_eq!(store.workspace(0).expect("usable").huds, Some(huds(&[hud(2, 31)])));
        assert!(store.set_workspace_hud(0, ME, hud(2, 31), false));
        assert_eq!(store.workspace(0).expect("usable").huds, None, "an empty list is no list");
    }

    #[test]
    fn which_huds_are_worn_and_which_are_skipped() {
        let all = huds(&[hud(1, 31), hud(2, 32), hud(3, 33), hud(4, 34), hud(5, 35), hud(6, 36)]);
        let item = |hud: &Hud| match hud.item.as_u128() - 9000 {
            1 | 6 => Item::Wearable,
            2 => Item::Missing,
            3 => Item::NotWearable,
            4 => Item::Worn,
            _ => Item::Blocked,
        };
        let (wear, counts) = plan_huds(&all, Some(ME), item);
        assert_eq!(wear, [hud(1, 31), hud(6, 36)], "with their own points, in their order");
        assert_eq!(
            counts,
            HudCounts {
                worn: 1,
                missing: 1,
                not_wearable: 1,
                blocked: 1,
                ..Default::default()
            }
        );
        // another account, or none: all skipped
        for account in [Some(Uuid::from_u128(8)), None] {
            let (wear, counts) = plan_huds(&all, account, item);
            assert!(wear.is_empty() && counts.other_account);
        }
    }

    #[test]
    fn a_context_is_empty_without_a_part() {
        assert!(Context::default().is_empty());
        assert!(
            Context {
                huds: Some(huds(&[])),
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !Context {
                camera: Some(Camera::View(CameraPreset::Side)),
                ..Default::default()
            }
            .is_empty()
        );
        let mut camera = CameraSettings::default();
        let view = CameraView::of(&camera);
        camera.apply_view(CameraPreset::Front, &Default::default());
        camera.fov = 1.0;
        assert_ne!(CameraView::of(&camera), view);
        view.put(&mut camera);
        assert_eq!(CameraView::of(&camera), view);
    }
}
