//! Folderstorm: the local assistant's changes run here, through the paths
//! the inventory window and Appearance use for the same click
//! (folderstorm/plans/mcp.md, "Phase 2"). The calls themselves wait in
//! `mcp_bridge`; this file only reaches the app's state.
//!
//! Phase 3: the camera moves as `pstcampos` and Escape move it, a picture
//! is a photo job of its own (`Purpose::Assistant`), and a thumbnail is the
//! Outfit Gallery's Take snapshot (`outfit_snapshot`).

use super::*;
use crate::mcp_bridge::camera::{self, Move, Pose, Skeleton, Stage};
use crate::mcp_bridge::studio::PhotoStep;
use crate::mcp_bridge::{self, View, changes::Work};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use uuid::Uuid;

impl App {
    fn local_assistant_view(&self) -> View<'_> {
        View {
            world: &self.world,
            demo: self.demo,
            protected: &self.settings.inventory.protected,
            // as RLVa reads it (app/rlva.rs): the outfit write, then its
            // detaches
            appearance_busy: self.appearance_ui.pending.is_some() || self.world.rlva_detach_in_flight(Instant::now()),
            stage: Stage {
                in_world: matches!(self.screen, Screen::World),
                camera_locked: camera::rlva_locked(&self.world.rlva),
                avatar: self.world.main_origin().map(|_| self.world.agent.position),
                // as far as a camera preset fixed in the world may be
                reach: self.settings.draw_distance.max(64.0),
            },
            camera: Pose {
                position: self.camera.position,
                target: self.camera.target,
                fov: self.camera.fov_y,
            },
        }
    }

    /// Once per frame: the bridge's calls, then the changes due now.
    pub(super) fn local_assistant_frame(&mut self) {
        let runs = mcp_bridge::frame(&self.settings.local_assistant, &self.local_assistant_view());
        for call in runs {
            self.local_assistant_run(call);
        }
    }

    fn local_assistant_run(&mut self, call: Pending) {
        let planned = match mcp_bridge::replan(&call, &self.local_assistant_view()) {
            Ok(planned) => planned,
            Err(e) => return call.answer(Err(e)),
        };
        let busy = |what: &str| {
            ToolError::new(
                ErrorCode::Busy,
                format!("{what} is still waiting for the server: try again in a moment"),
            )
        };
        match planned.work.clone() {
            Work::Nothing => call.answer(Ok(planned.reply)),
            Work::Inventory(change) => {
                if self.inventory_ui.pending.is_some() {
                    return call.answer(Err(busy("an inventory change")));
                }
                if crate::world::appearance::cof(&self.world.inventory).is_some_and(|cof| change.refresh.contains(&cof))
                    && self.appearance_ui.pending.is_some()
                {
                    return call.answer(Err(busy("an outfit change")));
                }
                // the send would drop it without a word (`send_from`)
                let command = NetCommand::EditInventory {
                    request: Uuid::nil(),
                    change: change.clone(),
                };
                if !self.world.rlva_allows_command(&command, Uuid::nil()) {
                    return call.answer(Err(ToolError::new(ErrorCode::NotAllowed, "an RLV restriction blocks this change")));
                }
                mcp_bridge::begin(call, &planned, false);
                let request = self.local_assistant_submit(change);
                mcp_bridge::sent(Some(request), None);
            }
            Work::Outfit(action) => {
                if self.appearance_ui.pending.is_some() {
                    return call.answer(Err(busy("an outfit change")));
                }
                mcp_bridge::begin(call, &planned, true);
                self.apply_appearance_action(action);
                let request = self.appearance_ui.pending.map(|(id, _)| id);
                let failure = request.is_none().then(|| self.appearance_ui.message.clone());
                mcp_bridge::sent(request, failure);
            }
            Work::Camera(mv) => match self.local_assistant_camera(mv) {
                Ok(()) => mcp_bridge::wait_move(call, planned.reply),
                Err(e) => call.answer(Err(e)),
            },
            Work::Snapshot(request) => mcp_bridge::queue_photo(call, request),
            Work::Thumbnail(id) => self.local_assistant_thumbnail(call, id, planned.reply),
        }
    }

    /// The avatar's skeleton as drawn, for the presets (the joints Appearance
    /// frames, `shape_editor::camera::view`).
    fn local_assistant_skeleton(&self) -> Option<Skeleton> {
        let now = Instant::now();
        let id = self.world.agent_id;
        let joint = |name: &str| self.scene.joint_world(&self.world, &id, name, now);
        let head = joint("mHead")?;
        let pelvis = joint("mPelvis")?;
        let foot = joint("mFootLeft")?;
        let yaw = self
            .world
            .objects
            .index_of_uuid(&id)
            .and_then(|index| Scene::object_transform(&self.world, index, now, 0))
            .map(|(_, rotation, _)| rotation * glam::Vec3::X)
            .filter(|at| at.truncate().length_squared() > 1e-6)
            .map_or(self.world.agent.yaw, |at| at.y.atan2(at.x));
        Some(Skeleton {
            head,
            pelvis,
            foot_z: foot.z,
            yaw,
        })
    }

    /// The user's own field of view back, if an exact shot changed it.
    fn local_assistant_fov_back(&mut self) {
        if let Some(fov) = mcp_bridge::fov_before() {
            self.settings.camera.fov = fov;
            mcp_bridge::set_fov_before(None);
        }
    }

    /// Move the camera; checked again for RLVa, which may have changed while
    /// the dialog was open.
    fn local_assistant_camera(&mut self, mv: Move) -> Result<(), ToolError> {
        if camera::rlva_locked(&self.world.rlva) {
            return Err(ToolError::new(ErrorCode::NotAllowed, "an RLV restriction holds the camera"));
        }
        // the photo mode's free camera would keep the view
        self.camera.clear_flycam();
        match mv {
            Move::Reset => {
                self.local_assistant_fov_back();
                self.reset_camera_view();
            }
            Move::Preset(preset) => {
                let skeleton = self
                    .local_assistant_skeleton()
                    .ok_or_else(|| ToolError::new(ErrorCode::NotReady, "the avatar is not drawn yet"))?;
                self.local_assistant_fov_back();
                let (eye, target) = camera::preset_view(preset, &skeleton, self.settings.camera.fov);
                self.camera.set_view(eye, target, &mut self.world.agent, &self.settings.camera);
            }
            Move::Shot { eye, fov } => {
                let (position, target) = eye.resolve(self.world.agent.position, self.world.agent.yaw);
                if let Some(fov) = fov {
                    if mcp_bridge::fov_before().is_none() {
                        mcp_bridge::set_fov_before(Some(self.settings.camera.fov));
                    }
                    self.settings.camera.fov = fov;
                }
                self.camera.set_view(position, target, &mut self.world.agent, &self.settings.camera);
            }
        }
        Ok(())
    }

    /// The gallery's Take snapshot for `id` (#2716): the same photo, square,
    /// saved as its picture; one at a time, as the gallery's.
    fn local_assistant_thumbnail(&mut self, call: Pending, id: Uuid, reply: serde_json::Value) {
        let busy = || {
            ToolError::new(
                ErrorCode::Busy,
                "another snapshot or inventory change is under way: try again in a moment",
            )
        };
        if mcp_bridge::thumbnail_busy() || self.inventory_images.photo.is_some() || self.inventory_ui.pending.is_some() {
            return call.answer(Err(busy()));
        }
        let message = self.appearance_ui.message.clone();
        self.outfit_snapshot(id);
        if self.inventory_images.photo.is_none() {
            // the gallery's own snapshot is still being encoded or sent
            self.appearance_ui.message = message;
            return call.answer(Err(busy()));
        }
        mcp_bridge::thumbnail_started(call, reply);
    }

    /// In the frame's drawing, beside the gallery's snapshot
    /// (`poll_outfit_photo`): a picture's photo starts once no other photo
    /// is being drawn and the camera has stopped.
    pub(super) fn local_assistant_photo(&mut self, renderer: &aurora_render::Renderer) {
        if let PhotoStep::Start(request) = mcp_bridge::photo_step(self.snapshots.drawing()) {
            let window = renderer.size();
            let world = self.world_viewport(window).map(|[_, _, w, h]| (w, h));
            let plan = crate::mcp_bridge::picture::plan(&request, window, world);
            mcp_bridge::photo_started(plan);
            self.photo_job(super::snapshot::Purpose::Assistant(plan.render), renderer);
        }
    }

    /// The picture's photo came back (`app/snapshot.rs`).
    pub(super) fn local_assistant_photo_drawn(&mut self, size: (u32, u32), pixels: Vec<u8>) {
        mcp_bridge::photo_drawn(size, pixels, self.settings.snapshot.folder());
    }

    /// `submit_inventory_change` (app/inventory.rs), without what belongs
    /// to the window's own clicks: no folder to select and rename once
    /// created, no clipboard to clear.
    fn local_assistant_submit(&mut self, mut change: aurora_net::inventory::operations::Mutation) -> Uuid {
        let request = Uuid::new_v4();
        change.defer_folder_contents = true;
        log::info!(
            "local assistant inventory change: {} operations, {} item copies",
            change.operations.len(),
            change.copies.len()
        );
        self.inventory_ui.begin_mutation(request, &change);
        self.inventory_ui.pending_refresh.clone_from(&change.refresh);
        self.inventory_ui.created_selection = None;
        self.inventory_ui.consume_clipboard = false;
        self.send(NetCommand::EditInventory { request, change });
        request
    }
}
