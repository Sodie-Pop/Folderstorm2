//! Folderstorm: a workspace's context in the app (FEATURES.md row 1.7;
//! `crate::folderstorm::workspaces::context`; folderstorm/plans/
//! workspaces.md, "1.7 in detail"): the graphics profile selected as
//! Preferences › Graphics selects it, the camera view or preset applied as
//! the camera controls apply it, and the HUDs added through Aurora's own
//! outfit path (one Current Outfit update, RLVa, the attachment limit).
//! The only HUDs ever taken off, in the same outfit change, are the ones
//! the workspaces put on (`context::Ours`), at a switch or by *Previous*.

use super::*;
use crate::folderstorm::workspaces::context::{
    self, Ask, Before, Camera, CameraView, Context, Done, GraphicsBefore, Hud, HudCounts, HudSwap, Huds, Item, OnNow, Part,
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

/// The items the outfit has on now: worn, or linked in the Current Outfit
/// (a HUD stays linked while it is away, through a teleport). None until
/// the Current Outfit is known.
pub(super) fn huds_on(world: &crate::world::World) -> Option<std::collections::BTreeSet<uuid::Uuid>> {
    use crate::world::appearance::{cof, folder_links};
    let cof = cof(&world.inventory)?;
    let mut on: std::collections::BTreeSet<uuid::Uuid> = folder_links(&world.inventory, cof)
        .into_iter()
        .filter(|link| !link.folder)
        .map(|link| link.target)
        .collect();
    on.extend(world.outfit_attachment_items());
    Some(on)
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
            let (counts, swap) = self.workspace_huds(&huds, context.take_off.as_ref());
            done.told.huds = Some(counts);
            // (Previous takes them off and puts back the ones taken off)
            done.before.huds.clone_from(&swap);
            done.huds = swap;
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

    /// Adds the HUDs of `huds` that can be worn and are not, each at its
    /// own point, and takes off the HUDs of `take_off` (put on by the
    /// workspaces) that are on and that RLVa lets go: in one outfit change,
    /// as a wear and a detach from the Inventory. Never anything else. Says
    /// what it asked to wear and to take off.
    fn workspace_huds(&mut self, huds: &Huds, take_off: Option<&Huds>) -> (HudCounts, Option<HudSwap>) {
        let world = &self.world;
        let account = world.agent_id;
        let inventory = &world.inventory;
        let worn = world.outfit_attachment_points();
        let (wear, mut counts) = context::plan_huds(huds, Some(account), |hud| {
            use crate::world::inventory::actions::{in_type, library};
            match inventory.items.get(&hud.item) {
                None => Item::Missing,
                Some(item) if item.asset_type != 6 || library(inventory, item.id) || in_type(inventory, item.id, 14) => Item::NotWearable,
                Some(_) if worn.contains_key(&hud.item) => Item::Worn,
                Some(_) if !world.rlva_can_add(hud.item, hud.point, uuid::Uuid::nil()) => Item::Blocked,
                Some(_) => Item::Wearable,
            }
        });
        // the HUDs put on by the workspaces that are still on: taken off
        // unless RLVa keeps them (@detach, @remattach, a locked point)
        let on = huds_on(world).unwrap_or_default();
        let (remove, stuck) = context::plan_take_off(take_off, account, |hud| {
            let object = inventory.items.get(&hud.item).is_some_and(|item| item.asset_type == 6);
            if !object || !(worn.contains_key(&hud.item) || on.contains(&hud.item)) {
                OnNow::Gone
            } else if !world.rlva_can_remove(hud.item, uuid::Uuid::nil()) {
                OnNow::Stuck
            } else {
                OnNow::At(worn.get(&hud.item).copied().unwrap_or(hud.point))
            }
        });
        counts.stuck = stuck;
        if wear.is_empty() && remove.is_empty() {
            return (counts, None);
        }
        if self.appearance_ui.pending.is_some() {
            counts.busy = true;
            return (counts, None);
        }
        // as a wear from the Inventory: "add", each at its own point; and
        // as RLVa's forced detach: the Current Outfit's link taken out
        self.appearance_ui.message.clear();
        self.apply_appearance_action(Action::Force {
            remove: remove.iter().map(|hud| hud.item).collect(),
            wear: wear
                .iter()
                .map(|hud| WearRequest {
                    item: hud.item,
                    replace: false,
                    point: hud.point,
                })
                .collect(),
        });
        if self.appearance_ui.pending.is_none() {
            let why = std::mem::take(&mut self.appearance_ui.message);
            counts.refused = Some(match why.is_empty() {
                true => aurora_i18n::tr!("fs-context-huds-refused-plain").to_owned(),
                false => why,
            });
            return (counts, None);
        }
        counts.asked = wear.len();
        counts.taken_off = remove.len();
        let swap = HudSwap {
            account,
            added: wear,
            removed: remove,
        };
        (counts, Some(swap))
    }

    /// *Previous arrangement*: the graphics, the camera view and the HUDs
    /// as they were before the workspace: the HUDs the switch took off are
    /// worn again, the ones it put on taken off (the workspaces still look
    /// after each one handed here).
    fn workspace_context_restore(&mut self, before: Before) -> Done {
        let mut done = Done::default();
        done.told.restored = before.graphics.is_some() || before.camera.is_some();
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
        if let Some(swap) = before.huds {
            let wear = Huds {
                account: swap.account,
                items: swap.removed,
            };
            let take_off = Huds {
                account: swap.account,
                items: swap.added,
            };
            let (counts, swap) = self.workspace_huds(&wear, Some(&take_off));
            done.told.huds = Some(counts);
            // (pressing it again comes back)
            done.before.huds.clone_from(&swap);
            done.huds = swap;
        }
        done
    }

    /// `AURORA_DEMO_WORKSPACES=context`: the demo's graphics profiles ("My
    /// fast profile" selected), the workspace "Photos": the world map,
    /// "My photo profile", the Front view, and as HUDs the first two
    /// objects of the demo's Inventory (outside the Library), for the
    /// demo's account; and "Studio": Inventory, the third object as its
    /// HUD (switching to it takes off what "Photos" put on). Preferences
    /// opens on the tab, "Photos" selected.
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
        let account = self.world.agent_id;
        photos.huds = Some(Huds {
            account,
            items: objects
                .iter()
                .zip([35, 31])
                .map(|(item, point)| Hud { item: *item, point })
                .collect(),
        })
        .filter(|huds| !huds.items.is_empty());
        let mut studio = Workspace::new("Studio", None, ["inventory".to_owned()]);
        studio.huds = objects.get(2).map(|item| Huds {
            account,
            items: vec![Hud { item: *item, point: 33 }],
        });
        let store = &mut self.settings.folderstorm_workspaces;
        let name = photos.name.clone();
        for workspace in [photos, studio] {
            let name = workspace.name.clone();
            match store.add_workspace(workspace) {
                Ok(_) => {
                    store.set_favorite_workspace(&name, true);
                }
                Err(error) => log::warn!("AURORA_DEMO_WORKSPACES: workspace not seeded: {error:?}"),
            }
        }
        store.active_workspace = Some(name);
        self.panels.settings = true;
        self.options_ui
            .open(ui::options::Page::Workspaces, ui::options::workspaces_tab::SAVED);
    }
}
