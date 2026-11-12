//! `aurora-viewer --session-host`: the characters' host (session-host
//! plan, checkpoint 2). A small window that starts up to five characters,
//! each a viewer of its own (`--profile <name> --managed`), brings one of
//! them forward at a time, and watches over them. The decisions are the
//! `folderstorm-session` crate's desk (`host::Desk`); this module carries
//! them out: processes and their links ([`process`]), the settings file,
//! and the window ([`ui`]).
//!
//! Checkpoint 5: a background character's monitor shows the pictures its
//! viewer sends ([`monitors`]); the host keeps the latest one only, in
//! memory, for as long as the monitor is open.
//!
//! Checkpoint 6: the chat, the characters' controls and the monitors can
//! each have a window of their own (`ui/panels.rs`), remembered in the
//! settings file.
//!
//! Every window is drawn on the processor, like the crash window
//! (`crash/software.rs`): the host opens no graphics device, so it takes
//! nothing from the characters' card and no driver trouble reaches it.
//!
//! The host never logs a character in and never sees a password: each
//! viewer shows its own login screen. Closing or losing the host ends no
//! viewer: their command pipes close and they go on by themselves.

mod chat_ui;
mod comfort;
mod hotkeys;
mod monitors;
mod process;
mod ui;

use ::folderstorm_session::chat::{self, Chat};
use ::folderstorm_session::host::{Config, Desk, Effect, Note, Phase};
use ::folderstorm_session::ipc::{MAX_SLOTS, Screen, ToViewer};
use crossbeam_channel::{Receiver, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// `AURORA_DEMO_SESSION_HOST=<n>`, demo only: the host starts with `n`
/// demo characters (`demo-1` to `demo-n`, 1 to 5) and starts their viewers.
pub const DEMO_VARIABLE: &str = "AURORA_DEMO_SESSION_HOST";

/// `AURORA_DEMO_SESSION_HOST_PANELS=chat,controls,monitors` (or `all`),
/// demo only, with [`DEMO_VARIABLE`]: the host starts with those panels
/// in windows of their own (checkpoint 6).
pub const DEMO_PANELS_VARIABLE: &str = "AURORA_DEMO_SESSION_HOST_PANELS";

/// How long a change of the settings waits before it is written (a window
/// being dragged moves the stage many times).
const SAVE_AFTER: Duration = Duration::from_secs(1);

/// The demo's number of characters, when this process is the demo's host.
fn demo() -> Option<usize> {
    static DEMO: OnceLock<Option<usize>> = OnceLock::new();
    *DEMO.get_or_init(|| {
        std::env::var_os("AURORA_DEMO")?;
        let cli = crate::cli::get();
        // (a viewer the host started is never a host itself)
        if cli.managed || (cli.profile.is_some() && !cli.session_host) {
            return None;
        }
        demo_count(&std::env::var(DEMO_VARIABLE).ok()?)
    })
}

fn demo_count(value: &str) -> Option<usize> {
    value.trim().parse::<usize>().ok().filter(|n| (1..=MAX_SLOTS).contains(n))
}

/// Whether this process is the characters' host and not a viewer.
pub fn wanted() -> bool {
    if cfg!(test) {
        return false;
    }
    (crate::cli::get().session_host && !crate::cli::get().managed) || demo().is_some()
}

/// The host's hold on its settings folder.
static LOCK: OnceLock<std::fs::File> = OnceLock::new();

/// One host per settings folder: false (and the user is told) when one is
/// already open.
pub fn take_lock() -> bool {
    let dir = crate::settings::config_dir();
    let file = std::fs::create_dir_all(&dir).and_then(|()| {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(folderstorm_brand::SESSION_HOST_LOCK))
    });
    match file.map(|file| file.try_lock().map(|()| file)) {
        Ok(Ok(file)) => {
            let _ = LOCK.set(file);
            true
        }
        Ok(Err(std::fs::TryLockError::WouldBlock)) => {
            super::profile::tell(aurora_i18n::tr!("folderstorm-host-already-open"));
            false
        }
        Ok(Err(std::fs::TryLockError::Error(e))) | Err(e) => {
            // (no log yet: the console)
            eprintln!("session host lock not taken ({e}): starting without it");
            true
        }
    }
}

fn config_path() -> std::path::PathBuf {
    crate::settings::config_dir().join(folderstorm_brand::SESSION_HOST_FILE)
}

/// The saved characters and stage; the defaults when there is no file or
/// it does not read (it is then left as it is until the next change).
fn load_config() -> Config {
    match std::fs::read(config_path()) {
        Ok(bytes) => serde_json::from_slice::<Config>(&bytes).unwrap_or_else(|e| {
            log::warn!("session host: the settings file does not read ({e}), starting empty");
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

fn save_config(config: &Config) {
    let written = serde_json::to_vec_pretty(config)
        .map_err(std::io::Error::other)
        .and_then(|bytes| crate::atomic_file::write(&config_path(), &bytes));
    if let Err(e) = written {
        log::warn!("session host: the settings file was not written ({e})");
    }
}

/// The host without its window: the desk, the viewers it started and
/// what they report.
struct Host {
    desk: Desk,
    /// The characters' conversations and what is typed for them here.
    chat: Chat,
    /// A message came for a background character since the window looked.
    alert: bool,
    /// Checkpoint 4: the global hotkeys.
    comfort: comfort::Comfort,
    viewers: [Option<process::Viewer>; MAX_SLOTS],
    /// Checkpoint 5: the latest picture of each character's monitor.
    screens: [monitors::Screen; MAX_SLOTS],
    events: Receiver<process::Event>,
    events_tx: Sender<process::Event>,
    /// The demo's host: its characters are not saved.
    demo: bool,
    /// The settings changed at that time and are not written yet.
    unsaved: Option<Instant>,
    /// "Quit all" is done: the window closes.
    exit: bool,
}

impl Host {
    fn new() -> Self {
        let demo = demo();
        let config = demo.map_or_else(load_config, Config::demo);
        let (events_tx, events) = crossbeam_channel::unbounded();
        let mut host = Self {
            desk: Desk::new(config),
            chat: Chat::default(),
            alert: false,
            comfort: Default::default(),
            viewers: Default::default(),
            screens: Default::default(),
            events,
            events_tx,
            demo: demo.is_some(),
            unsaved: None,
            exit: false,
        };
        if host.demo {
            let slots: Vec<usize> = host.desk.slots().map(|(slot, _)| slot).collect();
            for slot in slots {
                host.desk.launch(slot, slot == 0);
                // the demo shows the monitors without a click
                host.desk.set_monitor(slot, Some(Default::default()));
            }
            let out = std::env::var(DEMO_PANELS_VARIABLE).unwrap_or_default();
            for panel in ::folderstorm_session::panels::parse_list(&out) {
                host.desk.set_panel_out(panel, true);
            }
        }
        host
    }

    /// What happened since the last call, then what the desk asks for.
    fn tick(&mut self, now: Instant) {
        let events: Vec<process::Event> = self.events.try_iter().collect();
        for process::Event { slot, launch, kind } in events {
            // the chat takes nothing of a viewer the desk does not manage
            // (let go, ended, or an earlier launch)
            let managed = self
                .desk
                .slot(slot)
                .is_some_and(|s| s.phase == Phase::Running && s.launch() == launch);
            match kind {
                process::Kind::Connected => self.desk.connected(slot, launch, now),
                process::Kind::Refused(refusal) => self.desk.refused(slot, launch, refusal),
                process::Kind::Status(status) => {
                    if managed {
                        self.chat.status(slot, launch, status.generation, status.screen == Screen::World);
                    }
                    self.desk.status(slot, launch, *status, now);
                }
                process::Kind::Heartbeat(count) => self.desk.heartbeat(slot, launch, count, now),
                process::Kind::Chat(batch) if managed => self.chat.batch(slot, launch, *batch),
                process::Kind::Attention(list) if managed => self.chat.attention_list(slot, launch, *list),
                process::Kind::SendResult(id, outcome) if managed => self.chat.result(slot, launch, id, outcome),
                process::Kind::Chat(_) | process::Kind::Attention(_) | process::Kind::SendResult(..) => {}
                process::Kind::Unreachable => self.desk.unreachable(slot, launch),
            }
        }
        for slot in 0..MAX_SLOTS {
            let Some(viewer) = &mut self.viewers[slot] else {
                continue;
            };
            if let Some(clean) = viewer.exited() {
                log::info!(
                    "session host: character {} ended ({})",
                    slot + 1,
                    if clean { "cleanly" } else { "not cleanly" }
                );
                self.desk.exited(slot, viewer.launch, clean, now);
                // what it was asked to say and did not answer is uncertain
                self.chat.ended(slot, viewer.launch);
                self.viewers[slot] = None;
            }
        }
        self.desk.tick(now);
        self.carry_out(now);
        self.tick_monitors(now);
        self.chat.set_active(self.desk.active());
        self.chat.tick(now);
        self.carry_out_chat();
        if self.unsaved.is_some_and(|since| now.duration_since(since) >= SAVE_AFTER) {
            self.save();
        }
    }

    fn carry_out(&mut self, now: Instant) {
        for effect in self.desk.take_effects() {
            match effect {
                Effect::Spawn { slot, launch } => self.spawn(slot, launch),
                Effect::Send { slot, launch, message } => {
                    if let Some(viewer) = self.viewers[slot].as_ref().filter(|v| v.launch == launch)
                        && !viewer.send(message)
                    {
                        log::warn!("session host: character {} does not read its commands", slot + 1);
                    }
                }
                Effect::AllowForeground { slot } => {
                    if let Some(viewer) = &self.viewers[slot] {
                        allow_foreground(viewer.pid());
                    }
                }
                Effect::Kill { slot } => {
                    if let Some(viewer) = &mut self.viewers[slot] {
                        log::warn!("session host: ending character {} at the user's request", slot + 1);
                        viewer.kill();
                    }
                }
                Effect::LetGo { slot, launch } => {
                    if let Some(viewer) = self.viewers[slot].as_mut().filter(|v| v.launch == launch) {
                        log::info!("session host: character {} is not managed: let go", slot + 1);
                        viewer.let_go();
                    }
                    // nothing is said to, or kept waiting for, a viewer let go
                    self.chat.ended(slot, launch);
                }
                Effect::Save => self.unsaved = self.unsaved.or(Some(now)),
                Effect::Exit => self.exit = true,
            }
        }
    }

    /// What the chat asks for: a line or a "read" for one viewer, on that
    /// viewer's own pipe and no other.
    fn carry_out_chat(&mut self) {
        for effect in self.chat.take_effects() {
            match effect {
                chat::Effect::Send { slot, launch, message } => {
                    let line = match &message {
                        ToViewer::SendChat { id, .. } => Some(*id),
                        _ => None,
                    };
                    let queued = self.viewers[slot]
                        .as_ref()
                        .filter(|v| v.launch == launch)
                        .is_some_and(|viewer| viewer.send(message));
                    if !queued && let Some(id) = line {
                        // never reached the pipe: not sent, for certain
                        log::warn!("session host: a line for character {} was not handed to it", slot + 1);
                        self.chat.not_written(id);
                    }
                }
                chat::Effect::Alert { .. } => self.alert = true,
            }
        }
    }

    fn spawn(&mut self, slot: usize, launch: u32) {
        let Some(profile) = self.desk.slot(slot).map(|s| s.profile.clone()) else {
            return;
        };
        if super::profile::lease_held(&profile) {
            // it would only bring that window forward and end
            self.desk.spawn_failed(slot, launch);
            self.desk.note(slot, Note::OpenElsewhere);
            return;
        }
        match process::spawn(slot, launch, &profile, &self.events_tx) {
            Ok(viewer) => self.viewers[slot] = Some(viewer),
            Err(e) => {
                log::error!("session host: character {} did not start ({e})", slot + 1);
                self.desk.spawn_failed(slot, launch);
            }
        }
    }

    fn save(&mut self) {
        self.unsaved = None;
        if !self.demo {
            save_config(&self.desk.config());
        }
    }
}

/// Lets a viewer's process bring its window to the front when the host,
/// which the user just clicked, asks it to (Windows refuses the foreground
/// to a process that was not given it).
#[cfg(windows)]
fn allow_foreground(pid: u32) {
    // SAFETY: no pointer; a refusal only leaves the window flashing in the
    // taskbar instead of coming forward.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid);
    }
}

#[cfg(not(windows))]
fn allow_foreground(_pid: u32) {}

/// The system's own notification sound, for a message to a background
/// character (the viewers' sounds are grid assets the host cannot fetch:
/// it never logs in).
#[cfg(windows)]
fn message_sound() {
    use windows_sys::Win32::{System::Diagnostics::Debug::MessageBeep, UI::WindowsAndMessaging::MB_ICONASTERISK};
    // SAFETY: no pointer; the sound is queued and the call returns at once.
    unsafe {
        MessageBeep(MB_ICONASTERISK);
    }
}

#[cfg(not(windows))]
fn message_sound() {}

/// The host's whole run: its window until it closes.
pub fn run() -> anyhow::Result<()> {
    log::info!("session host: starting ({})", super::managed::build_id());
    ui::run(Host::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_takes_one_to_five_characters() {
        assert_eq!(demo_count("3"), Some(3));
        assert_eq!(demo_count(" 5 "), Some(5));
        assert_eq!(demo_count("1"), Some(1));
        for bad in ["0", "6", "-1", "", "three", "3.5"] {
            assert_eq!(demo_count(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_test_run_is_never_a_host() {
        assert!(!wanted());
    }
}
