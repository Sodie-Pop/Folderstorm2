//! Folderstorm: the alert sounds of a character the session host keeps in
//! the background (FEATURES.md 5.2, `crate::folderstorm::background_alerts`).
//!
//! - **Heard**: `App::update_sounds` hands over the frame's events before
//!   it plays the viewer's own sounds (one hook): the conversations'
//!   messages, the offers' interface sounds and the nearby mentions that
//!   the world recorded. Nothing in Aurora's chat or offers is hooked.
//! - **Decided** by `background_alerts::rules`: a background character
//!   whose sounds the host cut, not in Do Not Disturb, its own sound not
//!   muted by the user, the sender not blocked, out of the quiet time.
//! - **Played** on the alerts' own output (`background_alerts::player`),
//!   at the shared volume times the character's own Main and Interface
//!   volumes.

use super::*;
use crate::folderstorm::background_alerts::{self as alerts, AlertEvent, Shared, Sound, files, player, rules, store};

/// The demo's made-up events come this often.
const DEMO_EVERY: Duration = Duration::from_secs(5);
/// What the alerts may play is fetched ahead at most this often.
const PREFETCH_EVERY: Duration = Duration::from_secs(2);

impl App {
    /// `App::update_sounds`, before the frame's events are played and told:
    /// the alerts they call for.
    pub(in crate::app) fn folderstorm_background_alerts(&mut self) {
        let w = &self.world;
        let incoming = rules::collect(
            &w.im_messages,
            &w.ui_sounds,
            &w.attention,
            |id| w.mutes.is_muted_id(id),
            |session| {
                w.social
                    .ims
                    .iter()
                    .find(|s| s.other == *session)
                    .and_then(|s| s.lines.back())
                    .map(|l| l.source)
                    .filter(|id| !id.is_nil())
            },
        );
        if !incoming.is_empty() {
            self.folderstorm_alerts_decide(incoming, Instant::now());
        }
    }

    /// The shared choices, on for the run in the demo.
    fn folderstorm_alerts_shared(now: Instant) -> Shared {
        let mut shared = store::shared().get(now).clone();
        if alerts::demo() {
            shared.enabled = true;
        }
        shared
    }

    fn folderstorm_alerts_decide(&mut self, incoming: Vec<rules::Incoming>, now: Instant) {
        let shared = Self::folderstorm_alerts_shared(now);
        let own = self.settings.folderstorm_characters.background_alerts.clone();
        let place = self.folderstorm_alerts_place();
        let quiet = self.folderstorm_alerts_quiet();
        for event in incoming {
            let cooldown = &mut self.folderstorm_characters.alerts.cooldown;
            match rules::decide(&shared, &own, place, quiet, event, cooldown, now) {
                rules::Verdict::Play(sound) => {
                    log::info!("background alert: {}", event.event.name());
                    self.folderstorm_alerts_play(event.event, &sound, &shared, now);
                }
                verdict => log::debug!(target: "fs_alerts", "{}: {verdict:?}", event.event.name()),
            }
        }
    }

    /// Where this window is for the session host; in the demo, a window no
    /// host manages counts as a background character until a click in it.
    fn folderstorm_alerts_place(&self) -> rules::Place {
        let managed = &self.folderstorm_characters.managed;
        if alerts::demo() && !managed.linked() && !self.folderstorm_characters.alerts_demo.0 {
            return rules::Place {
                managed: true,
                background: true,
                focused: false,
                sound_cut: true,
            };
        }
        rules::Place {
            managed: managed.linked(),
            background: managed.role().is_some_and(|role| role.background()),
            focused: self.folderstorm_focused(),
            sound_cut: managed.sound_muted(),
        }
    }

    /// Do Not Disturb, or the character's own sound muted by the user (its
    /// saved Main or Interface volume muted or off; never the host's
    /// transient mute, which is the reason for the alerts).
    fn folderstorm_alerts_quiet(&self) -> rules::Quiet {
        let audio = &self.settings.audio;
        rules::Quiet {
            dnd: self.world.status.dnd,
            muted: audio.muted[0] || audio.muted[1] || !audio.enabled[1],
        }
    }

    /// The alerts' volume times the character's own Main and Interface.
    fn folderstorm_alerts_gain(&self, shared: &Shared) -> f32 {
        let audio = &self.settings.audio;
        shared.volume * audio.volume[0] * audio.volume[1]
    }

    /// What a sound plays; None when it cannot be had here (an unknown
    /// interface sound, a bad file name, an inventory sound not in this
    /// character's inventory or not one an alert may play).
    fn folderstorm_alerts_clip(&self, sound: &Sound) -> Option<player::Clip> {
        match sound {
            Sound::Silent => None,
            Sound::BuiltIn { name } => alerts::built_in(name).map(|e| player::Clip::Asset(e.uuid)),
            Sound::File { name } => files::valid_name(name).then(|| player::Clip::File(name.clone())),
            Sound::Inventory { item } => alerts::inventory_asset(&self.world.inventory, *item).map(player::Clip::Asset),
        }
    }

    fn folderstorm_alerts_play(&mut self, event: AlertEvent, sound: &Sound, shared: &Shared, now: Instant) {
        let fallback = event.fallback_asset();
        let clip = self.folderstorm_alerts_clip(sound).unwrap_or(player::Clip::Asset(fallback));
        let gain = self.folderstorm_alerts_gain(shared);
        self.folderstorm_characters.alerts.play(clip, fallback, gain, event.name(), now);
    }

    /// Every frame: the shared choices' pending write, Preferences' Test
    /// buttons and inventory sounds, what is fetched ahead, the demo, and
    /// the sounds waiting for their clip or the output.
    pub(super) fn folderstorm_alerts_tick(&mut self) {
        let now = Instant::now();
        store::shared().flush(now);
        let tests = alerts::take_tests(&self.egui_ctx);
        if !tests.is_empty() {
            let shared = Self::folderstorm_alerts_shared(now);
            for (event, sound) in tests {
                log::info!("background alerts: test of the {} sound", event.name());
                self.folderstorm_alerts_play(event, &sound, &shared, now);
            }
        }
        if alerts::inventory_wanted(&self.egui_ctx) {
            let generation = self.world.inventory.generation;
            let known = &mut self.folderstorm_characters.alerts_inventory;
            if *known != Some(generation) {
                *known = Some(generation);
                alerts::publish_inventory(&self.egui_ctx, alerts::inventory_sounds(&self.world.inventory));
            }
        }
        if alerts::demo() {
            self.folderstorm_alerts_demo(now);
        }
        self.folderstorm_alerts_prefetch(now);
        let device = self.settings.audio.output_device.clone();
        let played = self
            .folderstorm_characters
            .alerts
            .tick(&mut self.scene.sounds, &device, files::folder(), now);
        for outcome in played {
            match outcome {
                player::Played::Played(what) => log::debug!(target: "fs_alerts", "{what}: played"),
                player::Played::FellBack(what) => {
                    log::info!("background alerts: the {what} sound cannot be had, its default plays instead")
                }
                player::Played::Dropped(what) => log::info!("background alerts: the {what} sound did not play"),
            }
        }
    }

    /// While alerts may play here, what they would play is fetched ahead.
    fn folderstorm_alerts_prefetch(&mut self, now: Instant) {
        let checked = &mut self.folderstorm_characters.alerts_prefetched;
        if checked.is_some_and(|at| now.saturating_duration_since(at) < PREFETCH_EVERY) {
            return;
        }
        *checked = Some(now);
        if !self.folderstorm_alerts_place().plays() || !self.in_world() {
            return;
        }
        let shared = Self::folderstorm_alerts_shared(now);
        let own = &self.settings.folderstorm_characters.background_alerts;
        let mut clips = Vec::new();
        for event in AlertEvent::ALL {
            let Some(sound) = alerts::resolve(&shared, own, event) else {
                continue;
            };
            for clip in [self.folderstorm_alerts_clip(&sound), Some(player::Clip::Asset(event.fallback_asset()))]
                .into_iter()
                .flatten()
            {
                if !clips.contains(&clip) {
                    clips.push(clip);
                }
            }
        }
        self.folderstorm_characters
            .alerts
            .prefetch(&mut self.scene.sounds, files::folder(), clips);
    }

    /// `AURORA_DEMO_BACKGROUND_ALERTS`: the interface sounds made up once;
    /// outside the session host, a made-up event every few seconds until a
    /// click in the window.
    fn folderstorm_alerts_demo(&mut self, now: Instant) {
        let state = &mut self.folderstorm_characters.alerts_demo;
        if state.1.is_none() {
            log::info!("background alerts demo: alerts on for the run");
            for entry in alerts::built_ins() {
                self.scene.sounds.insert(entry.uuid, crate::demo::ui_sound(entry.uuid));
            }
        }
        let managed = self.folderstorm_characters.managed.linked();
        let state = &mut self.folderstorm_characters.alerts_demo;
        if !managed && !state.0 && self.egui_ctx.input(|i| i.pointer.any_pressed()) {
            log::info!("background alerts demo: clicked, this window is in front now");
            state.0 = true;
        }
        let (due, count) = state.1.unwrap_or((now + DEMO_EVERY / 2, 0));
        if now < due || managed || !self.in_world() {
            state.1 = Some((due, count));
            return;
        }
        state.1 = Some((now + DEMO_EVERY, count + 1));
        let incoming = alerts::demo_event(count);
        log::info!("background alerts demo: a made-up {}", incoming.event.name());
        self.folderstorm_alerts_decide(vec![incoming], now);
    }
}
