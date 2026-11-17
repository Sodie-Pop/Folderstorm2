//! The alerts' own audio output, and the sounds waiting for their clip.
//!
//! A background character's viewer has its sounds cut by the session
//! host's transient mute, on the master channel of Aurora's mixer, which
//! scales every bus. Its alerts go to a second output of the same audio
//! crate, opened for them only (on a thread: a device can take seconds to
//! open) and closed after a while without any: the mute never reaches it,
//! and nothing else plays there.
//!
//! The grid's interface sounds and the inventory's sounds come through the
//! viewer's sound assets (fetched and cached as every sound, [`Assets`]);
//! a local file is read and decoded on a thread, once. A sound that cannot
//! be had (a file gone, an asset that does not load) plays the kind's
//! default interface sound instead, as Folderstorm (C++) fell back to its
//! built-in tone.

use super::files::{self, FileError};
use aurora_audio::{AudioEngine, Channel, SoundClip};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// A sound waits this long for its clip, then its fallback as long.
const WAIT: Duration = Duration::from_secs(10);
/// The output closes after this long without an alert.
const IDLE: Duration = Duration::from_secs(120);
/// An output that did not open is tried again after this long.
const RETRY: Duration = Duration::from_secs(60);
/// A decoded file is read again after this long (it may have changed).
const FILE_KEPT: Duration = Duration::from_secs(300);
/// Sounds waiting at most.
const MAX_PENDING: usize = 8;

/// Where the sound assets come from: the viewer's sound manager.
pub trait Assets {
    /// Fetch it, and keep it once loaded.
    fn want(&mut self, id: Uuid);
    /// Its clip, if loaded.
    fn clip(&mut self, id: &Uuid) -> Option<Arc<SoundClip>>;
    /// It was fetched (loaded, or failed).
    fn settled(&self, id: &Uuid) -> bool;
}

impl Assets for crate::scene::sounds::SoundManager {
    fn want(&mut self, id: Uuid) {
        self.want_kept(id);
    }
    fn clip(&mut self, id: &Uuid) -> Option<Arc<SoundClip>> {
        self.folderstorm_clip(id)
    }
    fn settled(&self, id: &Uuid) -> bool {
        self.asset_settled(id)
    }
}

/// What to play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clip {
    /// A sound asset: an interface sound, or an inventory sound's asset
    /// (never logged nor shown).
    Asset(Uuid),
    /// A file of the sounds' folder.
    File(String),
}

#[derive(Debug)]
struct Pending {
    clip: Clip,
    /// The kind's default interface sound; nil once it is the one waiting.
    fallback: Uuid,
    gain: f32,
    asked: Instant,
    what: &'static str,
}

#[derive(Debug, Default)]
enum Engine {
    #[default]
    Closed,
    Opening(Receiver<Result<AudioEngine, String>>),
    Ready {
        engine: AudioEngine,
        device: String,
    },
    Failed(Instant),
}

#[derive(Debug)]
enum FileClip {
    Loading(Receiver<Result<SoundClip, FileError>>),
    Ready(SoundClip, Instant),
    Failed(FileError, Instant),
}

/// What a character's viewer keeps of its alerts.
#[derive(Debug, Default)]
pub struct Player {
    engine: Engine,
    pending: Vec<Pending>,
    files: HashMap<String, FileClip>,
    /// The quiet time.
    pub cooldown: super::rules::Cooldown,
    last_played: Option<Instant>,
    /// What was last fetched ahead, so that the first alert is not late.
    prefetched: Vec<Clip>,
}

/// What became of a sound in [`Player::tick`] (for the log and the tests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Played {
    Played(&'static str),
    /// Its own sound could not be had: the fallback waits instead.
    FellBack(&'static str),
    /// Neither could be had, or no output opened.
    Dropped(&'static str),
}

impl Player {
    /// Plays `clip` as soon as it and the output are there, at `gain` (0 to
    /// 1); `fallback`: the kind's default interface sound.
    pub fn play(&mut self, clip: Clip, fallback: Uuid, gain: f32, what: &'static str, now: Instant) {
        if self.pending.len() >= MAX_PENDING {
            log::debug!("background alerts: too many sounds waiting, {what} dropped");
            return;
        }
        let gain = if gain.is_finite() { gain.clamp(0.0, 1.0) } else { 0.0 };
        if gain <= 0.0 {
            return;
        }
        let fallback = if clip == Clip::Asset(fallback) { Uuid::nil() } else { fallback };
        self.pending.push(Pending {
            clip,
            fallback,
            gain,
            asked: now,
            what,
        });
    }

    /// Fetches ahead what the alerts may play (when the choices or the
    /// character's place change), so that the first one is not late.
    pub fn prefetch(&mut self, assets: &mut impl Assets, folder: &Path, clips: Vec<Clip>) {
        if clips == self.prefetched {
            return;
        }
        for clip in &clips {
            match clip {
                Clip::Asset(id) => assets.want(*id),
                Clip::File(name) => self.load_file(folder, name),
            }
        }
        self.prefetched = clips;
    }

    fn load_file(&mut self, folder: &Path, name: &str) {
        if self.files.contains_key(name) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (folder, owned) = (folder.to_path_buf(), name.to_owned());
        let spawned = std::thread::Builder::new()
            .name("fs-alerts-file".into())
            .spawn(move || {
                let _ = tx.send(files::load(&folder, &owned));
            });
        let entry = match spawned {
            Ok(_) => FileClip::Loading(rx),
            Err(_) => FileClip::Failed(FileError::Unreadable, Instant::now()),
        };
        self.files.insert(name.to_owned(), entry);
    }

    /// The clip of a file, once decoded: Some(Err) when it cannot be had.
    fn file_clip(&mut self, folder: &Path, name: &str, now: Instant) -> Option<Result<SoundClip, FileError>> {
        self.load_file(folder, name);
        let entry = self.files.get_mut(name)?;
        if let FileClip::Loading(rx) = entry {
            *entry = match rx.try_recv() {
                Ok(Ok(clip)) => FileClip::Ready(clip, now),
                Ok(Err(e)) => FileClip::Failed(e, now),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => FileClip::Failed(FileError::Unreadable, now),
            };
        }
        match entry {
            FileClip::Ready(clip, _) => Some(Ok(clip.clone())),
            FileClip::Failed(e, _) => Some(Err(*e)),
            FileClip::Loading(_) => None,
        }
    }

    /// Every frame: opens the output when a sound waits, plays what is
    /// ready, falls back or gives up on what cannot be had, closes the
    /// output after a while without alerts. `device`: the character's
    /// output device (empty: the system's).
    pub fn tick(&mut self, assets: &mut impl Assets, device: &str, folder: &Path, now: Instant) -> Vec<Played> {
        self.files.retain(|_, f| match f {
            FileClip::Ready(_, at) | FileClip::Failed(_, at) => now.saturating_duration_since(*at) < FILE_KEPT,
            FileClip::Loading(_) => true,
        });
        let mut out = Vec::new();
        if self.pending.is_empty() {
            if let Engine::Ready { .. } = self.engine
                && self.last_played.is_none_or(|at| now.saturating_duration_since(at) >= IDLE)
            {
                log::info!("background alerts: no alert for a while, their output closes");
                self.engine = Engine::Closed;
            }
            return out;
        }
        self.open(device, now);
        if let Engine::Failed(_) = self.engine {
            for p in self.pending.drain(..) {
                out.push(Played::Dropped(p.what));
            }
            return out;
        }
        let pending = std::mem::take(&mut self.pending);
        for mut p in pending {
            let got = match &p.clip {
                Clip::Asset(id) => {
                    assets.want(*id);
                    match assets.clip(id) {
                        Some(clip) => Got::Ready((*clip).clone()),
                        None if assets.settled(id) => Got::Missing,
                        None => Got::Waiting,
                    }
                }
                Clip::File(name) => {
                    let name = name.clone();
                    match self.file_clip(folder, &name, now) {
                        Some(Ok(clip)) => Got::Ready(clip),
                        Some(Err(_)) => Got::Missing,
                        None => Got::Waiting,
                    }
                }
            };
            let late = now.saturating_duration_since(p.asked) >= WAIT;
            let ready = matches!(self.engine, Engine::Ready { .. });
            match next(&got, ready, late, !p.fallback.is_nil()) {
                Next::Play => {
                    if let (Engine::Ready { engine, .. }, Got::Ready(clip)) = (&self.engine, &got) {
                        engine.play_clip(Channel::Ui, clip, p.gain);
                        self.last_played = Some(now);
                        out.push(Played::Played(p.what));
                    }
                }
                Next::Wait => self.pending.push(p),
                Next::FallBack => {
                    out.push(Played::FellBack(p.what));
                    p.clip = Clip::Asset(std::mem::take(&mut p.fallback));
                    p.asked = now;
                    self.pending.push(p);
                }
                Next::Drop => out.push(Played::Dropped(p.what)),
            }
        }
        out
    }

    /// The output: opened on a thread when needed, its device followed.
    fn open(&mut self, device: &str, now: Instant) {
        match &mut self.engine {
            Engine::Closed => self.spawn(device),
            Engine::Failed(at) if now.saturating_duration_since(*at) >= RETRY => self.spawn(device),
            Engine::Failed(_) => {}
            Engine::Opening(rx) => match rx.try_recv() {
                Ok(Ok(engine)) => {
                    log::info!("background alerts: their output is open");
                    self.engine = Engine::Ready {
                        engine,
                        device: device.to_owned(),
                    }
                }
                Ok(Err(e)) => {
                    log::warn!("background alerts: no output for them ({e})");
                    self.engine = Engine::Failed(now);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.engine = Engine::Failed(now),
            },
            Engine::Ready { engine, device: current } => {
                if current != device {
                    engine.set_output_device((!device.is_empty()).then(|| device.to_owned()));
                    *current = device.to_owned();
                }
            }
        }
    }

    fn spawn(&mut self, device: &str) {
        let (tx, rx) = std::sync::mpsc::channel();
        let device = (!device.is_empty()).then(|| device.to_owned());
        let spawned = std::thread::Builder::new()
            .name("fs-alerts-output".into())
            .spawn(move || {
                let _ = tx.send(AudioEngine::new_with_device(device).map_err(|e| e.to_string()));
            });
        self.engine = match spawned {
            Ok(_) => Engine::Opening(rx),
            Err(_) => Engine::Failed(Instant::now()),
        };
    }
}

/// A waiting sound's clip.
#[derive(Debug)]
pub(super) enum Got {
    Ready(SoundClip),
    /// It cannot be had (a file gone or not a sound, an asset that failed).
    Missing,
    Waiting,
}

/// What becomes of a waiting sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Next {
    Play,
    Wait,
    FallBack,
    Drop,
}

/// What becomes of a waiting sound, by its clip, whether the output is
/// open, whether it waited [`WAIT`] already, and whether it has a fallback.
pub(super) fn next(got: &Got, output: bool, late: bool, fallback: bool) -> Next {
    match got {
        Got::Ready(_) if output => Next::Play,
        // (the output is still opening)
        Got::Ready(_) if !late => Next::Wait,
        Got::Ready(_) => Next::Drop,
        Got::Missing if fallback => Next::FallBack,
        Got::Missing => Next::Drop,
        Got::Waiting if !late => Next::Wait,
        Got::Waiting if fallback => Next::FallBack,
        Got::Waiting => Next::Drop,
    }
}
