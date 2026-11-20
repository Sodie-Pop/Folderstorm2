use super::player::{Got, Next, next};
use super::rules::{Cooldown, Incoming, MIN_GAP, Place, Quiet, Verdict, collect, decide};
use super::store::Store;
use super::*;
use crate::ui_sound::{ImKind, ImMessage, UiSound};
use crate::world::attention::{Attention, Cause, Notice, Open};
use std::time::{Duration, Instant};

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn on() -> Shared {
    Shared {
        enabled: true,
        ..Shared::default()
    }
}

/// A character the host keeps in the background, its sound cut, the user
/// elsewhere.
const BACKGROUND: Place = Place {
    managed: true,
    background: true,
    focused: false,
    sound_cut: true,
};

fn incoming(event: AlertEvent, session: Option<u128>) -> Incoming {
    Incoming {
        event,
        session: session.map(id),
        sender_blocked: false,
    }
}

// ---- choices -------------------------------------------------------------

#[test]
fn each_kind_has_a_default_interface_sound_that_is_offered() {
    let offered = built_ins();
    for event in AlertEvent::ALL {
        let entry = built_in(event.default_sound()).expect("a sound of the catalogue");
        assert!(offered.iter().any(|e| e.name == entry.name), "{event:?}: offered");
        assert_eq!(event.fallback_asset(), entry.uuid);
        assert!(!event.fallback_asset().is_nil());
    }
    // each asset offered once, never a silent one
    let mut assets: Vec<Uuid> = offered.iter().map(|e| e.uuid).collect();
    let count = assets.len();
    assets.dedup();
    assets.sort();
    assets.dedup();
    assert_eq!(assets.len(), count);
    assert!(built_in("PieMenuHide").is_none(), "a sound without an asset is not one");
    assert!(built_in("NoSuchSound").is_none());
}

#[test]
fn alerts_are_off_until_turned_on_and_follow_each_kind_s_switch() {
    let own = Override::default();
    assert_eq!(resolve(&Shared::default(), &own, AlertEvent::Im), None, "off by default");
    let mut shared = on();
    assert_eq!(
        resolve(&shared, &own, AlertEvent::Im),
        Some(Sound::built_in("NewIncomingIMSession"))
    );
    assert_eq!(
        resolve(&shared, &own, AlertEvent::TeleportOffer),
        Some(Sound::built_in("TeleportOut"))
    );
    shared.events.group.on = false;
    assert_eq!(resolve(&shared, &own, AlertEvent::Group), None);
    shared.events.mention.sound = Sound::Silent;
    assert_eq!(resolve(&shared, &own, AlertEvent::Mention), None, "silent is no alert");
}

#[test]
fn a_character_s_own_sounds_replace_the_shared_ones() {
    let mut shared = on();
    shared.events.im.sound = Sound::File { name: "ding.wav".into() };
    let mut alt = Override::default();
    alt.sounds.im = Some(Sound::Inventory { item: id(7) });
    alt.sounds.group = Some(Sound::Silent);
    assert_eq!(resolve(&shared, &alt, AlertEvent::Im), Some(Sound::Inventory { item: id(7) }));
    assert_eq!(resolve(&shared, &alt, AlertEvent::Group), None, "silenced for this character only");
    assert_eq!(
        resolve(&shared, &Override::default(), AlertEvent::Group),
        Some(Sound::built_in("StartIM")),
        "another character keeps the shared sound"
    );
    assert_eq!(
        resolve(&shared, &alt, AlertEvent::NewIm),
        Some(Sound::built_in("NewIncomingIMSession")),
        "a kind it did not choose: the shared one"
    );
    // its own sound never turns on a kind that is off for all
    shared.events.im.on = false;
    assert_eq!(resolve(&shared, &alt, AlertEvent::Im), None);
    // nor anything when this character's alerts are off
    shared.events.im.on = true;
    alt.off = true;
    for event in AlertEvent::ALL {
        assert_eq!(resolve(&shared, &alt, event), None);
    }
}

// ---- who plays -------------------------------------------------------------

#[test]
fn only_a_background_character_whose_sound_is_cut_plays_alerts() {
    assert!(BACKGROUND.plays());
    let front = Place {
        background: false,
        sound_cut: false,
        ..BACKGROUND
    };
    assert!(!front.plays(), "the active character keeps its own sounds");
    assert!(
        !Place {
            focused: true,
            ..BACKGROUND
        }
        .plays(),
        "the user went to its window"
    );
    assert!(
        !Place {
            sound_cut: false,
            ..BACKGROUND
        }
        .plays(),
        "heard in the background: its own sounds play"
    );
    assert!(
        !Place {
            managed: false,
            ..BACKGROUND
        }
        .plays(),
        "no session host"
    );
    assert!(!Place::default().plays());
}

#[test]
fn the_front_character_do_not_disturb_a_mute_and_the_blocked_play_nothing() {
    let (shared, own, now) = (on(), Override::default(), Instant::now());
    let im = incoming(AlertEvent::Im, Some(1));
    let mut cd = Cooldown::default();
    let front = Place {
        background: false,
        ..BACKGROUND
    };
    assert_eq!(decide(&shared, &own, front, Quiet::default(), im, &mut cd, now), Verdict::Front);
    let dnd = Quiet { dnd: true, muted: false };
    assert_eq!(decide(&shared, &own, BACKGROUND, dnd, im, &mut cd, now), Verdict::Quiet);
    let muted = Quiet { dnd: false, muted: true };
    assert_eq!(decide(&shared, &own, BACKGROUND, muted, im, &mut cd, now), Verdict::Quiet);
    let blocked = Incoming {
        sender_blocked: true,
        ..im
    };
    assert_eq!(
        decide(&shared, &own, BACKGROUND, Quiet::default(), blocked, &mut cd, now),
        Verdict::Blocked
    );
    assert_eq!(
        decide(&Shared::default(), &own, BACKGROUND, Quiet::default(), im, &mut cd, now),
        Verdict::Off
    );
    // none of these counted for the quiet time
    assert_eq!(
        decide(&shared, &own, BACKGROUND, Quiet::default(), im, &mut cd, now),
        Verdict::Play(Sound::built_in("NewIncomingIMSession"))
    );
}

#[test]
fn a_busy_conversation_rings_once_per_quiet_time() {
    let (shared, own) = (on(), Override::default());
    let t0 = Instant::now();
    let mut cd = Cooldown::default();
    let group = incoming(AlertEvent::Group, Some(5));
    let play = |cd: &mut Cooldown, i: Incoming, at: Instant| decide(&shared, &own, BACKGROUND, Quiet::default(), i, cd, at);
    assert!(matches!(play(&mut cd, group, t0), Verdict::Play(_)));
    let s = Duration::from_secs;
    assert_eq!(play(&mut cd, group, t0 + s(3)), Verdict::Cooling, "within the quiet time");
    assert_eq!(play(&mut cd, group, t0 + s(9)), Verdict::Cooling);
    // another conversation, or another kind, is not held by it
    assert!(matches!(
        play(&mut cd, incoming(AlertEvent::Group, Some(6)), t0 + s(4)),
        Verdict::Play(_)
    ));
    assert!(matches!(
        play(&mut cd, incoming(AlertEvent::InventoryOffer, None), t0 + s(6)),
        Verdict::Play(_)
    ));
    assert!(
        matches!(play(&mut cd, group, t0 + s(10)), Verdict::Play(_)),
        "its quiet time is over"
    );
}

#[test]
fn two_alerts_of_one_character_are_a_second_apart_whatever_the_quiet_time() {
    let mut shared = on();
    shared.cooldown_secs = 0;
    let own = Override::default();
    let t0 = Instant::now();
    let mut cd = Cooldown::default();
    let mut play = |i: Incoming, at: Instant| decide(&shared, &own, BACKGROUND, Quiet::default(), i, &mut cd, at);
    assert!(matches!(play(incoming(AlertEvent::Im, Some(1)), t0), Verdict::Play(_)));
    // a message and its mention, in the same frame
    assert_eq!(play(incoming(AlertEvent::Mention, None), t0), Verdict::Cooling);
    assert_eq!(play(incoming(AlertEvent::Im, Some(2)), t0 + MIN_GAP / 2), Verdict::Cooling);
    assert!(
        matches!(play(incoming(AlertEvent::Im, Some(1)), t0 + MIN_GAP), Verdict::Play(_)),
        "no quiet time"
    );
}

#[test]
fn the_frame_s_events_are_read_from_what_the_world_recorded() {
    let alice = id(10);
    let group = id(20);
    let conference = id(30);
    let ims = [
        ImMessage {
            kind: ImKind::Private,
            session: alice,
            new_session: true,
            mention: false,
        },
        ImMessage {
            kind: ImKind::Private,
            session: alice,
            new_session: false,
            mention: false,
        },
        ImMessage {
            kind: ImKind::Group,
            session: group,
            new_session: false,
            mention: false,
        },
        ImMessage {
            kind: ImKind::Conference,
            session: conference,
            new_session: true,
            mention: false,
        },
    ];
    let sounds = [UiSound::NearbyChat, UiSound::TeleportOffer, UiSound::InventoryOffer];
    let mention = Attention {
        cause: Cause::Mention,
        notice: Some(Notice {
            title: "Bob".into(),
            body: "hi".into(),
            open: Open::NearbyChat,
        }),
    };
    // the group's last line is by someone blocked
    let blocked = |who: &Uuid| *who == id(99);
    let sender = |session: &Uuid| (*session == group).then_some(id(99)).or(Some(id(50)));
    let got = collect(&ims, &sounds, std::slice::from_ref(&mention), blocked, sender);
    let events: Vec<(AlertEvent, Option<Uuid>, bool)> = got.iter().map(|i| (i.event, i.session, i.sender_blocked)).collect();
    assert_eq!(
        events,
        vec![
            (AlertEvent::NewIm, Some(alice), false),
            (AlertEvent::Im, Some(alice), false),
            (AlertEvent::Group, Some(group), true),
            (AlertEvent::Conference, Some(conference), false),
            (AlertEvent::Mention, None, false),
            (AlertEvent::InventoryOffer, None, false),
            (AlertEvent::TeleportOffer, None, false),
        ]
    );
    // a resident blocked: their IM is from someone blocked
    let got = collect(&ims[1..2], &[], &[], |who| *who == alice, |_| None);
    assert!(got[0].sender_blocked);
    // a keyword in nearby chat is a mention; in a conversation, or a
    // notice, it is not
    let keyword = |open| Attention {
        cause: Cause::Keyword,
        notice: Some(Notice {
            title: String::new(),
            body: String::new(),
            open,
        }),
    };
    assert_eq!(collect(&[], &[], &[keyword(Open::NearbyChat)], |_| false, |_| None).len(), 1);
    assert!(collect(&[], &[], &[keyword(Open::Conversation(alice))], |_| false, |_| None).is_empty());
    let notice = Attention {
        cause: Cause::Notice,
        notice: None,
    };
    assert!(collect(&[], &[UiSound::NearbyChat], &[notice], |_| false, |_| None).is_empty());
}

// ---- settings --------------------------------------------------------------

#[test]
fn the_shared_choices_round_trip_and_read_older_or_partial_files() {
    let mut shared = on();
    shared.volume = 0.5;
    shared.cooldown_secs = 30;
    shared.events.group = EventChoice {
        on: false,
        sound: Sound::File { name: "bell.ogg".into() },
    };
    shared.events.mention.sound = Sound::Silent;
    let text = serde_json::to_string(&shared).expect("writes");
    assert_eq!(serde_json::from_str::<Shared>(&text).expect("reads"), shared);
    // nothing at all, or only some of it
    assert_eq!(serde_json::from_str::<Shared>("{}").expect("reads"), Shared::default());
    let partial: Shared = serde_json::from_str(r#"{"enabled": true, "events": {"im": {"on": false}}}"#).expect("reads");
    assert!(partial.enabled && !partial.events.im.on);
    assert_eq!(partial.events.im.sound, Sound::Silent, "a choice without its sound");
    assert_eq!(
        partial.events.group,
        Shared::default().events.group,
        "the kinds not written keep their defaults"
    );
    // a kind of sound from a later version reads as silent, and its
    // unknown fields are left
    let later: Shared =
        serde_json::from_str(r#"{"events": {"im": {"on": true, "sound": {"kind": "synth", "pitch": 3}}}, "future": 1}"#).expect("reads");
    assert_eq!(later.events.im.sound, Sound::Silent);
    // out of bounds, cleaned
    let wild = Shared {
        volume: f32::NAN,
        cooldown_secs: 9999,
        ..on()
    }
    .cleaned();
    assert_eq!((wild.volume, wild.cooldown_secs), (VOLUME_DEFAULT, COOLDOWN_MAX));
    assert_eq!(Shared { volume: 3.0, ..on() }.cleaned().volume, 1.0);
}

#[test]
fn a_character_s_own_choices_round_trip_in_its_settings_and_older_settings_read() {
    use crate::folderstorm_characters::Settings;
    let mut s = Settings::default();
    assert!(s.background_alerts.is_default());
    // nothing written while it holds nothing
    let text = serde_json::to_string(&s).expect("writes");
    assert!(!text.contains("background_alerts"), "{text}");
    s.background_alerts.off = true;
    s.background_alerts.sounds.teleport_offer = Some(Sound::Inventory { item: id(3) });
    s.background_alerts.sounds.im = Some(Sound::Silent);
    let text = serde_json::to_string(&s).expect("writes");
    assert_eq!(serde_json::from_str::<Settings>(&text).expect("reads"), s);
    // the group as an older version wrote it
    let old: Settings = serde_json::from_str(r#"{"behind_3d": "Off", "behind_fps": 5}"#).expect("reads");
    assert!(old.background_alerts.is_default());
    // and the whole settings file of an older version
    let all: crate::settings::Settings = serde_json::from_str(r#"{"folderstorm_characters": {"behind_3d": "Normal"}}"#).expect("reads");
    assert!(all.folderstorm_characters.background_alerts.is_default());
    // a profile copy leaves the group out, own sounds with it (an inventory
    // sound is one account's)
    let mut json = serde_json::json!({"folderstorm_characters": serde_json::to_value(&s).expect("value"), "font_scale": 1.0});
    assert!(::folderstorm_session::copy::strip_settings(&mut json));
    assert!(json.get("folderstorm_characters").is_none());
}

#[test]
fn the_shared_file_is_written_after_the_edits_and_read_again_when_another_window_changes_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join(store::FILE);
    let t0 = Instant::now();
    let mut store = Store::at(path.clone());
    assert_eq!(*store.get(t0), Shared::default(), "no file: the defaults");
    store.set(on(), t0);
    assert!(store.due(t0).is_none(), "a slider still moving");
    let (written, bytes) = store.due(t0 + Duration::from_secs(1)).expect("due");
    assert_eq!(written, path);
    assert!(store.due(t0 + Duration::from_secs(2)).is_none(), "once");
    std::fs::write(&path, &bytes).expect("write");
    // another window reads it
    let mut other = Store::at(path.clone());
    assert!(other.get(t0).enabled);
    // and sees a change made after (once the time to look again passed)
    let mut changed = on();
    changed.volume = 0.25;
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&path, serde_json::to_vec(&changed).expect("json")).expect("write");
    let later = t0 + Duration::from_secs(30);
    assert_eq!(other.get(later).volume, 0.25);
    // a broken file reads as the defaults
    std::fs::write(&path, b"{not json").expect("write");
    assert_eq!(*Store::at(path).get(t0), Shared::default());
}

// ---- files -----------------------------------------------------------------

/// A mono 16-bit PCM WAV file of `seconds` at 8 kHz.
fn wav(seconds: f32) -> Vec<u8> {
    let rate = 8000u32;
    let frames = (rate as f32 * seconds) as u32;
    let data = frames * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for i in 0..frames {
        let v = ((i as f32 * 0.3).sin() * 8000.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[test]
fn a_file_s_name_is_one_plain_name_of_a_kind_taken() {
    for good in ["ding.wav", "Bell-2.OGG", "my_sound.ogg"] {
        assert!(files::valid_name(good), "{good}");
    }
    for bad in ["", ".wav", "../x.wav", "a/b.wav", "a\\b.wav", "c:x.wav", "x.mp3", "x", "x.wav.exe"] {
        assert!(!files::valid_name(bad), "{bad}");
    }
    let taken = |name: &str| name == "my_bell.wav" || name == "my_bell-2.wav";
    assert_eq!(
        files::copy_name(std::path::Path::new("/somewhere/my bell.WAV"), taken).as_deref(),
        Some("my_bell-3.wav")
    );
    assert_eq!(
        files::copy_name(std::path::Path::new("ベル.ogg"), |_| false).as_deref(),
        Some("ベル.ogg")
    );
    assert_eq!(
        files::copy_name(std::path::Path::new("!!!.ogg"), |_| false).as_deref(),
        Some("sound.ogg")
    );
    assert_eq!(files::copy_name(std::path::Path::new("x.mp3"), |_| false), None);
    let long = format!("{}.wav", "a".repeat(200));
    let name = files::copy_name(std::path::Path::new(&long), |_| false).expect("a name");
    assert!(files::valid_name(&name), "{name}");
}

#[test]
fn a_chosen_file_is_checked_then_copied_into_the_sounds_folder() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = dir.path().join(files::FOLDER);
    let source = dir.path().join("short ding.wav");
    std::fs::write(&source, wav(0.3)).expect("write");
    let name = files::import(&source, &folder).expect("taken");
    assert_eq!(name, "short_ding.wav");
    assert!(folder.join(&name).is_file());
    // the original may go: the copy plays
    std::fs::remove_file(&source).expect("remove");
    let clip = files::load(&folder, &name).expect("plays");
    assert!(clip.duration() > Duration::from_millis(250));
    assert_eq!(files::list(&folder), vec![name.clone()]);
    // a second file of that name gets its own copy
    std::fs::write(&source, wav(0.2)).expect("write");
    assert_eq!(files::import(&source, &folder).expect("taken"), "short_ding-2.wav");
    // what is refused, and why
    let refused = |file: &str, bytes: &[u8]| {
        let path = dir.path().join(file);
        std::fs::write(&path, bytes).expect("write");
        files::import(&path, &folder)
    };
    assert_eq!(refused("tune.mp3", &wav(0.2)), Err(files::FileError::Kind));
    assert_eq!(refused("noise.wav", b"not a sound at all"), Err(files::FileError::NotSound));
    assert_eq!(refused("long.wav", &wav(11.0)), Err(files::FileError::TooLong));
    let mut big = wav(0.1);
    big.resize(files::MAX_BYTES as usize + 1, 0);
    assert_eq!(refused("big.wav", &big), Err(files::FileError::TooLarge));
    assert_eq!(
        files::import(&dir.path().join("gone.wav"), &folder),
        Err(files::FileError::Unreadable)
    );
    // only the copies are in the folder; a name with a path is never read
    assert_eq!(files::list(&folder).len(), 2);
    assert_eq!(files::load(&folder, "../short ding.wav").err(), Some(files::FileError::Unreadable));
    for e in [
        files::FileError::Kind,
        files::FileError::TooLarge,
        files::FileError::Unreadable,
        files::FileError::NotSound,
        files::FileError::TooLong,
        files::FileError::Copy,
    ] {
        assert!(!e.text().is_empty());
    }
}

// ---- inventory sounds -------------------------------------------------------

#[test]
fn an_inventory_sound_plays_only_from_the_character_s_own_inventory() {
    use crate::world::inventory::Inventory;
    use aurora_net::inventory::InvItem;
    let mut inv = Inventory::default();
    crate::world::inventory::demo::seed(&mut inv, id(42));
    let mine = id(8000);
    let trash = crate::world::appearance::system_folder(&inv, 14).expect("a Trash");
    let library = inv.lib_root;
    let item = |n: u128, parent: Uuid, name: &str, asset_type: i32, asset: Uuid| InvItem {
        id: id(n),
        parent,
        name: name.into(),
        asset_type,
        inv_type: if asset_type == 1 { 1 } else { 6 },
        asset_id: asset,
        owner: id(42),
        ..Default::default()
    };
    inv.add_items(vec![
        item(101, mine, "Bell", 1, id(1101)),
        item(102, mine, "Bell (link)", 24, id(101)),
        item(103, trash, "Old horn", 1, id(1103)),
        item(104, library, "Library chime", 1, id(1104)),
        item(105, mine, "Box", 6, id(1105)),
        item(106, mine, "Applause", 1, id(1106)),
    ]);
    assert_eq!(inventory_asset(&inv, id(101)), Some(id(1101)));
    assert_eq!(inventory_asset(&inv, id(102)), Some(id(1101)), "a link plays its original");
    for refused in [103, 104, 105, 999] {
        assert_eq!(inventory_asset(&inv, id(refused)), None, "{refused}");
    }
    // listed by name, item ids only (never an asset id)
    let listed = inventory_sounds(&inv);
    let names: Vec<&str> = listed.iter().map(|(_, n)| n.as_str()).collect();
    assert!(names.contains(&"Applause") && names.contains(&"Bell"), "{names:?}");
    assert!(!names.contains(&"Old horn") && !names.contains(&"Library chime") && !names.contains(&"Box"));
    let a = names.iter().position(|n| *n == "Applause");
    let b = names.iter().position(|n| *n == "Bell");
    assert!(a < b, "by name");
    assert!(listed.iter().all(|(item, _)| ![id(1101), id(1106)].contains(item)));
}

// ---- the player --------------------------------------------------------------

#[test]
fn a_waiting_sound_plays_waits_falls_back_or_is_dropped() {
    let clip = || Got::Ready(aurora_audio::SoundClip::from_pcm(&[0.0; 64], 1, 48_000));
    assert_eq!(next(&clip(), true, false, true), Next::Play);
    assert_eq!(next(&clip(), false, false, true), Next::Wait, "the output is opening");
    assert_eq!(next(&clip(), false, true, true), Next::Drop, "no output in time");
    assert_eq!(next(&Got::Missing, true, false, true), Next::FallBack);
    assert_eq!(next(&Got::Missing, true, false, false), Next::Drop, "the default itself failed");
    assert_eq!(next(&Got::Waiting, true, false, true), Next::Wait);
    assert_eq!(next(&Got::Waiting, true, true, true), Next::FallBack, "too slow");
    assert_eq!(next(&Got::Waiting, true, true, false), Next::Drop);
}

#[test]
fn the_demo_s_events_take_turns_and_show_the_quiet_time() {
    let events: Vec<AlertEvent> = (0..9).map(|n| demo_event(n).event).collect();
    for event in AlertEvent::ALL {
        assert!(events.contains(&event), "{event:?}");
    }
    assert_eq!(demo_event(1), demo_event(2), "the same conversation twice");
    assert_eq!(demo_event(9), demo_event(0));
}
