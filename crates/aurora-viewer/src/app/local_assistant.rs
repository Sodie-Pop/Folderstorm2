//! Folderstorm: the local assistant's changes run here, through the paths
//! the inventory window and Appearance use for the same click
//! (folderstorm/plans/mcp.md, "Phase 2"). The calls themselves wait in
//! `mcp_bridge`; this file only reaches the app's state.

use super::*;
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
            Work::Create(creation) => self.local_assistant_create(call, planned, creation),
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
