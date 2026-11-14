//! The session host's MCP controller, its pure rules
//! (folderstorm/plans/mcp-routing.md, section 3): what the host's settings
//! allow, which character a call names, whether that character can do what
//! is asked now, what the list shows (and what only with metadata), and the
//! questions waiting in the Ask dialog, decided again at every tick.
//!
//! A separate authority from each viewer's local assistant: nothing here
//! reads or changes a viewer's settings, logs anyone in or touches a
//! credential. Every action is the click the host's own window already has
//! (Switch, Warm / Economy, a monitor, "Open a login", Close), plus restart
//! (Close, then start again) and stop managing (let the viewer go).
//!
//! The listener (`folderstorm_mcp_bridge::controller`) only refuses a class
//! on Never at once; the host decides every call again here, when it takes
//! it ([`admit`]), and while it waits in the dialog ([`Questions::review`]).

use crate::host::{Desk, Phase, Quick, Slot};
use crate::ipc::{Screen, Standby};
use crate::profile::ProfileName;
use folderstorm_mcp_bridge::controller::{HostClass, HostLevels};
use folderstorm_mcp_bridge::policy::Level;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long a question waits for the user before it is refused.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(60);

/// The longest profile name a call may carry (a profile's own limit).
const PROFILE_MAX: usize = 32;

/// The controller's settings (`session-host.json`, `controller`). Off,
/// no metadata, every class Ask; each character's own access is with the
/// character (`SlotConfig::controller`, off).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "Enable controller MCP": the endpoint listens.
    pub enabled: bool,
    /// "Allow profile and monitor metadata": names, aliases, colours and
    /// monitor details in the list.
    pub metadata: bool,
    pub levels: HostLevels,
}

/// What a call asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// `host_characters`.
    List,
    Activate,
    Background(Standby),
    /// A character's monitor opens (true) or closes.
    Monitor(bool),
    /// The cycling monitor opens or closes.
    Cycling(bool),
    OpenLogin,
    Restart,
    Logout,
    StopManaging,
}

impl Action {
    /// The class it needs; None for the list.
    pub fn class(self) -> Option<HostClass> {
        Some(match self {
            Action::List => return None,
            Action::Activate => HostClass::Activate,
            Action::Background(_) => HostClass::BackgroundMode,
            Action::Monitor(_) | Action::Cycling(_) => HostClass::Monitors,
            Action::OpenLogin => HostClass::OpenLogin,
            Action::Restart => HostClass::Restart,
            Action::Logout => HostClass::Logout,
            Action::StopManaging => HostClass::StopManaging,
        })
    }

    /// Whether it names a character.
    fn targets(self) -> bool {
        !matches!(self, Action::List | Action::Cycling(_))
    }
}

/// A call, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub action: Action,
    /// The character, by its profile name.
    pub profile: Option<String>,
    /// The launch the assistant saw, if it gave it.
    pub launch: Option<u32>,
}

/// Why a character cannot do what is asked now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Not logged in (activate, a monitor).
    NotInWorld,
    /// Already logged in (open a login).
    LoggedIn,
    /// No viewer runs (log out, stop managing, restart a character never
    /// started).
    NoViewer,
    /// Its viewer is on its way.
    Starting,
    /// Its viewer is closing.
    Closing,
    /// Its viewer runs, let go by the host.
    Unmanaged,
    /// It is the active character (a monitor).
    Active,
    /// "Quit all" is under way.
    Quitting,
}

/// Why a call is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The controller is off.
    Off,
    /// The class is on Never.
    Never,
    /// The user refused it in the dialog (or closed the dialog).
    Refused,
    /// 60 s without an answer.
    TimedOut,
    /// No character with controller access has that profile.
    NotFound,
    /// The character's viewer was started again since (another launch).
    Restarted,
    /// The arguments.
    Invalid(&'static str),
    NotReady(Why),
}

/// What [`admit`] lets through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admitted {
    /// The list: answered at once.
    List,
    /// An action on that character (None: the cycling monitor), run at once
    /// (`ask` false) or after the user's Allow.
    Act { slot: Option<usize>, ask: bool },
}

/// The character tools' names, with the action each asks.
fn action_of(tool: &str, args: &Map<String, Value>) -> Result<Action, Refusal> {
    let open = || {
        args.get("open")
            .and_then(Value::as_bool)
            .ok_or(Refusal::Invalid("`open` (true or false) is missing"))
    };
    Ok(match tool {
        "host_characters" => Action::List,
        "host_activate" => Action::Activate,
        "host_background_mode" => match args.get("mode").and_then(Value::as_str) {
            Some("warm") => Action::Background(Standby::Warm),
            Some("economy") => Action::Background(Standby::Economy),
            _ => return Err(Refusal::Invalid("`mode` must be \"warm\" or \"economy\"")),
        },
        "host_monitor" => match args.get("cycling") {
            None | Some(Value::Bool(false)) => Action::Monitor(open()?),
            Some(Value::Bool(true)) => Action::Cycling(open()?),
            Some(_) => return Err(Refusal::Invalid("`cycling` must be true or false")),
        },
        "host_open_login" => Action::OpenLogin,
        "host_restart" => Action::Restart,
        "host_logout" => Action::Logout,
        "host_stop_managing" => Action::StopManaging,
        _ => return Err(Refusal::Invalid("unknown controller tool")),
    })
}

/// Read a call's arguments.
pub fn parse(tool: &str, arguments: &Value) -> Result<Request, Refusal> {
    let empty = Map::new();
    let args = match arguments {
        Value::Object(map) => map,
        Value::Null => &empty,
        _ => return Err(Refusal::Invalid("the arguments must be an object")),
    };
    let action = action_of(tool, args)?;
    let allowed: &[&str] = match action {
        Action::List => &[],
        Action::Background(_) => &["profile", "launch", "mode"],
        Action::Monitor(_) | Action::Cycling(_) => &["profile", "launch", "cycling", "open"],
        _ => &["profile", "launch"],
    };
    if args.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(Refusal::Invalid("an argument this tool does not take"));
    }
    let profile = match args.get("profile") {
        None => None,
        Some(Value::String(p)) if !p.trim().is_empty() && p.chars().count() <= PROFILE_MAX => Some(p.trim().to_owned()),
        Some(_) => return Err(Refusal::Invalid("`profile` must be a profile name from host_characters")),
    };
    let launch = match args.get("launch") {
        None => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|l| u32::try_from(l).ok())
                .filter(|l| *l >= 1)
                .ok_or(Refusal::Invalid("`launch` must be a launch number from host_characters"))?,
        ),
    };
    match (action.targets(), &profile) {
        (true, None) => return Err(Refusal::Invalid("`profile` is missing: name the character by its profile")),
        (false, Some(_)) if matches!(action, Action::Cycling(_)) => {
            return Err(Refusal::Invalid("give `profile` or `cycling`, not both"));
        }
        _ => {}
    }
    Ok(Request {
        action,
        profile,
        launch: launch.filter(|_| action.targets()),
    })
}

/// The slot of the character with controller access that has that
/// profile name.
pub fn resolve(desk: &Desk, profile: &str) -> Result<usize, Refusal> {
    let name = ProfileName::parse(profile).map_err(|_| Refusal::NotFound)?;
    desk.slots()
        .find(|(_, s)| s.controller && s.profile.key() == name.key())
        .map(|(slot, _)| slot)
        .ok_or(Refusal::NotFound)
}

/// Whether that character can do `action` now.
pub fn eligible(desk: &Desk, slot: usize, action: Action) -> Result<(), Refusal> {
    let s = desk.slot(slot).ok_or(Refusal::NotFound)?;
    let not = |why| Err(Refusal::NotReady(why));
    let viewer = |s: &Slot| match s.phase {
        Phase::Unmanaged => Some(Why::Unmanaged),
        Phase::Stopped | Phase::Ended { .. } => Some(Why::NoViewer),
        _ if s.closing => Some(Why::Closing),
        _ => None,
    };
    match action {
        Action::List | Action::Background(_) | Action::Cycling(_) | Action::Monitor(false) => Ok(()),
        Action::Activate => match viewer(s) {
            Some(why) => not(why),
            None if s.in_world() => Ok(()),
            None if s.phase == Phase::Starting => not(Why::Starting),
            None => not(Why::NotInWorld),
        },
        Action::Monitor(true) => match viewer(s) {
            Some(why) => not(why),
            None if desk.active() == Some(slot) => not(Why::Active),
            None if s.in_world() => Ok(()),
            None => not(Why::NotInWorld),
        },
        Action::OpenLogin => match desk.quick(slot) {
            Quick::Launch | Quick::Restart | Quick::LogIn => Ok(()),
            Quick::Wait => not(Why::Starting),
            Quick::Switch | Quick::Show => not(Why::LoggedIn),
            Quick::Nothing => not(viewer(s).unwrap_or(Why::Quitting)),
        },
        Action::Restart => {
            if desk.quitting() {
                return not(Why::Quitting);
            }
            match s.phase {
                Phase::Ended { .. } => Ok(()),
                Phase::Stopped => not(Why::NoViewer),
                Phase::Unmanaged => not(Why::Unmanaged),
                _ if s.closing => not(Why::Closing),
                Phase::Starting => not(Why::Starting),
                Phase::Running => Ok(()),
            }
        }
        Action::Logout | Action::StopManaging => match viewer(s) {
            Some(why) => not(why),
            None => Ok(()),
        },
    }
}

/// Decide a call when the host takes it: the settings, the character, its
/// launch, what it can do now.
pub fn admit(settings: &Settings, desk: &Desk, request: &Request) -> Result<Admitted, Refusal> {
    if !settings.enabled {
        return Err(Refusal::Off);
    }
    let Some(class) = request.action.class() else {
        return Ok(Admitted::List);
    };
    let level = settings.levels.level(class);
    if level == Level::Never {
        return Err(Refusal::Never);
    }
    let slot = match &request.profile {
        Some(profile) if request.action.targets() => {
            let slot = resolve(desk, profile)?;
            if request
                .launch
                .is_some_and(|launch| desk.slot(slot).is_some_and(|s| s.launch() != launch))
            {
                return Err(Refusal::Restarted);
            }
            eligible(desk, slot, request.action)?;
            Some(slot)
        }
        _ => None,
    };
    Ok(Admitted::Act {
        slot,
        ask: level == Level::Ask,
    })
}

/// Carry an admitted action out on the desk: the same calls as the host's
/// own clicks.
pub fn apply(desk: &mut Desk, slot: Option<usize>, action: Action, now: Instant) {
    match (action, slot) {
        (Action::Cycling(open), _) => desk.set_cycling(open),
        (Action::List, _) | (_, None) => {}
        (Action::Activate, Some(slot)) => desk.switch(slot, now),
        (Action::Background(standby), Some(slot)) => desk.set_standby(slot, standby),
        (Action::Monitor(true), Some(slot)) => desk.open_monitor(slot),
        (Action::Monitor(false), Some(slot)) => desk.set_monitor(slot, None),
        (Action::OpenLogin, Some(slot)) => {
            desk.quick_act(slot, now);
        }
        (Action::Restart, Some(slot)) => desk.restart(slot),
        (Action::Logout, Some(slot)) => desk.close(slot),
        (Action::StopManaging, Some(slot)) => {
            desk.release(slot);
        }
    }
}

/// A character's state as the list says it.
pub fn state(s: &Slot) -> &'static str {
    match s.phase {
        Phase::Stopped => "stopped",
        Phase::Starting => "starting",
        Phase::Unmanaged => "unmanaged",
        Phase::Ended { crashed: true } => "crashed",
        Phase::Ended { crashed: false } => "ended",
        Phase::Running => match s.status.as_ref().map(|st| st.screen) {
            None | Some(Screen::Starting) => "starting",
            Some(Screen::Login) => "login",
            Some(Screen::Loading) => "loading",
            Some(Screen::World) => "in_world",
        },
    }
}

/// One character's row; the names and the monitor only with metadata.
pub fn row(desk: &Desk, slot: usize, metadata: bool) -> Option<Value> {
    let s = desk.slot(slot)?;
    let mut row = json!({
        "slot": slot + 1,
        "profile": s.profile.shown(),
        "state": state(s),
        "active": desk.active() == Some(slot),
        "closing": s.closing,
        "background_mode": match s.standby {
            Standby::Warm => "warm",
            Standby::Economy => "economy",
        },
    });
    if s.launch() > 0 {
        row["launch"] = json!(s.launch());
    }
    if metadata {
        if let Some(name) = s.status.as_ref().map(|st| st.name.as_str()).filter(|n| !n.is_empty()) {
            row["character"] = json!(name);
        }
        if let Some(alias) = &s.alias {
            row["alias"] = json!(alias);
        }
        row["colour"] = serde_json::to_value(s.colour).unwrap_or(Value::Null);
        row["monitor"] = match s.monitor {
            None => json!({"open": false}),
            Some(spec) => json!({
                "open": true,
                "size": serde_json::to_value(spec.size).unwrap_or(Value::Null),
                "rate": serde_json::to_value(spec.rate).unwrap_or(Value::Null),
                "watch": watch_word(desk, slot),
            }),
        };
    }
    Some(row)
}

fn watch_word(desk: &Desk, slot: usize) -> &'static str {
    use crate::host::Watch;
    match desk.watch(slot) {
        Watch::Closed => "closed",
        Watch::NoViewer => "no_viewer",
        Watch::InFront => "in_front",
        Watch::Hidden => "hidden",
        Watch::TooMany => "too_many",
        Watch::Asked { lowered: true, .. } => "lowered",
        Watch::Asked { lowered: false, .. } => "live",
    }
}

/// `host_characters`: the characters with controller access.
pub fn list(settings: &Settings, desk: &Desk) -> Value {
    let rows: Vec<Value> = desk
        .slots()
        .filter(|(_, s)| s.controller)
        .filter_map(|(slot, _)| row(desk, slot, settings.metadata))
        .collect();
    let mut answer = json!({
        "characters": rows,
        "metadata": settings.metadata,
        "quitting": desk.quitting(),
    });
    if settings.metadata {
        answer["cycling_monitor"] = json!({"open": desk.monitors().cycling.open});
    }
    answer
}

/// A question waiting in the dialog.
#[derive(Debug)]
pub struct Question<T> {
    pub number: u64,
    /// The character (None: the cycling monitor), and its launch then.
    pub slot: Option<usize>,
    pub launch: u32,
    pub request: Request,
    /// The name the MCP client gives itself.
    pub client: Option<String>,
    pub asked: Instant,
    pub payload: T,
}

impl<T> Question<T> {
    /// Whole seconds left before it is refused.
    pub fn seconds_left(&self, now: Instant) -> u64 {
        ASK_TIMEOUT.saturating_sub(now.saturating_duration_since(self.asked)).as_secs()
    }
}

/// The questions, oldest first; one dialog at a time.
#[derive(Debug)]
pub struct Questions<T> {
    waiting: VecDeque<Question<T>>,
    next: u64,
}

impl<T> Default for Questions<T> {
    fn default() -> Self {
        Self {
            waiting: VecDeque::new(),
            next: 1,
        }
    }
}

impl<T> Questions<T> {
    /// A call that asks: it waits for the dialog.
    pub fn ask(&mut self, desk: &Desk, slot: Option<usize>, request: Request, client: Option<String>, payload: T, now: Instant) -> u64 {
        let number = self.next;
        self.next += 1;
        let launch = slot.and_then(|slot| desk.slot(slot)).map_or(0, Slot::launch);
        self.waiting.push_back(Question {
            number,
            slot,
            launch,
            request,
            client,
            asked: now,
            payload,
        });
        number
    }

    /// The question the dialog shows.
    pub fn front(&self) -> Option<&Question<T>> {
        self.waiting.front()
    }

    pub fn len(&self) -> usize {
        self.waiting.len()
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    /// The user answered question `number`: taken out (None when it is
    /// gone already: a click after its refusal does nothing).
    pub fn take(&mut self, number: u64) -> Option<Question<T>> {
        let index = self.waiting.iter().position(|q| q.number == number)?;
        self.waiting.remove(index)
    }

    /// Every question, taken out (the controller stops).
    pub fn drain(&mut self) -> Vec<Question<T>> {
        self.waiting.drain(..).collect()
    }

    /// Decided again: the questions refused now, taken out. An Ask turned to
    /// Allow still asks; anything else that would refuse the call when it
    /// is taken refuses it, and so do 60 s.
    pub fn review(&mut self, settings: &Settings, desk: &Desk, now: Instant) -> Vec<(Question<T>, Refusal)> {
        let mut refused = Vec::new();
        let mut kept = VecDeque::new();
        for q in self.waiting.drain(..) {
            match still(settings, desk, &q, now) {
                Ok(()) => kept.push_back(q),
                Err(why) => refused.push((q, why)),
            }
        }
        self.waiting = kept;
        refused
    }
}

/// Whether a question may still be allowed: the settings, the same
/// character on the same launch, what it can do now, the time.
pub fn still<T>(settings: &Settings, desk: &Desk, q: &Question<T>, now: Instant) -> Result<(), Refusal> {
    match admit(settings, desk, &q.request)? {
        Admitted::List => {}
        Admitted::Act { slot, .. } => {
            if slot != q.slot {
                return Err(Refusal::NotFound);
            }
            if slot.and_then(|slot| desk.slot(slot)).is_some_and(|s| s.launch() != q.launch) {
                return Err(Refusal::Restarted);
            }
        }
    }
    if now.saturating_duration_since(q.asked) >= ASK_TIMEOUT {
        return Err(Refusal::TimedOut);
    }
    Ok(())
}

#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
