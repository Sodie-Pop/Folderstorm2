//! The backup's work in the viewer: the file dialogs and the disk, each on
//! a thread of its own, and [`frame`], the app's one hook, which tells the
//! backup the account logged in, takes the threads' answers (an import is
//! applied there, to the settings) and makes the daily backup.
//!
//! Preferences (`ui/options/pages/folderstorm_backup.rs`) only draws the
//! [`State`] and asks for the work through the functions here.

use super::apply::{self, Plan};
use super::file::{Backup, Kind};
use super::folder::{self, Listed};
use super::section::{Env, Target};
use super::{Account, Folders};
use crate::settings::Settings;
use aurora_i18n::tr;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

/// The work running on a thread.
enum Job {
    /// Export…: the file written (None: the dialog was cancelled).
    Export(Receiver<Result<Option<PathBuf>, String>>),
    /// Import… or Restore: the backup read (None: cancelled).
    Read {
        answer: Receiver<Result<Option<Backup>, String>>,
        restore: bool,
    },
    /// An import confirmed: the safety backup and the files written.
    Prepare {
        answer: Receiver<Result<PathBuf, String>>,
        plan: Box<Plan>,
    },
    /// The daily backup.
    Daily(Receiver<Result<Option<PathBuf>, String>>),
}

/// What Preferences shows, and the work going on.
#[derive(Default)]
pub struct State {
    /// The account logged in, told by the app each frame.
    pub account: Option<Account>,
    /// What the last export did.
    pub export_status: String,
    /// What the last import did.
    pub import_status: String,
    /// A backup read, waiting in the import dialog.
    pub plan: Option<Plan>,
    job: Option<Job>,
    /// The backups folder's list; None: to read again.
    list: Option<Vec<Listed>>,
    /// The account and day the daily backup was last looked at.
    daily_checked: Option<(Option<uuid::Uuid>, String)>,
    /// `AURORA_DEMO_BACKUP` has made its setup (`super::demo`).
    pub(super) demo_done: bool,
}

impl State {
    /// A file dialog or the disk is busy.
    pub fn busy(&self) -> bool {
        self.job.as_ref().is_some_and(|job| !matches!(job, Job::Daily(_)))
    }
}

pub type Shared = Arc<Mutex<State>>;

/// The backup's state for this viewer.
pub fn state(ctx: &egui::Context) -> Shared {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Shared>(egui::Id::unique("folderstorm_backup")).clone())
}

/// Runs `f` with the state; nothing when it is poisoned (a thread panicked
/// holding it).
pub fn with<R>(ctx: &egui::Context, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    let shared = state(ctx);
    let mut state = shared.lock().ok()?;
    Some(f(&mut state))
}

fn spawn<T: Send + 'static>(ctx: &egui::Context, name: &str, work: impl FnOnce() -> T + Send + 'static) -> Option<Receiver<T>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
        let _ = tx.send(work());
        ctx.request_repaint();
    });
    match spawned {
        Ok(_) => Some(rx),
        Err(e) => {
            log::warn!("backup: no thread for {name} ({e})");
            None
        }
    }
}

fn dialog() -> rfd::FileDialog {
    rfd::FileDialog::new()
        .set_title(tr!("fs-backup-group"))
        .add_filter(tr!("fs-backup-file-type"), &[super::file::EXTENSION])
}

/// The setup as it is, for `account`.
pub fn capture(settings: &Settings, account: Option<Account>, folders: &Folders, kind: Kind) -> Backup {
    let env = Env {
        settings,
        account: account.as_ref().map(|a| a.id),
        main_config: &folders.main_config,
    };
    Backup::capture(&env, kind, account)
}

/// Export…: the setup taken now, then the file dialog and the writing on a
/// thread.
pub fn export(ctx: &egui::Context, settings: &Settings) {
    let folders = Folders::of_this_viewer();
    let shared = state(ctx);
    let Ok(mut st) = shared.lock() else { return };
    if st.busy() {
        return;
    }
    st.export_status.clear();
    let mut backup = capture(settings, st.account.clone(), &folders, Kind::Manual);
    let date = backup.manifest.created.get(..10).unwrap_or("backup").to_owned();
    let name = format!("{}-setup-{date}.{}", folderstorm_brand::SLUG, super::file::EXTENSION);
    st.job = spawn(ctx, "folderstorm-backup-export", move || {
        let Some(path) = dialog().set_file_name(name).save_file() else {
            return Ok(None);
        };
        backup.read_files(&folders.config);
        let written = backup
            .to_zip()
            .and_then(|bytes| crate::atomic_file::write(&path, &bytes));
        match written {
            Ok(()) => Ok(Some(path)),
            Err(e) => {
                log::warn!("backup: the export was not written ({e})");
                Err(tr!("fs-backup-write-failed").to_owned())
            }
        }
    })
    .map(Job::Export);
}

/// Reads a backup file, bounded.
fn read_file(path: &std::path::Path) -> Result<Backup, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(super::file::MAX_FILE + 1).read_to_end(&mut bytes))
        .map_err(|e| {
            log::warn!("backup: the file was not read ({e})");
            tr!("fs-backup-read-failed").to_owned()
        })?;
    Backup::from_zip(&bytes).map_err(|e| e.text().to_owned())
}

/// Import…: the file dialog and the reading on a thread; the dialog of
/// Preferences then shows what the file holds.
pub fn import(ctx: &egui::Context) {
    start_read(ctx, None);
}

/// Restore of an automatic backup: the same, without the file dialog.
pub fn restore(ctx: &egui::Context, path: PathBuf) {
    start_read(ctx, Some(path));
}

fn start_read(ctx: &egui::Context, path: Option<PathBuf>) {
    let shared = state(ctx);
    let Ok(mut st) = shared.lock() else { return };
    if st.busy() {
        return;
    }
    st.import_status.clear();
    let restore = path.is_some();
    let answer = spawn(ctx, "folderstorm-backup-read", move || {
        let Some(path) = path.or_else(|| dialog().pick_file()) else {
            return Ok(None);
        };
        read_file(&path).map(Some)
    });
    st.job = answer.map(|answer| Job::Read { answer, restore });
}

/// The import dialog's Import: the setup as it is taken now for the
/// safety backup, written with the files on a thread; the sections follow
/// in [`frame`].
pub fn confirm(ctx: &egui::Context, settings: &Settings) {
    let folders = Folders::of_this_viewer();
    let shared = state(ctx);
    let Ok(mut st) = shared.lock() else { return };
    if st.busy() {
        return;
    }
    let Some(plan) = st.plan.take() else { return };
    if !plan.any() {
        return;
    }
    let safety = capture(settings, st.account.clone(), &folders, Kind::BeforeImport);
    let for_thread = plan.clone();
    let answer = spawn(ctx, "folderstorm-backup-import", move || apply::prepare(safety, &for_thread, &folders.config));
    st.job = answer.map(|answer| Job::Prepare {
        answer,
        plan: Box::new(plan),
    });
}

/// Open backups folder.
pub fn open_folder() {
    let dir = folder::dir(&crate::settings::config_dir());
    let opened = std::fs::create_dir_all(&dir).and_then(|()| crate::open_path::open(&dir));
    if let Err(e) = opened {
        log::warn!("backup: the backups folder was not opened ({e})");
    }
}

/// The backups folder's list, read when it changed.
pub fn list(ctx: &egui::Context) -> Vec<Listed> {
    with(ctx, |st| {
        st.list
            .get_or_insert_with(|| folder::list(&crate::settings::config_dir()))
            .clone()
    })
    .unwrap_or_default()
}

/// Once a frame, from the app (`App::draw_options`): the account logged
/// in, the threads' answers, the daily backup. True when an import changed
/// the settings (to be saved).
pub fn frame(ctx: &egui::Context, settings: &mut Settings, login: Option<&aurora_net::LoginResponse>) -> bool {
    let shared = state(ctx);
    let Ok(mut st) = shared.lock() else { return false };
    st.account = login.map(Account::of);
    let mut changed = super::demo::frame(ctx, &mut st, settings);
    changed |= poll(&mut st, settings);
    daily(ctx, &mut st, settings);
    changed
}

fn poll(st: &mut State, settings: &mut Settings) -> bool {
    let Some(job) = st.job.take() else { return false };
    match job {
        Job::Export(answer) => match answer.try_recv() {
            Ok(Ok(Some(path))) => {
                st.export_status = tr!("fs-backup-exported", path = path.display().to_string());
            }
            Ok(Ok(None)) => {}
            Ok(Err(why)) => st.export_status = why,
            Err(TryRecvError::Empty) => st.job = Some(Job::Export(answer)),
            Err(TryRecvError::Disconnected) => {}
        },
        Job::Read { answer, restore } => match answer.try_recv() {
            Ok(Ok(Some(backup))) => st.plan = Some(Plan::new(backup, st.account.clone(), restore)),
            Ok(Ok(None)) => {}
            Ok(Err(why)) => st.import_status = why,
            Err(TryRecvError::Empty) => st.job = Some(Job::Read { answer, restore }),
            Err(TryRecvError::Disconnected) => {}
        },
        Job::Prepare { answer, plan } => match answer.try_recv() {
            Ok(Ok(safety)) => {
                let folders = Folders::of_this_viewer();
                let mut target = Target {
                    settings,
                    main_config: &folders.main_config,
                };
                let mut report = apply::apply(&plan, &mut target);
                report.safety = Some(safety);
                st.import_status = report.text();
                st.list = None;
                return true;
            }
            Ok(Err(why)) => {
                st.import_status = why;
                st.list = None;
            }
            Err(TryRecvError::Empty) => st.job = Some(Job::Prepare { answer, plan }),
            Err(TryRecvError::Disconnected) => {}
        },
        Job::Daily(answer) => match answer.try_recv() {
            Ok(_) | Err(TryRecvError::Disconnected) => st.list = None,
            Err(TryRecvError::Empty) => st.job = Some(Job::Daily(answer)),
        },
    }
    false
}

/// The daily backup: once a day per account, while logged in, when chosen.
fn daily(ctx: &egui::Context, st: &mut State, settings: &Settings) {
    if !settings.folderstorm_backup.daily || st.job.is_some() {
        return;
    }
    let Some(account) = st.account.clone() else { return };
    let stamp = crate::snapshot::local_stamp();
    let today = stamp.get(..10).unwrap_or_default().to_owned();
    let key = (Some(account.id), today.clone());
    if st.daily_checked.as_ref() == Some(&key) {
        return;
    }
    st.daily_checked = Some(key);
    let folders = Folders::of_this_viewer();
    if folder::has_daily(&folders.config, Some(account.id), &today) {
        return;
    }
    let mut backup = capture(settings, Some(account.clone()), &folders, Kind::Daily);
    let keep = settings.folderstorm_backup.kept();
    st.job = spawn(ctx, "folderstorm-backup-daily", move || {
        backup.read_files(&folders.config);
        let bytes = backup.to_zip().map_err(|e| e.to_string())?;
        let path = folder::write(&folders.config, Kind::Daily, &stamp, Some(account.id), &bytes).map_err(|e| {
            log::warn!("backup: the daily backup was not written ({e})");
            e.to_string()
        })?;
        folder::prune(&folders.config, Kind::Daily, Some(account.id), keep);
        log::info!("backup: daily backup written");
        Ok(Some(path))
    })
    .map(Job::Daily);
}
