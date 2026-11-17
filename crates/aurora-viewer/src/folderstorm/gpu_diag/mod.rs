//! Folderstorm: what a GPU device loss leaves behind (gpu-memory plan,
//! phase 1, option (d)). Aurora recovers from a loss (app/gpu_recovery.rs)
//! but keeps little of it: the crash session's `alive.txt` is rewritten
//! every 5 s and reset by the recovery, and `gpu-recoveries.txt` holds a
//! time, a duration and same/safe. With several characters on one card the
//! question is always "whose memory, how full, in front or behind", so:
//!
//! - every [`SAMPLE_EVERY`] the viewer notes a [`Sample`] of its memory
//!   picture (its own video memory use and driver budget, its textures,
//!   the whole card's use where Windows counts it, the other viewers of the
//!   instances registry, the window) in a [`History`] of the last
//!   [`HISTORY_SPAN`];
//! - at the loss, the [`Entry`] (that history, a sample taken at the loss,
//!   what the driver said, the window's role) goes to its own file next to
//!   the viewer's log, `<log name>.gpu-losses.txt`, which keeps the last
//!   [`KEEP_ENTRIES`] losses ([`append`]); the recovery fills in its outcome
//!   ([`set_outcome`]);
//! - the recovery notice says the likely [`Reason`] in plain words
//!   ([`notice`]), Aurora's own text when none can be told.
//!
//! Figures only: no name, no id of a person, no chat. The app's side is
//! `app/folderstorm_characters/gpu_diag.rs`; the hooks in Aurora's files are
//! marked `// Folderstorm:`.

#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// A sample of the memory picture this often.
pub const SAMPLE_EVERY: Duration = Duration::from_secs(5);
/// The run-up an entry shows.
pub const HISTORY_SPAN: Duration = Duration::from_secs(60);
/// Losses kept in the file.
pub const KEEP_ENTRIES: usize = 20;
/// The first line of an entry in the file.
const HEADER: &str = "== GPU loss ";
/// The line of an entry that the recovery fills in.
const OUTCOME: &str = "outcome: ";
/// The whole card counts as full past this share (per cent) of its memory.
const CARD_FULL_PCT: u64 = 95;
/// The viewer counts as out of room past this share of its driver budget.
const BUDGET_FULL_PCT: u64 = 97;

const MB: u64 = 1 << 20;

/// Where the window was, as fixed words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Place {
    #[default]
    Front,
    Behind,
    Minimised,
}

impl Place {
    fn word(self) -> &'static str {
        match self {
            Place::Front => "front",
            Place::Behind => "behind",
            Place::Minimised => "minimised",
        }
    }
}

/// Another viewer of the instances registry on this card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Peer {
    /// Video memory it holds, MB (0 when it does not know).
    pub used_mb: u64,
    /// Its window is in front and not minimised.
    pub in_front: bool,
}

/// The memory picture at one moment. Sizes in MB.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    /// What the driver counts for this process, and the budget it gives it
    /// (`VK_EXT_memory_budget`, aurora-render vram.rs).
    pub vram_used: Option<u64>,
    pub vram_budget: Option<u64>,
    /// This process's live textures and buffers (wgpu's counters).
    pub allocated: Option<u64>,
    /// Texture layers in use, the pages that hold them, the streamer's
    /// target and pressure step (of `pressure_max`).
    pub textures: u64,
    pub texture_pages: u64,
    pub texture_target: u64,
    pub pressure: u8,
    pub pressure_max: u8,
    /// Every program's use of the card (Windows' counters, gpu_load.rs),
    /// and the card's own memory.
    pub card_used: Option<u64>,
    pub card_total: Option<u64>,
    /// The other viewers on this card (instances.rs).
    pub peers: Vec<Peer>,
    /// The window's inner size and place.
    pub window: (u32, u32),
    pub place: Place,
}

/// Bytes to MB, for the samples.
pub fn mb(bytes: u64) -> u64 {
    bytes / MB
}

impl Sample {
    /// The memory figures the notice and the reason go by: the whole card
    /// when Windows counts it, else this process against its budget, and
    /// whether that is full.
    fn fill(&self) -> Option<Fill> {
        if let (Some(used), Some(total)) = (self.card_used, self.card_total.filter(|t| *t > 0)) {
            let full = used * 100 >= total * CARD_FULL_PCT || self.past_budget();
            return Some(Fill { used, total, full });
        }
        let (used, total) = (self.vram_used?, self.vram_budget.filter(|b| *b > 0)?);
        Some(Fill {
            used,
            total,
            full: self.past_budget(),
        })
    }

    fn past_budget(&self) -> bool {
        matches!((self.vram_used, self.vram_budget), (Some(u), Some(b)) if b > 0 && u * 100 >= b * BUDGET_FULL_PCT)
    }

    /// One line of the history: used/budget, textures/target, card, step,
    /// viewers, place.
    fn line(&self) -> String {
        let opt = |v: Option<u64>| v.map_or_else(|| "?".to_owned(), |v| v.to_string());
        format!(
            "vram {}/{} MB, textures {}/{} MB, card {}/{} MB, step {}, viewers {}, {}",
            opt(self.vram_used),
            opt(self.vram_budget),
            self.textures,
            self.texture_target,
            opt(self.card_used),
            opt(self.card_total),
            self.pressure,
            self.peers.len() + 1,
            self.place.word()
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fill {
    used: u64,
    total: u64,
    full: bool,
}

/// The samples of the last [`HISTORY_SPAN`], one every [`SAMPLE_EVERY`].
#[derive(Debug, Default)]
pub struct History {
    samples: VecDeque<(Instant, Sample)>,
}

impl History {
    /// Whether a sample is due at `now`.
    pub fn due(&self, now: Instant) -> bool {
        self.samples
            .back()
            .is_none_or(|(at, _)| now.saturating_duration_since(*at) >= SAMPLE_EVERY)
    }

    /// Notes `sample`, dropping those older than the span.
    pub fn push(&mut self, now: Instant, sample: Sample) {
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > HISTORY_SPAN)
        {
            self.samples.pop_front();
        }
        self.samples.push_back((now, sample));
    }

    /// Each sample with its age at `now`, the oldest first.
    pub fn aged(&self, now: Instant) -> Vec<(Duration, Sample)> {
        self.samples
            .iter()
            .map(|(at, s)| (now.saturating_duration_since(*at), s.clone()))
            .collect()
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

/// What a viewer keeps between its frames.
#[derive(Debug, Default)]
pub struct Recorder {
    pub history: History,
    /// The current renderer's loss is written down, with its likely reason;
    /// the recovery takes it.
    pub loss: Option<Reason>,
}

/// What the GPU had in hand at the loss (aurora-render device_lost.rs's
/// verdict on the last submission).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    /// The last frame was still running when the driver reset the card.
    Hang,
    /// The driver found a fault in the work, or after it.
    Fault,
    Undecided,
}

impl Work {
    /// From the fixed verdict words of aurora-render.
    pub fn from_verdict(verdict: &str) -> Option<Work> {
        let i = aurora_render::GPU_WORK_VERDICTS.iter().position(|v| *v == verdict)?;
        Some(match i {
            1 => Work::Hang,
            0 | 2 => Work::Fault,
            _ => Work::Undecided,
        })
    }

    fn word(self) -> &'static str {
        match self {
            Work::Hang => "hang (the last frame was still running)",
            Work::Fault => "fault (an error in, or after, the last frame)",
            Work::Undecided => "undecided (a slow frame that faulted, or a hang)",
        }
    }
}

/// What the driver and the session host said, at the loss.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    /// aurora-render `loss_details`: wgpu's reason, the class of its
    /// message (`out-of-memory`, `device-lost`, `timeout`, `other`) and how
    /// the loss was first seen. Fixed words.
    pub driver: Option<(&'static str, &'static str, &'static str)>,
    pub work: Option<Work>,
    /// The session host's role and standby for this character (fixed
    /// words), None when no host manages this viewer.
    pub role: Option<&'static str>,
    pub standby: Option<&'static str>,
    /// In front for the GPU rules (focused, or the host's active character).
    pub in_front: bool,
}

/// The likely cause, for the notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The video memory was full: `viewers` open on the card (this one
    /// included) and the memory figures, when known.
    Memory { viewers: u32, used_total: Option<(u64, u64)> },
    /// The card took too long to draw a frame (a TDR).
    Hang,
    /// The driver stopped on an error in the drawing.
    Fault,
    Unknown,
}

impl Reason {
    fn word(self) -> &'static str {
        match self {
            Reason::Memory { .. } => "video memory full",
            Reason::Hang => "a frame took too long",
            Reason::Fault => "a driver error while drawing",
            Reason::Unknown => "unknown",
        }
    }
}

/// The likely reason from the driver's words and the memory picture of
/// the last seconds (`recent`, the newest last). Memory first: on a full
/// card Windows demotes and pages, and a frame then runs past the reset's
/// limit, so a hang with the memory full is the memory's.
pub fn reason(context: &Context, recent: &[Sample]) -> Reason {
    let fills: Vec<Fill> = recent.iter().filter_map(Sample::fill).collect();
    let full = fills.iter().rev().find(|f| f.full);
    let viewers = recent.last().map_or(1, |s| s.peers.len() as u32 + 1);
    let said_memory = context.driver.is_some_and(|(_, cause, _)| cause == "out-of-memory");
    if said_memory || full.is_some() {
        let fill = full.or(fills.last());
        return Reason::Memory {
            viewers,
            used_total: fill.map(|f| (f.used, f.total)),
        };
    }
    match context.work {
        Some(Work::Hang) => Reason::Hang,
        Some(Work::Fault) => Reason::Fault,
        _ if context.driver.is_some_and(|(_, cause, _)| cause == "timeout") => Reason::Hang,
        _ => Reason::Unknown,
    }
}

/// GB with one decimal, without a trailing ".0", in the interface's
/// number format.
fn gb(mb: u64) -> String {
    let tenths = (mb * 10 + 512) / 1024;
    if tenths % 10 == 0 {
        aurora_i18n::integer((tenths / 10) as i64)
    } else {
        aurora_i18n::number(tenths as f64 / 10.0, 1)
    }
}

/// The reason in plain words, None when it is unknown. `managed`: the
/// session host runs this viewer (its other viewers are characters).
pub fn reason_text(reason: Reason, managed: bool) -> Option<String> {
    Some(match reason {
        Reason::Memory { viewers, used_total } => match used_total {
            Some((used, total)) if viewers > 1 => {
                let (count, used, total) = (viewers.to_string(), gb(used), gb(total));
                if managed {
                    aurora_i18n::tr!("fs-gpu-reason-memory-characters", count = count, used = used, total = total).to_string()
                } else {
                    aurora_i18n::tr!("fs-gpu-reason-memory-viewers", count = count, used = used, total = total).to_string()
                }
            }
            Some((used, total)) => aurora_i18n::tr!("fs-gpu-reason-memory-used", used = gb(used), total = gb(total)).to_string(),
            None => aurora_i18n::tr!("fs-gpu-reason-memory").to_string(),
        },
        Reason::Hang => aurora_i18n::tr!("fs-gpu-reason-hang").to_string(),
        Reason::Fault => aurora_i18n::tr!("fs-gpu-reason-fault").to_string(),
        Reason::Unknown => return None,
    })
}

/// The recovery notice's text: the reason and what the viewer did, or
/// Aurora's own text (`aurora`) when the reason is unknown; then where the
/// details are.
pub fn notice(reason: Reason, managed: bool, safe: bool, aurora: &str) -> String {
    let body = match reason_text(reason, managed) {
        Some(why) => {
            let then = if safe {
                aurora_i18n::tr!("fs-gpu-reset-then-safe")
            } else {
                aurora_i18n::tr!("fs-gpu-reset-then")
            };
            format!("{why} {then}")
        }
        None => aurora.to_owned(),
    };
    format!("{body}\n\n{}", aurora_i18n::tr!("fs-gpu-reset-log"))
}

/// One loss, as the file keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Local time of the loss, already formatted.
    pub when: String,
    pub context: Context,
    pub reason: Reason,
    /// The history, each sample with its age at the loss, the oldest first;
    /// the sample taken at the loss is the last, aged 0.
    pub history: Vec<(Duration, Sample)>,
}

/// What the recovery says of an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Written at the loss: nothing follows when the viewer closed.
    Pending,
    /// Not in front: the loss waits for the window to come to the front.
    Waiting,
    Recovered { ms: u128, safe: bool },
}

impl Outcome {
    fn line(self) -> String {
        let text = match self {
            Outcome::Pending => "not recovered yet (nothing more here: the viewer closed or ended)".to_owned(),
            Outcome::Waiting => "waiting: the window recovers when it comes to the front".to_owned(),
            Outcome::Recovered { ms, safe } => format!(
                "recovered in {ms} ms, {}",
                if safe { "with the safe graphics level" } else { "settings unchanged" }
            ),
        };
        format!("{OUTCOME}{text}")
    }
}

impl Entry {
    /// The entry's block in the file, with `outcome`.
    pub fn format(&self, outcome: Outcome) -> String {
        let mut out = String::new();
        let c = &self.context;
        let _ = writeln!(out, "{HEADER}{}", self.when);
        let _ = writeln!(out, "{}", outcome.line());
        let _ = writeln!(out, "likely reason: {}", self.reason.word());
        match c.driver {
            Some((reason, cause, seen)) => {
                let _ = writeln!(out, "driver: reason {reason}, message about {cause}, first seen by {seen}");
            }
            None => {
                let _ = writeln!(out, "driver: nothing said");
            }
        }
        let _ = writeln!(out, "gpu work at the loss: {}", c.work.map_or("unknown", Work::word));
        let now = self.history.last().map(|(_, s)| s);
        let place = now.map_or("unknown", |s| s.place.word());
        let (w, h) = now.map_or((0, 0), |s| s.window);
        let managed = match (c.role, c.standby) {
            (Some(role), Some(standby)) => format!("session host: {role}, {standby}"),
            (Some(role), None) => format!("session host: {role}"),
            _ => "not managed by a session host".to_owned(),
        };
        let _ = writeln!(
            out,
            "window: {w}x{h}, {place}, {} for the GPU rules; {managed}",
            if c.in_front { "in front" } else { "not in front" }
        );
        if let Some(s) = now {
            let opt = |v: Option<u64>| v.map_or_else(|| "?".to_owned(), |v| format!("{v} MB"));
            let _ = writeln!(
                out,
                "this viewer: video memory {} used of a {} budget, {} allocated",
                opt(s.vram_used),
                opt(s.vram_budget),
                opt(s.allocated)
            );
            let _ = writeln!(
                out,
                "textures: {} MB in use in {} MB of pages, target {} MB, pressure step {}/{}",
                s.textures, s.texture_pages, s.texture_target, s.pressure, s.pressure_max
            );
            let _ = writeln!(
                out,
                "card: {} used by every program, {} in all",
                opt(s.card_used),
                opt(s.card_total)
            );
            let peers = s
                .peers
                .iter()
                .map(|p| format!("{} MB {}", p.used_mb, if p.in_front { "in front" } else { "behind" }))
                .collect::<Vec<_>>();
            let _ = writeln!(
                out,
                "other viewers on the card: {}{}",
                peers.len(),
                if peers.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", peers.join(", "))
                }
            );
        }
        let _ = writeln!(out, "run-up (age: figures):");
        for (i, (age, s)) in self.history.iter().enumerate() {
            if i + 1 == self.history.len() {
                let _ = writeln!(out, "  at the loss: {}", s.line());
            } else {
                let _ = writeln!(out, "  -{:>2} s: {}", age.as_secs(), s.line());
            }
        }
        out
    }

    /// The one line the viewer's log gets.
    pub fn log_line(&self) -> String {
        let now = self.history.last().map(|(_, s)| s.line()).unwrap_or_default();
        format!("GPU loss: likely reason {}; {now}", self.reason.word())
    }
}

/// `file` (the text the file holds, maybe empty or from an older format)
/// with `block` added, keeping the last `keep` entries.
pub fn append(file: &str, block: &str, keep: usize) -> String {
    let mut entries = entries(file);
    entries.push(block.trim_end());
    let skip = entries.len().saturating_sub(keep);
    let mut out = entries[skip..].join("\n\n");
    out.push('\n');
    out
}

/// `file` with the outcome of its newest entry set; unchanged without one.
pub fn set_outcome(file: &str, outcome: Outcome) -> String {
    let Some(start) = file.rfind(HEADER) else {
        return file.to_owned();
    };
    let (before, last) = file.split_at(start);
    let mut done = false;
    let lines: Vec<String> = last
        .lines()
        .map(|line| {
            if !done && line.starts_with(OUTCOME) {
                done = true;
                outcome.line()
            } else {
                line.to_owned()
            }
        })
        .collect();
    let mut out = format!("{before}{}", lines.join("\n"));
    out.push('\n');
    out
}

/// The entries of the file, each starting at its header; text before the
/// first header is dropped.
fn entries(file: &str) -> Vec<&str> {
    let mut starts: Vec<usize> = file.match_indices(HEADER).map(|(i, _)| i).collect();
    starts.retain(|&i| i == 0 || file[..i].ends_with('\n'));
    starts
        .iter()
        .enumerate()
        .map(|(n, &i)| file[i..starts.get(n + 1).copied().unwrap_or(file.len())].trim_end())
        .collect()
}

/// The file next to the viewer's log: `aurora-<profile>.gpu-losses.txt`.
pub fn file_path() -> std::path::PathBuf {
    crate::logging::log_path().with_extension("gpu-losses.txt")
}

/// Adds `entry` to the file (written at the loss: it survives a recovery
/// that fails).
pub fn write_entry(entry: &Entry, outcome: Outcome) {
    let path = file_path();
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let text = append(&old, &entry.format(outcome), KEEP_ENTRIES);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, text) {
        log::warn!("GPU loss: the diagnostics file could not be written: {e}");
    }
}

/// Fills in the newest entry's outcome.
pub fn write_outcome(outcome: Outcome) {
    let path = file_path();
    let Ok(old) = std::fs::read_to_string(&path) else {
        return;
    };
    if let Err(e) = std::fs::write(&path, set_outcome(&old, outcome)) {
        log::warn!("GPU loss: the diagnostics file could not be updated: {e}");
    }
}

/// `AURORA_DEMO_GPU_REASON=memory|hang|fault`, with `AURORA_DEMO=1`: the
/// reason the demo's loss is taken for, to see each notice.
pub fn demo_reason(demo: bool) -> Option<Reason> {
    if !demo {
        return None;
    }
    parse_demo_reason(std::env::var("AURORA_DEMO_GPU_REASON").ok().as_deref())
}

fn parse_demo_reason(value: Option<&str>) -> Option<Reason> {
    match value?.trim() {
        "memory" => Some(Reason::Memory {
            viewers: 3,
            used_total: Some((11_878, 12_288)),
        }),
        "hang" => Some(Reason::Hang),
        "fault" => Some(Reason::Fault),
        _ => None,
    }
}

/// Help › Show the log folder, where the losses' file and the logs are.
pub fn help_menu_item(ui: &mut egui::Ui, p: &crate::theme::Palette) {
    if crate::ui::menu::item(ui, p, "folder-open-filled", aurora_i18n::tr!("fs-gpu-show-logs")) {
        crate::logging::open_logs_dir();
    }
}
