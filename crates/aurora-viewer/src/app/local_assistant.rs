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
                blocks_walk: self.native_anchor.blocks_walk(self.world.agent.seated, self.world.agent.ground_sit),
            },
            content_open: self.local_assistant_open_documents(),
        }
    }

    /// Folderstorm: the items open in the notecard window (the inventory's
    /// own preview or a document window) or in the script editor: content
    /// saves leave them to the user (mcp-content.md). A script editor on a
    /// script inside an object counts too (its task item id): task script
    /// saves leave it to the user (mcp-task-scripts.md).
    fn local_assistant_open_documents(&self) -> std::collections::HashSet<Uuid> {
        let inventory = &self.inventory_ui;
        inventory
            .preview
            .into_iter()
            .chain(inventory.windows.iter().filter_map(|w| w.state.preview))
            .chain(
                self.script_editors.documents.iter().map(|e| e.target.item),
            )
            .collect()
    }

    /// Once per frame: the bridge's calls, then the changes due now.
    pub(super) fn local_assistant_frame(&mut self) {
        // Folderstorm: content editing: the demo's fixtures, and the scripts
        // the assistant saved (their new asset, then the item read back)
        if self.demo
            && mcp_bridge::content::demo::wanted()
            && !self.world.inventory.root.is_nil()
            && !self.world.inventory.items.contains_key(&mcp_bridge::content::demo::NOTES)
        {
            let (documents, sources) = mcp_bridge::content::demo::seed(&mut self.world.inventory, self.world.agent_id);
            self.demo_documents.extend(documents);
            for (item, source) in sources {
                let target = aurora_net::scripts::ScriptTarget { object: Uuid::nil(), item };
                self.demo_script_sources.insert(target, source);
            }
        }
        for (item, asset) in mcp_bridge::content_updates() {
            if let Some(mut it) = self.world.inventory.items.get(&item).cloned() {
                it.asset_id = asset;
                self.world.inventory.add_items(vec![it]);
            }
            if !self.demo {
                self.send(NetCommand::FetchItems {
                    items: vec![item],
                    owner: self.world.agent_id,
                });
            }
        }
        let runs = mcp_bridge::frame(&self.settings.local_assistant, &self.local_assistant_view());
        for call in runs {
            self.local_assistant_run(call);
        }
        // inventory_landmark: the landmark cache asks for what it lacks
        for asset in mcp_bridge::wanted_landmarks() {
            self.world.landmarks.resolve(asset);
        }
        // Folderstorm: inventory_content_get: the text, as the editors ask for it
        for fetch in mcp_bridge::wanted_content() {
            self.local_assistant_fetch(fetch);
        }
        // Folderstorm: an object's properties and contents (mcp-objects.md)
        for want in mcp_bridge::object_wants() {
            self.local_assistant_object_want(want);
        }
        // Folderstorm: scripts inside objects (mcp-task-scripts.md)
        self.local_assistant_task_script_frame();
    }

    /// Folderstorm: what the task script tools wait for, asked the way the
    /// object's script editor asks (`LoadScript`, `LoadScriptExperiences`,
    /// `GetScriptRunning`), and the scripts they saved put in the Contents
    /// tab with the upload's asset, the prim listed again.
    fn local_assistant_task_script_frame(&mut self) {
        use aurora_net::build::BuildCmd;
        use aurora_net::scripts::ScriptTarget;
        for (object, item, asset) in mcp_bridge::task_script_updates() {
            if let Some(inv) = self.build.contents.objects.get_mut(&object)
                && let Some(it) = inv.items.iter_mut().find(|it| it.item_id == item)
            {
                it.asset_id = asset;
            }
            self.build.contents_changed(object);
        }
        let (fetches, queries) = mcp_bridge::task_script_wants();
        for fetch in fetches {
            let target = ScriptTarget {
                object: fetch.object,
                item: fetch.item,
            };
            let load = NetCommand::LoadScript {
                request: fetch.source_request,
                target,
                handle: fetch.handle,
                owner: fetch.owner,
                asset: fetch.asset,
            };
            // the send would drop it with a notice of its own
            if !self.world.rlva_allows_command(&load, Uuid::nil()) {
                mcp_bridge::task_script_fetch_failed(fetch.source_request, "an RLV restriction hides it");
                continue;
            }
            self.send(NetCommand::LoadScriptExperiences {
                request: fetch.experience_request,
                target,
                handle: fetch.handle,
            });
            self.send(load);
        }
        for query in queries {
            self.send(NetCommand::Build(BuildCmd::GetScriptRunning {
                handle: query.handle,
                object_id: query.object,
                item_id: query.item,
            }));
        }
    }

    /// Folderstorm: a task script change, sent as the object's script
    /// editor sends it (`SaveScript` to UpdateScriptTask, Mono, the running
    /// flag asked, the association read with the source; `SetScriptRunning`;
    /// `ScriptReset`), checked for RLVa just before, then the running state
    /// asked (mcp-task-scripts.md). Sent once: never again automatically.
    fn local_assistant_task_script(&mut self, call: Pending, reply: serde_json::Value, send: mcp_bridge::task_scripts::Send) {
        use aurora_net::build::BuildCmd;
        use mcp_bridge::task_scripts::Action;
        let target = aurora_net::scripts::ScriptTarget {
            object: send.object,
            item: send.item,
        };
        if matches!(send.action, Action::Save { .. }) && mcp_bridge::task_script_saving(send.object, send.item) {
            return call.answer(Err(ToolError::new(
                ErrorCode::Busy,
                "another save of this script is still waiting for the server: try again in a moment",
            )));
        }
        let check = match &send.action {
            Action::Save { .. } => NetCommand::SaveScript {
                request: Uuid::nil(),
                target,
                handle: send.handle,
                options: aurora_net::scripts::ScriptOptions {
                    mono: true,
                    luau: aurora_net::scripts::Luau::Off,
                    running: false,
                    experience: Uuid::nil(),
                },
                source: String::new(),
            },
            Action::Running(running) => NetCommand::Build(BuildCmd::SetScriptRunning {
                handle: send.handle,
                object_id: send.object,
                item_id: send.item,
                running: *running,
            }),
            Action::Reset => NetCommand::Build(BuildCmd::ScriptReset {
                handle: send.handle,
                object_id: send.object,
                item_id: send.item,
            }),
        };
        if !self.world.rlva_allows_command(&check, Uuid::nil()) {
            return call.answer(Err(ToolError::new(ErrorCode::NotAllowed, "an RLV restriction blocks this change")));
        }
        // watched from before the send: the demo answers inside it
        let request = mcp_bridge::task_script_sending(call, &send, reply);
        log::info!("local assistant task script change: {}", send.tool);
        match send.action {
            Action::Save {
                source,
                running,
                experience,
                ..
            } => {
                let Some(request) = request else { return };
                self.send(NetCommand::SaveScript {
                    request,
                    target,
                    handle: send.handle,
                    options: aurora_net::scripts::ScriptOptions {
                        mono: true,
                        luau: aurora_net::scripts::Luau::Off,
                        running,
                        experience,
                    },
                    source,
                });
            }
            Action::Running(_) | Action::Reset => {
                self.send(check);
                self.send(NetCommand::Build(BuildCmd::GetScriptRunning {
                    handle: send.handle,
                    object_id: send.object,
                    item_id: send.item,
                }));
            }
        }
    }

    /// Folderstorm: fetch a content read's text the way the viewer's editors
    /// do: a notecard through `PreviewInventoryItem` (the simulator checks
    /// the item's rights), a script through the script editor's `LoadScript`.
    fn local_assistant_fetch(&mut self, fetch: mcp_bridge::content::Fetch) {
        use mcp_bridge::content::Kind;
        let Some(item) = self.world.inventory.items.get(&fetch.item).cloned() else {
            return mcp_bridge::content_fetch_failed(fetch.request, "the item is gone".into());
        };
        let command = match fetch.kind {
            Kind::Notecard => NetCommand::PreviewInventoryItem(item.clone()),
            Kind::Script => NetCommand::LoadScript {
                request: fetch.request,
                target: aurora_net::scripts::ScriptTarget {
                    object: Uuid::nil(),
                    item: item.id,
                },
                handle: self.world.main_region.unwrap_or_default(),
                owner: item.owner,
                asset: item.asset_id,
            },
        };
        // the send would drop it with a notice of its own
        if !self.world.rlva_allows_command(&command, Uuid::nil()) {
            return mcp_bridge::content_fetch_failed(fetch.request, "an RLV restriction hides it".into());
        }
        if self.demo && fetch.kind == Kind::Notecard {
            // the demo's preview answers the editors directly, not the bridge
            let bytes = self
                .demo_documents
                .get(&item.id)
                .cloned()
                .unwrap_or_else(|| crate::world::inventory::demo::note_asset(false));
            return mcp_bridge::content_demo_notecard(item.id, item.asset_id, bytes);
        }
        self.send(command);
    }

    /// Folderstorm: ask the server what an object tool waits for. The
    /// properties are the family subset a hover asks for (it selects
    /// nothing); the offline demo answers its own selection instead.
    fn local_assistant_object_want(&mut self, want: crate::mcp_bridge::objects::Want) {
        use crate::mcp_bridge::objects::Want;
        use aurora_net::build::BuildCmd;
        match want {
            Want::Properties { handle, local_id, object } => {
                if self.demo {
                    self.send(NetCommand::Build(BuildCmd::Select {
                        handle,
                        local_ids: vec![local_id],
                    }));
                } else {
                    self.send(NetCommand::RequestObjectProperties { handle, object });
                }
            }
            Want::Contents { handle, local_id, object } => self.send(NetCommand::Build(BuildCmd::RequestTaskInventory {
                handle,
                local_id,
                object_id: object,
            })),
        }
    }

    /// Folderstorm: an object change, sent as the Contents tab or Take
    /// sends it, checked for RLVa just before (mcp-objects.md).
    fn local_assistant_object(&mut self, call: Pending, reply: serde_json::Value, send: crate::mcp_bridge::objects::Send) {
        use aurora_net::build::BuildCmd;
        let Some(command) = send.command.clone() else {
            return mcp_bridge::object_associate(call, reply);
        };
        let take = matches!(command, BuildCmd::DeRez { .. });
        let command = NetCommand::Build(command);
        if !self.world.rlva_allows_command(&command, Uuid::nil()) {
            return call.answer(Err(ToolError::new(ErrorCode::NotAllowed, "an RLV restriction blocks this change")));
        }
        // watched from before the send: the demo answers inside it
        mcp_bridge::object_sending(call, &send, reply, &self.world);
        if take {
            self.world.ui_sounds.push(crate::ui_sound::UiSound::ObjectRezOut);
        } else {
            self.build.contents_changed(send.object);
        }
        log::info!("local assistant object change: {}", send.tool);
        self.send(command);
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
            Work::Content(save) => self.local_assistant_content(call, &planned, save),
            Work::Object(send) => self.local_assistant_object(call, planned.reply, *send), // Folderstorm: mcp-objects.md
            Work::TaskScript(send) => self.local_assistant_task_script(call, planned.reply, *send), // Folderstorm: mcp-task-scripts.md
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
                log::info!(
                    "local assistant: {} of {:.1} m started",
                    mv.mode.name(),
                    navigation::distance(mv.mode, from, mv.target)
                );
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
                    return call.answer(Err(ToolError::new(
                        ErrorCode::NotAllowed,
                        "the viewer did not let the avatar take off here",
                    )));
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
                    return call.answer(Err(ToolError::new(
                        ErrorCode::NotAllowed,
                        "an RLV restriction refuses this teleport",
                    )));
                }
                log::info!(
                    "local assistant: teleport submitted ({})",
                    if remote { "another region" } else { "this region" }
                );
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

    /// Folderstorm: a notecard's text or a script's source saved as the
    /// editors save them: `SaveInventoryContent` (UpdateNotecardAgentInventory)
    /// or `SaveScript` with an inventory target (UpdateScriptAgent, Mono, not
    /// running, no experience). The answer (`InventoryContentSaved`,
    /// `ScriptSaved`) reaches the desk through `mcp_bridge::observe`. Sent
    /// once: never again automatically.
    fn local_assistant_content(&mut self, call: Pending, planned: &mcp_bridge::changes::Planned, save: mcp_bridge::content::Save) {
        use mcp_bridge::content::Kind;
        if mcp_bridge::content_saving(save.item) {
            return call.answer(Err(ToolError::new(
                ErrorCode::Busy,
                "another save of this item is still waiting for the server: try again in a moment",
            )));
        }
        let request = Uuid::new_v4();
        let command = match save.kind {
            Kind::Notecard => NetCommand::SaveInventoryContent {
                request,
                item: save.item,
                asset_type: 7,
                lua: false,
                data: save.data,
            },
            Kind::Script => {
                let Ok(source) = String::from_utf8(save.data) else {
                    return call.answer(Err(ToolError::new(ErrorCode::Internal, "the source is not UTF-8")));
                };
                NetCommand::SaveScript {
                    request,
                    target: aurora_net::scripts::ScriptTarget {
                        object: Uuid::nil(),
                        item: save.item,
                    },
                    handle: self.world.main_region.unwrap_or_default(),
                    options: aurora_net::scripts::ScriptOptions {
                        mono: true,
                        luau: aurora_net::scripts::Luau::Off,
                        running: false,
                        experience: Uuid::nil(),
                    },
                    source,
                }
            }
        };
        if !self.world.rlva_allows_command(&command, Uuid::nil()) {
            return call.answer(Err(ToolError::new(ErrorCode::NotAllowed, "an RLV restriction blocks this save")));
        }
        mcp_bridge::begin(call, planned, false);
        self.send(command);
        mcp_bridge::sent(Some(request), None);
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
