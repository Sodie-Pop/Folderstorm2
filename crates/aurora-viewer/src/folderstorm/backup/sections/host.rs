//! The session host's characters (row 5.1, the host's `session-host.json`
//! in the main profile's folder): for each one its slot, its profile's
//! name, the name and colour the host shows for it, its standby and its
//! sound behind. Nothing else of that file: its windows and places belong to
//! this PC, and the controller access is given on each PC.
//!
//! The host keeps its file while it runs: an import waits until it is
//! closed (its lock is free).

use super::super::section::{Env, Exported, How, Mode, Outcome, Section, Target};
use ::folderstorm_session::host::{Config, SlotConfig};
use ::folderstorm_session::ipc::MAX_SLOTS;
use aurora_i18n::tr;
use serde_json::{Map, Value};
use std::path::Path;

/// What a character carries in a backup.
const KEYS: &[&str] = &["slot", "profile", "alias", "colour", "standby", "sound"];

pub struct Characters;

fn path(main: &Path) -> std::path::PathBuf {
    main.join(folderstorm_brand::SESSION_HOST_FILE)
}

/// The host's file, as the host reads it; None when there is none.
fn read(main: &Path) -> Option<Config> {
    let bytes = std::fs::read(path(main)).ok()?;
    serde_json::from_slice::<Config>(&bytes).ok()
}

/// Whether a session host has its settings folder now.
fn host_running(main: &Path) -> bool {
    let Ok(file) = std::fs::OpenOptions::new()
        .write(true)
        .open(main.join(folderstorm_brand::SESSION_HOST_LOCK))
    else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(std::fs::TryLockError::Error(_)) => false,
    }
}

/// One character as a backup has it.
fn entry(slot: &SlotConfig) -> Value {
    match serde_json::to_value(slot) {
        Ok(Value::Object(all)) => Value::Object(all.into_iter().filter(|(k, _)| KEYS.contains(&k.as_str())).collect()),
        _ => Value::Null,
    }
}

impl Section for Characters {
    fn id(&self) -> &'static str {
        "session_host"
    }

    fn label(&self) -> String {
        tr!("fs-backup-section-session-host").to_owned()
    }

    fn export(&self, env: &Env) -> Option<Exported> {
        let config = read(env.main_config)?;
        let list: Vec<Value> = config.slots.iter().map(entry).filter(Value::is_object).collect();
        Some(Exported::value(Value::Array(list)))
    }

    fn import(&self, target: &mut Target, value: Value, how: &How) -> Result<Outcome, String> {
        let Value::Array(items) = value else {
            return Err(tr!("fs-backup-unreadable").to_owned());
        };
        if host_running(target.main_config) {
            return Err(tr!("fs-backup-host-running").to_owned());
        }
        let mut out = Outcome::default();
        let mut config = read(target.main_config).unwrap_or_default();
        let mut incoming = Vec::new();
        for item in items {
            let Value::Object(item) = item else { continue };
            let only: Map<String, Value> = item.into_iter().filter(|(k, _)| KEYS.contains(&k.as_str())).collect();
            if let Ok(slot) = serde_json::from_value::<SlotConfig>(Value::Object(only)) {
                incoming.push(slot);
            }
        }
        let same = |a: &SlotConfig, b: &SlotConfig| a.profile.trim().eq_ignore_ascii_case(b.profile.trim());
        let wanted = incoming.len();
        match how.mode {
            Mode::Replace => {
                let before = std::mem::take(&mut config.slots);
                for mut slot in incoming {
                    // what this PC keeps of a character it has already
                    if let Some(mine) = before.iter().find(|m| same(m, &slot)) {
                        let mut kept = mine.clone();
                        (kept.slot, kept.alias, kept.colour, kept.standby, kept.sound) =
                            (slot.slot, slot.alias.take(), slot.colour, slot.standby, slot.sound);
                        slot = kept;
                    }
                    config.slots.push(slot);
                }
            }
            Mode::Merge => {
                for mut slot in incoming {
                    if config.slots.iter().any(|m| same(m, &slot)) {
                        continue;
                    }
                    if config.slots.iter().any(|m| m.slot == slot.slot) {
                        let Some(free) = (0..MAX_SLOTS as u8).find(|n| !config.slots.iter().any(|m| m.slot == *n)) else {
                            continue;
                        };
                        slot.slot = free;
                    }
                    config.slots.push(slot);
                }
            }
        }
        let config = config.cleaned();
        let kept = config.slots.len();
        let written = serde_json::to_vec_pretty(&config)
            .map_err(std::io::Error::other)
            .and_then(|bytes| crate::atomic_file::write(&path(target.main_config), &bytes));
        if let Err(e) = written {
            log::warn!("backup: the session host's file was not written ({e})");
            return Err(tr!("fs-backup-write-failed").to_owned());
        }
        if how.mode == Mode::Replace && kept < wanted {
            out.note(tr!("fs-backup-note-limit", count = wanted - kept));
        }
        Ok(out)
    }
}
