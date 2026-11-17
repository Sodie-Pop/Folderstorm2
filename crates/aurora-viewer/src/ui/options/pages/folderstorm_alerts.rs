//! Folderstorm: Preferences › Sound › Interface sounds, the groups of the
//! alert sounds a character plays while the session host keeps it in the
//! background (`crate::folderstorm::background_alerts`, FEATURES.md 5.2).
//!
//! - **All characters**: on or off, the volume, the quiet time, and a
//!   sound per kind of alert, with Test. Kept in the main profile's folder
//!   (`background-alerts.json`), the same in every character's
//!   Preferences; not part of this profile's settings, so this page's
//!   reset leaves them (their own Reset button puts them back).
//! - **This character**: no alerts at all, or its own sound for some kinds,
//!   in this profile's settings. Only here may a sound come from the
//!   inventory: the character's own, by name, never its id.
//!
//! A local file is chosen in the system's dialog, checked and copied into
//! the main profile's folder (`background_alerts::files`).

use super::super::cx::{Cx, F, Nothing, field};
use super::folderstorm_search::words;
use crate::folderstorm::background_alerts::{self as alerts, AlertEvent, COOLDOWN_MAX, Shared, Sound, files, store};
use crate::ui::widgets;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Which choice a chosen file goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Shared(AlertEvent),
    Own(AlertEvent),
}

/// A file dialog is open.
static CHOOSING: AtomicBool = AtomicBool::new(false);
/// The files chosen and copied (or refused), until the page takes them.
static CHOSEN: Mutex<Vec<(Target, Result<String, files::FileError>)>> = Mutex::new(Vec::new());
/// The last refusal, shown until dismissed.
static REFUSED: Mutex<Option<String>> = Mutex::new(None);
/// The sounds' folder's files, as last listed, and when.
static LISTED: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
/// The folder is listed again at most this often while the page shows.
const LIST_EVERY: Duration = Duration::from_secs(2);
/// Inventory sounds listed at most in the choice (narrowed by its search).
const INVENTORY_SHOWN: usize = 100;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The folder's files, listed again now and then.
fn folder_files() -> Vec<String> {
    let mut listed = lock(&LISTED);
    let now = Instant::now();
    if listed
        .as_ref()
        .is_none_or(|(at, _)| now.saturating_duration_since(*at) >= LIST_EVERY)
    {
        *listed = Some((now, files::list(files::folder())));
    }
    listed.as_ref().map(|(_, names)| names.clone()).unwrap_or_default()
}

/// Opens the system's file dialog; the file is checked and copied off the
/// frame's thread.
fn choose(ctx: &egui::Context, target: Target) {
    if CHOOSING.swap(true, Ordering::SeqCst) {
        return;
    }
    let ctx = ctx.clone();
    let title = aurora_i18n::tr!("fs-alerts-choose-title").to_owned();
    let filter = aurora_i18n::tr!("fs-alerts-file-type").to_owned();
    let spawned = std::thread::Builder::new().name("fs-alerts-choose".into()).spawn(move || {
        let picked = rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter, &files::EXTENSIONS)
            .pick_file();
        if let Some(path) = picked {
            let result = files::import(&path, files::folder());
            lock(&CHOSEN).push((target, result));
            // listed again at once
            *lock(&LISTED) = None;
        }
        CHOOSING.store(false, Ordering::SeqCst);
        ctx.request_repaint();
    });
    if spawned.is_err() {
        CHOOSING.store(false, Ordering::SeqCst);
    }
}

/// This character's own sound for `event`, as a setting.
fn own_field(event: AlertEvent) -> F<Option<Sound>> {
    match event {
        AlertEvent::NewIm => field!(folderstorm_characters.background_alerts.sounds.new_im),
        AlertEvent::Im => field!(folderstorm_characters.background_alerts.sounds.im),
        AlertEvent::Group => field!(folderstorm_characters.background_alerts.sounds.group),
        AlertEvent::Conference => field!(folderstorm_characters.background_alerts.sounds.conference),
        AlertEvent::Mention => field!(folderstorm_characters.background_alerts.sounds.mention),
        AlertEvent::InventoryOffer => field!(folderstorm_characters.background_alerts.sounds.inventory_offer),
        AlertEvent::TeleportOffer => field!(folderstorm_characters.background_alerts.sounds.teleport_offer),
    }
}

/// How a sound is named in the choice: never an id.
fn describe(sound: &Sound, inventory: Option<&[(Uuid, String)]>) -> String {
    use aurora_i18n::tr;
    match sound {
        Sound::Silent => tr!("fs-alerts-silent").to_owned(),
        Sound::BuiltIn { name } => alerts::built_in(name).map_or_else(|| name.clone(), |e| (e.label)().to_owned()),
        Sound::File { name } => name.clone(),
        Sound::Inventory { item } => inventory
            .and_then(|list| list.iter().find(|(id, _)| id == item))
            .map_or_else(|| tr!("fs-alerts-inventory-unknown").to_owned(), |(_, name)| name.clone()),
    }
}

/// The choice of a sound. `inherit`: the label of "the shared one"
/// (None, a character's own choice only); `inventory`: the character's
/// inventory sounds, offered for its own choice only.
fn sound_choice(
    ui: &mut egui::Ui,
    p: &crate::theme::Palette,
    current: &mut Option<Sound>,
    inherit: Option<&str>,
    inventory: Option<&[(Uuid, String)]>,
) -> bool {
    use aurora_i18n::tr;
    let shown = match (&*current, inherit) {
        (None, Some(label)) => label.to_owned(),
        (Some(sound), _) => describe(sound, inventory),
        (None, None) => tr!("fs-alerts-silent").to_owned(),
    };
    let before = current.clone();
    let id = ui.scope_id().with("fs-alerts-sound");
    egui::ComboBox::from_id_salt(id)
        .width(super::super::COMBO_W + 40.0)
        .selected_text(shown)
        .height(420.0)
        .show_ui(ui, |ui| {
            if let Some(label) = inherit {
                ui.selectable_value(current, None, label);
            }
            ui.selectable_value(current, Some(Sound::Silent), tr!("fs-alerts-silent"));
            ui.separator();
            ui.label(egui::RichText::new(tr!("fs-alerts-built-in")).color(p.muted).small());
            for entry in alerts::built_ins() {
                let sound = Sound::built_in(entry.name);
                ui.selectable_value(current, Some(sound), (entry.label)());
            }
            let names = folder_files();
            if !names.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(tr!("fs-alerts-files")).color(p.muted).small());
                for name in names {
                    let sound = Sound::File { name: name.clone() };
                    ui.selectable_value(current, Some(sound), name);
                }
            }
            if let Some(list) = inventory {
                ui.separator();
                ui.label(egui::RichText::new(tr!("fs-alerts-inventory")).color(p.muted).small());
                let search_id = id.with("search");
                let mut search = ui.data_mut(|d| d.get_temp::<String>(search_id)).unwrap_or_default();
                ui.add(
                    egui::TextEdit::singleline(&mut search)
                        .desired_width(super::super::COMBO_W)
                        .hint_text(tr!("fs-alerts-inventory-search")),
                );
                let needle = search.trim().to_lowercase();
                let mut shown = 0;
                for (item, name) in list
                    .iter()
                    .filter(|(_, name)| needle.is_empty() || name.to_lowercase().contains(&needle))
                {
                    if shown == INVENTORY_SHOWN {
                        ui.label(egui::RichText::new(tr!("fs-alerts-inventory-more")).color(p.muted).small());
                        break;
                    }
                    shown += 1;
                    ui.selectable_value(current, Some(Sound::Inventory { item: *item }), name.as_str());
                }
                if list.is_empty() {
                    ui.label(egui::RichText::new(tr!("fs-alerts-inventory-none")).color(p.muted).small());
                }
                ui.data_mut(|d| d.insert_temp(search_id, search));
            }
        });
    *current != before
}

/// "Choose a file…" and "Test" after a choice.
fn buttons(ui: &mut egui::Ui, p: &crate::theme::Palette, target: Target, event: AlertEvent, test: Option<Sound>) {
    use aurora_i18n::tr;
    let size = egui::Vec2::new(0.0, 24.0);
    if widgets::action_button_if(
        ui,
        p,
        "folder-filled",
        tr!("fs-alerts-choose-file"),
        widgets::ActionKind::Flat,
        size,
        !CHOOSING.load(Ordering::SeqCst),
    )
    .clicked()
    {
        choose(ui.ctx(), target);
    }
    let playable = test.as_ref().is_some_and(|s| *s != Sound::Silent);
    if widgets::action_button_if(
        ui,
        p,
        "player-play-filled",
        tr!("fs-alerts-test"),
        widgets::ActionKind::Flat,
        size,
        playable,
    )
    .clicked()
        && let Some(sound) = test
    {
        alerts::request_test(ui.ctx(), event, sound);
    }
}

pub(in crate::ui::options) fn group(cx: &mut Cx) {
    use aurora_i18n::tr;
    let p = cx.p;
    let now = Instant::now();
    let mut shared = store::shared().get(now).clone();
    let before = shared.clone();
    // the files chosen since the last frame go to their choice
    for (target, result) in std::mem::take(&mut *lock(&CHOSEN)) {
        match (target, result) {
            (Target::Shared(event), Ok(name)) => shared.events.get_mut(event).sound = Sound::File { name },
            (Target::Own(event), Ok(name)) => {
                *cx.s.folderstorm_characters.background_alerts.sounds.get_mut(event) = Some(Sound::File { name });
                cx.changed = true;
            }
            (_, Err(e)) => *lock(&REFUSED) = Some(e.text()),
        }
    }
    let refused = lock(&REFUSED).clone();
    cx.group(tr!("fs-alerts-group"), |cx| {
        cx.note(tr!("fs-alerts-intro"));
        if let Some(text) = refused {
            cx.row(tr!("fs-alerts-file-refused"), "", &Nothing, |ui, _| {
                ui.label(egui::RichText::new(text).color(p.danger));
                if widgets::flat_button(ui, &p, tr!("fs-alerts-dismiss")).clicked() {
                    *lock(&REFUSED) = None;
                }
                false
            });
        }
        words(cx, &["fs-search-background-alerts"]).unfolded().row(
            tr!("fs-alerts-enable"),
            tr!("fs-alerts-enable-help"),
            &Nothing,
            |ui, _| {
                widgets::switch(ui, &p, &mut shared.enabled);
                false
            },
        );
        let off = !shared.enabled;
        cx.subs(off, |cx| {
            words(cx, &["fs-search-background-alerts"]).row(tr!("fs-alerts-volume"), tr!("fs-alerts-volume-help"), &Nothing, |ui, _| {
                let mut percent = (shared.volume * 100.0).round();
                if ui
                    .add(egui::Slider::new(&mut percent, 0.0..=100.0).step_by(1.0).suffix(" %"))
                    .changed()
                {
                    shared.volume = percent / 100.0;
                }
                false
            });
            words(cx, &["fs-search-background-alerts"]).row(
                tr!("fs-alerts-cooldown"),
                tr!("fs-alerts-cooldown-help"),
                &Nothing,
                |ui, _| {
                    ui.add(egui::Slider::new(&mut shared.cooldown_secs, 0..=COOLDOWN_MAX).suffix(format!(" {}", tr!("fs-alerts-seconds"))));
                    false
                },
            );
            for event in AlertEvent::ALL {
                words(cx, &["fs-search-background-alerts"])
                    .wide()
                    .row(event.label(), tr!("fs-alerts-event-help"), &Nothing, |ui, _| {
                        let choice = shared.events.get_mut(event);
                        widgets::switch(ui, &p, &mut choice.on);
                        let mut sound = Some(choice.sound.clone());
                        if sound_choice(ui, &p, &mut sound, None, None) {
                            choice.sound = sound.unwrap_or_default();
                        }
                        buttons(ui, &p, Target::Shared(event), event, Some(choice.sound.clone()));
                        false
                    });
            }
            cx.row(tr!("fs-alerts-reset"), tr!("fs-alerts-reset-help"), &Nothing, |ui, _| {
                if widgets::flat_button(ui, &p, tr!("common-default")).clicked() {
                    shared = Shared::default();
                }
                false
            });
        });
    });
    let character = match crate::folderstorm_characters::profile::current() {
        Some(profile) => tr!("fs-alerts-own-group", name = profile.shown()),
        None => tr!("fs-alerts-own-group-main").to_owned(),
    };
    cx.group(&character, |cx| {
        cx.note(tr!("fs-alerts-own-intro"));
        words(cx, &["fs-search-background-alerts"]).toggle(
            tr!("fs-alerts-own-off"),
            tr!("fs-alerts-own-off-help"),
            &field!(folderstorm_characters.background_alerts.off),
        );
        let off = cx.s.folderstorm_characters.background_alerts.off;
        cx.greyed(off);
        let inventory = alerts::inventory_list(cx.ui.ctx());
        for event in AlertEvent::ALL {
            let f = own_field(event);
            let get = f.get_mut;
            let shared_sound = shared.events.get(event).sound.clone();
            let inherit = tr!("fs-alerts-own-inherit", sound = describe(&shared_sound, None));
            let list = inventory.clone();
            words(cx, &["fs-search-background-alerts"])
                .wide()
                .row(event.label(), tr!("fs-alerts-own-help"), &f, |ui, s| {
                    let own = get(s);
                    let changed = sound_choice(ui, &p, own, Some(&inherit), Some(list.as_deref().map_or(&[][..], |l| &l[..])));
                    let test = own.clone().unwrap_or(shared_sound);
                    buttons(ui, &p, Target::Own(event), event, Some(test));
                    changed
                });
        }
        cx.greyed(false);
    });
    if shared != before {
        store::shared().set(shared, now);
    }
}
