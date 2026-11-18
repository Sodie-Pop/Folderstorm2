//! An import: what to take from a backup ([`Plan`]), then, in this order,
//!
//! 1. a safety backup of the setup as it is, in the backups folder: the
//!    import can be undone by restoring it ([`prepare`], off the frame; if
//!    it cannot be written, nothing is imported);
//! 2. the files the chosen sections use, checked, into their folders;
//! 3. each chosen section, by Merge or Replace ([`apply`], on the frame).

use super::file::{self, Backup, Kind};
use super::section::{How, Mode, Target};
use super::{Account, KEEP_BEFORE_IMPORT, folder, secrets, sections};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One section of the file, as the dialog offers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    /// Taken.
    pub on: bool,
    pub mode: Mode,
    /// Whether Merge is offered.
    pub merges: bool,
    /// Entries pointing to an account's Inventory.
    pub inventory: usize,
}

/// A backup read, waiting for the user's choices.
#[derive(Clone, Debug)]
pub struct Plan {
    pub backup: Arc<Backup>,
    pub choices: Vec<Choice>,
    /// Leave out what points to the backup's account's Inventory.
    pub skip_inventory: bool,
    /// The account logged in when the file was read.
    pub target: Option<Account>,
    /// A restore of an automatic backup (Replace by default).
    pub restore: bool,
}

impl Plan {
    /// Every section of the file this version knows, taken; Merge where it
    /// is offered, Replace for a restore. What points to another account's
    /// Inventory is left out by default.
    pub fn new(backup: Backup, target: Option<Account>, restore: bool) -> Plan {
        let choices = backup
            .manifest
            .sections
            .iter()
            .filter_map(|entry| {
                let section = sections::find(&entry.id)?;
                let value = backup.values.get(&entry.id)?;
                Some(Choice {
                    id: entry.id.clone(),
                    label: section.label(),
                    on: true,
                    mode: if restore || !section.merges() { Mode::Replace } else { Mode::Merge },
                    merges: section.merges(),
                    inventory: section.inventory_refs(value),
                })
            })
            .collect();
        let mut plan = Plan {
            backup: Arc::new(backup),
            choices,
            skip_inventory: false,
            target,
            restore,
        };
        plan.skip_inventory = plan.other_account();
        plan
    }

    /// The account the backup was made by.
    pub fn source(&self) -> Option<&Account> {
        self.backup.manifest.account.as_ref()
    }

    /// The backup is of another account than the one logged in.
    pub fn other_account(&self) -> bool {
        matches!((self.source(), &self.target), (Some(source), Some(target)) if source.id != target.id)
    }

    /// Entries of the sections taken that point to an Inventory.
    pub fn inventory_refs(&self) -> usize {
        self.choices.iter().filter(|c| c.on).map(|c| c.inventory).sum()
    }

    pub fn any(&self) -> bool {
        self.choices.iter().any(|c| c.on)
    }

    fn how(&self, mode: Mode) -> How {
        How {
            mode,
            source: self.source().map(|a| a.id),
            target: self.target.as_ref().map(|a| a.id),
            skip_inventory: self.skip_inventory,
        }
    }

    /// The files of the sections taken.
    fn files(&self) -> Vec<String> {
        let taken: Vec<&str> = self.choices.iter().filter(|c| c.on).map(|c| c.id.as_str()).collect();
        let mut files: Vec<String> = self
            .backup
            .manifest
            .sections
            .iter()
            .filter(|entry| taken.contains(&entry.id.as_str()))
            .flat_map(|entry| entry.files.clone())
            .collect();
        files.sort();
        files.dedup();
        files
    }
}

/// Steps 1 and 2, off the frame: writes `safety` (the setup as it is,
/// captured on the frame) into the backups folder, then the files of the
/// sections taken. The safety backup's path.
pub fn prepare(mut safety: Backup, plan: &Plan, config: &Path) -> Result<PathBuf, String> {
    safety.read_files(config);
    let account = safety.manifest.account.as_ref().map(|a| a.id);
    let bytes = safety.to_zip().map_err(|e| {
        log::warn!("backup: the safety backup was not made ({e})");
        aurora_i18n::tr!("fs-backup-safety-failed").to_owned()
    })?;
    let path = folder::write(config, Kind::BeforeImport, &safety.manifest.created, account, &bytes).map_err(|e| {
        log::warn!("backup: the safety backup was not written ({e})");
        aurora_i18n::tr!("fs-backup-safety-failed").to_owned()
    })?;
    folder::prune(config, Kind::BeforeImport, account, KEEP_BEFORE_IMPORT);
    write_files(plan, config);
    Ok(path)
}

/// The files of the sections taken, into their folders; one already there
/// is the same picture (its name is its hash) and is left as it is.
fn write_files(plan: &Plan, config: &Path) {
    for name in plan.files() {
        let (Some((folder, plain)), Some(bytes)) = (file::split(&name), plan.backup.files.get(&name)) else {
            continue;
        };
        if !(folder.accepts)(plain, bytes) {
            continue;
        }
        let dir = (folder.path)(config);
        let path = dir.join(plain);
        if path.exists() {
            continue;
        }
        let written = std::fs::create_dir_all(&dir).and_then(|()| crate::atomic_file::write(&path, bytes));
        if let Err(e) = written {
            log::warn!("backup: a file of {} was not written ({e})", folder.name);
        }
    }
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The sections put back, by label.
    pub done: Vec<String>,
    /// The sections that failed, with why.
    pub failed: Vec<(String, String)>,
    /// What was left out or changed on the way.
    pub notes: Vec<String>,
    /// The safety backup, to undo the import.
    pub safety: Option<PathBuf>,
}

impl Report {
    /// The report in a few lines, for Preferences.
    pub fn text(&self) -> String {
        use aurora_i18n::tr;
        let mut lines = vec![tr!("fs-backup-imported", count = self.done.len())];
        for (label, why) in &self.failed {
            lines.push(tr!("fs-backup-section-failed", section = label.as_str(), reason = why.as_str()));
        }
        lines.extend(self.notes.iter().cloned());
        if self.safety.is_some() {
            lines.push(tr!("fs-backup-undo-hint").to_owned());
        }
        lines.join("\n")
    }
}

/// Step 3, on the frame: each section taken, by its mode.
pub fn apply(plan: &Plan, target: &mut Target) -> Report {
    let mut report = Report::default();
    for choice in plan.choices.iter().filter(|c| c.on) {
        let (Some(section), Some(value)) = (sections::find(&choice.id), plan.backup.values.get(&choice.id)) else {
            continue;
        };
        let mut value = value.clone();
        secrets::scrub(&mut value);
        let mode = if section.merges() { choice.mode } else { Mode::Replace };
        match section.import(target, value, &plan.how(mode)) {
            Ok(outcome) => {
                report.done.push(choice.label.clone());
                report.notes.extend(outcome.notes);
            }
            Err(why) => {
                log::warn!("backup: section {} not imported ({why})", choice.id);
                report.failed.push((choice.label.clone(), why));
            }
        }
    }
    log::info!(
        "backup: imported {} sections, {} failed, {} notes",
        report.done.len(),
        report.failed.len(),
        report.notes.len()
    );
    report
}
