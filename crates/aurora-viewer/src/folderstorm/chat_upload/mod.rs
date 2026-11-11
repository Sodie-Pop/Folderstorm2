//! Chat image uploads (FEATURES.md 3.3; Folderstorm (C++)'s
//! `doc/plans/native-chat-image-uploads.md`): paste an image (Ctrl+V with a
//! picture and no text on the clipboard), drop an image file on a chat
//! field, or choose one with the field's picture button. The image goes to
//! an image host (Folderstorm's Zipline by default, `zipline.rs`) and its
//! link goes into the draft at the text cursor. Sending stays the user's.
//!
//! Every chat field (the conversations of both chat styles, the nearby chat
//! bar) reaches this module through `ui/chat/input.rs`, by the field's id;
//! the picture button is drawn by the conversations' fields. A finished
//! upload only ever goes into the field it was made from (the id holds the
//! conversation), when that field is shown again: never into another
//! conversation.
//!
//! Nothing leaves the viewer before the user turns the feature on, then
//! pastes, drops or chooses an image, and (by default) confirms it after
//! seeing what will be sent and where.

pub mod demo;
pub mod image;
pub mod settings;
pub mod token;
pub mod zipline;

#[cfg(test)]
mod tests;

use self::image::{Prepared, Refusal};
use crate::theme::Palette;
pub use settings::UploadSettings;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use token::TokenStore;

/// Images waiting or uploading at once in one chat field.
const MAX_PER_FIELD: usize = 4;
/// Dropped files taken at once.
const MAX_DROPPED: usize = 4;

/// Where an image comes from.
enum Source {
    Pasted(Arc<egui::ColorImage>),
    File(PathBuf),
}

#[derive(Clone)]
enum State {
    Preparing,
    /// Waiting for Upload or Cancel.
    Confirm,
    Uploading,
    /// The link, to put in the field.
    Ready(String),
    /// Why it failed, ready to show; `retry`: the image is still here.
    Failed {
        message: String,
        retry: bool,
    },
}

struct Job {
    id: u64,
    field: egui::Id,
    /// The file's name, or "Pasted image": shown, never sent.
    label: String,
    /// The host and folder chosen when the image was given.
    origin: String,
    folder: Option<String>,
    state: State,
    image: Option<Arc<Prepared>>,
    texture: Option<egui::TextureHandle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStatus {
    Checking,
    Saved,
    Absent,
    Failed,
}

#[derive(Default)]
struct Uploads {
    settings: UploadSettings,
    palette: Option<Palette>,
    jobs: Vec<Job>,
    next: u64,
    /// The last pasted picture taken (its address, the frame): a pass of
    /// the same frame does not take it twice.
    last_paste: Option<(usize, u64)>,
    /// Files dropped this frame and the fields that could take them.
    dropped: Option<Dropped>,
    demo_staged: bool,
    /// Whether the user saved a token, by host.
    tokens: HashMap<String, TokenStatus>,
}

/// Files dropped in a frame, given to one field at the next frame: the
/// field under the pointer, else the focused one.
struct Dropped {
    ctx: egui::Context,
    frame: u64,
    paths: Vec<PathBuf>,
    /// Each field drawn: its id, under the pointer, focused.
    fields: Vec<(egui::Id, bool, bool)>,
}

impl Dropped {
    fn target(&self) -> Option<egui::Id> {
        let over = self.fields.iter().find(|(_, over, _)| *over);
        let focused = self.fields.iter().find(|(_, _, focused)| *focused);
        over.or(focused).map(|(id, _, _)| *id)
    }
}

static UPLOADS: LazyLock<Mutex<Uploads>> = LazyLock::new(Mutex::default);
/// One upload at a time.
static LANE: Mutex<()> = Mutex::new(());
/// The file chooser is open.
static CHOOSING: AtomicBool = AtomicBool::new(false);

fn uploads() -> MutexGuard<'static, Uploads> {
    UPLOADS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---- hooks

/// Each frame, before the interface (`app.rs`): the settings and colours
/// in force. Turning the feature off or changing the host drops the images
/// not yet sent.
pub fn frame(settings: &UploadSettings, p: &Palette) {
    let mut u = uploads();
    u.palette = Some(*p);
    let dropped = u.dropped.take_if(|d| d.ctx.cumulative_frame_nr() > d.frame);
    if let Some(dropped) = dropped {
        drop(u);
        if let Some(field) = dropped.target() {
            for path in dropped.paths.into_iter().take(MAX_DROPPED) {
                stage(&dropped.ctx, field, Source::File(path));
            }
        }
        u = uploads();
    }
    if u.settings == *settings {
        return;
    }
    let origin = active_origin(settings);
    u.jobs.retain(|job| match job.state {
        State::Uploading | State::Ready(_) => true,
        _ => origin.as_deref() == Some(job.origin.as_str()),
    });
    u.settings = settings.clone();
}

/// The feature is on and has a host: the picture button shows.
pub fn usable() -> bool {
    active_origin(&uploads().settings).is_some()
}

/// The host uploads go to, when they can.
fn active_origin(settings: &UploadSettings) -> Option<String> {
    if demo::mode().is_some() {
        return Some(demo::HOST.to_owned());
    }
    if !settings.enabled {
        return None;
    }
    settings.origin()
}

/// The host and the upload folder an image given now goes to.
fn destination() -> Option<(String, Option<String>)> {
    let u = uploads();
    let origin = active_origin(&u.settings)?;
    if demo::mode().is_some() {
        return Some((origin, None));
    }
    let folder = settings::resolve_folder(&origin, u.settings.folder_id(), settings::built_in_folder());
    Some((origin, folder))
}

/// In a focused chat field, before the editor reads the keyboard
/// (`input::prepare`): a pasted picture is taken from the input (egui's
/// text paste keeps priority: a picture only arrives when the clipboard
/// holds no text).
pub fn paste(ui: &egui::Ui, field: egui::Id) {
    if !usable() {
        return;
    }
    let mut pasted = Vec::new();
    ui.input_mut(|i| {
        i.events.retain(|event| match event {
            egui::Event::PasteImage(image) => {
                pasted.push(image.clone());
                false
            }
            _ => true,
        });
    });
    let frame = ui.ctx().cumulative_frame_nr();
    for image in pasted {
        let key = (Arc::as_ptr(&image) as usize, frame);
        {
            let mut u = uploads();
            if u.last_paste == Some(key) {
                continue;
            }
            u.last_paste = Some(key);
        }
        stage(ui.ctx(), field, Source::Pasted(image));
    }
}

/// Before a chat field's editor (`input::show`): the links of finished
/// uploads go into its text at the cursor.
pub fn before_field(ctx: &egui::Context, field: egui::Id, text: &mut String) {
    let links: Vec<String> = {
        let mut u = uploads();
        let mut links = Vec::new();
        u.jobs.retain(|job| match &job.state {
            State::Ready(link) if job.field == field => {
                links.push(link.clone());
                false
            }
            _ => true,
        });
        links
    };
    for link in links {
        insert_into_field(ctx, field, text, &link);
    }
}

/// After a chat field's editor (`input::show`): files dropped on it, the
/// highlight while files are carried over it, and the state of its uploads.
pub fn after_field(ui: &mut egui::Ui, field: egui::Id, rect: egui::Rect) {
    let ctx = ui.ctx().clone();
    let Some(p) = uploads().palette else { return };
    if usable() {
        if demo::mode() == Some(demo::Mode::Staged) && ctx.cumulative_frame_nr() >= 240 {
            let first = !std::mem::replace(&mut uploads().demo_staged, true);
            if first {
                stage(&ctx, field, Source::Pasted(Arc::new(demo::picture())));
            }
        }
        let over = ctx
            .input(|i| i.pointer.latest_pos())
            .is_some_and(|pos| rect.expand(24.0).contains(pos));
        let focused = ctx.memory(|m| m.has_focus(field));
        let carried = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if carried && (over || focused) {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                ui.visuals().window_corner_radius,
                egui::Stroke::new(1.5, p.violet_light),
                egui::StrokeKind::Inside,
            );
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            let frame = ctx.cumulative_frame_nr();
            let mut u = uploads();
            if u.dropped.as_ref().is_none_or(|d| d.frame != frame) {
                u.dropped = Some(Dropped {
                    ctx: ctx.clone(),
                    frame,
                    paths: dropped,
                    fields: Vec::new(),
                });
            }
            if let Some(d) = u.dropped.as_mut()
                && !d.fields.iter().any(|(id, _, _)| *id == field)
            {
                d.fields.push((field, over, focused));
            }
        }
    }
    show_jobs(&ctx, &p, field, rect);
}

/// The picture button of a conversation's field: a click opens the file
/// chooser.
pub fn button(ctx: &egui::Context, response: &egui::Response, field: egui::Id) {
    if !response.clicked() || CHOOSING.swap(true, Ordering::SeqCst) {
        return;
    }
    let ctx = ctx.clone();
    let title = aurora_i18n::tr!("fs-chat-upload-choose-title").to_owned();
    let filter = aurora_i18n::tr!("fs-chat-upload-file-type").to_owned();
    let spawned = std::thread::Builder::new().name("fs-chat-upload-choose".into()).spawn(move || {
        let picked = rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter, &["png", "jpg", "jpeg", "gif", "webp"])
            .pick_file();
        CHOOSING.store(false, Ordering::SeqCst);
        if let Some(path) = picked {
            stage(&ctx, field, Source::File(path));
        }
    });
    if spawned.is_err() {
        CHOOSING.store(false, Ordering::SeqCst);
    }
}

// ---- jobs

fn stage(ctx: &egui::Context, field: egui::Id, source: Source) {
    let label = match &source {
        Source::Pasted(_) => aurora_i18n::tr!("fs-chat-upload-pasted").to_owned(),
        Source::File(path) => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    let Some((origin, folder)) = destination() else { return };
    // a host is reached through an upload folder or a token: without
    // either, say what to set instead of sending a request bound to fail
    let token_known = demo::mode().is_some() || token_status(ctx, &origin) != TokenStatus::Absent;
    let (id, max) = {
        let mut u = uploads();
        let busy = u
            .jobs
            .iter()
            .filter(|j| j.field == field && !matches!(j.state, State::Failed { .. }))
            .count();
        u.next += 1;
        let id = u.next;
        let state = if busy >= MAX_PER_FIELD {
            State::Failed {
                message: aurora_i18n::tr!("fs-chat-upload-error-busy").to_owned(),
                retry: false,
            }
        } else if folder.is_none() && !token_known {
            State::Failed {
                message: aurora_i18n::tr!("fs-chat-upload-error-no-folder").to_owned(),
                retry: false,
            }
        } else {
            State::Preparing
        };
        let preparing = matches!(state, State::Preparing);
        u.jobs.push(Job {
            id,
            field,
            label: label.clone(),
            origin,
            folder,
            state,
            image: None,
            texture: None,
        });
        if !preparing {
            ctx.request_repaint();
            return;
        }
        (id, u.settings.max_bytes())
    };
    ctx.request_repaint();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("fs-chat-upload-prepare".into()).spawn(move || {
        let result = match &source {
            Source::Pasted(image) => image::prepare_pasted(image, max),
            Source::File(path) => image::prepare_file(path, max),
        };
        prepared(&ctx, id, result.map(Arc::new).map_err(|r| refusal_text(&label, &r)));
    });
    if spawned.is_err() {
        fail(id, aurora_i18n::tr!("fs-chat-upload-error-network").to_owned(), false);
    }
}

fn prepared(ctx: &egui::Context, id: u64, result: Result<Arc<Prepared>, String>) {
    let mut u = uploads();
    let confirm = u.settings.confirm || demo::mode() == Some(demo::Mode::Staged);
    let Some(job) = u.jobs.iter_mut().find(|j| j.id == id) else {
        return;
    };
    if !matches!(job.state, State::Preparing) {
        return;
    }
    match result {
        Ok(image) => {
            job.image = Some(image);
            if confirm {
                job.state = State::Confirm;
            } else {
                start_upload(ctx, job);
            }
        }
        Err(message) => job.state = State::Failed { message, retry: false },
    }
    ctx.request_repaint();
}

fn fail(id: u64, message: String, retry: bool) {
    if let Some(job) = uploads().jobs.iter_mut().find(|j| j.id == id) {
        job.state = State::Failed { message, retry };
    }
}

/// Sends a job's image, on its own thread, one upload at a time.
fn start_upload(ctx: &egui::Context, job: &mut Job) {
    let Some(image) = job.image.clone() else { return };
    job.state = State::Uploading;
    let (id, origin) = (job.id, job.origin.clone());
    let folder = job.folder.clone();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("fs-chat-upload".into()).spawn(move || {
        let _lane = LANE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // cancelled while waiting for the lane
        if !uploads().jobs.iter().any(|j| j.id == id) {
            return;
        }
        let result = match demo::mode() {
            Some(mode) => upload(&demo::Fake(mode), &origin, token::Auth::None, folder.as_deref(), &image),
            None => {
                let auth = token::resolve(token::SystemStore.read(&origin));
                upload(&zipline::Network, &origin, auth, folder.as_deref(), &image)
            }
        };
        let mut u = uploads();
        if let Some(job) = u.jobs.iter_mut().find(|j| j.id == id) {
            job.state = match result {
                Ok(link) => {
                    job.image = None;
                    job.texture = None;
                    State::Ready(link)
                }
                Err(failure) => State::Failed {
                    message: failure_text(&failure),
                    retry: !matches!(failure, zipline::Failure::Malformed),
                },
            };
        }
        drop(u);
        ctx.request_repaint();
    });
    if spawned.is_err() {
        job.state = State::Failed {
            message: aurora_i18n::tr!("fs-chat-upload-error-network").to_owned(),
            retry: true,
        };
    }
}

/// One upload, from the request to the link.
pub fn upload(
    transport: &dyn zipline::Transport,
    origin: &str,
    auth: token::Auth,
    folder: Option<&str>,
    image: &Prepared,
) -> Result<String, zipline::Failure> {
    let had_token = auth.header().is_some();
    let request = zipline::request(origin, &auth, folder, image);
    drop(auth);
    let reply = transport.send(request)?;
    zipline::parse(origin, &reply, had_token)
}

// ---- text

/// `link` put in `text` at the character `at`, with a space on each side
/// where a word touches it. Returns the text and the cursor after the link.
pub fn insert_link(text: &str, at: usize, link: &str) -> (String, usize) {
    let count = text.chars().count();
    let at = at.min(count);
    let byte = text.char_indices().nth(at).map_or(text.len(), |(b, _)| b);
    let (before, after) = text.split_at(byte);
    let lead = before.chars().next_back().is_some_and(|c| !c.is_whitespace());
    let trail = after.chars().next().is_none_or(|c| !c.is_whitespace());
    let mut out = String::with_capacity(text.len() + link.len() + 2);
    out.push_str(before);
    if lead {
        out.push(' ');
    }
    out.push_str(link);
    if trail {
        out.push(' ');
    }
    let cursor = at + usize::from(lead) + link.chars().count() + usize::from(trail);
    out.push_str(after);
    (out, cursor)
}

/// Puts `link` in a field's text at its cursor (after its selection; at
/// the end when it never had one) and moves the cursor after it.
pub fn insert_into_field(ctx: &egui::Context, field: egui::Id, text: &mut String, link: &str) {
    let mut state = egui::TextEdit::load_state(ctx, field).unwrap_or_default();
    let at = state
        .cursor
        .char_range()
        .map_or(text.chars().count(), |r| r.primary.index.max(r.secondary.index).0);
    let (new, cursor) = insert_link(text, at, link);
    *text = new;
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cursor))));
    egui::TextEdit::store_state(ctx, field, state);
}

// ---- words

pub fn size_text(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b < MIB {
        aurora_i18n::tr!("monitor-unit-kb", value = aurora_i18n::number((b / 1024.0).max(1.0), 0))
    } else {
        aurora_i18n::tr!("memory-mb", value = aurora_i18n::number(b / MIB, 1))
    }
}

fn refusal_text(name: &str, refusal: &Refusal) -> String {
    use aurora_i18n::tr;
    match refusal {
        Refusal::TooLarge(size, max) => tr!(
            "fs-chat-upload-error-too-large",
            name = name.to_owned(),
            size = size_text(*size),
            max = size_text(*max)
        ),
        Refusal::Unsupported => tr!("fs-chat-upload-error-unsupported", name = name.to_owned()),
        Refusal::Corrupt => tr!("fs-chat-upload-error-corrupt", name = name.to_owned()),
        Refusal::TooManyPixels(w, h) => tr!(
            "fs-chat-upload-error-pixels",
            name = name.to_owned(),
            width = i64::from(*w),
            height = i64::from(*h)
        ),
        Refusal::Unreadable => tr!("fs-chat-upload-error-unreadable", name = name.to_owned()),
    }
}

fn failure_text(failure: &zipline::Failure) -> String {
    use aurora_i18n::tr;
    use zipline::Failure;
    match failure {
        Failure::Network => tr!("fs-chat-upload-error-network").to_owned(),
        Failure::Token { had_token: true } => tr!("fs-chat-upload-error-token-refused").to_owned(),
        Failure::Token { had_token: false } => tr!("fs-chat-upload-error-token-needed").to_owned(),
        Failure::Folder => tr!("fs-chat-upload-error-folder").to_owned(),
        Failure::TooLarge => tr!("fs-chat-upload-error-too-large-host").to_owned(),
        Failure::Quota => tr!("fs-chat-upload-error-quota").to_owned(),
        Failure::Type => tr!("fs-chat-upload-error-type").to_owned(),
        Failure::RateLimited => tr!("fs-chat-upload-error-rate").to_owned(),
        Failure::Refused(status) => tr!("fs-chat-upload-error-refused", status = i64::from(*status)),
        Failure::Malformed => tr!("fs-chat-upload-error-malformed").to_owned(),
    }
}

// ---- the user's token (Preferences)

/// Whether the user saved a token for `origin`; asked once, off the
/// interface's thread.
pub fn token_status(ctx: &egui::Context, origin: &str) -> TokenStatus {
    let mut u = uploads();
    if let Some(status) = u.tokens.get(origin) {
        return *status;
    }
    u.tokens.insert(origin.to_owned(), TokenStatus::Checking);
    drop(u);
    let (ctx, origin) = (ctx.clone(), origin.to_owned());
    let _ = std::thread::Builder::new().name("fs-chat-upload-token".into()).spawn(move || {
        let status = if token::SystemStore.read(&origin).is_some() {
            TokenStatus::Saved
        } else {
            TokenStatus::Absent
        };
        uploads().tokens.insert(origin, status);
        ctx.request_repaint();
    });
    TokenStatus::Checking
}

/// Saves (`Some`) or removes the user's token for `origin`.
pub fn set_token(ctx: &egui::Context, origin: &str, value: Option<String>) {
    uploads().tokens.insert(origin.to_owned(), TokenStatus::Checking);
    let (ctx, origin) = (ctx.clone(), origin.to_owned());
    let _ = std::thread::Builder::new().name("fs-chat-upload-token".into()).spawn(move || {
        let status = match value {
            Some(token) => match token::SystemStore.write(&origin, &token) {
                Ok(()) => TokenStatus::Saved,
                Err(e) => {
                    log::warn!("chat upload: the token was not saved: {e}");
                    TokenStatus::Failed
                }
            },
            None => match token::SystemStore.remove(&origin) {
                Ok(()) => TokenStatus::Absent,
                Err(e) => {
                    log::warn!("chat upload: the token was not removed: {e}");
                    TokenStatus::Failed
                }
            },
        };
        uploads().tokens.insert(origin, status);
        ctx.request_repaint();
    });
}

// ---- the chips over a field

/// What a chip needs, copied out of the lock.
struct Shown {
    id: u64,
    label: String,
    origin: String,
    state: State,
    image: Option<Arc<Prepared>>,
    texture: Option<egui::TextureHandle>,
}

enum Choice {
    Upload(u64),
    Cancel(u64),
    Retry(u64),
}

fn show_jobs(ctx: &egui::Context, p: &Palette, field: egui::Id, rect: egui::Rect) {
    let shown: Vec<Shown> = {
        let mut u = uploads();
        u.jobs
            .iter_mut()
            .filter(|j| j.field == field && !matches!(j.state, State::Ready(_)))
            .map(|j| {
                if j.texture.is_none()
                    && matches!(j.state, State::Confirm)
                    && let Some(thumb) = j.image.as_ref().and_then(|i| i.thumb.clone())
                {
                    j.texture = Some(ctx.load_texture(format!("fs-chat-upload-{}", j.id), thumb, egui::TextureOptions::LINEAR));
                }
                Shown {
                    id: j.id,
                    label: j.label.clone(),
                    origin: j.origin.clone(),
                    state: j.state.clone(),
                    image: j.image.clone(),
                    texture: j.texture.clone(),
                }
            })
            .collect()
    };
    if shown.is_empty() {
        return;
    }
    if shown.iter().any(|s| matches!(s.state, State::Preparing | State::Uploading)) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let mut choices = Vec::new();
    egui::Area::new(field.with("fs-chat-upload"))
        .fade_in(false)
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(rect.left_top() - egui::vec2(0.0, 6.0))
        .show(ctx, |ui| {
            crate::ui::widgets::overlay(ui, p, egui::Margin::same(8), |ui| {
                ui.set_max_width(340.0);
                for (n, job) in shown.iter().enumerate() {
                    if n > 0 {
                        ui.add_space(6.0);
                    }
                    match &job.state {
                        State::Confirm => confirm(ui, p, job, n == 0, &mut choices),
                        State::Preparing | State::Uploading => busy(ui, p, job, &mut choices),
                        State::Failed { message, retry } => failed(ui, p, job, message, *retry, &mut choices),
                        State::Ready(_) => {}
                    }
                }
            });
        });
    if choices.is_empty() {
        return;
    }
    let mut u = uploads();
    for choice in choices {
        match choice {
            Choice::Upload(id) | Choice::Retry(id) => {
                if let Some(job) = u.jobs.iter_mut().find(|j| j.id == id) {
                    start_upload(ctx, job);
                }
            }
            Choice::Cancel(id) => u.jobs.retain(|j| j.id != id),
        }
    }
    ctx.request_repaint();
}

fn small(text: impl Into<String>, color: egui::Color32) -> egui::RichText {
    egui::RichText::new(text).size(12.5).color(color)
}

fn icon(ui: &mut egui::Ui, name: &str, side: f32, tint: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
    if let Some(texture) = crate::ui::icons::global(name) {
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint,
        );
    }
}

fn confirm(ui: &mut egui::Ui, p: &Palette, job: &Shown, first: bool, choices: &mut Vec<Choice>) {
    use aurora_i18n::tr;
    let Some(image) = &job.image else { return };
    ui.horizontal_top(|ui| {
        if let Some(texture) = &job.texture {
            let size = texture.size_vec2();
            let scale = (96.0 / size.x.max(size.y)).min(1.0);
            ui.add(egui::Image::new((texture.id(), size * scale)));
        }
        ui.vertical(|ui| {
            ui.label(small(tr!("fs-chat-upload-confirm-title"), p.ink).strong());
            ui.label(small(job.label.clone(), p.ink));
            ui.label(small(
                tr!(
                    "fs-chat-upload-confirm-details",
                    format = image.format.name(),
                    width = i64::from(image.width),
                    height = i64::from(image.height),
                    size = size_text(image.bytes.len() as u64)
                ),
                p.muted,
            ));
            ui.label(small(
                tr!("fs-chat-upload-confirm-host", host = settings::host_name(&job.origin)),
                p.muted,
            ));
        });
    });
    ui.add_space(4.0);
    ui.label(small(tr!("fs-chat-upload-confirm-public"), p.amber));
    if image.stripped {
        ui.label(small(tr!("fs-chat-upload-confirm-stripped"), p.muted));
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let size = egui::vec2(0.0, 26.0);
        use crate::ui::widgets::{ActionKind, action_button};
        if action_button(ui, p, "upload", tr!("fs-chat-upload-upload"), ActionKind::Primary, size).clicked() {
            choices.push(Choice::Upload(job.id));
        }
        if action_button(ui, p, "x", tr!("common-cancel"), ActionKind::Flat, size).clicked()
            || (first && ui.input(|i| i.key_pressed(egui::Key::Escape)))
        {
            choices.push(Choice::Cancel(job.id));
        }
    });
    ui.label(small(tr!("fs-chat-upload-confirm-skip"), p.muted_dim));
}

fn busy(ui: &mut egui::Ui, p: &Palette, job: &Shown, choices: &mut Vec<Choice>) {
    use aurora_i18n::tr;
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(14.0).color(p.violet_light));
        let text = match (&job.state, &job.image) {
            (State::Uploading, Some(image)) => tr!(
                "fs-chat-upload-uploading",
                name = job.label.clone(),
                size = size_text(image.bytes.len() as u64)
            ),
            _ => tr!("fs-chat-upload-preparing", name = job.label.clone()),
        };
        ui.label(small(text, p.ink));
        if crate::ui::widgets::icon_button(ui, p, "x", tr!("fs-chat-upload-cancel-tip"), true) {
            choices.push(Choice::Cancel(job.id));
        }
    });
}

fn failed(ui: &mut egui::Ui, p: &Palette, job: &Shown, message: &str, retry: bool, choices: &mut Vec<Choice>) {
    use aurora_i18n::tr;
    ui.horizontal_top(|ui| {
        icon(ui, "alert-triangle-filled", 16.0, p.danger);
        ui.vertical(|ui| {
            ui.set_max_width(290.0);
            ui.label(small(message.to_owned(), p.ink));
            ui.horizontal(|ui| {
                use crate::ui::widgets::{ActionKind, action_button};
                let size = egui::vec2(0.0, 24.0);
                if retry
                    && job.image.is_some()
                    && action_button(ui, p, "refresh", tr!("fs-chat-upload-retry"), ActionKind::Flat, size).clicked()
                {
                    choices.push(Choice::Retry(job.id));
                }
                if action_button(ui, p, "x", tr!("fs-chat-upload-dismiss"), ActionKind::Flat, size).clicked() {
                    choices.push(Choice::Cancel(job.id));
                }
            });
        });
    });
}
