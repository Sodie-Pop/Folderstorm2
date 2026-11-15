//! Folderstorm: a workspace's context in the app (FEATURES.md row 1.7;
//! `crate::folderstorm::workspaces::context`; folderstorm/plans/
//! workspaces.md, "1.7 in detail"): the graphics profile selected as
//! Preferences › Graphics selects it, the camera view or preset applied as
//! the camera controls apply it, and the HUDs added through Aurora's own
//! outfit path (one Current Outfit update, RLVa, the attachment limit).
//! Nothing is ever taken off.

use super::*;
use crate::folderstorm::workspaces::context::{
    self, Ask, Before, Camera, CameraView, Context, Done, GraphicsBefore, Hud, HudCounts, Huds, Item, Part,
};
use crate::graphics_presets::saved::{self, Saved};
use crate::world::appearance::{Action, WearRequest};

/// The HUDs worn now, each with its point, in a steady order.
pub(super) fn worn_huds(world: &crate::world::World) -> Vec<Hud> {
    let mut huds: Vec<Hud> = world
        .outfit_attachment_points()
        .into_iter()
        .filter(|(_, point)| context::HUD_POINTS.contains(point))
        .map(|(item, point)| Hud { item, point })
        .collect();
    huds.sort_by_key(|hud| (hud.point, hud.item));
    huds
}

impl App {
    /// Does what the workspaces asked of a context (`apply::Outcome::
    /// context`) and tells them what was done (`apply::context_done`).
    pub(super) fn folderstorm_workspace_context(&mut self, ask: Ask) {
        let done = match ask {
            Ask::Apply(context) => self.workspace_context_apply(context),
            Ask::Restore(before) => self.workspace_context_restore(before),
        };
        log::info!("workspaces: context: {:?}", done.told);
        self.settings.save();
        apply::context_done(&self.egui_ctx, done);
    }

    fn graphics_now(&self) -> GraphicsBefore {
        GraphicsBefore {
            values: Saved::capture(&self.settings),
            selected: self.settings.graphics_saved_active.clone(),
        }
    }

    /// Why the camera is not to be moved now: RLVa locks it, or the
    /// camera is in mouselook (never forced out of it).
    fn camera_held(&self) -> Option<Part> {
        if self.world.rlva.camera_preset_locked(uuid::Uuid::nil()) {
            Some(Part::Locked)
        } else if self.camera.mouselook() {
            Some(Part::Mouselook)
        } else {
            None
        }
    }

    fn workspace_context_apply(&mut self, context: Context) -> Done {
        let mut done = Done::default();
        // graphics first, then the camera (the C++ plan's order)
        if let Some(name) = context.graphics {
            let before = self.graphics_now();
            let part = if saved::select(&mut self.settings, &name) {
                // as the system island's menu does
                self.snapshots.settings_changed = true;
                done.before.graphics = Some(before);
                Part::Done
            } else {
                Part::Missing
            };
            done.told.graphics = Some((name, part));
        }
        if let Some(camera) = context.camera {
            let before = CameraView::of(&self.settings.camera);
            let part = self.camera_held().unwrap_or_else(|| self.workspace_camera(&camera));
            if matches!(part, Part::Done | Part::Failed(_)) {
                done.before.camera = Some(before);
            }
            done.told.camera = Some((camera, part));
        }
        if let Some(huds) = context.huds {
            done.told.huds = Some(self.workspace_huds(&huds));
        }
        done
    }

    /// A view as the camera controls switch to it; a preset as they apply
    /// it (RLVa and mouselook already looked at).
    fn workspace_camera(&mut self, camera: &Camera) -> Part {
        match camera {
            Camera::View(view) => {
                self.switch_camera_view(*view);
                Part::Done
            }
            Camera::Preset(name) => {
                let found = self.settings.camera_shots.iter().position(|shot| shot.name.trim() == name.trim());
                match found.map(|index| self.apply_camera_preset(index)) {
                    None => Part::Missing,
                    Some(Ok(())) => Part::Done,
                    Some(Err(why)) => Part::Failed(why),
                }
            }
        }
    }

    /// Adds the workspace's HUDs that can be worn and are not, in one
    /// outfit change, each at its own point; never takes anything off.
    fn workspace_huds(&mut self, huds: &Huds) -> HudCounts {
        let world = &self.world;
        let inventory = &world.inventory;
        let worn = world.outfit_attachment_points();
        let (wear, mut counts) = context::plan_huds(huds, Some(world.agent_id), |hud| {
            use crate::world::inventory::actions::{in_type, library};
            match inventory.items.get(&hud.item) {
                None => Item::Missing,
                Some(item) if item.asset_type != 6 || library(inventory, item.id) || in_type(inventory, item.id, 14) => Item::NotWearable,
                Some(_) if worn.contains_key(&hud.item) => Item::Worn,
                Some(_) if !world.rlva_can_add(hud.item, hud.point, uuid::Uuid::nil()) => Item::Blocked,
                Some(_) => Item::Wearable,
            }
        });
        if wear.is_empty() {
            return counts;
        }
        if self.appearance_ui.pending.is_some() {
            counts.busy = true;
            return counts;
        }
        // as a wear from the Inventory: "add", each at its own point
        self.appearance_ui.message.clear();
        self.apply_appearance_action(Action::Force {
            remove: Vec::new(),
            wear: wear
                .iter()
                .map(|hud| WearRequest {
                    item: hud.item,
                    replace: false,
                    point: hud.point,
                })
                .collect(),
        });
        if self.appearance_ui.pending.is_some() {
            counts.asked = wear.len();
        } else {
            let why = std::mem::take(&mut self.appearance_ui.message);
            counts.refused = Some(match why.is_empty() {
                true => aurora_i18n::tr!("fs-context-huds-refused-plain").to_owned(),
                false => why,
            });
        }
        counts
    }

    /// *Previous arrangement*: the graphics and the camera view as they
    /// were before the workspace (HUDs stay on).
    fn workspace_context_restore(&mut self, before: Before) -> Done {
        let mut done = Done::default();
        done.told.restored = true;
        if let Some(graphics) = before.graphics {
            done.before.graphics = Some(self.graphics_now());
            graphics.values.apply(&mut self.settings);
            self.settings.graphics_saved_active = graphics.selected.filter(|name| self.settings.graphics_saved.contains_key(name));
            self.snapshots.settings_changed = true;
        }
        if let Some(view) = before.camera {
            match self.camera_held() {
                Some(part) => done.told.camera = Some((Camera::View(view.preset), part)),
                None => {
                    done.before.camera = Some(CameraView::of(&self.settings.camera));
                    view.put(&mut self.settings.camera);
                    self.camera.reset_to_defaults(&mut self.world.agent, &self.settings.camera);
                }
            }
        }
        done
    }

    /// `AURORA_DEMO_WORKSPACES=context`: the demo's graphics profiles ("My
    /// fast profile" selected), and the workspace "Photos": the world map,
    /// "My photo profile", the Front view, and as HUDs the first two
    /// objects of the demo's Inventory (outside the Library), for the
    /// demo's account. Preferences opens on the tab, the workspace
    /// selected.
    pub(super) fn workspace_context_demo_seed(&mut self) {
        use crate::folderstorm::workspaces::workspace::Workspace;
        saved::demo(&mut self.settings);
        saved::select(&mut self.settings, "My fast profile");
        let inventory = &self.world.inventory;
        let mut objects: Vec<uuid::Uuid> = inventory
            .items
            .values()
            .filter(|item| item.asset_type == 6 && !crate::world::inventory::actions::library(inventory, item.id))
            .map(|item| item.id)
            .collect();
        objects.sort();
        let mut photos = Workspace::new("Photos", None, ["worldmap".to_owned()]);
        photos.graphics = Some("My photo profile".to_owned());
        photos.camera = Some(Camera::View(crate::camera::CameraPreset::Front));
        photos.huds = Some(Huds {
            account: self.world.agent_id,
            items: objects.into_iter().zip([35, 31]).map(|(item, point)| Hud { item, point }).collect(),
        })
        .filter(|huds| !huds.items.is_empty());
        let store = &mut self.settings.folderstorm_workspaces;
        let name = photos.name.clone();
        match store.add_workspace(photos) {
            Ok(_) => {
                store.set_favorite_workspace(&name, true);
            }
            Err(error) => log::warn!("AURORA_DEMO_WORKSPACES: workspace not seeded: {error:?}"),
        }
        store.active_workspace = Some(name);
        self.panels.settings = true;
        self.options_ui.open(ui::options::Page::Layout, 3);
    }
}
