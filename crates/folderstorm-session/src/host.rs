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

use crate::behind::Behind3d;
use crate::ipc::{Assignment, MAX_SLOTS, Refusal, Role, Screen, Stage, Standby, Status, ToViewer};
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

/// One character of the host's settings file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotConfig {
    pub slot: u8,
    pub profile: String,
    #[serde(default)]
    pub standby: Standby,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            stage: true,
            stage_rect: None,
        }
    }
}

impl Config {
    /// The file as the desk can use it: profile names that pass
    /// [`ProfileName::parse`], one character per slot and per profile, at
    /// most [`MAX_SLOTS`], a stage a window can take. The rest is dropped.
    pub fn cleaned(mut self) -> Self {
        let mut kept: Vec<SlotConfig> = Vec::new();
        for slot in self.slots {
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
}

/// One character of the desk.
#[derive(Clone, Debug)]
pub struct Slot {
    pub profile: ProfileName,
    pub standby: Standby,
    pub phase: Phase,
    /// The viewer's last report.
    pub status: Option<Status>,
    pub note: Option<Note>,
    /// Quit was sent to it.
    pub closing: bool,
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
}

impl Slot {
    fn new(profile: ProfileName, standby: Standby) -> Self {
        Self {
            profile,
            standby,
            phase: Phase::Stopped,
            status: None,
            note: None,
            closing: false,
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
        }
    }

    /// Its process is there.
    pub fn live(&self) -> bool {
        matches!(self.phase, Phase::Starting | Phase::Running)
    }

    /// The number of its launch: what the host tags its threads with.
    pub fn launch(&self) -> u32 {
        self.launch
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
    quitting: Option<Instant>,
    effects: Vec<Effect>,
}

impl Desk {
    pub fn new(config: Config) -> Self {
        let config = config.cleaned();
        let mut slots: [Option<Slot>; MAX_SLOTS] = Default::default();
        for s in config.slots {
            if let (Some(place), Ok(name)) = (slots.get_mut(usize::from(s.slot)), ProfileName::parse(&s.profile)) {
                *place = Some(Slot::new(name, s.standby));
            }
        }
        Self {
            slots,
            active: None,
            stage: config.stage,
            stage_rect: config.stage_rect,
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
                })
                .collect(),
            stage: self.stage,
            stage_rect: self.stage_rect,
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
        if self.slot(slot).is_none_or(Slot::live) {
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
        let Some(s) = self.slot_mut(slot).filter(|s| !s.live()) else {
            return;
        };
        let launch = s.launch + 1;
        *s = Slot {
            launch,
            activate,
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
            if self.slot(slot).is_some_and(|s| s.standby == Standby::Economy) {
                self.send(slot, ToViewer::Minimise);
            }
        }
    }

    /// The viewer's first report was refused: it is not managed (its
    /// process goes on, as a viewer started by hand would).
    pub fn refused(&mut self, slot: usize, launch: u32, refusal: Refusal) {
        if let Some(s) = self.current(slot, launch) {
            s.note = Some(Note::Refused(refusal));
        }
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
        let Some(s) = self.current(slot, launch).filter(|s| s.live()) else {
            return;
        };
        s.phase = Phase::Ended {
            crashed: !clean && !s.closing,
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
        match self.slot(slot).map(|s| s.phase) {
            Some(Phase::Running) => self.activate(slot, true, now),
            Some(Phase::Starting) => {
                if let Some(s) = self.slot_mut(slot) {
                    s.activate = true;
                }
            }
            Some(Phase::Stopped | Phase::Ended { .. }) => self.launch(slot, true),
            None => {}
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
        if self.slot(slot).is_some_and(Slot::live) {
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

    /// Now and then: assignments not acknowledged are sent again.
    pub fn tick(&mut self, now: Instant) {
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

    /// The character whose voice the active one waits for: it was allowed
    /// voice and has not said it gave it up.
    pub fn voice_waits_for(&self) -> Option<usize> {
        let active = self.active?;
        self.slots().find(|(i, s)| *i != active && s.voice_out).map(|(i, _)| i)
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
                .is_some_and(|s| s.phase == Phase::Running && s.standby == Standby::Economy)
        {
            self.send(before, ToViewer::Minimise);
        }
    }

    /// Sends each running viewer its assignment when it changed. `now`:
    /// when it is sent (None: nothing can have changed but the standby).
    fn sync(&mut self, now: Option<Instant>) {
        let active = self.active;
        for slot in 0..MAX_SLOTS {
            let voice_held_elsewhere = self.slots().any(|(i, s)| i != slot && s.voice_out);
            let Some(s) = self.slot_mut(slot).filter(|s| s.phase == Phase::Running) else {
                continue;
            };
            let is_active = active == Some(slot);
            let mut wanted = Assignment {
                seq: 0,
                role: if is_active { Role::Active } else { Role::Background },
                behind: Behind3d::Off,
                standby: s.standby,
                sound_muted: !is_active,
                voice_allowed: is_active && !voice_held_elsewhere,
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
        }
        .cleaned();
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
        assert!(sends(&effects, 1).is_empty(), "slot 1 keeps its assignment");
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
        d.refused(0, 1, Refusal::Token);
        let s = d.slot(0).expect("slot");
        assert_eq!((s.phase, s.note), (Phase::Starting, Some(Note::Refused(Refusal::Token))));
        d.exited(0, 1, true, now);
        d.launch(0, true);
        d.spawn_failed(0, 2);
        let s = d.slot(0).expect("slot");
        assert_eq!((s.phase, s.note), (Phase::Stopped, Some(Note::SpawnFailed)));
        d.note(0, Note::OpenElsewhere);
        assert_eq!(d.slot(0).expect("slot").note, Some(Note::OpenElsewhere));
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
