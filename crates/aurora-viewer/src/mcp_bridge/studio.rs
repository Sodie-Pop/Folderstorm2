//! Folderstorm: where the local assistant's camera moves, pictures and
//! thumbnails wait on the frame thread (folderstorm/plans/mcp.md, "Phase 3,
//! part 1"): a move until the camera stops, a picture until the camera
//! stops and no other photo is being drawn, then for its photo and its
//! encoding on a worker, a thumbnail until the gallery's snapshot ends.
//! Polled once per frame with the time given, so the tests drive it
//! without a clock, a window or a GPU.

use super::camera::Pose;
use super::picture::{self, Plan, Request, Source};
use folderstorm_mcp_bridge::protocol::{CHANGE_WAIT, IMAGE_BYTES_MAX, IMAGE_KEY, SNAPSHOT_WAIT};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// The camera is still once it has not moved for this long.
pub const STILL: Duration = Duration::from_millis(150);
/// A move answers, and a picture starts, after this long at most even if
/// the camera still moves (it follows a walking avatar).
pub const SETTLE_MAX: Duration = Duration::from_secs(3);
/// Moves smaller than this, in metres or radians, are not moves.
const EPSILON: f32 = 1e-3;
/// A picture is given up after this long (the bridge waits a little more).
pub const PHOTO_DEADLINE: Duration = Duration::from_secs(SNAPSHOT_WAIT.as_secs() - 3);
/// A thumbnail is given up after this long.
pub const THUMBNAIL_DEADLINE: Duration = Duration::from_secs(SNAPSHOT_WAIT.as_secs() + CHANGE_WAIT.as_secs() - 3);

struct MoveWait {
    call: Pending,
    reply: Value,
    since: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhotoState {
    /// Waiting for the camera to stop and for any photo being drawn.
    Waiting,
    /// Its photo is being drawn.
    Drawing(Plan),
}

struct PhotoWait {
    call: Pending,
    request: Request,
    state: PhotoState,
    since: Instant,
}

struct ThumbnailWait {
    call: Pending,
    reply: Value,
    since: Instant,
}

/// What `photo_step` asks of the app.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PhotoStep {
    Idle,
    /// Plan this picture for the window and start its photo, then call
    /// `photo_started` with the plan.
    Start(Request),
}

#[derive(Default)]
pub struct Studio {
    moves: Vec<MoveWait>,
    photos: VecDeque<PhotoWait>,
    encoding: Vec<(Pending, Receiver<Result<Value, ToolError>>)>,
    thumbnail: Option<ThumbnailWait>,
    /// The camera last frame and since when it has not moved.
    pose: Option<Pose>,
    still_since: Option<Instant>,
    /// The view's field of view before an exact shot set one: given back
    /// by `camera_reset` and `camera_preset`.
    pub fov_before: Option<f32>,
}

fn moved(a: &Pose, b: &Pose) -> bool {
    a.position.distance(b.position) > EPSILON || a.target.distance(b.target) > EPSILON || (a.fov - b.fov).abs() > EPSILON
}

impl Studio {
    /// The bridge stopped: the waiting calls end with it ("the call was
    /// dropped"); the user's field of view is still remembered.
    pub fn drop_calls(&mut self) {
        *self = Studio {
            fov_before: self.fov_before,
            ..Default::default()
        };
    }

    /// Once per frame, first: where the camera is.
    pub fn observe(&mut self, pose: Pose, now: Instant) {
        if self.pose.is_none_or(|last| moved(&last, &pose)) {
            self.still_since = Some(now);
        }
        self.pose = Some(pose);
    }

    /// The camera has not moved for `STILL`.
    pub fn still(&self, now: Instant) -> bool {
        self.still_since.is_some_and(|since| now.saturating_duration_since(since) >= STILL)
    }

    /// A move was made: it answers once the camera stops.
    pub fn wait_move(&mut self, call: Pending, reply: Value, now: Instant) {
        // the camera starts moving from this frame on
        self.still_since = Some(now);
        self.moves.push(MoveWait { call, reply, since: now });
    }

    /// A picture to take, after the moves before it.
    pub fn queue_photo(&mut self, call: Pending, request: Request, now: Instant) {
        self.photos.push_back(PhotoWait {
            call,
            request,
            state: PhotoState::Waiting,
            since: now,
        });
    }

    /// A thumbnail started through the gallery's snapshot.
    pub fn thumbnail_started(&mut self, call: Pending, reply: Value, now: Instant) {
        if let Some(earlier) = self.thumbnail.take() {
            earlier
                .call
                .answer(Err(ToolError::new(ErrorCode::Internal, "the thumbnail was replaced")));
        }
        self.thumbnail = Some(ThumbnailWait { call, reply, since: now });
    }

    /// Whether a thumbnail of the assistant is under way.
    pub fn thumbnail_busy(&self) -> bool {
        self.thumbnail.is_some()
    }

    /// The gallery's snapshot ended (`error` None: the server confirmed the
    /// picture).
    pub fn thumbnail_done(&mut self, error: Option<&str>) {
        let Some(wait) = self.thumbnail.take() else { return };
        match error {
            None => {
                let mut reply = wait.reply;
                reply["done"] = json!(true);
                wait.call.answer(Ok(reply));
            }
            Some(e) => wait
                .call
                .answer(Err(ToolError::new(ErrorCode::Failed, format!("the picture was not set: {e}")))),
        }
    }

    /// Once per frame (in the frame's drawing, where the photos start):
    /// whether to start the oldest picture's photo now. `drawing`: a photo
    /// is being drawn. A picture whose photo ended without its pixels fails.
    pub fn photo_step(&mut self, drawing: bool, now: Instant) -> PhotoStep {
        let still = self.still(now);
        let moving = !self.moves.is_empty();
        let Some(front) = self.photos.front() else {
            return PhotoStep::Idle;
        };
        match front.state {
            PhotoState::Drawing(_) if !drawing => {
                if let Some(lost) = self.photos.pop_front() {
                    lost.call
                        .answer(Err(ToolError::new(ErrorCode::Failed, "the picture could not be drawn")));
                }
                PhotoStep::Idle
            }
            PhotoState::Drawing(_) => PhotoStep::Idle,
            PhotoState::Waiting if drawing => PhotoStep::Idle,
            PhotoState::Waiting => {
                let waited = now.saturating_duration_since(front.since) >= SETTLE_MAX;
                if (still && !moving) || waited {
                    PhotoStep::Start(front.request)
                } else {
                    PhotoStep::Idle
                }
            }
        }
    }

    /// The oldest picture's photo started, planned as `plan`.
    pub fn photo_started(&mut self, plan: Plan) {
        if let Some(front) = self.photos.front_mut().filter(|p| p.state == PhotoState::Waiting) {
            front.state = PhotoState::Drawing(plan);
        }
    }

    /// The photo of the oldest picture came back (`size`, RGBA): cut,
    /// scaled, encoded and maybe saved on a worker. `folder` is the Photo
    /// window's.
    pub fn photo_drawn(&mut self, size: (u32, u32), pixels: Vec<u8>, folder: PathBuf, stamp: String) {
        if !self.photos.front().is_some_and(|p| matches!(p.state, PhotoState::Drawing(_))) {
            // a picture given up meanwhile, or another's photo
            return;
        }
        let Some(PhotoWait {
            call,
            request,
            state: PhotoState::Drawing(plan),
            ..
        }) = self.photos.pop_front()
        else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new().name("mcp-picture".into()).spawn(move || {
            let _ = tx.send(encode(pixels, size, &plan, &request, &folder, &stamp));
        });
        match spawned {
            Ok(_) => self.encoding.push((call, rx)),
            Err(e) => call.answer(Err(ToolError::new(ErrorCode::Internal, format!("no worker for the picture: {e}")))),
        }
    }

    /// Once per frame: the camera moves that are over, the pictures
    /// encoded, and what waited too long.
    pub fn tick(&mut self, pose: &Pose, now: Instant) {
        let still = self.still(now);
        for wait in std::mem::take(&mut self.moves) {
            if still || now.saturating_duration_since(wait.since) >= SETTLE_MAX {
                let mut reply = wait.reply;
                if let (Some(object), Value::Object(at)) = (reply.as_object_mut(), pose.json()) {
                    object.extend(at);
                }
                wait.call.answer(Ok(reply));
            } else {
                self.moves.push(wait);
            }
        }
        for (call, rx) in std::mem::take(&mut self.encoding) {
            match rx.try_recv() {
                Ok(result) => call.answer(result),
                Err(TryRecvError::Empty) => self.encoding.push((call, rx)),
                Err(TryRecvError::Disconnected) => call.answer(Err(ToolError::new(ErrorCode::Internal, "the picture was lost"))),
            }
        }
        // a picture still waiting (its photo may still come: it is dropped)
        while let Some(front) = self.photos.front() {
            if now.saturating_duration_since(front.since) < PHOTO_DEADLINE {
                break;
            }
            if let Some(late) = self.photos.pop_front() {
                late.call
                    .answer(Err(ToolError::new(ErrorCode::Timeout, "the picture was not drawn in time")));
            }
        }
        if self
            .thumbnail
            .as_ref()
            .is_some_and(|t| now.saturating_duration_since(t.since) >= THUMBNAIL_DEADLINE)
            && let Some(late) = self.thumbnail.take()
        {
            late.call.answer(Err(ToolError::new(
                ErrorCode::Timeout,
                "the picture was not confirmed in time; it may still be set: look again before retrying",
            )));
        }
    }

    /// Calls in each state, for the tests: moves, pictures waiting or
    /// drawn, pictures encoding, thumbnail.
    #[cfg(test)]
    pub fn counts(&self) -> (usize, usize, usize, bool) {
        (self.moves.len(), self.photos.len(), self.encoding.len(), self.thumbnail.is_some())
    }
}

/// The picture's answer: the image (base64) and what it is.
fn encode(
    pixels: Vec<u8>,
    size: (u32, u32),
    plan: &Plan,
    request: &Request,
    folder: &std::path::Path,
    stamp: &str,
) -> Result<Value, ToolError> {
    use base64::Engine as _;
    let encoded = picture::finish(pixels, size, plan, request.format, IMAGE_BYTES_MAX).map_err(|e| ToolError::new(ErrorCode::Failed, e))?;
    let mut reply = json!({
        "width": encoded.size.0,
        "height": encoded.size.1,
        "format": encoded.format.name(),
        "bytes": encoded.bytes.len(),
        "source": match request.source {
            Source::Window => "window",
            Source::World => "world",
        },
        "interface": plan.render.interface,
        "huds": plan.render.huds,
    });
    if let Some(quality) = encoded.quality {
        reply["quality"] = json!(quality);
    }
    if request.save {
        match picture::save(folder, stamp, &encoded) {
            Ok(path) => reply["saved_to"] = json!(path.display().to_string()),
            Err(e) => reply["save_error"] = json!(e),
        }
    }
    reply[IMAGE_KEY] = json!({
        "mime_type": encoded.format.mime(),
        "data": base64::engine::general_purpose::STANDARD.encode(&encoded.bytes),
    });
    Ok(reply)
}
