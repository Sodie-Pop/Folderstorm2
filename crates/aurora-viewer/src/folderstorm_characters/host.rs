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
//! Checkpoint 7: the attention inbox (what waits for an answer in the
//! characters' own windows, across all of them; nothing is answered
//! here), each character's colour and alias ([`faces`]; only ever in the
//! settings file and in these windows) and the condensed layout.
//!
//! Checkpoint 8: the bar. Docked, every viewer the host manages draws it
//! at the top of its own window from what the host tells it
//! ([`Host::sync_bars`]) and sends its clicks back; popped out, the host
//! shows it in a compact window of its own (`ui/bar.rs`). While a bar is
//! on screen the host's own window hides.
//!
//! Checkpoint 9: each monitor has a window of its own, with a chat part
//! that speaks as its character, and a cycling monitor shows each
//! background character in turn (`ui/monitor_windows.rs`); a character's
//! conversations are chips, in those windows and in the shared chat
//! ([`chips`]).
//!
//! Session routing (folderstorm/plans/mcp-routing.md): the host's MCP
//! controller ([`controller`]), a separate authority from every viewer's
//! local assistant, off by default; its settings and its Ask window are in
//! `ui/controller.rs`.
//!
//! Checkpoint 11: conversations from the host. Each character's friends
//! and groups, as its viewer tells them (`contacts::Book`, never saved),
//! "New IM" and "Friends" in the shared chat and the monitors' chat
//! (`contacts_ui.rs`): the host asks the viewer to open the conversation,
//! the viewer decides and opens it as the user would there. A line's web
//! links open in the system's browser after the viewer's own judge of
//! the site (`chat_ui.rs`).
//!
//! Every window is drawn on the processor, like the crash window
//! (`crash/software.rs`): the host opens no graphics device, so it takes
//! nothing from the characters' card and no driver trouble reaches it.
//!
//! The host never logs a character in and never sees a password: each
//! viewer shows its own login screen. Closing or losing the host ends no
//! viewer: their command pipes close and they go on by themselves.

mod chat_ui;
mod chips;
mod comfort;
mod contacts_ui;
mod controller; // (session routing: the MCP controller)
mod faces;
mod hotkeys;
mod monitors;
mod process;
mod ui;

use ::folderstorm_session::bar::{self, Placement, QuickAction};
use ::folderstorm_session::chat::{self, Chat};
use ::folderstorm_session::contacts::Book;
use ::folderstorm_session::host::{Config, Desk, Effect, Note, Phase};
use ::folderstorm_session::identity::Colour;
use ::folderstorm_session::inbox::Inbox;
use ::folderstorm_session::ipc::{AttentionItem, AttentionKind, MAX_SLOTS, Screen, ToViewer};
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

/// `AURORA_DEMO_SESSION_HOST_POLISH=colours,condensed,inbox` (or `all`),
/// demo only, with [`DEMO_VARIABLE`] (checkpoint 7): the demo's
/// characters have colours and the first two an alias; the host starts
/// in its condensed layout; the inbox lists sample questions for the
/// characters that are in the world (the host's own samples: no viewer
/// has them, and "Switch to answer" only switches).
pub const DEMO_POLISH_VARIABLE: &str = "AURORA_DEMO_SESSION_HOST_POLISH";

/// `AURORA_DEMO_SESSION_HOST_BAR=docked|popped`, demo only, with
/// [`DEMO_VARIABLE`] (checkpoint 8): where the bar starts, at the top of
/// the demo's viewers or in the host's compact window. Either way the
/// demo's quick actions also have the two toggles.
pub const DEMO_BAR_VARIABLE: &str = "AURORA_DEMO_SESSION_HOST_BAR";

/// `AURORA_DEMO_SESSION_HOST_MONITORS=windows|cycling` (or both, or
/// `all`), demo only, with [`DEMO_VARIABLE`] (checkpoint 9): `windows`
/// opens a monitor's window for every demo character but the first, the
/// first of them with its chat part; `cycling` opens the cycling monitor,
/// with its chat part.
pub const DEMO_MONITORS_VARIABLE: &str = "AURORA_DEMO_SESSION_HOST_MONITORS";

/// `AURORA_DEMO_SESSION_HOST_SETTINGS=<section>`, demo only, with
/// [`DEMO_VARIABLE`] (checkpoint 10): the settings' window opens on that
/// section (`toolbar`, `characters`, `chat`, `sound`, `monitors`,
/// `hotkeys`, `advanced`, `updates`).
pub const DEMO_SETTINGS_VARIABLE: &str = "AURORA_DEMO_SESSION_HOST_SETTINGS";

/// `AURORA_DEMO_SESSION_HOST_CONTACTS=friends|new-im`, demo only, with
/// [`DEMO_VARIABLE`] (checkpoint 11): the shared chat opens on the first
/// character with its "Friends" or "New IM" sheet. The demo's viewers
/// tell their own friends and groups (the demo's buddy list and groups).
pub const DEMO_CONTACTS_VARIABLE: &str = "AURORA_DEMO_SESSION_HOST_CONTACTS";

fn demo_contacts(value: &str) -> Option<::folderstorm_session::contacts::Which> {
    use ::folderstorm_session::contacts::Which;
    match value.trim().to_ascii_lowercase().as_str() {
        "friends" | "friend" => Some(Which::Friends),
        "new-im" | "newim" | "im" | "new" => Some(Which::Everyone),
        _ => None,
    }
}

/// What [`DEMO_MONITORS_VARIABLE`] asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DemoMonitors {
    windows: bool,
    cycling: bool,
}

fn demo_monitors(list: &str) -> DemoMonitors {
    let names: Vec<String> = list.split(',').map(|name| name.trim().to_ascii_lowercase()).collect();
    let has = |wanted: &[&str]| names.iter().any(|name| name == "all" || wanted.contains(&name.as_str()));
    DemoMonitors {
        windows: has(&["windows", "window"]),
        cycling: has(&["cycling", "cycle"]),
    }
}

/// What [`DEMO_BAR_VARIABLE`] asks for.
fn demo_bar(value: &str) -> Option<Placement> {
    match value.trim().to_ascii_lowercase().as_str() {
        "docked" | "1" => Some(Placement::Docked),
        "popped" | "popped-out" | "out" => Some(Placement::Popped),
        _ => None,
    }
}

/// What [`DEMO_POLISH_VARIABLE`] asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DemoPolish {
    colours: bool,
    condensed: bool,
    inbox: bool,
}

fn demo_polish(list: &str) -> DemoPolish {
    let names: Vec<String> = list.split(',').map(|name| name.trim().to_ascii_lowercase()).collect();
    let has = |wanted: &[&str]| names.iter().any(|name| name == "all" || wanted.contains(&name.as_str()));
    DemoPolish {
        colours: has(&["colours", "colors"]),
        condensed: has(&["condensed"]),
        inbox: has(&["inbox"]),
    }
}

/// The demo's sample questions for one character: the host's own, to
/// show the inbox without anyone sending an offer.
fn sample_questions(slot: usize) -> Vec<AttentionItem> {
    // (numbers no viewer's notification has)
    let id = |n: u64| u64::MAX - (slot as u64) * 8 - n;
    let item = |n: u64, kind, title: &str, text: &str| AttentionItem {
        id: id(n),
        kind,
        title: title.to_owned(),
        text: text.to_owned(),
    };
    match slot % 3 {
        0 => vec![item(0, AttentionKind::Teleport, "Jane Doe", "Join me at the harbour")],
        1 => vec![
            item(0, AttentionKind::Friendship, "Sam Resident", "Shall we be friends?"),
            item(1, AttentionKind::Script, "Door", "Open, close or lock?"),
        ],
        _ => vec![item(0, AttentionKind::Inventory, "Kim Resident", "offers you \"Lantern\"")],
    }
}

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
    /// Checkpoint 11: the characters' friends and groups, and what the
    /// host asked their viewers to open.
    book: Book,
    /// Checkpoint 7: what waits for an answer in the characters' own
    /// windows, across all of them. Only ever shown.
    inbox: Inbox,
    /// The demo's sample questions are listed in the inbox.
    sample_inbox: bool,
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
    /// Checkpoint 8: what each viewer was last told of the bar, with the
    /// launch it was told to.
    bar_sent: [Option<(u32, bar::State)>; MAX_SLOTS],
    /// The clicks on the viewers' bars since the window looked.
    bar_asked: Vec<bar::Request>,
    /// Checkpoint 10: when the active character's chats are read.
    auto_read: ::folderstorm_session::settings::AutoRead,
    /// Checkpoint 10, demo only: the settings' window opens on that
    /// section ([`DEMO_SETTINGS_VARIABLE`]).
    demo_settings: Option<::folderstorm_session::settings::Section>,
    /// Session routing: the MCP controller.
    controller: controller::Controller,
    /// Checkpoint 11, demo only: the shared chat opens with this sheet
    /// ([`DEMO_CONTACTS_VARIABLE`]).
    demo_contacts: Option<::folderstorm_session::contacts::Which>,
}

/// The most clicks of the viewers' bars kept between two looks of the
/// window: more can only be a viewer gone wrong.
const MAX_BAR_ASKED: usize = 32;

impl Host {
    fn new() -> Self {
        let demo = demo();
        let config = demo.map_or_else(load_config, Config::demo);
        let (events_tx, events) = crossbeam_channel::unbounded();
        let mut host = Self {
            desk: Desk::new(config),
            chat: Chat::default(),
            book: Book::default(),
            inbox: Inbox::default(),
            sample_inbox: false,
            alert: false,
            comfort: Default::default(),
            viewers: Default::default(),
            screens: Default::default(),
            events,
            events_tx,
            demo: demo.is_some(),
            unsaved: None,
            exit: false,
            bar_sent: Default::default(),
            bar_asked: Vec::new(),
            auto_read: Default::default(),
            demo_settings: None,
            controller: Default::default(),
            demo_contacts: None,
        };
        if host.demo {
            let slots: Vec<usize> = host.desk.slots().map(|(slot, _)| slot).collect();
            for slot in slots {
                host.desk.launch(slot, slot == 0);
            }
            host.demo_monitors(demo_monitors(&std::env::var(DEMO_MONITORS_VARIABLE).unwrap_or_default()));
            let out = std::env::var(DEMO_PANELS_VARIABLE).unwrap_or_default();
            for panel in ::folderstorm_session::panels::parse_list(&out) {
                host.desk.set_panel_out(panel, true);
            }
            host.demo_polish(demo_polish(&std::env::var(DEMO_POLISH_VARIABLE).unwrap_or_default()));
            if let Ok(value) = std::env::var(DEMO_BAR_VARIABLE) {
                host.demo_bar(demo_bar(&value));
            }
            if let Ok(value) = std::env::var(controller::DEMO_VARIABLE) {
                host.demo_controller(&value, Instant::now());
            }
            host.demo_settings = std::env::var(DEMO_SETTINGS_VARIABLE)
                .ok()
                .and_then(|value| ::folderstorm_session::settings::Section::parse(&value));
            host.demo_contacts = std::env::var(DEMO_CONTACTS_VARIABLE).ok().and_then(|value| demo_contacts(&value));
        }
        host
    }

    /// Checkpoint 9, demo only: the monitors [`DEMO_MONITORS_VARIABLE`]
    /// asks for.
    fn demo_monitors(&mut self, monitors: DemoMonitors) {
        if monitors.windows {
            // every character behind the first, which the demo puts in front
            let behind: Vec<usize> = self.desk.slots().map(|(slot, _)| slot).skip(1).collect();
            for (index, slot) in behind.into_iter().enumerate() {
                self.desk.open_monitor(slot);
                self.desk.edit_monitor_window(slot, |window| window.chat = index == 0);
            }
        }
        if monitors.cycling {
            self.desk.edit_monitors(|config| {
                config.cycling.open = true;
                config.cycling.window.chat = true;
            });
        }
    }

    /// Checkpoint 8, demo only: the bar where [`DEMO_BAR_VARIABLE`] asks,
    /// with the two toggles among its quick actions.
    fn demo_bar(&mut self, placement: Option<Placement>) {
        self.desk.edit_bar(|bar| {
            bar.placement = placement.unwrap_or(bar.placement);
            bar.set_quick(6, QuickAction::MuteBackground);
            bar.set_quick(7, QuickAction::VoiceFollows);
        });
    }

    /// The bar as the host shows it now, in a viewer or in its own window.
    fn bar_state(&self) -> bar::State {
        self.desk.bar_state(self.inbox.count(), self.chat.total_unread())
    }

    /// The viewers that draw a bar and were not told this one yet, with
    /// their launch.
    fn bars_due(&self, state: &bar::State) -> Vec<(usize, u32)> {
        self.desk
            .slots()
            .filter(|(_, s)| s.phase == Phase::Running)
            .map(|(slot, s)| (slot, s.launch()))
            .filter(|(slot, launch)| {
                !self.bar_sent[*slot]
                    .as_ref()
                    .is_some_and(|(told, sent)| told == launch && sent == state)
            })
            .collect()
    }

    /// Checkpoint 8: every managed viewer holds the bar as it is now.
    /// Told whole, when it changed; one that did not take it (its pipe is
    /// full) is told at the next look.
    fn sync_bars(&mut self) {
        let state = self.bar_state();
        for slot in 0..MAX_SLOTS {
            if !self.desk.slot(slot).is_some_and(|s| s.phase == Phase::Running) {
                self.bar_sent[slot] = None;
            }
        }
        for (slot, launch) in self.bars_due(&state) {
            let told = self.viewers[slot]
                .as_ref()
                .filter(|viewer| viewer.launch == launch)
                .is_some_and(|viewer| viewer.send(ToViewer::Bar(Box::new(state.clone()))));
            if told {
                self.bar_sent[slot] = Some((launch, state.clone()));
            }
        }
    }

    /// Checkpoint 7, demo only: the look [`DEMO_POLISH_VARIABLE`] asks for.
    fn demo_polish(&mut self, polish: DemoPolish) {
        let slots: Vec<usize> = self.desk.slots().map(|(slot, _)| slot).collect();
        if polish.colours {
            for (slot, colour) in slots
                .iter()
                .zip([Colour::Teal, Colour::Orange, Colour::Violet, Colour::Green, Colour::Pink])
            {
                self.desk.set_colour(*slot, colour);
            }
            for (slot, alias) in slots.iter().zip(["Main", "Shop keeper"]) {
                // (two names that pass the alias's check)
                let _ = self.desk.set_alias(*slot, alias);
            }
        }
        self.desk.set_condensed(polish.condensed);
        self.sample_inbox = polish.inbox;
    }

    /// The inbox follows what the chat keeps of each viewer's list: the
    /// chat already drops a list of another launch or login, and empties
    /// a character's when it logs out, ends or is let go.
    /// (Checkpoint 10: only the questions Attention lists: of the kinds
    /// chosen, of the characters included.)
    fn sync_inbox(&mut self, now: Instant) {
        for slot in 0..MAX_SLOTS {
            let (items, more) = self.chat.attention(slot);
            let login = self.chat.login(slot);
            let mut shown = items.to_vec();
            if self.sample_inbox && self.demo {
                shown.extend(sample_questions(slot));
            }
            shown.retain(|item| self.desk.attention_lists(slot, item.kind));
            let included = self.desk.slot(slot).is_none_or(|s| s.attention);
            let more = if included { more } else { 0 };
            self.inbox.set(slot, login, &shown, more, now);
        }
    }

    /// What happened since the last call, then what the desk asks for.
    fn tick(&mut self, now: Instant) {
        // (checkpoint 10) what the chats show and what is read
        self.chat.set_shown(self.desk.chat_shown());
        self.chat.set_mark_displayed(self.desk.reading().mark_displayed);
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
                process::Kind::Opened(id, outcome) if managed => self.book.answer(slot, launch, id, outcome, &mut self.chat),
                process::Kind::Contacts(list) if managed => self.book.set(slot, launch, self.chat.login(slot), *list),
                // a click on that viewer's bar: the window carries it out
                // (`ui/bar.rs`), as it does its own clicks
                process::Kind::Bar(request) if managed && request.valid() => {
                    if self.bar_asked.len() < MAX_BAR_ASKED {
                        self.bar_asked.push(request);
                    }
                }
                process::Kind::Chat(_)
                | process::Kind::Attention(_)
                | process::Kind::SendResult(..)
                | process::Kind::Bar(_)
                | process::Kind::Opened(..)
                | process::Kind::Contacts(_) => {}
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
                self.book.ended(slot, viewer.launch);
                self.viewers[slot] = None;
            }
        }
        // (session routing) the MCP controller's calls, before the desk's
        // effects are carried out
        self.tick_controller(now);
        self.desk.tick(now);
        self.carry_out(now);
        self.tick_monitors(now);
        self.chat.set_active(self.desk.active());
        if let Some(slot) = self.auto_read.tick(self.desk.active(), now, &self.desk.reading()) {
            self.chat.read_in_host(slot);
        }
        self.chat.tick(now);
        self.carry_out_chat();
        for slot in 0..MAX_SLOTS {
            self.book.sync(slot, self.chat.login(slot));
        }
        self.book.tick(now);
        self.carry_out_book();
        self.sync_inbox(now);
        self.sync_bars();
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
                    self.book.ended(slot, launch);
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
                chat::Effect::Alert { slot, kind, said, card } => {
                    // (checkpoint 10: as the character's alerts say)
                    use ::folderstorm_session::ipc::LineKind;
                    let wanted = |line| self.desk.alert_wanted(slot, kind, line);
                    if (said && wanted(LineKind::Said)) || (card && wanted(LineKind::Card)) {
                        self.alert = true;
                    }
                }
            }
        }
    }

    /// Checkpoint 11: what the book asks for, a request to open on one
    /// viewer's own pipe and no other.
    fn carry_out_book(&mut self) {
        for send in self.book.take_effects() {
            let queued = self.viewers[send.slot]
                .as_ref()
                .filter(|v| v.launch == send.launch)
                .is_some_and(|viewer| viewer.send(send.message));
            if !queued {
                log::warn!("session host: a request for character {} was not handed to it", send.slot + 1);
                self.book.not_written(send.id);
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
    fn the_demo_s_polish_is_a_list_of_names() {
        assert_eq!(demo_polish(""), DemoPolish::default());
        assert_eq!(demo_polish("nothing,1"), DemoPolish::default());
        assert_eq!(
            demo_polish(" Colours , inbox"),
            DemoPolish {
                colours: true,
                condensed: false,
                inbox: true
            }
        );
        assert_eq!(demo_polish("colors").colours, demo_polish("colours").colours);
        assert_eq!(
            demo_polish("all"),
            DemoPolish {
                colours: true,
                condensed: true,
                inbox: true
            }
        );
        // the samples of two characters never share a number
        let numbers: Vec<u64> = (0..MAX_SLOTS).flat_map(sample_questions).map(|item| item.id).collect();
        let mut distinct = numbers.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), numbers.len());
    }

    #[test]
    fn the_demo_s_monitors_are_windows_the_cycling_one_or_both() {
        assert_eq!(demo_monitors(""), DemoMonitors::default());
        assert_eq!(demo_monitors("monitors,1"), DemoMonitors::default());
        assert_eq!(
            demo_monitors(" Windows "),
            DemoMonitors {
                windows: true,
                cycling: false
            }
        );
        assert_eq!(
            demo_monitors("cycling"),
            DemoMonitors {
                windows: false,
                cycling: true
            }
        );
        let both = DemoMonitors {
            windows: true,
            cycling: true,
        };
        assert_eq!(demo_monitors("cycling,windows"), both);
        assert_eq!(demo_monitors("all"), both);
        // nothing asked: no monitor is open
        let mut host = chat_ui::tests::host();
        host.demo_monitors(DemoMonitors::default());
        assert!(host.desk.slots().all(|(_, s)| s.monitor.is_none()) && !host.desk.monitors().cycling.open);
        // windows: every character but the first, the first of them with
        // its chat part
        host.demo_monitors(both);
        assert!(host.desk.slot(0).is_some_and(|s| s.monitor.is_none()));
        assert!(host.desk.slot(1).is_some_and(|s| s.monitor.is_some() && s.monitor_window.chat));
        let cycling = host.desk.monitors().cycling;
        assert!(cycling.open && cycling.window.chat);
    }

    #[test]
    fn the_demo_s_bar_is_docked_or_popped() {
        assert_eq!(demo_bar("docked"), Some(Placement::Docked));
        assert_eq!(demo_bar(" Popped "), Some(Placement::Popped));
        assert_eq!(demo_bar("out"), Some(Placement::Popped));
        for other in ["", "0", "floating"] {
            assert_eq!(demo_bar(other), None, "{other}");
        }
        let mut host = chat_ui::tests::host();
        host.demo_bar(Some(Placement::Popped));
        assert_eq!(host.desk.bar().placement, Placement::Popped);
        let quick = &host.desk.bar().quick;
        assert!(quick.contains(&QuickAction::MuteBackground) && quick.contains(&QuickAction::VoiceFollows));
        // an unknown value leaves the bar where it was
        host.demo_bar(None);
        assert_eq!(host.desk.bar().placement, Placement::Popped);
    }

    #[test]
    fn each_running_viewer_is_told_the_bar_once_and_again_when_it_changes() {
        let now = Instant::now();
        let mut host = chat_ui::tests::host();
        // no viewer runs: nobody to tell
        let state = host.bar_state();
        assert!(state.docked && state.characters.len() >= 2);
        assert!(host.bars_due(&state).is_empty());
        for slot in 0..2 {
            host.desk.launch(slot, slot == 0);
            host.desk.connected(slot, 1, now);
        }
        let state = host.bar_state();
        assert_eq!(host.bars_due(&state), [(0, 1), (1, 1)]);
        // told (as `sync_bars` notes it once the pipe took it)
        host.bar_sent[0] = Some((1, state.clone()));
        host.bar_sent[1] = Some((1, state.clone()));
        assert!(host.bars_due(&state).is_empty());
        // something the bar shows changed: both are told again
        host.desk.set_mute_background(true);
        let changed = host.bar_state();
        assert_ne!(changed, state);
        assert_eq!(host.bars_due(&changed).len(), 2);
        // another launch of a slot was told nothing yet
        host.bar_sent[0] = Some((1, changed.clone()));
        host.bar_sent[1] = Some((7, changed.clone()));
        assert_eq!(host.bars_due(&changed), [(1, 1)]);
        // a viewer that ended is forgotten (no process here: `sync_bars`
        // has nobody to write to, and notes nothing)
        host.desk.exited(1, 1, true, now);
        host.sync_bars();
        assert!(host.bar_sent[1].is_none() && host.bar_sent[0].is_some());
    }

    #[test]
    fn the_demo_s_contacts_are_friends_or_new_im() {
        use ::folderstorm_session::contacts::Which;
        assert_eq!(demo_contacts(" Friends "), Some(Which::Friends));
        assert_eq!(demo_contacts("new-im"), Some(Which::Everyone));
        assert_eq!(demo_contacts("im"), Some(Which::Everyone));
        for other in ["", "groups", "1"] {
            assert_eq!(demo_contacts(other), None, "{other}");
        }
    }

    #[test]
    fn a_test_run_is_never_a_host() {
        assert!(!wanted());
    }
}
