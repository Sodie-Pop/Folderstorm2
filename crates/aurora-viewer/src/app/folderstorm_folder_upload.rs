//! Folderstorm: the folder upload's glue between the Inventory, the
//! uploader and `folderstorm/folder_upload` (FEATURES.md 2.11).

use super::*;
use crate::folderstorm::folder_upload::{self as upload, Effect, Refusal, Source, demo, plan};

impl App {
    /// Inventory › + › Upload a folder here…
    pub(super) fn folderstorm_upload_folder(&mut self, destination: Uuid) {
        self.folderstorm_folder_upload_start(destination, Source::Dialog);
    }

    /// Starts a folder upload into `destination`, its folder from `source`.
    pub(super) fn folderstorm_folder_upload_start(&mut self, destination: Uuid, source: Source) {
        if let Err(refusal) = self.folderstorm_folder_upload_begin(destination) {
            self.world
                .system_notice(refusal.message(), crate::world::notice_topics::Topic::Warning);
            return;
        }
        self.imports.open(crate::imports::Choice::Bulk);
        self.imports.tab = crate::imports::ui::Tab::Folder;
        let tx = self.folderstorm_folder_upload.sender();
        self.scene.jobs.execute(move || upload::pick(source, tx));
    }

    fn folderstorm_folder_upload_begin(&mut self, destination: Uuid) -> Result<(), Refusal> {
        if !self.demo && self.world.login.is_none() {
            return Err(Refusal::Offline);
        }
        if !crate::world::inventory::actions::destination(&self.world.inventory, destination) {
            return Err(Refusal::Destination);
        }
        let idle = !self.imports.busy() && self.imports.pending_files.is_none() && !self.imports.confirm;
        self.folderstorm_folder_upload
            .begin(destination, self.world.agent_id, idle, &mut self.settings.imports)
    }

    /// Once per frame, before the uploader (app/imports.rs). The routing set
    /// here is the batch's only: the uploader does not save it.
    pub(super) fn folderstorm_folder_upload_frame(&mut self, ctx: &egui::Context) {
        if self.demo {
            self.folderstorm_folder_upload_demo();
        }
        let snapshot = upload::snapshot(&self.imports, self.world.agent_id);
        let mut effects = self.folderstorm_folder_upload.step(&snapshot);
        while !effects.is_empty() {
            let mut more = Vec::new();
            for effect in effects {
                match effect {
                    Effect::Load(path) => {
                        self.imports
                            .add_folder_path(path, &self.scene.jobs, ctx, self.settings.imports.texture());
                        let started = self.imports.pending_files.is_some();
                        if !started {
                            self.world
                                .system_notice(Refusal::Busy.message(), crate::world::notice_topics::Topic::Warning);
                        }
                        more.extend(self.folderstorm_folder_upload.loading(started));
                    }
                    Effect::Restore { saved, applied } => {
                        if plan::restore(&mut self.settings.imports, &saved, &applied) {
                            self.settings.save();
                        }
                    }
                    Effect::Receipt { folder, receipt } => {
                        let topic = if receipt.failed.is_empty() {
                            crate::world::notice_topics::Topic::Items
                        } else {
                            crate::world::notice_topics::Topic::Warning
                        };
                        self.world.system_notice(upload::ui::receipt_text(&folder, &receipt), topic);
                    }
                }
            }
            effects = more;
        }
        let context = self.folderstorm_folder_upload.loaded().map(|run| upload::ui::Context {
            destination: run.destination,
            folder: run.source.as_deref().map(upload::ui::folder_name).unwrap_or_default(),
            skipped: run.skipped.clone(),
        });
        upload::ui::publish(ctx, context);
    }

    /// `AURORA_DEMO_FOLDER_UPLOAD`: the flow on the inventory root, without
    /// the dialog, then the confirmation or the offline upload.
    fn folderstorm_folder_upload_demo(&mut self) {
        let Some(mode) = demo::Mode::from_env() else {
            return;
        };
        let root = self.world.inventory.root;
        let state = &mut self.folderstorm_folder_upload;
        if state.demo_step == 0 && self.frame_count >= 200 && self.world.inventory.folders.contains_key(&root) {
            state.demo_step = 1;
            let base = std::path::PathBuf::from(".scratch")
                .join("folder-upload-demo")
                .join(uuid::Uuid::new_v4().to_string());
            match demo::fixtures(&base) {
                Ok(folder) => {
                    // A folder of the same name already there: the name warning.
                    self.world.inventory.created_folders(vec![aurora_net::inventory::InvFolder {
                        id: crate::demo::action_id(2110),
                        parent: root,
                        name: demo::FOLDER.into(),
                        type_default: -1,
                        version: 1,
                        ..Default::default()
                    }]);
                    self.folderstorm_folder_upload_start(root, Source::Path(folder));
                }
                Err(error) => log::warn!("folder upload demo: fixtures failed: {error}"),
            }
            return;
        }
        if state.demo_step != 1 || mode == demo::Mode::Preview || state.loaded().is_none() {
            return;
        }
        let prices = aurora_net::inventory::upload::Prices {
            texture: Some(10),
            large_texture: Some(40),
            sound: Some(10),
            animation: Some(10),
        };
        if self.imports.total(&prices).is_none() || !self.imports.folder_plan_valid(&self.world) {
            return;
        }
        state.demo_step = 2;
        if mode == demo::Mode::Confirm {
            self.imports.confirm = true;
            self.imports.confirm_choice = Some(crate::imports::Choice::Bulk);
        } else {
            log::info!("folder upload demo: confirmed");
            self.imports
                .start_confirmed(&self.world, &self.settings.inventory, &self.settings.imports, &prices);
        }
    }
}
