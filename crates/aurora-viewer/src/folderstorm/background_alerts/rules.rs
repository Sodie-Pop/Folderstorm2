//! Which events play an alert for which character, and the quiet time.
//! Pure: the app hands in what happened and where the window is.

use super::{AlertEvent, Override, Shared, Sound, resolve};
use crate::ui_sound::{ImKind, ImMessage, UiSound};
use crate::world::attention::{Attention, Cause, Open};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Two alerts of one character never play closer than this, whatever
/// they are (a message and its mention, an offer sent as an IM): the
/// second is dropped, as Folderstorm (C++) coalesced bursts.
pub const MIN_GAP: Duration = Duration::from_secs(1);

/// Where the character's window is, for the session host.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Place {
    /// A session host manages this viewer.
    pub managed: bool,
    /// The host keeps it in the background (not the active character).
    pub background: bool,
    /// The window has the keyboard: the user went to it, it is in front.
    pub focused: bool,
    /// The host cut its sounds (`managed::sound_muted`). A background
    /// character "heard in the background" keeps its own interface sounds,
    /// and plays no alert besides them.
    pub sound_cut: bool,
}

impl Place {
    /// Alerts play here: a character the host keeps in the background,
    /// whose sounds are cut, that the user is not in.
    pub fn plays(self) -> bool {
        self.managed && self.background && !self.focused && self.sound_cut
    }
}

/// What keeps the character quiet whatever happens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quiet {
    /// Do Not Disturb (no conversation sound in it, LLIMMgr::addMessage).
    pub dnd: bool,
    /// The user muted its sound: Main or Interface muted or turned off in
    /// its own volumes (saved; not the host's transient mute).
    pub muted: bool,
}

/// One event, as the rules see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Incoming {
    pub event: AlertEvent,
    /// Its conversation (the quiet time is per kind and conversation).
    pub session: Option<Uuid>,
    /// Said by someone the character blocked.
    pub sender_blocked: bool,
}

/// What [`decide`] made of an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Play(Sound),
    /// Alerts off, this kind off, this character's alerts off, or silent.
    Off,
    /// The character is in front, or not the host's background one.
    Front,
    /// Do Not Disturb, or the user muted its sound.
    Quiet,
    /// From someone blocked.
    Blocked,
    /// Within the quiet time.
    Cooling,
}

/// The quiet time: when each kind played for each conversation, and when
/// the last alert played.
#[derive(Debug, Default)]
pub struct Cooldown {
    last: HashMap<(AlertEvent, Option<Uuid>), Instant>,
    any: Option<Instant>,
}

impl Cooldown {
    /// Whether an alert may play now; if so, it counts as played.
    pub fn allow(&mut self, event: AlertEvent, session: Option<Uuid>, now: Instant, cooldown: Duration) -> bool {
        if self.any.is_some_and(|at| now.saturating_duration_since(at) < MIN_GAP) {
            return false;
        }
        let key = (event, session);
        if self.last.get(&key).is_some_and(|at| now.saturating_duration_since(*at) < cooldown) {
            return false;
        }
        // (bounded: what is past its quiet time is forgotten)
        self.last.retain(|_, at| now.saturating_duration_since(*at) < cooldown.max(MIN_GAP));
        self.last.insert(key, now);
        self.any = Some(now);
        true
    }
}

/// What an event plays for this character now. The quiet time counts only
/// the alerts that play.
pub fn decide(
    shared: &Shared,
    own: &Override,
    place: Place,
    quiet: Quiet,
    incoming: Incoming,
    cooldown: &mut Cooldown,
    now: Instant,
) -> Verdict {
    let Some(sound) = resolve(shared, own, incoming.event) else {
        return Verdict::Off;
    };
    if !place.plays() {
        return Verdict::Front;
    }
    if quiet.dnd || quiet.muted {
        return Verdict::Quiet;
    }
    if incoming.sender_blocked {
        return Verdict::Blocked;
    }
    let wait = Duration::from_secs(u64::from(shared.cooldown_secs.min(super::COOLDOWN_MAX)));
    if !cooldown.allow(incoming.event, incoming.session, now, wait) {
        return Verdict::Cooling;
    }
    Verdict::Play(sound)
}

/// The events of a frame, from what the world recorded for the viewer's
/// own sounds and attention before they are played and told
/// (`App::update_sounds`, then `update_attention`):
/// - `ims`: the conversation messages (`World::im_messages`);
/// - `ui_sounds`: the interface sounds asked for (`World::ui_sounds`): an
///   inventory offer and a teleport offer each ask for theirs whether or
///   not it plays;
/// - `attention`: a nearby chat line that mentions the character, or holds
///   one of its keywords (`World::attention`; the blocked never get there);
/// - `blocked`: whether a resident is blocked; `sender`: who wrote the
///   last line of a conversation.
pub fn collect(
    ims: &[ImMessage],
    ui_sounds: &[UiSound],
    attention: &[Attention],
    blocked: impl Fn(&Uuid) -> bool,
    sender: impl Fn(&Uuid) -> Option<Uuid>,
) -> Vec<Incoming> {
    let mut out = Vec::new();
    for m in ims {
        let event = match m.kind {
            ImKind::Private if m.new_session => AlertEvent::NewIm,
            ImKind::Private => AlertEvent::Im,
            ImKind::Group => AlertEvent::Group,
            ImKind::Conference => AlertEvent::Conference,
        };
        // a resident's conversation is named after them; a group's or a
        // conference's line is its last one's
        let from = match m.kind {
            ImKind::Private => Some(m.session),
            ImKind::Group | ImKind::Conference => sender(&m.session),
        };
        out.push(Incoming {
            event,
            session: Some(m.session),
            sender_blocked: from.is_some_and(|id| blocked(&id)),
        });
    }
    let mention = attention
        .iter()
        .any(|a| matches!(a.cause, Cause::Mention | Cause::Keyword) && a.notice.as_ref().is_some_and(|n| n.open == Open::NearbyChat));
    if mention {
        out.push(Incoming {
            event: AlertEvent::Mention,
            session: None,
            sender_blocked: false,
        });
    }
    for (sound, event) in [
        (UiSound::InventoryOffer, AlertEvent::InventoryOffer),
        (UiSound::TeleportOffer, AlertEvent::TeleportOffer),
    ] {
        if ui_sounds.contains(&sound) {
            out.push(Incoming {
                event,
                session: None,
                sender_blocked: false,
            });
        }
    }
    out
}
