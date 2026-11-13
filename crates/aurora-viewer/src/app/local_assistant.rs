//! Folderstorm: the local assistant's changes run here, through the paths
//! the inventory window and Appearance use for the same click
//! (folderstorm/plans/mcp.md, "Phase 2"). The calls themselves wait in
//! `mcp_bridge`; this file only reaches the app's state.
//!
//! Phase 3: the camera moves as `pstcampos` and Escape move it, a picture
//! is a photo job of its own (`Purpose::Assistant`), and a thumbnail is the
//! Outfit Gallery's Take snapshot (`outfit_snapshot`).
//!
//! Phase 3, part 4: a move is the viewer's own autopilot (as the double
//! click on the ground and Go to start it), take-off and landing the Fly
//! button, a teleport the double-click teleport's `request_user_teleport`.

use super::*;
use crate::mcp_bridge::camera::{self, Move, Pose, Skeleton, Stage};
use crate::mcp_bridge::studio::PhotoStep;
use crate::mcp_bridge::{self, View, changes::Work, navigation};
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
            // phase 3, part 3: only what the viewer already holds, never fetched for it
            object_props: vec![self.area_search_feed.props(), &self.interactions.props, &self.build.props],
            sounds: Some(&self.scene.sounds.history),
            // phase 3, part 4: what the viewer itself does with the avatar
            navigation: crate::mcp_bridge::navigation::Native {
                pilot: self.autopilot.is_some(),
                blocks_walk: self
                    .native_anchor
                    .blocks_walk(self.world.agent.seated, self.world.agent.ground_sit),
            },
        }
    }

    /// Once per frame: the bridge's calls, then the changes due now.
    pub(super) fn local_assistant_frame(&mut self) {
        let runs = mcp_bridge::frame(&self.settings.local_assistant, &self.local_assistant_view());
        for call in runs {
            self.local_assistant_run(call);
        }
        // inventory_landmark: the landmark cache asks for what it lacks
        for asset in mcp_bridge::wanted_landmarks() {
            self.world.landmarks.resolve(asset);
        }
    }

    fn local_assistant_run(&mut self, call: Pending) {
        // decided again with the permissions and the login in force
        let Some((call, planned)) = mcp_bridge::replan(call, &self.local_assistant_view()) else {
            return;
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
            Work::Create(creation) => self.local_assistant_create(call, planned, creation),
            Work::Navigate(work) => self.local_assistant_navigate(call, work),
            Work::Image { target, texture } => {
                if self.inventory_ui.pending.is_some() {
                    return call.answer(Err(busy("an inventory change")));
                }
                // the Image window's own path ("Choose image"), without a
                // window: its end is the server's answer to `request`, or
                // an error the path reports (`mcp_bridge::image_finished`)
                let request = Uuid::new_v4();
                mcp_bridge::begin(call, &planned, false);
                mcp_bridge::sent(Some(request), None);
                self.inventory_image_action(
                    request,
                    target,
                    crate::ui::inventory::thumbnail::Input::Texture {
                        asset: texture,
                        resize: true,
                    },
                );
            }
        }
    }

    /// Phase 3, part 4: walk or fly to a point, take off or land, teleport,
    /// each checked again for what the dialog's time may have changed.
    fn local_assistant_navigate(&mut self, call: Pending, work: navigation::Work) {
        use crate::autopilot::{AutoPilot, FlightTarget};
        let busy = || {
            ToolError::new(
                ErrorCode::Busy,
                "the avatar is already walking or following (the viewer's own autopilot): it is not interrupted",
            )
        };
        match work {
            navigation::Work::Move(mv) => {
                let agent = &self.world.agent;
                if self.autopilot.is_some() {
                    return call.answer(Err(busy()));
                }
                if agent.is_sitting() || self.native_anchor.blocks_walk(agent.seated, agent.ground_sit) {
                    return call.answer(Err(ToolError::new(ErrorCode::NotAllowed, "the avatar is sitting")));
                }
                // as `walk_to` and `travel_to_avatar` start theirs
                let now = Instant::now();
                let from = agent.position;
                let pilot = match mv.mode {
                    navigation::Mode::Walk => AutoPilot::start(from, mv.target, false, now).with_stop_distance(mv.stop_distance),
                    navigation::Mode::Fly => {
                        let mut pilot = AutoPilot::avatar(from, mv.target, None, agent.flying, now).with_stop_distance(mv.stop_distance);
                        // Go to's flight target: take off if needed, hover at the point
                        pilot.observe_flight(
                            FlightTarget {
                                flying: Some(true),
                                landing_height: mv.target.z,
                            },
                            from,
                        );
                        pilot
                    }
                };
                self.world.following_avatar = None;
                self.follow_recovery = None;
                self.follow_ui.open = false;
                self.autopilot = Some(pilot);
                self.autopilot_stop_pending = false;
                self.autopilot_flags = 0;
                self.release_native_movelock();
                log::info!("local assistant: {} of {:.1} m started", mv.mode.name(), navigation::distance(mv.mode, from, mv.target));
                mcp_bridge::navigation_moving(call, mv, navigation::PilotId(now));
            }
            navigation::Work::Flying(flying) => {
                if self.autopilot.is_some() {
                    return call.answer(Err(busy()));
                }
                if flying == self.world.agent.flying {
                    return call.answer(Ok(serde_json::json!({"flying": flying, "changed": false})));
                }
                // LLAgent::setFlying's rules, through the Fly button
                self.toggle_fly();
                if self.world.agent.flying != flying {
                    return call.answer(Err(ToolError::new(ErrorCode::NotAllowed, "the viewer did not let the avatar take off here")));
                }
                call.receipt(folderstorm_mcp_bridge::receipts::Receipt::mark_sent);
                call.answer(Ok(serde_json::json!({"flying": flying, "changed": true})));
            }
            navigation::Work::Teleport(teleport) => {
                match mcp_bridge::teleport_wait() {
                    Some(wait) if wait.is_zero() => {}
                    _ => {
                        return call.answer(Err(ToolError::new(
                            ErrorCode::Busy,
                            "another teleport of yours is under way, or the last one was too recent: try again in a moment",
                        )));
                    }
                }
                if self.world.teleporting {
                    return call.answer(Err(ToolError::new(ErrorCode::Busy, "a teleport is under way")));
                }
                let command = teleport.command();
                let remote = Some(teleport.handle) != self.world.main_region;
                // RLVa at the send boundary, as for the user's own teleport
                if !self.world.rlva_allows_command(&command, Uuid::nil())
                    || !self.request_user_teleport(command, teleport.label.clone(), remote)
                {
                    return call.answer(Err(ToolError::new(ErrorCode::NotAllowed, "an RLV restriction refuses this teleport")));
                }
                log::info!("local assistant: teleport submitted ({})", if remote { "another region" } else { "this region" });
                mcp_bridge::navigation_teleporting(call, teleport);
            }
        }
    }

    /// Phase 3, part 4: once per frame, just before the autopilot runs (it
    /// reads the input of this frame): the assistant's move or teleport
    /// under way learns what happened, and only its own autopilot is ever
    /// stopped.
    pub(super) fn local_assistant_navigation(&mut self) {
        let agent = &self.world.agent;
        let frame = navigation::Frame {
            pilot: self.autopilot.as_ref().map(|ap| navigation::PilotId(ap.started)),
            position: agent.position,
            region: self.world.main_region,
            global: self.world.agent_global(),
            flying: agent.flying,
            sitting: agent.is_sitting(),
            teleporting: self.world.teleporting,
            teleport_failed: self.world.tp_failed,
            manual: navigation::manual_input(&self.input, self.mouse_mode == MouseMode::Steer),
            in_world: self.in_world(),
            location_hidden: self.world.rlva.has("showloc"),
        };
        for act in mcp_bridge::navigation_frame(&frame) {
            match act {
                navigation::Act::StopPilot(pilot) => {
                    if self.autopilot.as_ref().is_some_and(|ap| navigation::PilotId(ap.started) == pilot) {
                        self.stop_autopilot(true);
                    }
                }
                navigation::Act::CancelTeleport => {
                    if self.world.teleporting {
                        self.cancel_teleport();
                    }
                }
            }
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

    /// Just before the gallery's snapshot `request` is sent as an entry's
    /// picture (`poll_inventory_images`): the assistant's thumbnail is
    /// decided again with the login and the levels in force. True: it was
    /// refused, or asks the user again, and the snapshot stops here
    /// (Appearance says so, as for any snapshot that fails).
    pub(super) fn local_assistant_thumbnail_stopped(&mut self, request: Uuid) -> bool {
        if mcp_bridge::thumbnail_send(&self.local_assistant_view()) {
            return false;
        }
        self.finish_inventory_image(request, Some(aurora_i18n::tr!("fs-mcp-thumbnail-stopped").into()));
        true
    }

    /// In the frame's drawing, beside the gallery's snapshot
    /// (`poll_outfit_photo`): a picture's photo starts once no other photo
    /// is being drawn and the camera has stopped.
    pub(super) fn local_assistant_photo(&mut self, renderer: &aurora_render::Renderer) {
        // decided again just before: the login or the levels may have changed
        if let PhotoStep::Start(request) = mcp_bridge::photo_step(self.snapshots.drawing(), &self.local_assistant_view()) {
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

    /// New Script, New Notecard… of the inventory's menu
    /// (`InvAction::Create`), and a landmark of where the avatar stands as
    /// the star's quick entries make it (`CreateLandmarkUi::quick`).
    fn local_assistant_create(
        &mut self,
        call: Pending,
        mut planned: crate::mcp_bridge::changes::Planned,
        mut creation: crate::mcp_bridge::inventory_edits::Creation,
    ) {
        use aurora_net::inventory::operations::NewItem;
        if creation.kind == NewItem::Landmark {
            if self.world.main().is_none() || self.world.parcel.is_none() {
                return call.answer(Err(ToolError::new(
                    ErrorCode::NotReady,
                    "not in a region yet: a landmark is made where the avatar stands",
                )));
            }
            let name = creation.name.clone().unwrap_or_else(|| {
                crate::ui::landmark_create::default_title(&self.world.parcel_name, &self.world.region_name(), self.world.agent.position)
            });
            creation.name = Some(name.clone());
            planned.reply["name"] = serde_json::json!(name);
            planned.work = Work::Create(creation.clone());
        }
        let name = creation.name.clone().unwrap_or_default();
        let command = NetCommand::CreateInventoryItem {
            parent: creation.parent,
            kind: creation.kind,
            name: name.clone(),
            permissions: None,
        };
        // the send would drop it without a word (@showloc for a landmark)
        if !self.world.rlva_allows_command(&command, Uuid::nil()) {
            return call.answer(Err(ToolError::new(ErrorCode::NotAllowed, "an RLV restriction blocks this change")));
        }
        mcp_bridge::begin(call, &planned, false);
        self.apply_inventory_action(crate::ui::inventory::InvAction::Create {
            parent: creation.parent,
            kind: creation.kind,
            name,
        });
        mcp_bridge::sent(None, None);
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
