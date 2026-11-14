//! Folder-preserving bulk upload (FEATURES.md 2.11, plan in
//! folderstorm/plans/folder-upload.md): *Upload a folder here…* in an
//! Inventory folder's **+** menu picks a folder on disk and uploads it as a
//! new inventory folder with the same tree, after a preview with the total
//! L$ cost and an explicit confirmation.
//!
//! Port of Folderstorm (C++)'s `start_bulk_folder_upload`
//! (indra/newview/llviewermenufile.cpp, branch `codex/sl-folder-upload`,
//! Folderstorm contributors, originally LGPL 2.1). Aurora's unified
//! uploader (`crate::imports`) already scans folders, mirrors their tree,
//! prices, confirms, creates the folders after the confirmation and uploads
//! with per-file results; this module drives it and never copies it:
//!
//! 1. [`State::begin`] remembers the user's routing and sets Folderstorm's
//!    (a mirror of the disk folder into the chosen inventory folder);
//! 2. the folder comes from a dialog or from a caller ([`Source`]); a
//!    worker lists what will be left out ([`plan::scan`]), then the
//!    uploader loads the folder through its own loader;
//! 3. the uploader's confirmation shows [`plan::Preview`] (`ui.rs`);
//! 4. [`plan::Tracker`] follows the confirmed files and gives a
//!    [`plan::Receipt`] once the queue is idle;
//! 5. the routing goes back when the uploader is closed.
//!
//! A local-assistant tool can later use the same steps with
//! [`Source::Path`]; the user still confirms in the viewer.

pub mod demo;
pub mod plan;
pub mod ui;

#[cfg(test)]
mod tests;

use crate::imports::Preferences;
use plan::{Receipt, Routing, Row, Skipped, Tracker};
use std::path::PathBuf;
use std::sync::mpsc;
use uuid::Uuid;

/// Where the disk folder comes from.
pub enum Source {
    /// The system's folder dialog.
    Dialog,
    /// A folder chosen elsewhere (a demo, later a local-assistant tool).
    Path(PathBuf),
}

/// A folder chosen, with what the uploader will leave out of it; `None`
/// when the dialog was cancelled.
pub struct Picked {
    pub path: Option<PathBuf>,
    pub skipped: Skipped,
}

/// Why a folder upload cannot start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Not logged in.
    Offline,
    /// The folder is not one that takes new items (Trash, Library, Current
    /// Outfit, a protected folder…).
    Destination,
    /// The uploader is loading, confirming or uploading.
    Busy,
}

impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Refusal::Offline => aurora_i18n::tr!("fs-folder-upload-offline"),
            Refusal::Destination => aurora_i18n::tr!("fs-folder-upload-destination"),
            Refusal::Busy => aurora_i18n::tr!("fs-folder-upload-busy"),
        }
    }
}

/// The uploader as this module sees it, once per frame.
#[derive(Debug, Default, Clone)]
pub struct Snapshot {
    pub agent: Uuid,
    /// The uploader's window is open.
    pub open: bool,
    /// The uploader is loading files.
    pub loading: bool,
    /// Folders or files are being created or uploaded.
    pub busy: bool,
    pub rows: Vec<Row>,
    /// L$ confirmed by the server since the last confirmation.
    pub spent: i32,
}

/// What the viewer has to do after a [`State::step`].
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Load this folder into the uploader (`Imports::add_folder_path`).
    Load(PathBuf),
    /// Put the routing back (`plan::restore`).
    Restore { saved: Routing, applied: Routing },
    /// A confirmed upload is over; `folder` is the disk folder's name.
    Receipt { folder: String, receipt: Receipt },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Waiting for the dialog and the scan.
    Picking,
    /// The uploader is loading the folder.
    Loading,
    /// The folder is in the uploader: preview, confirmation, upload.
    Loaded,
}

/// The folder upload in progress (one at a time).
#[derive(Debug, Clone)]
pub struct Run {
    pub destination: Uuid,
    pub agent: Uuid,
    pub source: Option<PathBuf>,
    pub skipped: Skipped,
    saved: Routing,
    applied: Routing,
    stage: Stage,
    tracker: Option<Tracker>,
}

pub struct State {
    run: Option<Run>,
    /// `AURORA_DEMO_FOLDER_UPLOAD`'s progress (app/folderstorm_folder_upload.rs).
    pub demo_step: u8,
    tx: mpsc::Sender<Picked>,
    rx: mpsc::Receiver<Picked>,
}

impl Default for State {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            run: None,
            demo_step: 0,
            tx,
            rx,
        }
    }
}

impl State {
    pub fn run(&self) -> Option<&Run> {
        self.run.as_ref()
    }

    /// The channel the dialog's worker answers on.
    pub fn sender(&self) -> mpsc::Sender<Picked> {
        self.tx.clone()
    }

    /// Starts a folder upload into `destination` once the caller checked it
    /// takes new items: remembers the routing and sets Folderstorm's. The
    /// caller then gets the folder ([`pick`]) and opens the uploader on [`Effect::Load`].
    pub fn begin(&mut self, destination: Uuid, agent: Uuid, uploader_idle: bool, prefs: &mut Preferences) -> Result<(), Refusal> {
        if self.run.is_some() || !uploader_idle {
            return Err(Refusal::Busy);
        }
        // A dialog's answer for an older run must not start this one.
        while self.rx.try_recv().is_ok() {}
        let saved = Routing::read(prefs);
        let applied = Routing::mirror_into(destination);
        applied.write(prefs);
        self.run = Some(Run {
            destination,
            agent,
            source: None,
            skipped: Skipped::default(),
            saved,
            applied,
            stage: Stage::Picking,
            tracker: None,
        });
        Ok(())
    }

    /// The uploader took the folder of an [`Effect::Load`]; `false` when it
    /// refused it (then the run ends).
    pub fn loading(&mut self, started: bool) -> Vec<Effect> {
        match &mut self.run {
            Some(run) if started => {
                run.stage = Stage::Loading;
                Vec::new()
            }
            Some(_) => self.end(),
            None => Vec::new(),
        }
    }

    /// The preview's context while the folder is in the uploader.
    pub fn loaded(&self) -> Option<&Run> {
        self.run.as_ref().filter(|run| run.stage == Stage::Loaded)
    }

    /// One frame: the dialog's answer, the loading, the confirmed upload
    /// and the end of the run.
    pub fn step(&mut self, snapshot: &Snapshot) -> Vec<Effect> {
        let Some(run) = &mut self.run else {
            while self.rx.try_recv().is_ok() {}
            return Vec::new();
        };
        if run.agent != snapshot.agent {
            return self.end();
        }
        let mut effects = Vec::new();
        match run.stage {
            Stage::Picking => match self.rx.try_recv() {
                Ok(Picked { path: Some(path), skipped }) => {
                    run.source = Some(path.clone());
                    run.skipped = skipped;
                    effects.push(Effect::Load(path));
                }
                Ok(Picked { path: None, .. }) | Err(mpsc::TryRecvError::Disconnected) => return self.end(),
                Err(mpsc::TryRecvError::Empty) => {}
            },
            Stage::Loading => {
                if !snapshot.loading {
                    run.stage = Stage::Loaded;
                }
            }
            Stage::Loaded => {
                match &mut run.tracker {
                    None if snapshot.busy => {
                        let tracker = Tracker::begin(&snapshot.rows);
                        run.tracker = (!tracker.is_empty()).then_some(tracker);
                    }
                    Some(tracker) => {
                        tracker.observe(&snapshot.rows);
                        if !snapshot.busy {
                            effects.push(Effect::Receipt {
                                folder: run.source.as_deref().map(ui::folder_name).unwrap_or_default(),
                                receipt: tracker.receipt(snapshot.spent),
                            });
                            run.tracker = None;
                        }
                    }
                    None => {}
                }
                if run.tracker.is_none() && !snapshot.busy && !snapshot.loading && !snapshot.open {
                    effects.extend(self.end());
                }
            }
        }
        effects
    }

    /// Ends the run and puts the routing back.
    fn end(&mut self) -> Vec<Effect> {
        self.run
            .take()
            .map(|run| Effect::Restore {
                saved: run.saved,
                applied: run.applied,
            })
            .into_iter()
            .collect()
    }
}

/// The folder for a run, on a worker: the dialog when asked, then the list
/// of what will be left out. Answers on `tx`.
pub fn pick(source: Source, tx: mpsc::Sender<Picked>) {
    let path = match source {
        Source::Dialog => rfd::FileDialog::new()
            .set_title(aurora_i18n::tr!("fs-folder-upload-dialog"))
            .pick_folder(),
        Source::Path(path) => Some(path),
    };
    let skipped = path.as_deref().map(|path| plan::scan(path, plan::SCAN_LIMIT)).unwrap_or_default();
    let _ = tx.send(Picked { path, skipped });
}

/// The uploader's drafts as plain rows (local previews are not uploads).
pub fn rows(imports: &crate::imports::Imports) -> Vec<Row> {
    use crate::imports::Status;
    imports
        .entries
        .iter()
        .filter(|entry| !entry.local && imports.choice.accepts(entry))
        .map(|entry| Row {
            id: entry.id,
            name: entry.name.clone(),
            kind: entry.source.as_ref().map(crate::imports::Source::kind),
            upload_path: entry.upload_path.clone(),
            selected: entry.selected,
            state: match &entry.status {
                Status::Ready => plan::RowState::Ready,
                Status::Error(reason) => plan::RowState::Error(reason.clone()),
                Status::Preparing | Status::Uploading => plan::RowState::Waiting,
                Status::Done(_) => plan::RowState::Done,
                Status::Cancelled => plan::RowState::Cancelled,
            },
        })
        .collect()
}

/// The uploader as [`State::step`] reads it.
pub fn snapshot(imports: &crate::imports::Imports, agent: Uuid) -> Snapshot {
    Snapshot {
        agent,
        open: imports.any_window_open(),
        loading: imports.pending_files.is_some(),
        busy: imports.busy(),
        rows: rows(imports),
        spent: imports.spent,
    }
}
