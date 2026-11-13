//! The session host's desk: up to five characters, one of them active
//! (session-host plan, sections 8, 10 and 11, checkpoint 2). Pure: it is
//! told what happened (a click, a viewer's report, a process that ended,
//! the time) and answers with [`Effect`]s for the host to carry out. It
//! starts no process, opens no pipe and reads no clock.
//!
//! The rules it keeps:
//! - **One active character.** It alone is in front, heard, and allowed
//!   voice; the others are background: behind (Warm) or minimised
//!   (Economy), 3D off, sound cut, voice off.
//! - **Voice is given up before it is given.** The new active character is
//!   allowed voice only once every other viewer has acknowledged an
//!   assignment without it and said its voice is off, or has ended. A
//!   viewer that does not answer keeps the voice waiting: it may still be
//!   transmitting.
//! - **The host follows the user.** A background window that gets the
//!   keyboard (a click on it, Alt-Tab) becomes the active character.
//! - **No false deaths (R6).** Healthy, slow (its main loop has not turned
//!   for [`SLOW_AFTER`]) and not answering (no heartbeat for
//!   [`SILENT_AFTER`]) are only shown. Nothing here ends a process: that
//!   is the user's confirmed choice ([`Desk::end`]). A viewer is gone when
//!   its process has ended, and then it can be started again.
//! - **Nothing stale.** Every launch of a slot has a number; what comes
//!   from an earlier launch of that slot is dropped.
//! - **Monitors (checkpoint 5).** A background character whose monitor is
//!   open **and on screen in the host's window** is asked for pictures
//!   ([`Role::Monitor`]), within the budget all monitors share; closed,
//!   scrolled away, or with the host's window hidden or minimised, it is a
//!   plain background character again, its 3D off. The active character
//!   has no monitor: it is in front. A character whose monitor is open is
//!   not minimised behind (a minimised window has no picture).
//! - **The bar (checkpoint 8).** What the bar shows of the desk
//!   ([`Desk::bar_state`]) and what its clicks do ([`Desk::bar_request`]):
//!   nothing the host's own window could not do. "Mute background sound"
//!   cuts every background character whatever its own choice; "Voice
//!   follows active" off leaves the voice with the character that holds
//!   it ([`Desk::voice_holder`]): still one voice, given up before it is
//!   given. While the bar is docked and the user wants one taskbar
//!   button, a character behind the active one is asked off the taskbar
//!   (`Assignment::off_taskbar`), never when no character is in front.

use crate::bar;
use crate::behind::Behind3d;
use crate::hotkeys;
use crate::identity::{self, AliasError, Colour};
use crate::ipc::{Assignment, MAX_SLOTS, Refusal, Role, Screen, Stage, Standby, Status, ToViewer};
use crate::monitor::{self, FrameHeader, MAX_MONITORS};
use crate::monitor_windows;
use crate::panels;
use crate::profile::ProfileName;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// A viewer whose main loop has not turned for this long is shown as slow.
pub const SLOW_AFTER: Duration = Duration::from_secs(10);
/// A viewer whose link thread has been silent for this long is shown as
/// not answering.
pub const SILENT_AFTER: Duration = Duration::from_secs(30);
/// How long "Quit all" waits before listing the viewers still open.
pub const QUIT_WAIT: Duration = Duration::from_secs(20);
/// An assignment not acknowledged after this long is sent again.
pub const RESEND_AFTER: Duration = Duration::from_secs(2);
/// After the host placed a window, how long that window's reported place
/// is not taken as the stage: it may still be on its way there.
pub const STAGE_SETTLE: Duration = Duration::from_millis(1500);
/// A viewer whose process runs but has not reported for this long since
/// it started is noted, and End is offered (it may be held by a dialog, a
/// slow disk, or never reach the host).
pub const START_NOTE_AFTER: Duration = Duration::from_secs(60);

/// What is heard of a character while another one is active (plan
/// section 10). Session only: a viewer's saved volumes never change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundSound {
    /// Nothing: every sound of its viewer is cut.
    #[default]
    Muted,
    /// Everything, as if it were the active character (its voice stays
    /// off).
    All,
}

/// One character of the host's settings file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotConfig {
    pub slot: u8,
    pub profile: String,
    #[serde(default)]
    pub standby: Standby,
    /// Checkpoint 4: heard in the background or not.
    #[serde(default)]
    pub sound: BackgroundSound,
    /// Checkpoint 5: its monitor is open, with that size and rate.
    #[serde(default)]
    pub monitor: Option<monitor::Spec>,
    /// Checkpoint 7: its colour in the host's windows.
    #[serde(default)]
    pub colour: Colour,
    /// Checkpoint 7: the name the host shows for it in place of its own.
    /// Only ever in this file and in the host's windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// Checkpoint 9: its monitor's own window (place, pin, chat part).
    #[serde(default, skip_serializing_if = "monitor_windows::Window::is_default")]
    pub monitor_window: monitor_windows::Window,
}

/// The host's settings file (`session-host.json`): the characters, and the
/// stage. Never a credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub slots: Vec<SlotConfig>,
    /// The characters' windows share one place: the window brought forward
    /// takes the place of the one that was active.
    pub stage: bool,
    pub stage_rect: Option<Stage>,
    /// The host sounds when a character that is not the active one gets a
    /// message (their own sounds are cut).
    pub chat_sound: bool,
    /// Checkpoint 4: the global hotkeys, as texts.
    pub hotkeys: hotkeys::Config,
    /// Checkpoint 6: the panels that have a window of their own, and
    /// where.
    pub panels: panels::Config,
    /// Checkpoint 7: the characters are shown one line each.
    pub condensed: bool,
    /// Checkpoint 8: the bar, docked in the viewers or popped out.
    pub bar: bar::Config,
    /// Checkpoint 8: every background character is cut, whatever its own
    /// `sound` (which is kept).
    pub mute_background: bool,
    /// Checkpoint 8: the voice goes to the character that becomes active
    /// (as it always did). Off: it stays with the one that holds it.
    pub voice_follows: bool,
    /// Checkpoint 9: what a new monitor opens with, and the cycling
    /// monitor. (Each character's own monitor is with its character.)
    pub monitors: monitor_windows::Config,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            stage: true,
            stage_rect: None,
            chat_sound: false,
            hotkeys: hotkeys::Config::default(),
            panels: panels::Config::default(),
            condensed: false,
            bar: bar::Config::default(),
            mute_background: false,
            voice_follows: true,
            monitors: monitor_windows::Config::default(),
        }
    }
}

impl Config {
    /// The file as the desk can use it: profile names that pass
    /// [`ProfileName::parse`], one character per slot and per profile, at
    /// most [`MAX_SLOTS`], a stage a window can take, aliases that pass
    /// [`identity::alias`]. The rest is dropped.
    pub fn cleaned(mut self) -> Self {
        let mut kept: Vec<SlotConfig> = Vec::new();
        for mut slot in self.slots {
            // (an alias that is none is dropped, never cut into another)
            slot.alias = slot.alias.and_then(|alias| identity::alias(&alias).ok().flatten());
            slot.monitor_window = slot.monitor_window.cleaned();
            let Ok(name) = ProfileName::parse(&slot.profile) else {
                continue;
            };
            let taken = kept
                .iter()
                .any(|k| k.slot == slot.slot || ProfileName::parse(&k.profile).is_ok_and(|other| other.key() == name.key()));
            if usize::from(slot.slot) < MAX_SLOTS && !taken {
                kept.push(slot);
            }
        }
        kept.sort_by_key(|k| k.slot);
        self.slots = kept;
        self.stage_rect = self.stage_rect.filter(Stage::valid);
        self.hotkeys = self.hotkeys.cleaned();
        self.panels = self.panels.cleaned();
        self.bar = self.bar.cleaned();
        self.monitors = self.monitors.cleaned();
        self
    }

    /// The demo's characters: `demo-1` to `demo-<count>` (1 to
    /// [`MAX_SLOTS`]).
    pub fn demo(count: usize) -> Self {
        Self {
            slots: (0..count.clamp(1, MAX_SLOTS))
                .map(|i| SlotConfig {
                    slot: i as u8,
                    profile: format!("demo-{}", i + 1),
                    standby: Standby::Warm,
                    sound: BackgroundSound::Muted,
                    monitor: None,
                    colour: Colour::None,
                    alias: None,
                    monitor_window: monitor_windows::Window::default(),
                })
                .collect(),
            ..Self::default()
        }
    }
}

/// Where a slot's viewer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Not started by this host yet.
    Stopped,
    /// Its process is started; it has not reported yet.
    Starting,
    /// It reports.
    Running,
    /// Its process runs, but the host does not manage it: its first report
    /// was refused, or it never reached the host. The host let it go (its
    /// command pipe is closed: the viewer goes on by itself, as one started
    /// by hand) and only watches for its end.
    Unmanaged,
    /// Its process ended: closed (by the user, there or here), or not
    /// cleanly.
    Ended { crashed: bool },
}

/// How a running viewer answers (rule R6). Only ever shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Slow,
    NotAnswering,
}

/// Something to tell the user about a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// The viewer could not be started.
    SpawnFailed,
    /// The profile is already open in a viewer this host did not start.
    OpenElsewhere,
    /// The viewer's first report was refused.
    Refused(Refusal),
    /// The viewer never reached the host's endpoint (only other clients
    /// came, or the endpoint failed).
    NoLink,
}

/// One character of the desk.
#[derive(Clone, Debug)]
pub struct Slot {
    pub profile: ProfileName,
    pub standby: Standby,
    /// What is heard of it while another character is active.
    pub sound: BackgroundSound,
    /// Its monitor is open, with that size and rate.
    pub monitor: Option<monitor::Spec>,
    /// Its colour in the host's windows.
    pub colour: Colour,
    /// The name the host shows for it in place of its own.
    pub alias: Option<String>,
    /// Checkpoint 9: what is remembered of its monitor's window.
    pub monitor_window: monitor_windows::Window,
    /// Its monitor is on screen in one of the host's windows.
    seen: bool,
    pub phase: Phase,
    /// The viewer's last report.
    pub status: Option<Status>,
    pub note: Option<Note>,
    /// Quit was sent to it.
    pub closing: bool,
    /// The user confirmed End for this launch: its end is no crash.
    pub ended: bool,
    launch: u32,
    activate: bool,
    heartbeat: Option<Instant>,
    loop_count: u64,
    loop_moved: Option<Instant>,
    seq: u64,
    sent: Option<(Assignment, Instant)>,
    /// It was allowed voice and has not acknowledged giving it up.
    voice_out: bool,
    was_focused: bool,
    placed: Option<Instant>,
    /// When the desk first saw it starting (`tick`).
    starting_since: Option<Instant>,
}

impl Slot {
    fn new(profile: ProfileName, standby: Standby) -> Self {
        Self {
            profile,
            standby,
            sound: BackgroundSound::Muted,
            monitor: None,
            colour: Colour::None,
            alias: None,
            monitor_window: monitor_windows::Window::default(),
            seen: false,
            phase: Phase::Stopped,
            status: None,
            note: None,
            closing: false,
            ended: false,
            launch: 0,
            activate: false,
            heartbeat: None,
            loop_count: 0,
            loop_moved: None,
            seq: 0,
            sent: None,
            voice_out: false,
            was_focused: false,
            placed: None,
            starting_since: None,
        }
    }

    /// The avatar's name once logged in, the profile's before.
    pub fn real_name(&self) -> &str {
        match self.status.as_ref().filter(|s| !s.name.is_empty()) {
            Some(status) => &status.name,
            None => self.profile.shown(),
        }
    }

    /// The name it is shown by: its alias when it has one.
    pub fn shown_name(&self) -> &str {
        self.alias.as_deref().unwrap_or_else(|| self.real_name())
    }

    /// Logged in: its viewer runs and is in the world.
    pub fn in_world(&self) -> bool {
        self.phase == Phase::Running && self.status.as_ref().is_some_and(|s| s.screen == Screen::World)
    }

    /// Its process is there and managed (or on its way to be).
    pub fn live(&self) -> bool {
        matches!(self.phase, Phase::Starting | Phase::Running)
    }

    /// Its process is there, managed or not.
    pub fn has_process(&self) -> bool {
        self.live() || self.phase == Phase::Unmanaged
    }

    /// The number of its launch: what the host tags its threads with.
    pub fn launch(&self) -> u32 {
        self.launch
    }

    /// How it waits behind: as chosen, but never minimised while its
    /// monitor is open (a minimised window has no picture to show).
    pub fn waits(&self) -> Standby {
        if self.monitor.is_some() { Standby::Warm } else { self.standby }
    }

    /// The viewer holds the latest assignment sent to it.
    fn acknowledged(&self) -> bool {
        match (&self.sent, &self.status) {
            (Some((sent, _)), Some(status)) => status.assign_seq >= sent.seq,
            (None, _) => true,
            (Some(_), None) => false,
        }
    }
}

/// What the host has to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Start the viewer of `slot`, as its launch number `launch`.
    Spawn { slot: usize, launch: u32 },
    /// Write to that viewer's standard input.
    Send { slot: usize, launch: u32, message: ToViewer },
    /// Let that viewer's process take the foreground (Windows), before the
    /// `Show` that follows.
    AllowForeground { slot: usize },
    /// End that viewer's process: the user confirmed it.
    Kill { slot: usize },
    /// Let that viewer go: close its command pipe (it then goes on by
    /// itself), keep watching its process for its end.
    LetGo { slot: usize, launch: u32 },
    /// The settings changed: write the file.
    Save,
    /// "Quit all" is done: close the host.
    Exit,
}

/// Why a character was not added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddError {
    Name(crate::profile::NameError),
    /// A slot already runs that profile.
    Duplicate,
    Full,
}

/// The desk.
#[derive(Debug)]
pub struct Desk {
    slots: [Option<Slot>; MAX_SLOTS],
    active: Option<usize>,
    stage: bool,
    stage_rect: Option<Stage>,
    chat_sound: bool,
    hotkeys: hotkeys::Config,
    panels: panels::Config,
    condensed: bool,
    bar: bar::Config,
    mute_background: bool,
    voice_follows: bool,
    /// With `voice_follows` off: the character the voice stays with.
    voice_home: Option<usize>,
    /// Checkpoint 9: the monitors' defaults and the cycling monitor.
    monitors: monitor_windows::Config,
    /// The character the cycling monitor shows on screen now.
    cycling_seen: Option<usize>,
    /// Monitors that moved from one character to another since the
    /// windows last looked ([`Desk::take_monitor_moves`]).
    monitor_moves: Vec<(usize, usize)>,
    quitting: Option<Instant>,
    effects: Vec<Effect>,
}

/// Where a character's monitor is (plan section 12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
    /// No monitor is open for it.
    Closed,
    /// Its viewer does not run, is closing, or is not managed.
    NoViewer,
    /// It is the active character: in front, nothing to watch.
    InFront,
    /// Its monitor is not on screen in the host's window (scrolled away,
    /// another page, the window minimised or hidden): nothing is asked.
    Hidden,
    /// More monitors are on screen than run at once.
    TooMany,
    /// Its viewer is asked for pictures. `lowered`: less often than
    /// chosen, to fit the budget all monitors share.
    Asked { order: monitor::Order, lowered: bool },
}

/// What a character's quick action does now (plan section 13): the one
/// button, and the one hotkey, that does the right thing for where its
/// viewer is. Whatever it is, a slot never gets a second process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quick {
    /// It runs behind the active character: it becomes the active one.
    Switch,
    /// It is the active character: its window comes forward.
    Show,
    /// It waits at its login screen: its window comes forward, for the
    /// user to log in there.
    LogIn,
    /// Its viewer is starting: it comes forward when it reports.
    Wait,
    /// It has no viewer: one is started.
    Launch,
    /// Its viewer ended: one is started again.
    Restart,
    /// Nothing can be done now: it is closing, its viewer runs unmanaged,
    /// "Quit all" is under way, or the slot is empty.
    #[default]
    Nothing,
}

/// What a character holds of the voice (plan section 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice {
    /// It is the active character and its viewer says voice is allowed
    /// there: the only one that may connect and transmit.
    Allowed,
    /// It is the active character and waits for that other one to give
    /// the voice up.
    WaitsFor(usize),
    /// It was allowed voice and has not said yet that it gave it up.
    GivingUp,
    /// Voice is off there.
    Off,
}

#[cfg(test)]
mod bar_tests;
#[cfg(test)]
mod comfort_tests;
#[cfg(test)]
mod monitor_tests;
#[cfg(test)]
mod monitor_window_tests;
#[cfg(test)]
mod panel_tests;
#[cfg(test)]
mod polish_tests;

impl Desk {
    pub fn new(config: Config) -> Self {
        let config = config.cleaned();
        let mut slots: [Option<Slot>; MAX_SLOTS] = Default::default();
        for s in config.slots {
            if let (Some(place), Ok(name)) = (slots.get_mut(usize::from(s.slot)), ProfileName::parse(&s.profile)) {
                *place = Some(Slot {
                    sound: s.sound,
                    monitor: s.monitor,
                    colour: s.colour,
                    alias: s.alias,
                    monitor_window: s.monitor_window,
                    ..Slot::new(name, s.standby)
                });
            }
        }
        Self {
            slots,
            active: None,
            stage: config.stage,
            stage_rect: config.stage_rect,
            chat_sound: config.chat_sound,
            hotkeys: config.hotkeys,
            panels: config.panels,
            condensed: config.condensed,
            bar: config.bar,
            mute_background: config.mute_background,
            voice_follows: config.voice_follows,
            voice_home: None,
            monitors: config.monitors,
            cycling_seen: None,
            monitor_moves: Vec::new(),
            quitting: None,
            effects: Vec::new(),
        }
    }

    /// The settings to save.
    pub fn config(&self) -> Config {
        Config {
            slots: self
                .slots()
                .map(|(i, s)| SlotConfig {
                    slot: i as u8,
                    profile: s.profile.shown().to_owned(),
                    standby: s.standby,
                    sound: s.sound,
                    monitor: s.monitor,
                    colour: s.colour,
                    alias: s.alias.clone(),
                    monitor_window: s.monitor_window,
                })
                .collect(),
            stage: self.stage,
            stage_rect: self.stage_rect,
            chat_sound: self.chat_sound,
            hotkeys: self.hotkeys.clone(),
            panels: self.panels,
            condensed: self.condensed,
            bar: self.bar.clone(),
            mute_background: self.mute_background,
            voice_follows: self.voice_follows,
            monitors: self.monitors,
        }
    }

    /// The characters, with their slot numbers.
    pub fn slots(&self) -> impl Iterator<Item = (usize, &Slot)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
    }

    pub fn slot(&self, slot: usize) -> Option<&Slot> {
        self.slots.get(slot)?.as_ref()
    }

    fn slot_mut(&mut self, slot: usize) -> Option<&mut Slot> {
        self.slots.get_mut(slot)?.as_mut()
    }

    /// The slot of that launch, if it is still the slot's current one.
    fn current(&mut self, slot: usize, launch: u32) -> Option<&mut Slot> {
        self.slot_mut(slot).filter(|s| s.launch == launch)
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn stage(&self) -> bool {
        self.stage
    }

    /// How many viewers have a process.
    pub fn live(&self) -> usize {
        self.slots().filter(|(_, s)| s.live()).count()
    }

    pub fn full(&self) -> bool {
        self.slots.iter().all(Option::is_some)
    }

    /// What the host has to do since the last call.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Adds a character in the first free slot.
    pub fn add(&mut self, profile: &str) -> Result<usize, AddError> {
        let name = ProfileName::parse(profile).map_err(AddError::Name)?;
        if self.slots().any(|(_, s)| s.profile.key() == name.key()) {
            return Err(AddError::Duplicate);
        }
        let free = self.slots.iter().position(Option::is_none).ok_or(AddError::Full)?;
        self.slots[free] = Some(Slot::new(name, Standby::default()));
        self.effects.push(Effect::Save);
        Ok(free)
    }

    /// Removes a character from the desk (its profile stays on disk);
    /// false while its viewer runs.
    pub fn remove(&mut self, slot: usize) -> bool {
        if self.slot(slot).is_none_or(Slot::has_process) {
            return false;
        }
        self.slots[slot] = None;
        self.effects.push(Effect::Save);
        true
    }

    pub fn set_standby(&mut self, slot: usize, standby: Standby) {
        let Some(s) = self.slot_mut(slot).filter(|s| s.standby != standby) else {
            return;
        };
        s.standby = standby;
        self.effects.push(Effect::Save);
        self.sync(None);
        // (a window already behind is left where it is: the choice applies
        // at the next switch)
    }

    pub fn set_stage(&mut self, stage: bool) {
        if self.stage != stage {
            self.stage = stage;
            self.effects.push(Effect::Save);
        }
    }

    /// The host sounds for a message to a background character.
    pub fn chat_sound(&self) -> bool {
        self.chat_sound
    }

    pub fn set_chat_sound(&mut self, on: bool) {
        if self.chat_sound != on {
            self.chat_sound = on;
            self.effects.push(Effect::Save);
        }
    }

    /// What is heard of a character while another one is active.
    pub fn set_sound(&mut self, slot: usize, sound: BackgroundSound) {
        let Some(s) = self.slot_mut(slot).filter(|s| s.sound != sound) else {
            return;
        };
        s.sound = sound;
        self.effects.push(Effect::Save);
        self.sync(None);
    }

    /// Checkpoint 7: a character's colour in the host's windows. The desk
    /// sends no message for it: since checkpoint 8 it is part of the bar's
    /// state, which the host hands to the viewers it manages for their
    /// bar and nothing else.
    pub fn set_colour(&mut self, slot: usize, colour: Colour) {
        let Some(s) = self.slot_mut(slot).filter(|s| s.colour != colour) else {
            return;
        };
        s.colour = colour;
        self.effects.push(Effect::Save);
    }

    /// Checkpoint 7: the name the host shows for a character in place of
    /// its own; an empty text takes the alias away. As the colour: no
    /// message of the desk's, only the bar's state. A text that is no
    /// alias changes nothing.
    pub fn set_alias(&mut self, slot: usize, text: &str) -> Result<(), AliasError> {
        let alias = identity::alias(text)?;
        if let Some(s) = self.slot_mut(slot).filter(|s| s.alias != alias) {
            s.alias = alias;
            self.effects.push(Effect::Save);
        }
        Ok(())
    }

    /// Checkpoint 7: the characters are shown one line each.
    pub fn condensed(&self) -> bool {
        self.condensed
    }

    pub fn set_condensed(&mut self, condensed: bool) {
        if self.condensed != condensed {
            self.condensed = condensed;
            self.effects.push(Effect::Save);
        }
    }

    /// The global hotkeys' settings.
    pub fn hotkeys(&self) -> &hotkeys::Config {
        &self.hotkeys
    }

    /// The user's choice for one hotkey; true when the settings changed
    /// (the host then registers the hotkeys again).
    pub fn set_hotkey(&mut self, action: hotkeys::Action, text: &str) -> bool {
        let changed = self.hotkeys.set(action, text);
        if changed {
            self.effects.push(Effect::Save);
        }
        changed
    }

    /// Replaces the hotkeys' settings whole (enabled or not, back to the
    /// defaults); true when they changed.
    pub fn set_hotkeys(&mut self, config: hotkeys::Config) -> bool {
        let config = config.cleaned();
        let changed = self.hotkeys != config;
        if changed {
            self.hotkeys = config;
            self.effects.push(Effect::Save);
        }
        changed
    }

    /// What a character's quick action does now.
    pub fn quick(&self, slot: usize) -> Quick {
        let Some(s) = self.slot(slot) else {
            return Quick::Nothing;
        };
        match s.phase {
            _ if s.closing => Quick::Nothing,
            Phase::Running if s.status.as_ref().is_some_and(|st| st.screen == Screen::Login) => Quick::LogIn,
            Phase::Running if self.active == Some(slot) => Quick::Show,
            Phase::Running => Quick::Switch,
            Phase::Starting => Quick::Wait,
            // its process runs, let go by the host: never a second one
            Phase::Unmanaged => Quick::Nothing,
            Phase::Stopped | Phase::Ended { .. } if self.quitting.is_some() => Quick::Nothing,
            Phase::Stopped => Quick::Launch,
            Phase::Ended { .. } => Quick::Restart,
        }
    }

    /// Carries a character's quick action out, and says which it was.
    pub fn quick_act(&mut self, slot: usize, now: Instant) -> Quick {
        let quick = self.quick(slot);
        if quick != Quick::Nothing {
            self.switch(slot, now);
        }
        quick
    }

    /// The character "next" or "previous" goes to: the running one after
    /// (or before) the active one, round the desk. None when no other
    /// character runs. A hotkey never starts a viewer this way.
    pub fn neighbour(&self, forward: bool) -> Option<usize> {
        let ready: Vec<usize> = self
            .slots()
            .filter(|(_, s)| s.phase == Phase::Running && !s.closing)
            .map(|(i, _)| i)
            .collect();
        let found = match self.active {
            None if forward => ready.first(),
            None => ready.last(),
            Some(active) if forward => ready.iter().find(|i| **i > active).or(ready.first()),
            Some(active) => ready.iter().rev().find(|i| **i < active).or(ready.last()),
        };
        found.copied().filter(|slot| Some(*slot) != self.active)
    }

    /// A global hotkey was pressed. A character's hotkey is its quick
    /// action; next and previous bring a running neighbour forward.
    /// Returns the slot it went to, if any.
    pub fn hotkey(&mut self, action: hotkeys::Action, now: Instant) -> Option<usize> {
        let slot = match action {
            hotkeys::Action::Character(slot) => slot,
            hotkeys::Action::Next => self.neighbour(true)?,
            hotkeys::Action::Previous => self.neighbour(false)?,
        };
        (self.quick_act(slot, now) != Quick::Nothing).then_some(slot)
    }

    /// The character the voice belongs with: the active one while the
    /// voice follows; otherwise the one it stays with for as long as its
    /// viewer runs, then the active one again.
    pub fn voice_holder(&self) -> Option<usize> {
        if self.voice_follows {
            return self.active;
        }
        self.voice_home
            .filter(|home| self.slot(*home).is_some_and(|s| s.phase == Phase::Running))
            .or(self.active)
    }

    /// What a character holds of the voice.
    pub fn voice(&self, slot: usize) -> Voice {
        let Some(s) = self.slot(slot).filter(|s| s.phase == Phase::Running) else {
            return Voice::Off;
        };
        if self.voice_holder() != Some(slot) {
            return if s.voice_out { Voice::GivingUp } else { Voice::Off };
        }
        if let Some(other) = self.voice_waits_for() {
            return Voice::WaitsFor(other);
        }
        let allowed = s.sent.is_some_and(|(sent, _)| sent.voice_allowed) && s.acknowledged();
        if allowed && s.status.as_ref().is_some_and(|st| st.voice_allowed) {
            Voice::Allowed
        } else {
            Voice::Off
        }
    }

    /// Checkpoint 8: the bar's settings.
    pub fn bar(&self) -> &bar::Config {
        &self.bar
    }

    /// Changes the bar's settings; true when something changed (it is
    /// then saved, and the viewers' assignments follow: the taskbar's
    /// rule depends on where the bar is).
    pub fn edit_bar(&mut self, edit: impl FnOnce(&mut bar::Config)) -> bool {
        let mut edited = self.bar.clone();
        edit(&mut edited);
        let edited = edited.cleaned();
        if edited == self.bar {
            return false;
        }
        self.bar = edited;
        self.effects.push(Effect::Save);
        self.sync(None);
        true
    }

    /// Every background character is cut, whatever its own choice.
    pub fn mute_background(&self) -> bool {
        self.mute_background
    }

    pub fn set_mute_background(&mut self, mute: bool) {
        if self.mute_background != mute {
            self.mute_background = mute;
            self.effects.push(Effect::Save);
            self.sync(None);
        }
    }

    /// The voice goes to the character that becomes active.
    pub fn voice_follows(&self) -> bool {
        self.voice_follows
    }

    /// Off: the voice stays with the character that holds it now. On: it
    /// goes to the active one, once the one that held it gave it up.
    pub fn set_voice_follows(&mut self, follows: bool) {
        if self.voice_follows != follows {
            // (where it stays is where it is)
            self.voice_home = self.voice_holder();
            self.voice_follows = follows;
            self.effects.push(Effect::Save);
            self.sync(None);
        }
    }

    /// What the bar shows of the desk. `inbox`: how many questions wait;
    /// `chat_unread`: the shared chat's unread lines.
    pub fn bar_state(&self, inbox: u32, chat_unread: u32) -> bar::State {
        bar::State {
            docked: self.bar.placement == bar::Placement::Docked,
            collapsed: self.bar.collapsed,
            characters: self
                .slots()
                .map(|(i, s)| bar::Character {
                    slot: i as u8,
                    name: s.shown_name().to_owned(),
                    real: s.alias.as_ref().map(|_| s.real_name().to_owned()),
                    colour: s.colour,
                    presence: match (s.in_world(), self.active == Some(i)) {
                        (false, _) => bar::Presence::Out,
                        (true, true) => bar::Presence::Active,
                        (true, false) => bar::Presence::Inactive,
                    },
                    quick: self.quick(i),
                    monitor: s.monitor.is_some(),
                })
                .collect(),
            inbox,
            chat_open: self.panels.out(panels::Panel::Chat),
            chat_unread,
            cycling: self.monitors.cycling.open,
            mute_background: self.mute_background,
            voice_follows: self.voice_follows,
            quick: self.bar.quick.clone(),
            quick_labels: self.bar.quick_labels,
            toolbar_labels: self.bar.toolbar_labels,
            span_window: self.bar.span_window,
        }
        .cleaned()
    }

    /// How many viewers draw the docked bar now: managed, with a window,
    /// not closing.
    pub fn bars_shown(&self) -> usize {
        if self.bar.placement != bar::Placement::Docked {
            return 0;
        }
        self.slots()
            .filter(|(_, s)| s.phase == Phase::Running && !s.closing && s.status.as_ref().is_some_and(|st| st.screen != Screen::Starting))
            .count()
    }

    /// A click on the bar, from a viewer or from the host's compact
    /// window: what the desk does for it. What concerns the host's
    /// windows alone ([`bar::Request::Show`], the pin) is the host's.
    /// Nothing here does more than the host's own window does: a pick is
    /// the character's quick action, a monitor is the card's choice.
    pub fn bar_request(&mut self, request: bar::Request, now: Instant) {
        if !request.valid() {
            return;
        }
        match request {
            bar::Request::Pick(slot) => {
                self.quick_act(usize::from(slot), now);
            }
            bar::Request::OpenLogin(asked) => {
                // only ever a character that is not logged in: never a
                // switch to one that is
                let state = self.bar_state(0, 0);
                let slot = match asked {
                    Some(slot) => state.logins().find(|c| c.slot == slot).map(|c| c.slot),
                    None => state.next_login().map(|c| c.slot),
                };
                if let Some(slot) = slot {
                    self.quick_act(usize::from(slot), now);
                }
            }
            bar::Request::Chat(open) => self.set_panel_out(panels::Panel::Chat, open),
            // (checkpoint 9: each monitor has its own window)
            bar::Request::Monitor(slot, open) => {
                let slot = usize::from(slot);
                if open {
                    self.open_monitor(slot);
                } else {
                    self.set_monitor(slot, None);
                }
            }
            bar::Request::OpenMonitors => {
                let behind: Vec<usize> = self
                    .slots()
                    .filter(|(i, s)| s.in_world() && self.active != Some(*i) && s.monitor.is_none())
                    .map(|(i, _)| i)
                    .collect();
                for slot in behind {
                    self.open_monitor(slot);
                }
            }
            bar::Request::CloseMonitors => {
                for slot in 0..MAX_SLOTS {
                    self.set_monitor(slot, None);
                }
                self.set_cycling(false);
            }
            bar::Request::Cycling(open) => self.set_cycling(open),
            bar::Request::Inbox => self.set_panel_out(panels::Panel::Inbox, true),
            bar::Request::MuteBackground(mute) => self.set_mute_background(mute),
            bar::Request::VoiceFollows(follows) => self.set_voice_follows(follows),
            bar::Request::Dock(docked) => {
                let placement = if docked { bar::Placement::Docked } else { bar::Placement::Popped };
                self.edit_bar(|bar| bar.placement = placement);
            }
            bar::Request::Collapse(collapsed) => {
                self.edit_bar(|bar| bar.collapsed = collapsed);
            }
            bar::Request::Pin(pinned) => {
                self.edit_bar(|bar| bar.pinned = pinned);
            }
            bar::Request::Show(_) => {}
        }
    }

    /// Checkpoint 6: the panels that have a window of their own.
    pub fn panels(&self) -> &panels::Config {
        &self.panels
    }

    /// A panel leaves the host's window for its own, or comes back.
    pub fn set_panel_out(&mut self, panel: panels::Panel, out: bool) {
        if self.panels.set_out(panel, out) {
            self.effects.push(Effect::Save);
        }
    }

    /// A panel's window was closed: the panel is docked back. Nothing
    /// else happens: no character is closed and the host stays open.
    pub fn panel_closed(&mut self, panel: panels::Panel) {
        if self.panels.closed(panel) {
            self.effects.push(Effect::Save);
        }
    }

    /// A panel's window was moved or resized: remembered for next time.
    pub fn panel_placed(&mut self, panel: panels::Panel, place: panels::Place) {
        if self.panels.placed(panel, place) {
            self.effects.push(Effect::Save);
        }
    }

    /// Opens (with that size and rate), changes or closes (None) a
    /// character's monitor. Closing it returns its viewer to a plain
    /// background character: 3D off.
    pub fn set_monitor(&mut self, slot: usize, monitor: Option<monitor::Spec>) {
        let Some(s) = self.slot_mut(slot).filter(|s| s.monitor != monitor) else {
            return;
        };
        s.monitor = monitor;
        if monitor.is_none() {
            s.seen = false;
        }
        self.effects.push(Effect::Save);
        self.sync(None);
    }

    /// Opens a character's monitor with the size and rate a new monitor
    /// has ([`monitor_windows::Config::defaults`]); one that is open
    /// keeps its own.
    pub fn open_monitor(&mut self, slot: usize) {
        if self.slot(slot).is_some_and(|s| s.monitor.is_none()) {
            self.set_monitor(slot, Some(self.monitors.defaults));
        }
    }

    /// Checkpoint 9: the monitors' defaults and the cycling monitor.
    pub fn monitors(&self) -> &monitor_windows::Config {
        &self.monitors
    }

    /// Changes them; true when something changed (it is then saved, and
    /// the viewers' assignments follow: the cycling monitor asks for
    /// pictures too).
    pub fn edit_monitors(&mut self, edit: impl FnOnce(&mut monitor_windows::Config)) -> bool {
        let mut edited = self.monitors;
        edit(&mut edited);
        let edited = edited.cleaned();
        if edited == self.monitors {
            return false;
        }
        self.monitors = edited;
        if !edited.cycling.open {
            self.cycling_seen = None;
        }
        self.effects.push(Effect::Save);
        self.sync(None);
        true
    }

    /// Opens or closes the cycling monitor.
    pub fn set_cycling(&mut self, open: bool) {
        self.edit_monitors(|monitors| monitors.cycling.open = open);
    }

    /// Changes what is remembered of a character's monitor's window (its
    /// place, its pin, its chat part); true when something changed. No
    /// viewer is told anything of it.
    pub fn edit_monitor_window(&mut self, slot: usize, edit: impl FnOnce(&mut monitor_windows::Window)) -> bool {
        let Some(s) = self.slot_mut(slot) else {
            return false;
        };
        let mut edited = s.monitor_window;
        edit(&mut edited);
        if edited == s.monitor_window {
            return false;
        }
        s.monitor_window = edited;
        self.effects.push(Effect::Save);
        true
    }

    /// The monitors that moved from one character to another since the
    /// last call, each (from, to), in the order they moved: the host
    /// moves their windows the same way.
    pub fn take_monitor_moves(&mut self) -> Vec<(usize, usize)> {
        std::mem::take(&mut self.monitor_moves)
    }

    /// A monitor follows the character that is not in front
    /// ([`monitor_windows::follow`]): `to` came to the front, `from` left
    /// it. The monitor of `to`, with its window, becomes `from`'s; what
    /// `from` remembered of a window of its own becomes `to`'s.
    fn monitor_follows(&mut self, from: Option<usize>, to: usize) {
        let open: [bool; MAX_SLOTS] = std::array::from_fn(|slot| self.slot(slot).is_some_and(|s| s.monitor.is_some()));
        // (a character whose viewer is gone left nothing to watch)
        let from = from.filter(|from| self.slot(*from).is_some_and(|s| s.phase == Phase::Running && !s.closing));
        let Some((to, from)) = monitor_windows::follow(open, from, to) else {
            return;
        };
        let Some((spec, window)) = self.slot_mut(to).map(|s| (s.monitor.take(), s.monitor_window)) else {
            return;
        };
        if let Some(s) = self.slot_mut(to) {
            s.seen = false;
        }
        let Some(s) = self.slot_mut(from) else {
            return;
        };
        s.monitor = spec;
        let left = std::mem::replace(&mut s.monitor_window, window);
        // (seen until the window says otherwise: it is the same window)
        s.seen = true;
        if let Some(s) = self.slot_mut(to) {
            s.monitor_window = left;
        }
        self.monitor_moves.push((to, from));
        self.effects.push(Effect::Save);
    }

    /// The character the cycling monitor shows on screen now (None: its
    /// window is closed, hidden, minimised or covered, or shows nobody).
    /// Only that character is asked for the cycling monitor's pictures.
    pub fn cycling_seen(&mut self, slot: Option<usize>, now: Instant) {
        let slot = slot.filter(|slot| self.monitors.cycling.open && self.slot(*slot).is_some());
        if std::mem::replace(&mut self.cycling_seen, slot) != slot {
            self.sync(Some(now));
        }
    }

    /// The characters the cycling monitor goes round, in slot order: in
    /// the world, behind the active one, not closing.
    pub fn cycling_candidates(&self) -> Vec<usize> {
        self.slots()
            .filter(|(i, s)| Some(*i) != self.active && s.in_world() && !s.closing)
            .map(|(i, _)| i)
            .collect()
    }

    /// Where the cycling monitor is with that character ([`Desk::watch`],
    /// for the cycling monitor's window).
    pub fn cycling_watch(&self, slot: usize) -> Watch {
        let cycling = self.monitors.cycling;
        let Some(s) = self.slot(slot).filter(|_| cycling.open) else {
            return Watch::Closed;
        };
        if s.phase != Phase::Running || s.closing {
            Watch::NoViewer
        } else if self.active == Some(slot) {
            Watch::InFront
        } else if self.cycling_seen != Some(slot) {
            Watch::Hidden
        } else {
            match self.orders()[slot] {
                Some(order) => Watch::Asked {
                    order,
                    lowered: order.interval_ms > cycling.spec.rate.interval_ms(),
                },
                None => Watch::TooMany,
            }
        }
    }

    /// Which monitors are on screen in one of the host's windows now
    /// (`panels::seen`): none of a window that is minimised or hidden. A monitor nobody can see
    /// asks nothing of its viewer.
    pub fn monitors_seen(&mut self, seen: [bool; MAX_SLOTS], now: Instant) {
        let mut changed = false;
        for (slot, seen) in seen.into_iter().enumerate() {
            if let Some(s) = self.slot_mut(slot) {
                let seen = seen && s.monitor.is_some();
                changed |= std::mem::replace(&mut s.seen, seen) != seen;
            }
        }
        if changed {
            self.sync(Some(now));
        }
    }

    /// The characters watched now, in slot order: running in the
    /// background, not closing, their monitor open and on screen, or
    /// shown by the cycling monitor (both: the larger size and the faster
    /// rate of the two). The first [`MAX_MONITORS`] only.
    fn watched(&self) -> Vec<(usize, monitor::Spec)> {
        let cycling = self.monitors.cycling;
        self.slots()
            .filter(|(i, s)| Some(*i) != self.active && s.phase == Phase::Running && !s.closing)
            .filter_map(|(i, s)| {
                let own = s.monitor.filter(|_| s.seen);
                let cycled = (cycling.open && self.cycling_seen == Some(i)).then_some(cycling.spec);
                monitor_windows::cover(own, cycled).map(|spec| (i, spec))
            })
            .take(MAX_MONITORS)
            .collect()
    }

    /// What each slot's viewer is asked for its monitor, within the budget
    /// all monitors share.
    fn orders(&self) -> [Option<monitor::Order>; MAX_SLOTS] {
        let watched = self.watched();
        let specs: Vec<monitor::Spec> = watched.iter().map(|(_, spec)| *spec).collect();
        let mut orders = [None; MAX_SLOTS];
        for ((slot, _), order) in watched.iter().zip(monitor::grant(&specs)) {
            orders[*slot] = Some(order);
        }
        orders
    }

    /// Where a character's monitor is.
    pub fn watch(&self, slot: usize) -> Watch {
        let Some((s, spec)) = self.slot(slot).and_then(|s| s.monitor.map(|spec| (s, spec))) else {
            return Watch::Closed;
        };
        if s.phase != Phase::Running || s.closing {
            Watch::NoViewer
        } else if self.active == Some(slot) {
            Watch::InFront
        } else if !s.seen {
            Watch::Hidden
        } else {
            match self.orders()[slot] {
                Some(order) => Watch::Asked {
                    order,
                    lowered: order.interval_ms > spec.rate.interval_ms(),
                },
                None => Watch::TooMany,
            }
        }
    }

    /// Whether a picture that came from a viewer is shown: its launch is
    /// the slot's current one, its monitor is being asked for pictures,
    /// and the picture fits the size asked. Anything else (a picture on
    /// its way when the monitor closed, the character came to the front,
    /// or the size changed) is dropped.
    pub fn frame_wanted(&self, slot: usize, launch: u32, header: &FrameHeader) -> bool {
        self.slot(slot)
            .filter(|s| s.launch == launch && s.phase == Phase::Running)
            .and_then(|s| s.sent)
            .and_then(|(sent, _)| sent.role.monitor())
            .is_some_and(|order| header.fits(order.size))
    }

    /// Tells the user why a slot did not start.
    pub fn note(&mut self, slot: usize, note: Note) {
        if let Some(s) = self.slot_mut(slot) {
            s.note = Some(note);
        }
    }

    /// Starts the viewer of a slot that has none. `activate`: it becomes
    /// the active character when it reports.
    pub fn launch(&mut self, slot: usize, activate: bool) {
        if self.quitting.is_some() {
            return;
        }
        let Some(s) = self.slot_mut(slot).filter(|s| !s.has_process()) else {
            return;
        };
        let launch = s.launch + 1;
        *s = Slot {
            launch,
            activate,
            sound: s.sound,
            monitor: s.monitor,
            seen: s.seen,
            phase: Phase::Starting,
            ..Slot::new(s.profile.clone(), s.standby)
        };
        self.effects.push(Effect::Spawn { slot, launch });
    }

    /// The viewer of that launch could not be started.
    pub fn spawn_failed(&mut self, slot: usize, launch: u32) {
        if let Some(s) = self.current(slot, launch) {
            s.phase = Phase::Stopped;
            s.note = Some(Note::SpawnFailed);
        }
    }

    /// The viewer's first report was accepted: it runs.
    pub fn connected(&mut self, slot: usize, launch: u32, now: Instant) {
        let Some(s) = self.current(slot, launch).filter(|s| s.phase == Phase::Starting) else {
            return;
        };
        s.phase = Phase::Running;
        s.note = None;
        s.heartbeat = Some(now);
        s.loop_moved = Some(now);
        if s.activate || self.active.is_none() {
            self.activate(slot, true, now);
        } else {
            self.sync(Some(now));
            if self.slot(slot).is_some_and(|s| self.waits(s) == Standby::Economy) {
                self.send(slot, ToViewer::Minimise);
            }
        }
    }

    /// The viewer's first report was refused: it is not managed (its
    /// process goes on, as a viewer started by hand would).
    pub fn refused(&mut self, slot: usize, launch: u32, refusal: Refusal) {
        self.let_go(slot, launch, Note::Refused(refusal));
    }

    fn let_go(&mut self, slot: usize, launch: u32, note: Note) {
        let Some(s) = self.current(slot, launch).filter(|s| s.phase == Phase::Starting) else {
            return;
        };
        s.phase = Phase::Unmanaged;
        s.note = Some(note);
        s.activate = false;
        self.effects.push(Effect::LetGo { slot, launch });
    }

    /// A report of a running viewer.
    pub fn status(&mut self, slot: usize, launch: u32, status: Status, now: Instant) {
        let active = self.active;
        let Some(s) = self.current(slot, launch).filter(|s| s.phase == Phase::Running) else {
            return;
        };
        let status = status.cleaned();
        let got_focus = status.focused && !std::mem::replace(&mut s.was_focused, status.focused);
        let settled = s.placed.is_none_or(|at| now.duration_since(at) >= STAGE_SETTLE);
        let window = status.window;
        let voice_reported = status.voice_allowed;
        s.status = Some(status);
        // given up once the viewer holds an assignment without voice and
        // says its voice is off: holding the assignment alone is not enough,
        // its voice may still be connected
        if s.acknowledged() && s.sent.is_some_and(|(a, _)| !a.voice_allowed) && !voice_reported {
            s.voice_out = false;
        }
        let closing = s.closing;
        if got_focus && active != Some(slot) && !closing {
            // the user went to that window: the host follows
            self.activate(slot, false, now);
        } else {
            self.sync(Some(now));
        }
        // the stage is where the active character's window is
        if self.active == Some(slot)
            && self.stage
            && settled
            && !window.minimised
            && !window.fullscreen
            && let Some(rect) = window.rect
            && self.stage_rect != Some(rect)
        {
            self.stage_rect = Some(rect);
            self.effects.push(Effect::Save);
        }
    }

    /// The viewer never reached the endpoint of that launch: it cannot be
    /// managed (it finds no host and goes on by itself).
    pub fn unreachable(&mut self, slot: usize, launch: u32) {
        self.let_go(slot, launch, Note::NoLink);
    }

    /// The viewer's link thread is alive, and its main loop has made
    /// `loop_count` turns.
    pub fn heartbeat(&mut self, slot: usize, launch: u32, loop_count: u64, now: Instant) {
        let Some(s) = self.current(slot, launch).filter(|s| s.phase == Phase::Running) else {
            return;
        };
        s.heartbeat = Some(now);
        if loop_count != s.loop_count {
            s.loop_count = loop_count;
            s.loop_moved = Some(now);
        }
    }

    /// The process of that launch ended; `clean`: with a success code.
    pub fn exited(&mut self, slot: usize, launch: u32, clean: bool, now: Instant) {
        let Some(s) = self.current(slot, launch).filter(|s| s.has_process()) else {
            return;
        };
        s.phase = Phase::Ended {
            crashed: !clean && !s.closing && !s.ended,
        };
        s.closing = false;
        s.voice_out = false;
        s.sent = None;
        s.status = None;
        if self.active == Some(slot) {
            self.active = None;
        }
        self.sync(Some(now));
        self.finish_quit();
    }

    /// The user picks a character: brought forward if it runs, started if
    /// not.
    pub fn switch(&mut self, slot: usize, now: Instant) {
        // the last character picked is the one that comes forward: one
        // still starting from an earlier pick does not take over later
        for (i, s) in self.slots.iter_mut().enumerate() {
            if let Some(s) = s.as_mut().filter(|_| i != slot) {
                s.activate = false;
            }
        }
        match self.slot(slot).map(|s| s.phase) {
            Some(Phase::Running) => self.activate(slot, true, now),
            Some(Phase::Starting) => {
                if let Some(s) = self.slot_mut(slot) {
                    s.activate = true;
                }
            }
            Some(Phase::Stopped | Phase::Ended { .. }) => self.launch(slot, true),
            Some(Phase::Unmanaged) | None => {}
        }
    }

    /// Asks a viewer to quit (log out, save, exit).
    pub fn close(&mut self, slot: usize) {
        if self.slot(slot).is_some_and(|s| s.live() && !s.closing) {
            self.send(slot, ToViewer::Quit);
            if let Some(s) = self.slot_mut(slot) {
                s.closing = true;
            }
        }
    }

    /// Ends a viewer's process. Only ever from the user's confirmed choice.
    pub fn end(&mut self, slot: usize) {
        if let Some(s) = self.slot_mut(slot).filter(|s| s.has_process()) {
            s.ended = true;
            self.effects.push(Effect::Kill { slot });
        }
    }

    /// Asks every viewer to quit; the host closes when all have.
    pub fn quit_all(&mut self, now: Instant) {
        self.quitting = Some(now + QUIT_WAIT);
        for slot in 0..MAX_SLOTS {
            self.close(slot);
        }
        self.finish_quit();
    }

    /// "Quit all" is under way.
    pub fn quitting(&self) -> bool {
        self.quitting.is_some()
    }

    /// "Quit all" has waited long enough: the viewers still open are listed
    /// for the user (one may be asking about unsaved work).
    pub fn quit_overdue(&self, now: Instant) -> bool {
        self.quitting.is_some_and(|until| now >= until) && self.live() > 0
    }

    /// The user gives "Quit all" up: the viewers still open stay managed.
    pub fn cancel_quit(&mut self) {
        self.quitting = None;
        for s in self.slots.iter_mut().flatten() {
            s.closing = false;
        }
    }

    fn finish_quit(&mut self) {
        if self.quitting.is_some() && self.live() == 0 {
            self.quitting = None;
            self.effects.push(Effect::Exit);
        }
    }

    /// Now and then: assignments not acknowledged are sent again, and how
    /// long each viewer has been starting is kept.
    pub fn tick(&mut self, now: Instant) {
        for s in self.slots.iter_mut().flatten() {
            if s.phase == Phase::Starting {
                s.starting_since.get_or_insert(now);
            }
        }
        for slot in 0..MAX_SLOTS {
            let Some(s) = self.slot_mut(slot).filter(|s| s.phase == Phase::Running && !s.acknowledged()) else {
                continue;
            };
            let Some((assignment, at)) = &mut s.sent else {
                continue;
            };
            if now.duration_since(*at) >= RESEND_AFTER {
                *at = now;
                let message = ToViewer::Assign(*assignment);
                self.send(slot, message);
            }
        }
    }

    /// The viewer's process has run for [`START_NOTE_AFTER`] without
    /// reporting: noted, with End offered. Never ended by itself.
    pub fn starting_too_long(&self, slot: usize, now: Instant) -> bool {
        self.slot(slot)
            .filter(|s| s.phase == Phase::Starting)
            .and_then(|s| s.starting_since)
            .is_some_and(|since| now.saturating_duration_since(since) >= START_NOTE_AFTER)
    }

    /// How long the viewer has been on its way, from its launch until it
    /// shows its first frame (its first report past [`Screen::Starting`]):
    /// shown beside "Starting…", so that a slow start never looks stuck.
    /// None when it is not starting.
    pub fn starting_for(&self, slot: usize, now: Instant) -> Option<Duration> {
        let s = self.slot(slot)?;
        let on_its_way = match s.phase {
            Phase::Starting => true,
            Phase::Running => !s.closing && s.status.as_ref().is_none_or(|st| st.screen == Screen::Starting),
            _ => false,
        };
        let since = s.starting_since.filter(|_| on_its_way)?;
        Some(now.saturating_duration_since(since))
    }

    /// How a viewer answers. Never a reason to end it.
    pub fn health(&self, slot: usize, now: Instant) -> Health {
        let Some(s) = self.slot(slot).filter(|s| s.phase == Phase::Running) else {
            return Health::Healthy;
        };
        let since = |at: Option<Instant>| at.map_or(Duration::MAX, |at| now.saturating_duration_since(at));
        if since(s.heartbeat) > SILENT_AFTER {
            Health::NotAnswering
        } else if s.status.as_ref().is_some_and(|st| st.screen != Screen::Starting) && since(s.loop_moved) > SLOW_AFTER {
            Health::Slow
        } else {
            Health::Healthy
        }
    }

    /// The character whose voice the one that should hold it waits for:
    /// it was allowed voice and has not said it gave it up.
    pub fn voice_waits_for(&self) -> Option<usize> {
        let holder = self.voice_holder()?;
        self.slots().find(|(i, s)| *i != holder && s.voice_out).map(|(i, _)| i)
    }

    /// Another running slot logged in on the same account and grid.
    pub fn same_account(&self, slot: usize) -> Option<usize> {
        let mine = self.slot(slot)?.status.as_ref().filter(|s| !s.agent.is_empty())?;
        self.slots()
            .find(|(i, s)| {
                *i != slot
                    && s.status
                        .as_ref()
                        .is_some_and(|other| other.agent == mine.agent && other.grid.eq_ignore_ascii_case(&mine.grid))
            })
            .map(|(i, _)| i)
    }

    /// How a character waits behind: as [`Slot::waits`], and never
    /// minimised while the cycling monitor is open (it shows every
    /// background character in turn).
    fn waits(&self, s: &Slot) -> Standby {
        if self.monitors.cycling.open { Standby::Warm } else { s.waits() }
    }

    fn send(&mut self, slot: usize, message: ToViewer) {
        if let Some(s) = self.slot(slot).filter(|s| s.live()) {
            let launch = s.launch;
            self.effects.push(Effect::Send { slot, launch, message });
        }
    }

    /// `slot` becomes the active character. `show`: its window is brought
    /// forward (not when the user already went to it).
    fn activate(&mut self, slot: usize, show: bool, now: Instant) {
        let before = self.active.replace(slot);
        // (checkpoint 9) a monitor follows the character that is not in
        // front: this one's becomes the monitor of the one that left
        self.monitor_follows(before, slot);
        // roles first: each window knows what it is before it moves
        self.sync(Some(now));
        if show {
            let stage = self.stage_rect.filter(|_| self.stage);
            self.effects.push(Effect::AllowForeground { slot });
            self.send(slot, ToViewer::Show { stage });
            if let Some(s) = self.slot_mut(slot) {
                s.placed = Some(now);
            }
        }
        if let Some(before) = before.filter(|b| *b != slot)
            && self
                .slot(before)
                .is_some_and(|s| s.phase == Phase::Running && self.waits(s) == Standby::Economy)
        {
            self.send(before, ToViewer::Minimise);
        }
    }

    /// Sends each running viewer its assignment when it changed. `now`:
    /// when it is sent (None: nothing can have changed but the standby).
    fn sync(&mut self, now: Option<Instant>) {
        let active = self.active;
        // (with the voice not following, it stays where it is now)
        let holder = self.voice_holder();
        self.voice_home = holder.filter(|_| !self.voice_follows);
        let mute_background = self.mute_background;
        let cycling_open = self.monitors.cycling.open;
        // one taskbar button: only behind a character that is in front
        let someone_in_front = active.is_some_and(|a| self.slot(a).is_some_and(|s| s.phase == Phase::Running));
        let one_button = self.bar.one_taskbar_button && self.bar.placement == bar::Placement::Docked && someone_in_front;
        for (slot, order) in self.orders().into_iter().enumerate() {
            let voice_held_elsewhere = self.slots().any(|(i, s)| i != slot && s.voice_out);
            let Some(s) = self.slot_mut(slot).filter(|s| s.phase == Phase::Running) else {
                continue;
            };
            let is_active = active == Some(slot);
            let holds_voice = holder == Some(slot);
            let mut wanted = Assignment {
                seq: 0,
                role: match order {
                    _ if is_active => Role::Active,
                    Some(order) => Role::Monitor(order),
                    None => Role::Background,
                },
                behind: Behind3d::Off,
                standby: if cycling_open { Standby::Warm } else { s.waits() },
                // (a character that holds the voice behind is heard: a
                // viewer that is cut neither plays nor transmits it)
                sound_muted: !is_active && (mute_background || (s.sound == BackgroundSound::Muted && !holds_voice)),
                voice_allowed: holds_voice && !voice_held_elsewhere,
                off_taskbar: one_button && !is_active,
            };
            if s.sent.is_some_and(|(sent, _)| Assignment { seq: 0, ..sent } == wanted) {
                continue;
            }
            s.seq += 1;
            wanted.seq = s.seq;
            let at = now.or(s.sent.map(|(_, at)| at)).or(s.heartbeat);
            let Some(at) = at else {
                continue;
            };
            s.sent = Some((wanted, at));
            s.voice_out |= wanted.voice_allowed;
            self.send(slot, ToViewer::Assign(wanted));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::WindowState;

    fn desk(count: usize) -> (Desk, Instant) {
        (Desk::new(Config::demo(count)), Instant::now())
    }

    /// Launches a slot and has its viewer report.
    fn start(d: &mut Desk, slot: usize, activate: bool, now: Instant) -> u32 {
        d.launch(slot, activate);
        let launch = d.slot(slot).expect("slot").launch();
        d.connected(slot, launch, now);
        launch
    }

    /// The viewer's report that it holds the latest assignment sent, its
    /// voice as that assignment says (off once it gave it up).
    fn ack(d: &mut Desk, slot: usize, now: Instant) {
        let s = d.slot(slot).expect("slot");
        let (launch, sent) = (s.launch(), s.sent.expect("sent").0);
        let status = Status {
            assign_seq: sent.seq,
            screen: Screen::World,
            voice_allowed: sent.voice_allowed,
            ..s.status.clone().unwrap_or_default()
        };
        d.status(slot, launch, status, now);
    }

    fn assigned(d: &Desk, slot: usize) -> Assignment {
        d.slot(slot).expect("slot").sent.expect("sent").0
    }

    fn sends(effects: &[Effect], slot: usize) -> Vec<&ToViewer> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Send { slot: s, message, .. } if *s == slot => Some(message),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_settings_file_is_cleaned_before_use() {
        let slot = |slot, profile: &str| SlotConfig {
            slot,
            profile: profile.into(),
            standby: Standby::Warm,
            sound: BackgroundSound::Muted,
            monitor: None,
            colour: Colour::None,
            alias: None,
            monitor_window: monitor_windows::Window::default(),
        };
        let config = Config {
            slots: vec![
                slot(1, "Alt1"),
                slot(0, "Main2"),
                slot(2, "../evil"),
                slot(3, "alt1"),
                slot(1, "Other"),
                slot(9, "Far"),
                slot(4, "main"),
            ],
            stage: true,
            stage_rect: Some(Stage {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                maximised: false,
            }),
            chat_sound: true,
            hotkeys: hotkeys::Config::default(),
            panels: panels::Config::default(),
            condensed: false,
            ..Config::default()
        }
        .cleaned();
        assert!(config.chat_sound);
        assert_eq!(config.slots, vec![slot(0, "Main2"), slot(1, "Alt1")]);
        assert_eq!(config.stage_rect, None);
        // a file from a later version or half written still reads
        let read: Config = serde_json::from_str(r#"{"slots":[{"slot":0,"profile":"a"}],"more":1}"#).expect("read");
        assert_eq!(read.slots[0].standby, Standby::Warm);
        assert!(read.stage);
        assert_eq!(Config::demo(3).slots.len(), 3);
        assert_eq!(Config::demo(99).slots.len(), MAX_SLOTS);
        assert_eq!(Config::demo(0).slots[0].profile, "demo-1");
    }

    #[test]
    fn characters_are_added_up_to_five_once_each() {
        let mut d = Desk::new(Config::default());
        assert_eq!(d.add("Alt1"), Ok(0));
        assert_eq!(d.add("ALT1"), Err(AddError::Duplicate));
        assert!(matches!(d.add("a b"), Err(AddError::Name(_))));
        for i in 2..=MAX_SLOTS {
            assert_eq!(d.add(&format!("Alt{i}")), Ok(i - 1));
        }
        assert!(d.full());
        assert_eq!(d.add("Alt9"), Err(AddError::Full));
        assert!(d.take_effects().contains(&Effect::Save));
        // a hole is filled again; a running character is not removed
        assert!(d.remove(2));
        assert_eq!(d.add("Alt9"), Ok(2));
        d.launch(2, true);
        assert!(!d.remove(2));
        assert_eq!(d.config().slots.len(), MAX_SLOTS);
    }

    #[test]
    fn the_first_viewer_to_report_is_active_heard_and_shown() {
        let (mut d, now) = desk(3);
        d.launch(0, true);
        assert_eq!(d.take_effects(), vec![Effect::Spawn { slot: 0, launch: 1 }]);
        assert_eq!(d.slot(0).expect("slot").phase, Phase::Starting);
        d.connected(0, 1, now);
        assert_eq!(d.active(), Some(0));
        let a = assigned(&d, 0);
        assert_eq!((a.role, a.sound_muted, a.voice_allowed), (Role::Active, false, true));
        let effects = d.take_effects();
        // the role, then the right to the foreground, then the window
        assert_eq!(
            effects,
            vec![
                Effect::Send {
                    slot: 0,
                    launch: 1,
                    message: ToViewer::Assign(a)
                },
                Effect::AllowForeground { slot: 0 },
                Effect::Send {
                    slot: 0,
                    launch: 1,
                    message: ToViewer::Show { stage: None }
                },
            ]
        );
    }

    #[test]
    fn the_others_are_background_muted_without_voice_and_3d_off() {
        let (mut d, now) = desk(3);
        start(&mut d, 0, true, now);
        start(&mut d, 1, false, now);
        assert_eq!(d.active(), Some(0));
        let b = assigned(&d, 1);
        assert_eq!(
            (b.role, b.behind, b.sound_muted, b.voice_allowed),
            (Role::Background, Behind3d::Off, true, false)
        );
    }

    #[test]
    fn switching_revokes_the_voice_before_giving_it() {
        let (mut d, now) = desk(2);
        start(&mut d, 0, true, now);
        start(&mut d, 1, false, now);
        ack(&mut d, 0, now);
        ack(&mut d, 1, now);
        d.take_effects();

        d.switch(1, now);
        assert_eq!(d.active(), Some(1));
        let (old, new) = (assigned(&d, 0), assigned(&d, 1));
        assert_eq!((old.role, old.voice_allowed, old.sound_muted), (Role::Background, false, true));
        // active at once, but no voice yet: slot 0 has not given it up
        assert_eq!((new.role, new.sound_muted, new.voice_allowed), (Role::Active, false, false));
        assert_eq!(d.voice_waits_for(), Some(0));
        // the old character is told first
        let effects = d.take_effects();
        let first_old = effects.iter().position(|e| matches!(e, Effect::Send { slot: 0, .. }));
        let first_new = effects.iter().position(|e| matches!(e, Effect::Send { slot: 1, .. }));
        assert!(first_old < first_new);

        // slot 0 acknowledges: now slot 1 may use voice
        ack(&mut d, 0, now);
        assert!(assigned(&d, 1).voice_allowed);
        assert_eq!(d.voice_waits_for(), None);
        assert!(assigned(&d, 1).seq > new.seq);
    }

    #[test]
    fn the_voice_waits_until_the_old_character_says_its_voice_is_off() {
        let (mut d, now) = desk(2);
        start(&mut d, 0, true, now);
        start(&mut d, 1, false, now);
        ack(&mut d, 0, now);
        ack(&mut d, 1, now);
        d.switch(1, now);
        // slot 0 holds the assignment without voice, but its voice module
        // is still connected: not given up
        let s = d.slot(0).expect("slot");
        let still_on = Status {
            assign_seq: assigned(&d, 0).seq,
            screen: Screen::World,
            voice_allowed: true,
            ..Default::default()
        };
        let launch = s.launch();
        d.status(0, launch, still_on.clone(), now);
        assert_eq!(d.voice_waits_for(), Some(0));
        assert!(!assigned(&d, 1).voice_allowed);
        // an older assignment's report saying off is not enough either: the
        // voice-on one may still be in its pipe
        d.status(
            0,
            launch,
            Status {
                assign_seq: assigned(&d, 0).seq - 1,
                voice_allowed: false,
                ..still_on.clone()
            },
            now,
        );
        assert_eq!(d.voice_waits_for(), Some(0));
        // off, holding the assignment: now slot 1 may use voice
        d.status(
            0,
            launch,
            Status {
                voice_allowed: false,
                ..still_on
            },
            now,
        );
        assert_eq!(d.voice_waits_for(), None);
        assert!(assigned(&d, 1).voice_allowed);
    }

    #[test]
    fn a_viewer_that_does_not_answer_keeps_the_voice_waiting_until_it_ends() {
        let (mut d, now) = desk(2);
        let first = start(&mut d, 0, true, now);
        start(&mut d, 1, false, now);
        d.switch(1, now);
        // no acknowledgement, however long: it may still be transmitting
        let later = now + Duration::from_secs(600);
        d.tick(later);
        assert!(!assigned(&d, 1).voice_allowed);
        assert_eq!(d.voice_waits_for(), Some(0));
        // its process ends: nothing transmits any more
        d.exited(0, first, false, later);
        assert!(assigned(&d, 1).voice_allowed);
    }

    #[test]
    fn switching_back_before_the_acknowledgement_never_gives_two_voices() {
        let (mut d, now) = desk(2);
        start(&mut d, 0, true, now);
        start(&mut d, 1, false, now);
        d.switch(1, now);
        d.switch(0, now);
        assert_eq!(d.active(), Some(0));
        assert!(assigned(&d, 0).voice_allowed, "it never gave it up, and nobody else got it");
        assert!(!assigned(&d, 1).voice_allowed);
        let voices = d.slots().filter(|(i, _)| assigned(&d, *i).voice_allowed).count();
        assert_eq!(voices, 1);
    }

    #[test]
    fn economy_minimises_the_character_left_and_warm_leaves_it_behind() {
        let (mut d, now) = desk(3);
        d.set_standby(0, Standby::Economy);
        start(&mut d, 0, true, now);
        start(&mut d, 1, false, now);
        start(&mut d, 2, false, now);
        d.take_effects();
        d.switch(1, now);
        let effects = d.take_effects();
        assert!(sends(&effects, 0).contains(&&ToViewer::Minimise));
        assert_eq!(assigned(&d, 0).standby, Standby::Economy);
        d.switch(2, now);
        let effects = d.take_effects();
        assert!(!sends(&effects, 1).contains(&&ToViewer::Minimise), "Warm: left behind");
        // an Economy character started in the background is minimised too
        let (mut d, now) = desk(2);
        d.set_standby(1, Standby::Economy);
        start(&mut d, 0, true, now);
        d.take_effects();
        start(&mut d, 1, false, now);
        assert!(sends(&d.take_effects(), 1).contains(&&ToViewer::Minimise));
    }

    #[test]
    fn the_window_brought_forward_takes_the_stage_of_the_active_one() {
        let (mut d, now) = desk(2);
        let rect = Stage {
            x: 100,
            y: 50,
            width: 1600,
            height: 900,
            maximised: false,
        };
        let a = start(&mut d, 0, true, now);
        start(&mut d, 1, false, now);
        let later = now + STAGE_SETTLE;
        let report = |rect| Status {
            screen: Screen::World,
            assign_seq: 1,
            window: WindowState {
                rect: Some(rect),
                ..Default::default()
            },
            ..Default::default()
        };
        d.status(0, a, report(rect), later);
        assert_eq!(d.config().stage_rect, Some(rect));
        assert!(d.take_effects().contains(&Effect::Save));
        d.switch(1, later);
        assert!(sends(&d.take_effects(), 1).contains(&&ToViewer::Show { stage: Some(rect) }));

        // just placed, slot 1 still reports where it was: not the stage
        let elsewhere = Stage { x: 900, ..rect };
        let b = d.slot(1).expect("slot").launch();
        d.status(1, b, report(elsewhere), later + Duration::from_millis(100));
        assert_eq!(d.config().stage_rect, Some(rect));
        // minimised or full screen: not a place for the others
        let mut odd = report(elsewhere);
        odd.window.minimised = true;
        d.status(1, b, odd, later + STAGE_SETTLE * 2);
        assert_eq!(d.config().stage_rect, Some(rect));
        // moved by the user: the stage follows
        d.status(1, b, report(elsewhere), later + STAGE_SETTLE * 2);
        assert_eq!(d.config().stage_rect, Some(elsewhere));

        // with the stage off, windows stay where they are
        d.set_stage(false);
        d.take_effects();
        d.switch(0, later + STAGE_SETTLE * 3);
        assert!(sends(&d.take_effects(), 0).contains(&&ToViewer::Show { stage: None }));
    }

    #[test]
    fn the_host_follows_a_window_the_user_went_to() {
        let (mut d, now) = desk(2);
        start(&mut d, 0, true, now);
        let b = start(&mut d, 1, false, now);
        d.take_effects();
        let focused = Status {
            focused: true,
            screen: Screen::World,
            ..Default::default()
        };
        d.status(1, b, focused.clone(), now);
        assert_eq!(d.active(), Some(1));
        let effects = d.take_effects();
        assert!(
            !effects.iter().any(|e| matches!(
                e,
                Effect::Send {
                    message: ToViewer::Show { .. },
                    ..
                }
            )),
            "the user is already there: nothing is moved or raised"
        );
        assert_eq!(assigned(&d, 0).role, Role::Background);

        // the user switches back from the host while slot 1 still says it
        // has the keyboard: only a change to focused counts, no ping-pong
        d.switch(0, now);
        d.status(1, b, focused, now);
        assert_eq!(d.active(), Some(0));
    }

    #[test]
    fn the_last_character_picked_comes_forward() {
        let (mut d, now) = desk(3);
        start(&mut d, 0, true, now);
        // two starting characters picked one after the other
        d.switch(1, now);
        d.switch(2, now);
        d.connected(2, 1, now);
        assert_eq!(d.active(), Some(2));
        // the first picked reports later: it stays behind
        d.connected(1, 1, now);
        assert_eq!(d.active(), Some(2));
        // picking a running one also cancels a pick still starting
        let (mut d, now) = desk(2);
        start(&mut d, 0, true, now);
        start(&mut d, 1, false, now);
        d.close(1);
        let first = d.slot(1).expect("slot").launch();
        d.exited(1, first, true, now);
        d.switch(1, now);
        d.switch(0, now);
        d.connected(1, first + 1, now);
        assert_eq!(d.active(), Some(0));
    }

    #[test]
    fn what_comes_from_an_earlier_launch_is_dropped() {
        let (mut d, now) = desk(1);
        let first = start(&mut d, 0, true, now);
        d.exited(0, first, true, now);
        d.launch(0, true);
        let second = d.slot(0).expect("slot").launch();
        assert_ne!(first, second);
        // the old process's threads still talk
        d.connected(0, first, now);
        assert_eq!(d.slot(0).expect("slot").phase, Phase::Starting);
        d.status(0, first, Status::default(), now);
        d.heartbeat(0, first, 5, now);
        d.exited(0, first, false, now);
        d.spawn_failed(0, first);
        d.refused(0, first, Refusal::Token);
        let s = d.slot(0).expect("slot");
        assert_eq!((s.phase, s.note, s.status.is_none()), (Phase::Starting, None, true));
        d.connected(0, second, now);
        assert_eq!(d.slot(0).expect("slot").phase, Phase::Running);
    }

    #[test]
    fn a_viewer_gone_can_be_started_again_and_the_others_do_not_move() {
        let (mut d, now) = desk(2);
        let a = start(&mut d, 0, true, now);
        start(&mut d, 1, false, now);
        ack(&mut d, 1, now);
        d.take_effects();
        // the active one crashes
        d.exited(0, a, false, now);
        assert_eq!(d.slot(0).expect("slot").phase, Phase::Ended { crashed: true });
        assert_eq!(d.active(), None);
        let effects = d.take_effects();
        // slot 1 stays what it was: a background character, where it is.
        // (Checkpoint 8: with nobody in front it gets its taskbar button
        // back, the one thing its new assignment changes.)
        let kept = assigned(&d, 1);
        assert_eq!(
            sends(&effects, 1),
            vec![&ToViewer::Assign(Assignment {
                off_taskbar: false,
                ..kept
            })]
        );
        assert_eq!((kept.role, kept.sound_muted, kept.voice_allowed), (Role::Background, true, false));
        assert!(!effects.iter().any(|e| matches!(e, Effect::Kill { .. } | Effect::Spawn { .. })));
        // Restart: the same profile again, never by itself
        d.switch(0, now);
        assert_eq!(d.take_effects(), vec![Effect::Spawn { slot: 0, launch: a + 1 }]);
        d.connected(0, a + 1, now);
        assert_eq!(d.active(), Some(0));
        // closed by the user: not a crash, whatever the code
        d.close(0);
        d.exited(0, a + 1, false, now);
        assert_eq!(d.slot(0).expect("slot").phase, Phase::Ended { crashed: false });
        d.exited(1, 1, true, now);
        assert_eq!(d.slot(1).expect("slot").phase, Phase::Ended { crashed: false });
    }

    #[test]
    fn a_process_ended_at_the_user_s_request_did_not_crash() {
        let (mut d, now) = desk(1);
        let launch = start(&mut d, 0, true, now);
        d.end(0);
        assert!(d.take_effects().contains(&Effect::Kill { slot: 0 }));
        // killed: not a clean exit, yet no crash
        d.exited(0, launch, false, now);
        let s = d.slot(0).expect("slot");
        assert_eq!(s.phase, Phase::Ended { crashed: false });
        assert!(s.ended);
        // started again, a crash of that launch is one
        let launch = start(&mut d, 0, true, now);
        assert!(!d.slot(0).expect("slot").ended);
        d.exited(0, launch, false, now);
        assert_eq!(d.slot(0).expect("slot").phase, Phase::Ended { crashed: true });
    }

    #[test]
    fn slow_is_not_dead_and_nothing_is_ended_by_itself() {
        let (mut d, now) = desk(1);
        let a = start(&mut d, 0, true, now);
        assert_eq!(d.health(0, now + Duration::from_secs(25)), Health::Healthy, "still starting");
        d.status(
            0,
            a,
            Status {
                screen: Screen::World,
                ..Default::default()
            },
            now,
        );
        d.heartbeat(0, a, 1, now);
        assert_eq!(d.health(0, now + SLOW_AFTER), Health::Healthy);
        // the link thread goes on, the main loop does not turn
        let mut t = now;
        for _ in 0..20 {
            t += Duration::from_secs(1);
            d.heartbeat(0, a, 1, t);
        }
        assert_eq!(d.health(0, t), Health::Slow);
        // it turns again
        d.heartbeat(0, a, 2, t);
        assert_eq!(d.health(0, t), Health::Healthy);
        // silence
        let silent = t + SILENT_AFTER + Duration::from_secs(1);
        assert_eq!(d.health(0, silent), Health::NotAnswering);
        d.take_effects();
        d.tick(silent + Duration::from_secs(3600));
        let effects = d.take_effects();
        assert!(
            effects.iter().all(|e| matches!(e, Effect::Send { .. })),
            "an hour of silence ends nothing: {effects:?}"
        );
        assert_eq!(d.slot(0).expect("slot").phase, Phase::Running);
        // it comes back
        d.heartbeat(0, a, 3, silent);
        assert_eq!(d.health(0, silent), Health::Healthy);
        // ending it is the user's choice alone
        d.end(0);
        assert_eq!(d.take_effects(), vec![Effect::Kill { slot: 0 }]);
    }

    #[test]
    fn a_viewer_starting_for_a_minute_is_noted_never_ended() {
        let (mut d, now) = desk(1);
        d.launch(0, true);
        d.tick(now);
        assert!(!d.starting_too_long(0, now + START_NOTE_AFTER - Duration::from_secs(1)));
        assert!(d.starting_too_long(0, now + START_NOTE_AFTER));
        d.tick(now + START_NOTE_AFTER);
        assert!(
            !d.take_effects().iter().any(|e| matches!(e, Effect::Kill { .. })),
            "only the user ends it"
        );
        // a viewer let go is not starting any more: no note
        let (mut u, _) = desk(1);
        u.launch(0, true);
        u.tick(now);
        u.refused(0, 1, Refusal::Token);
        assert!(!u.starting_too_long(0, now + START_NOTE_AFTER));
        // it reports at last: no note any more
        d.connected(0, 1, now + START_NOTE_AFTER);
        assert!(!d.starting_too_long(0, now + START_NOTE_AFTER * 2));
    }

    #[test]
    fn a_starting_viewer_shows_how_long_until_its_first_frame() {
        let (mut d, now) = desk(1);
        assert_eq!(d.starting_for(0, now), None, "not started");
        d.launch(0, true);
        d.tick(now);
        let later = now + Duration::from_secs(12);
        assert_eq!(d.starting_for(0, later), Some(Duration::from_secs(12)));
        // linked, its window not drawn yet: still on its way
        d.connected(0, 1, later);
        assert_eq!(d.starting_for(0, later), Some(Duration::from_secs(12)));
        let starting = Status {
            screen: Screen::Starting,
            ..Status::default()
        };
        d.status(0, 1, starting.clone(), later);
        assert!(d.starting_for(0, later).is_some());
        // its first frame: the login screen
        let login = Status {
            screen: Screen::Login,
            ..starting
        };
        d.status(0, 1, login, later);
        assert_eq!(d.starting_for(0, later), None);
    }

    #[test]
    fn an_assignment_not_acknowledged_is_sent_again() {
        let (mut d, now) = desk(1);
        start(&mut d, 0, true, now);
        let a = assigned(&d, 0);
        d.take_effects();
        d.tick(now + RESEND_AFTER / 2);
        assert!(d.take_effects().is_empty());
        d.tick(now + RESEND_AFTER);
        assert_eq!(sends(&d.take_effects(), 0), vec![&ToViewer::Assign(a)]);
        ack(&mut d, 0, now + RESEND_AFTER);
        d.take_effects();
        d.tick(now + RESEND_AFTER * 5);
        assert!(d.take_effects().is_empty());
    }

    #[test]
    fn quit_all_asks_each_viewer_waits_then_lists_those_left() {
        let (mut d, now) = desk(3);
        let a = start(&mut d, 0, true, now);
        let b = start(&mut d, 1, false, now);
        d.take_effects();
        d.quit_all(now);
        let effects = d.take_effects();
        assert_eq!(sends(&effects, 0), vec![&ToViewer::Quit]);
        assert_eq!(sends(&effects, 1), vec![&ToViewer::Quit]);
        assert!(d.quitting());
        // nothing new starts meanwhile
        d.switch(2, now);
        assert!(d.take_effects().is_empty());
        d.exited(0, a, true, now);
        assert!(!d.take_effects().contains(&Effect::Exit));
        assert!(!d.quit_overdue(now + QUIT_WAIT / 2));
        // slot 1 asks about unsaved work: listed, never ended by itself
        assert!(d.quit_overdue(now + QUIT_WAIT));
        assert!(!d.take_effects().iter().any(|e| matches!(e, Effect::Kill { .. })));
        d.exited(1, b, true, now + QUIT_WAIT);
        assert_eq!(d.take_effects(), vec![Effect::Exit]);

        // with nothing open the host closes at once
        let (mut d, now) = desk(1);
        d.quit_all(now);
        assert_eq!(d.take_effects(), vec![Effect::Exit]);
        // given up, the viewers left stay managed
        let (mut d, now) = desk(1);
        start(&mut d, 0, true, now);
        d.quit_all(now);
        d.cancel_quit();
        assert!(!d.quitting());
        assert!(!d.slot(0).expect("slot").closing);
    }

    #[test]
    fn a_refused_or_unstarted_viewer_is_told_not_managed() {
        let (mut d, now) = desk(1);
        d.launch(0, true);
        d.take_effects();
        d.refused(0, 1, Refusal::Token);
        let s = d.slot(0).expect("slot");
        assert_eq!((s.phase, s.note), (Phase::Unmanaged, Some(Note::Refused(Refusal::Token))));
        // let go: its command pipe closes and it goes on by itself
        assert!(!s.live() && s.has_process());
        assert_eq!(d.take_effects(), vec![Effect::LetGo { slot: 0, launch: 1 }]);
        // nothing is sent to it, it is not started again or removed while
        // its process runs, and quitting does not wait for it
        d.switch(0, now);
        d.close(0);
        assert!(!d.remove(0));
        assert!(d.take_effects().is_empty());
        d.quit_all(now);
        assert_eq!(d.take_effects(), vec![Effect::Exit]);
        d.cancel_quit();
        // its end is still seen
        d.exited(0, 1, true, now);
        assert_eq!(d.slot(0).expect("slot").phase, Phase::Ended { crashed: false });
        d.launch(0, true);
        d.spawn_failed(0, 2);
        let s = d.slot(0).expect("slot");
        assert_eq!((s.phase, s.note), (Phase::Stopped, Some(Note::SpawnFailed)));
        d.note(0, Note::OpenElsewhere);
        assert_eq!(d.slot(0).expect("slot").note, Some(Note::OpenElsewhere));
        // a viewer that never reached the endpoint is let go the same way
        d.launch(0, true);
        d.take_effects();
        d.unreachable(0, 3);
        let s = d.slot(0).expect("slot");
        assert_eq!((s.phase, s.note), (Phase::Unmanaged, Some(Note::NoLink)));
        assert_eq!(d.take_effects(), vec![Effect::LetGo { slot: 0, launch: 3 }]);
        // a late refusal or report of that launch changes nothing
        d.refused(0, 3, Refusal::Token);
        d.connected(0, 3, now);
        assert_eq!(d.slot(0).expect("slot").phase, Phase::Unmanaged);
        assert!(d.take_effects().is_empty());
    }

    #[test]
    fn two_slots_on_one_account_are_told() {
        let (mut d, now) = desk(3);
        let logged = |agent: &str, grid: &str| Status {
            screen: Screen::World,
            agent: agent.into(),
            grid: grid.into(),
            ..Default::default()
        };
        for slot in 0..3 {
            start(&mut d, slot, slot == 0, now);
        }
        d.status(0, 1, logged("a1", "agni"), now);
        d.status(1, 1, logged("a1", "Agni"), now);
        d.status(2, 1, logged("a1", "osgrid"), now);
        assert_eq!(d.same_account(0), Some(1));
        assert_eq!(d.same_account(1), Some(0));
        assert_eq!(d.same_account(2), None, "the same id on another grid is another account");
        d.status(1, 1, Status::default(), now);
        assert_eq!(d.same_account(0), None, "not logged in: no account");
    }
}
