//! A workspace's context (FEATURES.md row 1.7; folderstorm/plans/
//! workspaces.md, "1.7 in detail" and "HUDs taken off at a switch"): three
//! optional parts beside its windows: a saved graphics profile by name, a
//! camera view or preset by name, and HUDs to wear, by the user's own
//! Inventory item ids.
//!
//! Folderstorm (C++) (`doc/plans/workspace-context-and-snapshot.md`): every
//! group "Do not change" by default; HUDs are only added, never an
//! unrelated attachment removed, and *Previous* does not detach them; a
//! missing, locked or restricted part is skipped and said. Here a switch
//! to another workspace with HUDs also takes off the HUDs the workspaces
//! put on ([`Ours`]) that it does not wear, and never another one;
//! *Previous* undoes that.
//!
//! What is here is pure: the record's parts and their limits, what the app
//! is asked to do ([`Ask`]) and what it says it did ([`Done`]), which HUDs
//! are worn or skipped ([`plan_huds`]), and the HUDs the workspaces put on
//! ([`Ours`]). The app does each part with the viewer's own code
//! (`app/folderstorm_workspaces/context.rs`).

use super::store::Store;
use super::workspace::id_text;
use crate::camera::{CameraPreset, CameraSettings};
use crate::graphics_presets::saved::Saved;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// At most this many HUDs in a workspace.
pub const MAX_HUDS: usize = 16;
/// Seconds a HUD asked to be worn has to arrive before it is no longer
/// looked after as one the workspaces put on ([`Ours::observe`]).
pub const ARRIVES_WITHIN: f64 = 120.0;
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
    /// The HUDs the workspaces put on that this one does not wear: taken
    /// off with its own put on ([`Ours::take_off_for`]). Not a part of the
    /// record: worked out when it is applied.
    pub take_off: Option<Huds>,
}

impl Context {
    pub fn is_empty(&self) -> bool {
        let none = |huds: &Option<Huds>| huds.as_ref().is_none_or(|huds| huds.items.is_empty());
        self.graphics.is_none() && self.camera.is_none() && none(&self.huds) && none(&self.take_off)
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

/// The HUDs an apply asked to wear and to take off, for one account.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HudSwap {
    pub account: Uuid,
    /// Asked to be worn: none of them was worn before.
    pub added: Vec<Hud>,
    /// Asked to be taken off: HUDs the workspaces had put on, each with
    /// the point it was on.
    pub removed: Vec<Hud>,
}

impl HudSwap {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// What *Previous arrangement* puts back of a context: the parts the
/// workspace changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Before {
    pub graphics: Option<GraphicsBefore>,
    pub camera: Option<CameraView>,
    /// The HUDs the switch put on (taken off again, while the workspaces
    /// still look after them) and took off (worn again).
    pub huds: Option<HudSwap>,
}

impl Before {
    pub fn is_empty(&self) -> bool {
        self.graphics.is_none() && self.camera.is_none() && self.huds.as_ref().is_none_or(HudSwap::is_empty)
    }
}

/// What the app is asked to do, once, in the frame after the request.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// A workspace's context.
    Apply(Context),
    /// *Previous arrangement*: the graphics, the camera and the HUDs as
    /// they were.
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
    /// HUDs the workspaces put on, asked to be taken off.
    pub taken_off: usize,
    /// HUDs the workspaces put on that RLVa keeps on (`@detach`,
    /// `@remattach`, a locked point or folder): left on.
    pub stuck: usize,
    /// Chosen by another account: all skipped.
    pub other_account: bool,
    /// An outfit change was already under way: none was asked, none
    /// taken off.
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
    /// The HUDs it asked to wear and to take off: what the workspaces
    /// look after from now on ([`Ours::changed`]).
    pub huds: Option<HudSwap>,
}

/// A HUD the workspaces put on, looked after until it comes off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OurHud {
    pub hud: Hud,
    /// Seen worn since it was asked.
    pub arrived: bool,
    /// When it was asked, in egui's seconds.
    pub asked_at: f64,
}

/// The HUDs the workspaces put on for the account logged in: the ones a
/// workspace asked to wear that were not worn then, and that arrived. Only
/// these are ever taken off by a switch: a HUD worn before, or put on by
/// hand, never. For the session and one login only (`apply`'s session,
/// forgotten at a logout or with another account).
#[derive(Clone, Debug, PartialEq)]
pub struct Ours {
    pub account: Uuid,
    pub huds: Vec<OurHud>,
}

impl Ours {
    pub fn holds(&self, item: Uuid) -> bool {
        self.huds.iter().any(|our| our.hud.item == item)
    }

    /// What a switch to a workspace wearing `next` takes off: the HUDs put
    /// on for the same account that `next` does not wear. None when there
    /// is none, or `next` is another account's.
    pub fn take_off_for(&self, next: &Huds) -> Option<Huds> {
        let items: Vec<Hud> = self.huds.iter().map(|our| our.hud).filter(|hud| !next.holds(hud.item)).collect();
        (next.account == self.account && !items.is_empty()).then_some(Huds {
            account: self.account,
            items,
        })
    }

    /// The HUDs an apply asked to wear and to take off (`swap`), at `now`.
    /// Another account's swap replaces what is looked after.
    pub fn changed(ours: &mut Option<Ours>, swap: &HudSwap, now: f64) {
        let mine = ours.take().filter(|ours| ours.account == swap.account);
        let mut next = mine.unwrap_or(Ours {
            account: swap.account,
            huds: Vec::new(),
        });
        next.huds.retain(|our| !swap.removed.iter().any(|hud| hud.item == our.hud.item));
        for hud in &swap.added {
            next.huds.retain(|our| our.hud.item != hud.item);
            next.huds.push(OurHud {
                hud: *hud,
                arrived: false,
                asked_at: now,
            });
        }
        *ours = Some(next).filter(|next| !next.huds.is_empty());
    }

    /// What the outfit has on now (`on`: the items worn or linked in the
    /// Current Outfit), at `now`: a HUD seen there has arrived; one that
    /// had arrived and is not there any more was taken off (by hand, by a
    /// script or another outfit) and is forgotten; one that did not arrive
    /// within [`ARRIVES_WITHIN`] is forgotten. False when nothing is left.
    pub fn observe(&mut self, on: &BTreeSet<Uuid>, now: f64) -> bool {
        self.huds.retain_mut(|our| {
            if on.contains(&our.hud.item) {
                our.arrived = true;
                true
            } else {
                !our.arrived && now - our.asked_at <= ARRIVES_WITHIN
            }
        });
        !self.huds.is_empty()
    }
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

/// A HUD the workspaces put on, as the app finds it when it is to be
/// taken off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnNow {
    /// Not on any more (or not an object of the Inventory): nothing to do.
    Gone,
    /// On, and RLVa keeps it on.
    Stuck,
    /// On, at this point: taken off.
    At(u8),
}

/// Which of `take_off` are taken off, for the account logged in
/// (`account`): none of another account's; each as `hud` finds it. Gives
/// them with the point they are on, and how many RLVa keeps on.
pub fn plan_take_off(take_off: Option<&Huds>, account: Uuid, hud: impl Fn(&Hud) -> OnNow) -> (Vec<Hud>, usize) {
    let mut remove = Vec::new();
    let mut stuck = 0;
    for one in take_off
        .filter(|take_off| take_off.account == account)
        .iter()
        .flat_map(|take_off| &take_off.items)
    {
        match hud(one) {
            OnNow::Gone => {}
            OnNow::Stuck => stuck += 1,
            OnNow::At(point) => remove.push(Hud {
                item: one.item,
                point: Some(point).filter(|point| HUD_POINTS.contains(point)).unwrap_or(one.point),
            }),
        }
    }
    (remove, stuck)
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

    fn swap(added: &[Hud], removed: &[Hud]) -> HudSwap {
        HudSwap {
            account: ME,
            added: added.to_vec(),
            removed: removed.to_vec(),
        }
    }

    fn items(ours: &Option<Ours>) -> Vec<Uuid> {
        ours.iter().flat_map(|ours| &ours.huds).map(|our| our.hud.item).collect()
    }

    #[test]
    fn the_huds_put_on_are_remembered_and_only_those_are_taken_off() {
        let mut ours = None;
        // the first workspace put on 1 and 2 (3 was worn before: not asked)
        Ours::changed(&mut ours, &swap(&[hud(1, 31), hud(2, 32)], &[]), 10.0);
        assert_eq!(items(&ours), [hud(1, 31).item, hud(2, 32).item]);
        let held = ours.clone().expect("remembered");
        // the next one wears 2 and 3: only 1 comes off (3 was never ours)
        let next = huds(&[hud(2, 32), hud(3, 33)]);
        assert_eq!(held.take_off_for(&next), Some(huds(&[hud(1, 31)])));
        // one that wears both: nothing
        assert_eq!(held.take_off_for(&huds(&[hud(1, 35), hud(2, 32)])), None);
        // another account's list: nothing
        let theirs = Huds {
            account: Uuid::from_u128(8),
            items: vec![hud(3, 33)],
        };
        assert_eq!(held.take_off_for(&theirs), None);
        // the switch took 1 off and put 4 on; 2 stays ours
        Ours::changed(&mut ours, &swap(&[hud(4, 34)], &[hud(1, 31)]), 20.0);
        assert_eq!(items(&ours), [hud(2, 32).item, hud(4, 34).item]);
        // another account's swap replaces them
        let mut other = swap(&[hud(5, 35)], &[]);
        other.account = Uuid::from_u128(8);
        Ours::changed(&mut ours, &other, 30.0);
        assert_eq!(items(&ours), [hud(5, 35).item]);
        // all taken off: nothing left
        Ours::changed(
            &mut ours,
            &HudSwap {
                account: Uuid::from_u128(8),
                added: vec![],
                removed: vec![hud(5, 35)],
            },
            40.0,
        );
        assert_eq!(ours, None);
    }

    #[test]
    fn a_hud_put_on_is_looked_after_until_it_comes_off() {
        let mut ours = None;
        Ours::changed(&mut ours, &swap(&[hud(1, 31), hud(2, 32)], &[]), 0.0);
        let mut ours = ours.expect("remembered");
        // 1 arrives; 2 not yet
        assert!(ours.observe(&BTreeSet::from([hud(1, 31).item]), 1.0));
        assert_eq!(ours.huds.iter().map(|our| our.arrived).collect::<Vec<_>>(), [true, false]);
        // a teleport: still linked in the Current Outfit, so still on
        assert!(ours.observe(&BTreeSet::from([hud(1, 31).item]), 2.0));
        // 2 did not arrive in time: forgotten
        assert!(ours.observe(&BTreeSet::from([hud(1, 31).item]), ARRIVES_WITHIN + 1.0));
        assert!(!ours.holds(hud(2, 32).item));
        // 1 taken off by hand: forgotten, never taken off by a switch
        // even if it is worn again by hand
        assert!(!ours.observe(&BTreeSet::new(), ARRIVES_WITHIN + 2.0));
        assert!(ours.huds.is_empty());
    }

    #[test]
    fn a_hud_rlva_keeps_on_is_counted_and_stays_ours() {
        let take_off = huds(&[hud(1, 31), hud(2, 32), hud(3, 33)]);
        let (remove, stuck) = plan_take_off(Some(&take_off), ME, |one| match one.item.as_u128() - 9000 {
            1 => OnNow::At(36),
            2 => OnNow::Stuck,
            _ => OnNow::Gone,
        });
        assert_eq!(remove, [hud(1, 36)], "at the point it is on now");
        assert_eq!(stuck, 1);
        // another account's, or none: nothing
        assert_eq!(plan_take_off(Some(&take_off), Uuid::from_u128(8), |_| OnNow::At(31)), (vec![], 0));
        assert_eq!(plan_take_off(None, ME, |_| OnNow::At(31)), (vec![], 0));
        // what was taken off leaves; the one RLVa kept stays ours
        let mut ours = None;
        Ours::changed(&mut ours, &swap(&[hud(1, 31), hud(2, 32)], &[]), 0.0);
        Ours::changed(&mut ours, &swap(&[], &remove), 1.0);
        assert_eq!(items(&ours), [hud(2, 32).item]);
    }
}
