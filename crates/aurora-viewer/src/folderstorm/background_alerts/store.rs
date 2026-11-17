//! The shared choices' file, `background-alerts.json` in the main
//! profile's folder: every character's viewer reads it, and Preferences in
//! any of them writes it. A change made in another viewer is read again
//! within a few seconds; the last one written wins.

use super::Shared;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime};

pub const FILE: &str = "background-alerts.json";
/// How often the file is looked at for another viewer's change.
const CHECK_EVERY: Duration = Duration::from_secs(2);
/// A change is written once the edits stop for this long (a slider).
const SAVE_AFTER: Duration = Duration::from_millis(600);

/// The choices as last read or edited, and the file's state.
#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    value: Shared,
    /// The file's modification time when it was last read.
    stamp: Option<SystemTime>,
    checked: Option<Instant>,
    /// Edited and not written yet, since.
    edited: Option<Instant>,
    /// A write is under way: the file on the disk is older than `value`.
    writing: Arc<AtomicBool>,
}

impl Store {
    pub fn at(path: PathBuf) -> Self {
        let (value, stamp) = read(&path);
        Self {
            path,
            value,
            stamp,
            checked: None,
            edited: None,
            writing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The choices, read again when another viewer changed the file (at
    /// most every [`CHECK_EVERY`]; never over an edit not written yet).
    pub fn get(&mut self, now: Instant) -> &Shared {
        let due = self.checked.is_none_or(|at| now.saturating_duration_since(at) >= CHECK_EVERY);
        if due && self.edited.is_none() && !self.writing.load(Ordering::Acquire) {
            self.checked = Some(now);
            if modified(&self.path) != self.stamp {
                let (value, stamp) = read(&self.path);
                if value != self.value {
                    log::info!("background alerts: the shared choices changed in another window");
                }
                self.value = value;
                self.stamp = stamp;
            }
        }
        &self.value
    }

    /// Edits the choices; written shortly after ([`Store::flush`]).
    pub fn set(&mut self, value: Shared, now: Instant) {
        let value = value.cleaned();
        if value != self.value {
            self.value = value;
            self.edited = Some(now);
        }
    }

    /// What to write now, if an edit waits long enough: the file's path and
    /// its bytes.
    pub fn due(&mut self, now: Instant) -> Option<(PathBuf, Vec<u8>)> {
        let since = self.edited?;
        if now.saturating_duration_since(since) < SAVE_AFTER {
            return None;
        }
        self.edited = None;
        let bytes = serde_json::to_vec_pretty(&self.value).ok()?;
        Some((self.path.clone(), bytes))
    }

    /// Writes an edit that waits long enough, off the frame's thread.
    pub fn flush(&mut self, now: Instant) {
        let Some((path, bytes)) = self.due(now) else {
            return;
        };
        self.writing.store(true, Ordering::Release);
        let writing = self.writing.clone();
        let spawned = std::thread::Builder::new()
            .name("fs-alerts-save".into())
            .spawn(move || {
                write(&path, &bytes);
                writing.store(false, Ordering::Release);
            });
        if spawned.is_err() {
            self.writing.store(false, Ordering::Release);
            log::warn!("background alerts: the shared choices were not saved (no thread)");
        }
    }
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        log::warn!("background alerts: cannot create {}: {e}", dir.display());
        return;
    }
    if let Err(e) = crate::atomic_file::write(path, bytes) {
        log::warn!("background alerts: the shared choices were not saved: {e}");
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The file's choices, or the defaults when there is none or it does not
/// read (an older or broken file never stops the viewer).
fn read(path: &Path) -> (Shared, Option<SystemTime>) {
    let stamp = modified(path);
    let value = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Shared>(&bytes)
            .map(Shared::cleaned)
            .unwrap_or_else(|e| {
                log::warn!("background alerts: {} does not read ({e}): the defaults apply", path.display());
                Shared::default()
            }),
        Err(_) => Shared::default(),
    };
    (value, stamp)
}

/// This installation's shared choices.
pub fn shared() -> MutexGuard<'static, Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE
        .get_or_init(|| Mutex::new(Store::at(crate::folderstorm_characters::profile::main_config_dir().join(FILE))))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
