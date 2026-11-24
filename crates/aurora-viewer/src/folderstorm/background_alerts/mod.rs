//! Folderstorm: alert sounds for the characters the session host keeps in
//! the background (FEATURES.md row 5.2). A background character's viewer
//! has every sound cut (the host's transient mute, `managed::sound_muted`):
//! these alerts let the user tell by ear that it got an instant message, a
//! group or conference line, a mention in nearby chat, an inventory or a
//! teleport offer. The character in front keeps its own sounds and plays
//! none of these.
//!
//! Folderstorm (C++) had the same in its session host
//! (`doc/plans/multi-character-usability.md` §4,
//! `tools/session-host/README.md` "Sound and notifications"): categories
//! per character, a chosen WAV, Preview, a host alert volume, native Do Not
//! Disturb, master or interface mute and muted senders respected, bursts
//! coalesced, everything off by default.
//!
//! Here the background character's own viewer plays its alert, on an audio
//! output of its own ([`player`]) that the host's mute does not reach:
//! Aurora's mixer cuts every bus through its master channel, and sparing
//! interface sounds there would be a change in upstream's audio paths
//! (`folderstorm/plans/session-host.md`, "Sound per character"). The viewer
//! has what the host lacks: the grid's interface sounds and the user's
//! inventory sounds, its Do Not Disturb and its blocked residents.
//!
//! - [`Shared`]: the choices of every character, in one file of the main
//!   profile's folder ([`store`]): on or off, the volume, the quiet time, a
//!   sound per kind of alert;
//! - [`Override`]: one character's own choices, in its profile's settings
//!   (`folderstorm_characters.background_alerts`): no alerts at all, or its
//!   own sound for some kinds (an inventory sound only here: an inventory is
//!   one account's);
//! - [`rules`]: which events play for which character, and the quiet time;
//! - [`files`]: the local sound files, copied into the main profile's
//!   folder, size and length limited;
//! - [`player`]: the private output and the sounds waiting for their clip.
//!
//! The `impl App` glue is in `app/folderstorm_characters/alerts.rs`; the
//! Preferences rows in `ui/options/pages/folderstorm_alerts.rs` (Sound ›
//! Interface sounds).

pub mod files;
pub mod player;
pub mod rules;
pub mod store;
#[cfg(test)]
mod tests;

use crate::ui_sound::{CATALOG, Entry};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What a background character got.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlertEvent {
    /// The first message of a new conversation with a resident.
    NewIm,
    /// Another message of a conversation with a resident.
    Im,
    /// A line of a group chat.
    Group,
    /// A line of a conference.
    Conference,
    /// A nearby chat line that mentions the character (the grid's mention
    /// link, or one of the user's keywords).
    Mention,
    /// An inventory offer.
    InventoryOffer,
    /// A teleport offer.
    TeleportOffer,
}

impl AlertEvent {
    pub const ALL: [AlertEvent; 7] = [
        Self::NewIm,
        Self::Im,
        Self::Group,
        Self::Conference,
        Self::Mention,
        Self::InventoryOffer,
        Self::TeleportOffer,
    ];

    pub fn label(self) -> &'static str {
        use aurora_i18n::tr;
        match self {
            Self::NewIm => tr!("fs-alerts-event-new-im"),
            Self::Im => tr!("fs-alerts-event-im"),
            Self::Group => tr!("fs-alerts-event-group"),
            Self::Conference => tr!("fs-alerts-event-conference"),
            Self::Mention => tr!("fs-alerts-event-mention"),
            Self::InventoryOffer => tr!("fs-alerts-event-inventory-offer"),
            Self::TeleportOffer => tr!("fs-alerts-event-teleport-offer"),
        }
    }

    /// The interface sound it plays by default, and when the chosen sound
    /// cannot be played (Firestorm's `UISnd` names).
    pub fn default_sound(self) -> &'static str {
        match self {
            Self::NewIm | Self::Im => "NewIncomingIMSession",
            Self::Group | Self::Conference => "StartIM",
            Self::Mention => "Alert",
            Self::InventoryOffer => "ObjectRezIn",
            Self::TeleportOffer => "TeleportOut",
        }
    }

    /// The asset of [`AlertEvent::default_sound`].
    pub fn fallback_asset(self) -> Uuid {
        built_in(self.default_sound()).map_or(Uuid::nil(), |e| e.uuid)
    }

    /// For the log.
    pub fn name(self) -> &'static str {
        match self {
            Self::NewIm => "new conversation",
            Self::Im => "instant message",
            Self::Group => "group chat",
            Self::Conference => "conference",
            Self::Mention => "mention",
            Self::InventoryOffer => "inventory offer",
            Self::TeleportOffer => "teleport offer",
        }
    }
}

/// A sound to play.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Sound {
    /// One of the viewer's interface sounds, by its Firestorm name
    /// (`UISnd<name>`), with its default asset (Linden Lab's library sounds
    /// every viewer plays).
    BuiltIn { name: String },
    /// A file of the sounds' folder ([`files::folder`]), by its name there.
    File { name: String },
    /// A sound of the character's own inventory, by its item id: never its
    /// asset id, which is read from the inventory when it plays and never
    /// shown.
    Inventory { item: Uuid },
    /// Nothing (also what a kind written by a later version reads as).
    #[default]
    #[serde(other)]
    Silent,
}

impl Sound {
    pub fn built_in(name: &str) -> Self {
        Self::BuiltIn { name: name.to_owned() }
    }
}

/// One kind of alert in the shared choices: whether it plays, and what.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventChoice {
    #[serde(default = "yes")]
    pub on: bool,
    #[serde(default)]
    pub sound: Sound,
}

fn yes() -> bool {
    true
}

/// A value per kind of alert.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, bound(deserialize = "T: Deserialize<'de>, PerEvent<T>: Default"))]
pub struct PerEvent<T> {
    pub new_im: T,
    pub im: T,
    pub group: T,
    pub conference: T,
    pub mention: T,
    pub inventory_offer: T,
    pub teleport_offer: T,
}

impl<T> PerEvent<T> {
    pub fn get(&self, event: AlertEvent) -> &T {
        match event {
            AlertEvent::NewIm => &self.new_im,
            AlertEvent::Im => &self.im,
            AlertEvent::Group => &self.group,
            AlertEvent::Conference => &self.conference,
            AlertEvent::Mention => &self.mention,
            AlertEvent::InventoryOffer => &self.inventory_offer,
            AlertEvent::TeleportOffer => &self.teleport_offer,
        }
    }

    pub fn get_mut(&mut self, event: AlertEvent) -> &mut T {
        match event {
            AlertEvent::NewIm => &mut self.new_im,
            AlertEvent::Im => &mut self.im,
            AlertEvent::Group => &mut self.group,
            AlertEvent::Conference => &mut self.conference,
            AlertEvent::Mention => &mut self.mention,
            AlertEvent::InventoryOffer => &mut self.inventory_offer,
            AlertEvent::TeleportOffer => &mut self.teleport_offer,
        }
    }

    fn from_fn(f: impl Fn(AlertEvent) -> T) -> Self {
        Self {
            new_im: f(AlertEvent::NewIm),
            im: f(AlertEvent::Im),
            group: f(AlertEvent::Group),
            conference: f(AlertEvent::Conference),
            mention: f(AlertEvent::Mention),
            inventory_offer: f(AlertEvent::InventoryOffer),
            teleport_offer: f(AlertEvent::TeleportOffer),
        }
    }
}

impl Default for PerEvent<EventChoice> {
    fn default() -> Self {
        Self::from_fn(|event| EventChoice {
            on: true,
            sound: Sound::built_in(event.default_sound()),
        })
    }
}

impl Default for PerEvent<Option<Sound>> {
    fn default() -> Self {
        Self::from_fn(|_| None)
    }
}

/// The quiet time's bounds and default, in seconds.
pub const COOLDOWN_MAX: u32 = 120;
pub const COOLDOWN_DEFAULT: u32 = 10;
/// The default volume (0 to 1).
pub const VOLUME_DEFAULT: f32 = 0.8;

/// The choices of every character (`background-alerts.json` in the main
/// profile's folder).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shared {
    /// Off by default, as in Folderstorm (C++).
    pub enabled: bool,
    /// 0 to 1, times the character's own Main and Interface volumes.
    pub volume: f32,
    /// The same kind of alert from the same conversation stays quiet this
    /// many seconds after it played.
    pub cooldown_secs: u32,
    pub events: PerEvent<EventChoice>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            enabled: false,
            volume: VOLUME_DEFAULT,
            cooldown_secs: COOLDOWN_DEFAULT,
            events: PerEvent::default(),
        }
    }
}

impl Shared {
    /// Within bounds, whatever the file said.
    pub fn cleaned(mut self) -> Self {
        self.volume = if self.volume.is_finite() {
            self.volume.clamp(0.0, 1.0)
        } else {
            VOLUME_DEFAULT
        };
        self.cooldown_secs = self.cooldown_secs.min(COOLDOWN_MAX);
        self
    }
}

/// One character's own choices (its profile's settings).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Override {
    /// No alerts for this character, whatever the shared choices.
    pub off: bool,
    /// Its own sound for a kind of alert; None: the shared one.
    pub sounds: PerEvent<Option<Sound>>,
}

impl Override {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The sound an event plays for a character, before the rules of the
/// moment ([`rules::decide`]): None when that character plays nothing for
/// it.
pub fn resolve(shared: &Shared, own: &Override, event: AlertEvent) -> Option<Sound> {
    let choice = shared.events.get(event);
    if !shared.enabled || own.off || !choice.on {
        return None;
    }
    let sound = own.sounds.get(event).clone().unwrap_or_else(|| choice.sound.clone());
    (sound != Sound::Silent).then_some(sound)
}

/// An interface sound by its Firestorm name, if it has a sound.
pub fn built_in(name: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.name == name && !e.uuid.is_nil())
}

/// The interface sounds offered, in the order of Preferences › Sound ›
/// Interface sounds, each sound once (several share one asset).
pub fn built_ins() -> Vec<&'static Entry> {
    let mut seen = std::collections::HashSet::new();
    CATALOG.iter().filter(|e| !e.uuid.is_nil() && seen.insert(e.uuid)).collect()
}

/// The asset an inventory sound plays: a sound of the character's own
/// inventory, a link followed, never one of the Library or of the Trash
/// (`gesture_boards::resolve`, as the gesture boards' sound tiles). The
/// Legacy Viewer plays any sound of the agent's inventory locally
/// (LLPreviewSound::auditionSound, no permission asked); the asset id is
/// only handed to the sound player, never shown nor logged.
pub fn inventory_asset(inv: &crate::world::inventory::Inventory, item: Uuid) -> Option<Uuid> {
    use crate::folderstorm::gesture_boards::{Item, Kind, resolve};
    match resolve(inv, item) {
        Item::Ready {
            original,
            kind: Kind::Sound,
            ..
        } => inv.items.get(&original).map(|it| it.asset_id).filter(|id| !id.is_nil()),
        _ => None,
    }
}

/// The sounds of the character's own inventory that an alert may play
/// ([`inventory_asset`]), by name: their item ids and names only.
pub fn inventory_sounds(inv: &crate::world::inventory::Inventory) -> Vec<(Uuid, String)> {
    let mut out: Vec<(Uuid, String)> = inv
        .items
        .values()
        .filter(|it| it.asset_type == 1 && inventory_asset(inv, it.id).is_some())
        .map(|it| (it.id, it.name.clone()))
        .collect();
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.cmp(&b.0)));
    out
}

// ---- between Preferences and the app (egui's frame data)

fn tests_id() -> egui::Id {
    egui::Id::unique("fs-alerts-tests")
}

fn inventory_id() -> egui::Id {
    egui::Id::unique("fs-alerts-inventory")
}

fn inventory_wanted_id() -> egui::Id {
    egui::Id::unique("fs-alerts-inventory-wanted")
}

/// Preferences' Test button: `sound` played now for `event`, wherever the
/// character is, at the alerts' volume.
pub fn request_test(ctx: &egui::Context, event: AlertEvent, sound: Sound) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Vec<(AlertEvent, Sound)>>(tests_id())
            .push((event, sound))
    });
}

pub fn take_tests(ctx: &egui::Context) -> Vec<(AlertEvent, Sound)> {
    ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<Vec<(AlertEvent, Sound)>>(tests_id())))
}

/// The inventory's sounds as last told by the app ([`inventory_sounds`]),
/// and a request for them while Preferences shows the choice.
pub fn inventory_list(ctx: &egui::Context) -> Option<std::sync::Arc<Vec<(Uuid, String)>>> {
    let frame = ctx.cumulative_frame_nr_for(egui::ViewportId::ROOT);
    ctx.data_mut(|d| {
        d.insert_temp(inventory_wanted_id(), frame);
        d.get_temp(inventory_id())
    })
}

/// Preferences asked for the inventory's sounds in the last frames.
pub fn inventory_wanted(ctx: &egui::Context) -> bool {
    let frame = ctx.cumulative_frame_nr_for(egui::ViewportId::ROOT);
    ctx.data(|d| d.get_temp::<u64>(inventory_wanted_id()))
        .is_some_and(|asked| asked.saturating_add(2) >= frame)
}

pub fn publish_inventory(ctx: &egui::Context, list: Vec<(Uuid, String)>) {
    ctx.data_mut(|d| d.insert_temp(inventory_id(), std::sync::Arc::new(list)));
}

/// `AURORA_DEMO_BACKGROUND_ALERTS=1`, demo only: the alerts on for the run
/// (nothing saved), the interface sounds made up; a viewer the session host
/// does not manage counts as a background character until a click in it,
/// and gets a made-up event every few seconds.
pub fn demo() -> bool {
    static VALUE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var_os("AURORA_DEMO").is_some()
            && std::env::var("AURORA_DEMO_BACKGROUND_ALERTS")
                .is_ok_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "on" | "yes"))
    })
}

/// The demo's events, in turn: a quiet time shows on the second line of a
/// conversation within it.
pub fn demo_event(count: u32) -> rules::Incoming {
    let a = Some(Uuid::from_u128(0x5a1e_0001));
    let g = Some(Uuid::from_u128(0x5a1e_0002));
    let (event, session) = match count % 9 {
        0 => (AlertEvent::NewIm, a),
        1 | 2 => (AlertEvent::Im, a),
        3 | 4 => (AlertEvent::Group, g),
        5 => (AlertEvent::Conference, Some(Uuid::from_u128(0x5a1e_0003))),
        6 => (AlertEvent::Mention, None),
        7 => (AlertEvent::InventoryOffer, None),
        _ => (AlertEvent::TeleportOffer, None),
    };
    rules::Incoming {
        event,
        session,
        sender_blocked: false,
    }
}
