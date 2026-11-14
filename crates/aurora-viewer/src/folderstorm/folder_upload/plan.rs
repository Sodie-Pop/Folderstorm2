//! The pure part of a folder upload: what the chosen disk folder holds that
//! will not be uploaded, the preview shown before the user confirms, the
//! routing given to the uploader for this upload only, and the receipt.
//!
//! None of it touches the network or the uploader itself: the uploader
//! (`crate::imports`) decodes, prices, creates the folders and uploads.
//! These types describe its state as plain data, for the confirmation
//! dialog now and for a local-assistant tool later.

use crate::imports::{Preferences, folders::Layout};
use aurora_net::inventory::upload::Kind;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use uuid::Uuid;

/// The extensions the uploader's folder scan takes, in the order of
/// `imports::folders::discover` (keep the two lists the same).
pub const SUPPORTED: &[&str] = &[
    "tga", "bmp", "jpg", "jpeg", "png", "j2c", "j2k", "jp2", "wav", "bvh", "anim", "dae", "gltf", "glb",
];

/// Entries looked at by [`scan`] before it stops (the uploader's own scan
/// stops at 100,000 too).
pub const SCAN_LIMIT: usize = 100_000;

/// Directory levels [`scan`] goes down (the uploader's limit).
const DEPTH_LIMIT: usize = 64;

/// A file the uploader's folder scan takes.
pub fn supported(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| SUPPORTED.iter().any(|ext| e.eq_ignore_ascii_case(ext)))
}

/// System files nobody means to upload; left out of the list of skipped files.
fn noise(name: &str) -> bool {
    name.starts_with('.') || name.eq_ignore_ascii_case("thumbs.db") || name.eq_ignore_ascii_case("desktop.ini")
}

/// What the chosen folder holds that will not reach the inventory.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Files of an unsupported type, relative to the chosen folder
    /// (`Hat/notes.txt`), in name order.
    pub files: Vec<String>,
    /// Folders with no supported file anywhere inside: the uploader creates
    /// a folder only for the files it uploads.
    pub empty_folders: usize,
    /// Shortcuts, symbolic links and reparse points, never followed.
    pub links: usize,
    /// The scan stopped at [`SCAN_LIMIT`] entries or the depth limit.
    pub truncated: bool,
}

/// Lists what the uploader will leave out of `root`, with its rules: links
/// and reparse points are not followed, unsupported types are left out,
/// folders without a supported file are not created. Runs on a worker.
pub fn scan(root: &Path, limit: usize) -> Skipped {
    struct Dir {
        parent: Option<usize>,
        supported: usize,
    }
    let mut skipped = Skipped::default();
    let mut dirs = vec![Dir {
        parent: None,
        supported: 0,
    }];
    let mut pending = vec![(root.to_owned(), 0usize, Vec::<String>::new())];
    let mut visited = 0usize;
    while let Some((directory, index, relative)) = pending.pop() {
        let Ok(children) = std::fs::read_dir(&directory) else {
            continue;
        };
        for child in children.flatten() {
            visited += 1;
            if visited > limit {
                skipped.truncated = true;
                pending.clear();
                break;
            }
            let Ok(meta) = child.path().symlink_metadata() else {
                continue;
            };
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    skipped.links += 1;
                    continue;
                }
            }
            if meta.file_type().is_symlink() {
                skipped.links += 1;
                continue;
            }
            let name = child.file_name().to_string_lossy().into_owned();
            if meta.is_dir() {
                if relative.len() + 1 > DEPTH_LIMIT {
                    skipped.truncated = true;
                    continue;
                }
                dirs.push(Dir {
                    parent: Some(index),
                    supported: 0,
                });
                let mut path = relative.clone();
                path.push(name);
                pending.push((child.path(), dirs.len() - 1, path));
            } else if meta.is_file() {
                if supported(&child.path()) {
                    dirs[index].supported += 1;
                } else if !noise(&name) {
                    let mut path = relative.clone();
                    path.push(name);
                    skipped.files.push(path.join("/"));
                }
            }
        }
    }
    // Children always come after their parent: add each folder's count to
    // its parent, from the last to the first.
    for index in (1..dirs.len()).rev() {
        if let Some(parent) = dirs[index].parent {
            dirs[parent].supported += dirs[index].supported;
        }
    }
    skipped.empty_folders = dirs.iter().skip(1).filter(|dir| dir.supported == 0).count();
    skipped.files.sort_by_key(|file| file.to_lowercase());
    skipped
}

/// The uploader's routing settings, which a folder upload sets for its own
/// batch and puts back afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routing {
    pub use_default_folders: bool,
    pub include_source_folder: bool,
    pub batch_folder: Uuid,
    pub folder_layout: Option<Layout>,
}

impl Routing {
    pub fn read(prefs: &Preferences) -> Self {
        Self {
            use_default_folders: prefs.use_default_folders,
            include_source_folder: prefs.include_source_folder,
            batch_folder: prefs.batch_folder,
            folder_layout: prefs.folder_layout,
        }
    }

    pub fn write(&self, prefs: &mut Preferences) {
        prefs.use_default_folders = self.use_default_folders;
        prefs.include_source_folder = self.include_source_folder;
        prefs.batch_folder = self.batch_folder;
        prefs.folder_layout = self.folder_layout;
    }

    /// Folderstorm (C++)'s routing: one new folder named after the disk
    /// folder inside `destination`, its sub-folders below it.
    pub fn mirror_into(destination: Uuid) -> Self {
        Self {
            use_default_folders: false,
            include_source_folder: true,
            batch_folder: destination,
            folder_layout: Some(Layout::Mirror),
        }
    }
}

/// Puts the user's routing back, unless they changed it during the upload
/// (then their new choice stays). Returns whether it was put back.
pub fn restore(prefs: &mut Preferences, saved: &Routing, applied: &Routing) -> bool {
    if Routing::read(prefs) != *applied {
        return false;
    }
    saved.write(prefs);
    true
}

/// Where a draft of the uploader stands, as plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowState {
    Ready,
    Error(String),
    /// Waiting in the paid queue or being uploaded.
    Waiting,
    Done,
    Cancelled,
}

/// One draft of the uploader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: Uuid,
    /// The inventory item's name.
    pub name: String,
    pub kind: Option<Kind>,
    /// The inventory folders below the destination, `None` when the draft
    /// goes to a default folder.
    pub upload_path: Option<Vec<String>>,
    pub selected: bool,
    pub state: RowState,
}

/// What the confirmation shows before anything is created or paid.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Preview {
    /// Selected, ready drafts: what Upload sends.
    pub files: usize,
    /// Those drafts by type, in [`KINDS`] order, without the empty ones.
    pub by_kind: Vec<(Kind, usize)>,
    /// Inventory folders the upload creates.
    pub new_folders: usize,
    /// Top folders of the plan whose name a folder of the destination
    /// already has: a second folder of that name is created beside it.
    pub existing_names: Vec<String>,
    /// Item names given to two drafts or more in the same new folder.
    pub shared_names: Vec<String>,
    /// Selected drafts in error, which hold the upload back.
    pub errors: usize,
    pub cost: Option<i32>,
    pub balance: Option<i32>,
}

/// The order of the types in a preview.
pub const KINDS: [Kind; 6] = [
    Kind::Texture,
    Kind::Sound,
    Kind::Animation,
    Kind::Material,
    Kind::Mesh,
    Kind::Snapshot,
];

impl Preview {
    /// `existing` holds the names of the folders already in the destination;
    /// `cost` is the uploader's total for the batch (`Imports::total`).
    pub fn build(rows: &[Row], existing: &[String], cost: Option<i32>, balance: Option<i32>) -> Self {
        let ready: Vec<&Row> = rows.iter().filter(|row| row.selected && row.state == RowState::Ready).collect();
        let mut folders = BTreeSet::new();
        let mut names: BTreeMap<(Vec<String>, String), usize> = BTreeMap::new();
        for row in &ready {
            let path = row.upload_path.clone().unwrap_or_default();
            for depth in 1..=path.len() {
                folders.insert(path[..depth].to_vec());
            }
            *names.entry((path, row.name.to_lowercase())).or_default() += 1;
        }
        let tops: BTreeSet<&String> = folders.iter().filter(|path| path.len() == 1).map(|path| &path[0]).collect();
        let existing_names = tops
            .into_iter()
            .filter(|top| existing.iter().any(|name| name.eq_ignore_ascii_case(top)))
            .cloned()
            .collect();
        let mut shared_names: Vec<String> = Vec::new();
        for row in &ready {
            let path = row.upload_path.clone().unwrap_or_default();
            if names.get(&(path, row.name.to_lowercase())).is_some_and(|count| *count > 1)
                && !shared_names.iter().any(|name| name.eq_ignore_ascii_case(&row.name))
            {
                shared_names.push(row.name.clone());
            }
        }
        Self {
            files: ready.len(),
            by_kind: KINDS
                .iter()
                .map(|kind| (*kind, ready.iter().filter(|row| row.kind == Some(*kind)).count()))
                .filter(|(_, count)| *count > 0)
                .collect(),
            new_folders: folders.len(),
            existing_names,
            shared_names,
            errors: rows
                .iter()
                .filter(|row| row.selected && matches!(row.state, RowState::Error(_)))
                .count(),
            cost,
            balance,
        }
    }

    /// The balance once the upload is paid, when both are known.
    pub fn balance_after(&self) -> Option<i32> {
        Some(self.balance?.saturating_sub(self.cost?))
    }

    /// Enough L$ for the whole batch (a free batch always is).
    pub fn affordable(&self) -> bool {
        match self.cost {
            Some(0) => true,
            Some(cost) => self.balance.is_some_and(|balance| balance >= cost),
            None => false,
        }
    }
}

/// The outcome of one confirmed upload, file by file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub uploaded: Vec<String>,
    /// Name and reason.
    pub failed: Vec<(String, String)>,
    pub cancelled: Vec<String>,
    /// L$ the server confirmed for this upload.
    pub spent: i32,
}

impl Receipt {
    pub fn files(&self) -> usize {
        self.uploaded.len() + self.failed.len() + self.cancelled.len()
    }
}

/// Follows the drafts sent by one confirmation until the queue is idle.
#[derive(Debug, Default, Clone)]
pub struct Tracker {
    order: Vec<Uuid>,
    rows: BTreeMap<Uuid, (String, RowState)>,
}

impl Tracker {
    /// The drafts the confirmation put in the queue.
    pub fn begin(rows: &[Row]) -> Self {
        let mut tracker = Self::default();
        for row in rows.iter().filter(|row| row.state == RowState::Waiting) {
            tracker.order.push(row.id);
            tracker.rows.insert(row.id, (row.name.clone(), row.state.clone()));
        }
        tracker
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Takes the drafts' new states. The uploader drops a draft from its
    /// list once the server acknowledged it, and only then: a draft that was
    /// waiting and is gone was uploaded.
    pub fn observe(&mut self, rows: &[Row]) {
        for (id, (name, state)) in &mut self.rows {
            match rows.iter().find(|row| row.id == *id) {
                Some(row) => {
                    name.clone_from(&row.name);
                    *state = row.state.clone();
                }
                None if *state == RowState::Waiting => *state = RowState::Done,
                None => {}
            }
        }
    }

    pub fn receipt(&self, spent: i32) -> Receipt {
        let mut receipt = Receipt {
            spent,
            ..Default::default()
        };
        for id in &self.order {
            let Some((name, state)) = self.rows.get(id) else {
                continue;
            };
            match state {
                RowState::Done => receipt.uploaded.push(name.clone()),
                RowState::Error(reason) => receipt.failed.push((name.clone(), reason.clone())),
                // Still waiting when the queue stopped: it was not sent.
                RowState::Cancelled | RowState::Waiting | RowState::Ready => receipt.cancelled.push(name.clone()),
            }
        }
        receipt
    }
}
