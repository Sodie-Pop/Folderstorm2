//! Folderstorm: the local assistant's camera read and file upload tools
//! (folderstorm/plans/mcp-uploads.md), after Folderstorm (C++)'s
//! `camera_get`, `inventory_upload_*` and the texture mode of
//! `inventory_snapshot_upload`. On the frame thread, kept by the
//! [`super::Desk`]:
//!
//! - **Plans**: a preview reads, hashes and decodes local files on a worker
//!   ([`scan`]), prices them with the account's prices and keeps the bytes
//!   for 120 s (4 plans, 128 MiB in all). What is uploaded is exactly what
//!   was priced.
//! - **The execute** takes its plan once; a plan above L$0 always asks,
//!   the dialog showing the total, the balance and the files with their
//!   price. The run then creates the folders one acknowledged request at a
//!   time, encodes each file on a worker and sends it with its frozen price
//!   (`expected_cost`), one at a time. Before each send: Upload not Never,
//!   the destination still takes items, the account's price not higher,
//!   the balance enough. A failure, an unclear answer, a higher price or a
//!   cancel stops what is not sent; nothing is ever sent twice.
//! - **The picture** of `inventory_snapshot_upload` comes from the studio
//!   (`camera_snapshot`'s path) as PNG bytes and becomes a one-file plan;
//!   its thumbnail goes through the Image window's path once uploaded.
//! - **Mesh review** opens a Collada file in the uploader's Mesh window
//!   (the app's part): nothing is uploaded or spent by the assistant.
//!
//! Creators' permissions (AGENTS.md § 2): the user's own files only; no
//! answer, receipt or status carries an asset id or a local path.

pub mod demo;
pub mod scan;

use super::changes::{Planned, Work};
use super::tools::{Ctx, err, path as folder_path, ready, scope, uuid_arg};
use crate::world::inventory::{Inventory, actions};
use aurora_net::inventory::InvItem;
use aurora_net::inventory::operations::{Mutation, Operation, Reply};
use aurora_net::inventory::upload::{self, Kind, Prices};
use folderstorm_mcp_bridge::protocol::ASK_TIMEOUT;
use folderstorm_mcp_bridge::protocol::uploads::{
    EXECUTE, FOLDERS_MAX, MESH_REVIEW, PLAN_BYTES_MAX, PLAN_SECONDS, PLANS_MAX, PREVIEW_WAIT, SNAPSHOT, TEXTURE_DIMENSIONS,
};
use folderstorm_mcp_bridge::receipts::{self, Receipt, Row, RowOutcome};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{Class, Decision, ErrorCode, Permissions, ToolError};
use scan::{Input, Rules, Scan, Scanned};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// LLFolderType::FT_SNAPSHOT_CATEGORY: the Photo Album.
const FT_SNAPSHOT: i32 = 15;
/// Files the Ask dialog lists at most.
const LINES_SHOWN: usize = 5;
/// Runs remembered for `inventory_upload_status` once they ended.
const ENDED_MAX: usize = 8;
/// How long the picture set from a new texture may take.
const THUMBNAIL_WAIT: Duration = Duration::from_secs(60);
/// Previews reading their files at most.
const SCANS_MAX: usize = 4;

/// What the upload tools read of the account, given each frame by the app.
#[derive(Debug, Clone, Default)]
pub struct Account {
    /// The account's upload prices (benefits, else the economy's price).
    pub prices: Prices,
    /// The L$ balance, when known.
    pub balance: Option<i32>,
    /// The longest sound, in seconds.
    pub sound_limit: u32,
    /// The uploader's texture options (Preferences).
    pub texture: aurora_assets::import_texture::Options,
}

/// Whether `tool` is one of this group's changes.
pub fn is_change(tool: &str) -> bool {
    tool == EXECUTE || tool == MESH_REVIEW || tool == SNAPSHOT
}

/// The classes a call needs beyond its catalogue's: an execute whose plan
/// sets a picture needs Edit too (frozen in its arguments, [`Uploads::freeze`]).
pub fn extra_classes(tool: &str, args: &Value) -> Vec<Class> {
    if tool == EXECUTE && args["frozen"]["thumbnail"].as_bool() == Some(true) {
        vec![Class::Edit]
    } else {
        Vec::new()
    }
}

fn invalid(message: impl Into<String>) -> ToolError {
    err(ErrorCode::InvalidArguments, message)
}

fn not_allowed(message: impl Into<String>) -> ToolError {
    err(ErrorCode::NotAllowed, message)
}

fn known<'a>(args: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, ToolError> {
    match args.as_object() {
        Some(a) if a.keys().all(|k| keys.contains(&k.as_str())) => Ok(a),
        _ => Err(invalid("unknown argument")),
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Texture => "texture",
        Kind::Snapshot => "snapshot",
        Kind::Sound => "sound",
        Kind::Animation => "animation",
        Kind::Material => "material",
        Kind::Mesh => "mesh",
    }
}

/// A plan's file.
#[derive(Clone)]
struct File {
    scan: Scan,
    price: i32,
}

impl File {
    fn json(&self, index: usize) -> Value {
        let s = &self.scan;
        let mut row = json!({
            "index": index,
            "file": s.file,
            "name": s.name,
            "kind": kind_name(s.kind),
            "folder": s.folder.join("/"),
            "bytes": s.bytes,
            "sha256": s.sha256,
            "price": self.price,
        });
        if matches!(s.kind, Kind::Texture | Kind::Snapshot) {
            row["width"] = json!(s.size.0);
            row["height"] = json!(s.size.1);
        }
        if let Some(seconds) = s.seconds {
            row["seconds"] = json!((f64::from(seconds) * 10.0).round() / 10.0);
        }
        row
    }
}

/// A preview kept until it runs or expires.
struct Plan {
    id: String,
    destination: Uuid,
    root: Option<String>,
    files: Vec<File>,
    /// The folders to create below the destination, parents first.
    folders: Vec<Vec<String>>,
    total: i32,
    rules: Rules,
    /// The entry whose picture the new texture becomes (a snapshot plan).
    thumbnail: Option<Uuid>,
    created: Instant,
    /// The execute that took it.
    used: Option<String>,
}

impl Plan {
    fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.scan.bytes).sum()
    }

    /// 120 s unused; a plan taken by an execute waiting for the dialog
    /// lasts the dialog's 60 s more (a run takes it out).
    fn expired(&self, now: Instant) -> bool {
        let life = Duration::from_secs(PLAN_SECONDS) + if self.used.is_some() { ASK_TIMEOUT } else { Duration::ZERO };
        now.saturating_duration_since(self.created) >= life
    }
}

/// The folders a folder upload creates: every folder holding files and its
/// parents, parents first, each once.
fn folders_of(files: &[File]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    for file in files {
        for depth in 1..=file.scan.folder.len() {
            let path = file.scan.folder[..depth].to_vec();
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out.sort_by_key(Vec::len);
    out
}

/// A preview reading its files on a worker.
struct ScanWait {
    call: Pending,
    rx: Receiver<Result<Scanned, String>>,
    destination: Uuid,
    rules: Rules,
    thumbnail: Option<Uuid>,
    deadline: Instant,
}

/// A file of a run.
#[derive(Debug)]
enum FileState {
    Waiting,
    Encoding(Receiver<Result<Vec<u8>, String>>),
    Sent(Uuid),
    Done(Uuid),
    Failed(String),
    Unknown(String),
    NotSent,
    Cancelled,
}

impl FileState {
    fn name(&self) -> &'static str {
        match self {
            FileState::Waiting | FileState::Encoding(_) => "waiting",
            FileState::Sent(_) => "sending",
            FileState::Done(_) => "done",
            FileState::Failed(_) => "failed",
            FileState::Unknown(_) => "unknown",
            FileState::NotSent => "not_sent",
            FileState::Cancelled => "cancelled",
        }
    }

    fn sent(&self) -> bool {
        matches!(
            self,
            FileState::Sent(_) | FileState::Done(_) | FileState::Failed(_) | FileState::Unknown(_)
        )
    }

    fn busy(&self) -> bool {
        matches!(self, FileState::Encoding(_) | FileState::Sent(_))
    }
}

/// Why a run stopped sending.
#[derive(Debug, Clone)]
struct Stop {
    code: ErrorCode,
    reason: String,
}

#[derive(Debug, Clone, PartialEq)]
enum Picture {
    Waiting,
    Sent(Uuid, Instant),
    Done,
    Failed(String),
}

/// What the app sends for a run.
#[derive(Debug, Clone)]
pub enum Act {
    /// Create one folder (`NetCommand::EditInventory`).
    Folder { request: Uuid, change: Mutation },
    /// Upload one file (`NetCommand::UploadAsset`), with the uploads'
    /// permissions of Preferences for its kind (the app's part).
    Upload { request: Uuid, upload: upload::Request },
    /// Set `target`'s picture from the new texture through the Image
    /// window's path. The asset id stays in the viewer.
    Thumbnail { request: Uuid, target: Uuid, asset: Uuid },
}

/// An execute under way.
struct Run {
    call: Option<Pending>,
    operation_id: String,
    plan: Plan,
    folder_ids: HashMap<Vec<String>, Uuid>,
    folder_pending: Option<(Uuid, Uuid, Vec<String>)>,
    next_folder: usize,
    states: Vec<FileState>,
    /// The new texture's asset, for the picture only (never answered).
    asset: Option<Uuid>,
    picture: Option<Picture>,
    spent: i32,
    stop: Option<Stop>,
}

impl Run {
    fn report(&self, state: &str) -> Value {
        let files: Vec<Value> = self
            .plan
            .files
            .iter()
            .zip(&self.states)
            .enumerate()
            .map(|(index, (file, outcome))| {
                let mut row = json!({"index": index, "name": file.scan.name, "folder": file.scan.folder.join("/"), "price": file.price, "outcome": outcome.name()});
                match outcome {
                    FileState::Done(item) => row["item_id"] = json!(item),
                    FileState::Failed(reason) | FileState::Unknown(reason) => row["reason"] = json!(reason),
                    _ => {}
                }
                row
            })
            .collect();
        let mut report = json!({
            "operation_id": self.operation_id,
            "plan_id": self.plan.id,
            "state": state,
            "files": files,
            "folders_created": self.folder_ids.len(),
            "spent": self.spent,
            "total": self.plan.total,
        });
        if let Some(stop) = &self.stop {
            report["stopped"] = json!(stop.reason);
        }
        if let Some(picture) = &self.picture {
            report["thumbnail"] = match picture {
                Picture::Waiting | Picture::Sent(..) => json!("waiting"),
                Picture::Done => json!("done"),
                Picture::Failed(reason) => json!({"failed": reason}),
            };
        }
        report
    }

    fn item_ids(&self) -> Vec<Uuid> {
        self.states
            .iter()
            .filter_map(|s| match s {
                FileState::Done(item) => Some(*item),
                _ => None,
            })
            .collect()
    }

    /// Stop sending: the files not sent yet are not sent (`cancelled` when
    /// the assistant or the user stopped it).
    fn halt(&mut self, code: ErrorCode, reason: impl Into<String>) {
        if self.stop.is_none() {
            self.stop = Some(Stop {
                code,
                reason: reason.into(),
            });
        }
        let cancelled = code == ErrorCode::Cancelled;
        for state in &mut self.states {
            if matches!(state, FileState::Waiting) {
                *state = if cancelled { FileState::Cancelled } else { FileState::NotSent };
            }
        }
        if self.folder_pending.is_none() {
            self.next_folder = self.plan.folders.len();
        }
        if matches!(self.picture, Some(Picture::Waiting)) {
            self.picture = Some(Picture::Failed(self.stop.as_ref().map(|s| s.reason.clone()).unwrap_or_default()));
        }
    }

    fn mark_sent(&self) {
        if let Some(call) = &self.call {
            call.receipt(Receipt::mark_sent);
        }
    }
}

/// An execute's end, kept for the status.
struct Ended {
    operation_id: String,
    plan_id: String,
    report: Value,
}

/// This login's upload plans, previews and run.
#[derive(Default)]
pub struct Uploads {
    account: Account,
    plans: VecDeque<Plan>,
    scans: Vec<ScanWait>,
    run: Option<Run>,
    acts: Vec<Act>,
    ended: VecDeque<Ended>,
}

/// The answer when the run's Upload class went to Never.
const PERMISSIONS_CHANGED: &str = "the user set Upload to Never: the files not sent yet were not sent";

impl Uploads {
    /// The account as the app reads it this frame.
    pub fn set_account(&mut self, account: Account) {
        self.account = account;
    }

    /// Whether previews or a run wait for something (the tick then needs
    /// the inventory).
    pub fn busy(&self) -> bool {
        !self.scans.is_empty() || self.run.is_some() || !self.plans.is_empty()
    }

    /// What the app sends now.
    pub fn take_acts(&mut self) -> Vec<Act> {
        std::mem::take(&mut self.acts)
    }

    fn rules(&self, max_texture: u32) -> Rules {
        Rules {
            max_texture,
            sound_limit: self.account.sound_limit.max(1),
            texture: self.account.texture,
        }
    }

    /// `inventory_upload_preview`: the arguments checked here, the files
    /// read on a worker; [`Uploads::tick`] answers.
    pub fn start_preview(&mut self, call: Pending, cx: &Ctx, now: Instant) {
        let result = (|| {
            ready(cx)?;
            let args = known(
                &call.arguments,
                &["files", "directory", "folder_id", "path", "max_texture_dimension"],
            )?;
            let destination = destination(&call.arguments, cx, None)?;
            let max_texture = match args.get("max_texture_dimension") {
                None | Some(Value::Null) => TEXTURE_DIMENSIONS[0],
                Some(v) => v
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| TEXTURE_DIMENSIONS.contains(n))
                    .ok_or_else(|| invalid("`max_texture_dimension` is 1024 or 2048"))?,
            };
            let input = match (args.get("files"), args.get("directory")) {
                (Some(files), None) => {
                    let list = files.as_array().ok_or_else(|| invalid("`files` is a list of absolute paths"))?;
                    let paths = list
                        .iter()
                        .map(|p| {
                            p.as_str()
                                .map(PathBuf::from)
                                .ok_or_else(|| invalid("`files` holds something that is not a path"))
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Input::Files(paths)
                }
                (None, Some(directory)) => Input::Directory(PathBuf::from(
                    directory.as_str().ok_or_else(|| invalid("`directory` is an absolute path"))?,
                )),
                _ => return Err(invalid("give `files` or `directory`, one of them")),
            };
            Ok((destination, max_texture, input))
        })();
        match result {
            Ok((destination, max_texture, input)) => self.scan(call, input, destination, max_texture, None, now),
            Err(e) => call.answer(Err(e)),
        }
    }

    fn scan(&mut self, call: Pending, input: Input, destination: Uuid, max_texture: u32, thumbnail: Option<Uuid>, now: Instant) {
        if self.scans.len() >= SCANS_MAX {
            return call.answer(Err(err(
                ErrorCode::Busy,
                "other previews are still reading their files: try again in a moment",
            )));
        }
        let rules = self.rules(max_texture);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new().name("mcp-upload-scan".into()).spawn(move || {
            let _ = tx.send(scan::run(input, &rules));
        });
        match spawned {
            Ok(_) => self.scans.push(ScanWait {
                call,
                rx,
                destination,
                rules,
                thumbnail,
                deadline: now + PREVIEW_WAIT,
            }),
            Err(e) => call.answer(Err(err(ErrorCode::Internal, format!("no worker for the preview: {e}")))),
        }
    }

    /// `inventory_snapshot_upload`'s arguments, checked before its picture
    /// is taken: the studio's request (PNG, no interface, not saved).
    pub fn snapshot_plan(&self, call: &Pending, cx: &Ctx) -> Result<Planned, ToolError> {
        ready(cx)?;
        let args = &call.arguments;
        let mut picture = Map::new();
        for (key, value) in known(
            args,
            &[
                "folder_id",
                "path",
                "name",
                "thumbnail_id",
                "source",
                "width",
                "height",
                "max_edge",
                "crop",
                "huds",
            ],
        )? {
            if !matches!(key.as_str(), "folder_id" | "path" | "name" | "thumbnail_id") {
                picture.insert(key.clone(), value.clone());
            }
        }
        picture.entry("source").or_insert(json!("world"));
        picture.insert("format".into(), json!("png"));
        let mut request = super::camera::snapshot_request(&Value::Object(picture))?;
        request.save = false;
        request.interface = false;
        let destination = destination(args, cx, actions::system(cx.inv, FT_SNAPSHOT))?;
        snapshot_name(args)?;
        let thumbnail = thumbnail_target(args, cx)?;
        let summary = match thumbnail {
            Some(target) => aurora_i18n::tr!(
                "fs-mcp-ask-snapshot-upload-thumbnail",
                folder = folder_name(cx.inv, destination),
                target = entry_name(cx.inv, target)
            )
            .to_owned(),
            None => aurora_i18n::tr!("fs-mcp-ask-snapshot-upload", folder = folder_name(cx.inv, destination)).to_owned(),
        };
        Ok(Planned {
            work: Work::Snapshot(request),
            summary,
            reply: json!({}),
            created: None,
        })
    }

    /// The studio drew `call`'s picture: it becomes a one-file plan.
    pub fn stage_snapshot(&mut self, call: Pending, png: Result<Vec<u8>, String>, cx: &Ctx, now: Instant) {
        let png = match png {
            Ok(png) => png,
            Err(e) => return call.answer(Err(err(ErrorCode::Failed, format!("the picture was not taken: {e}")))),
        };
        let args = call.arguments.clone();
        let checked = (|| {
            let destination = destination(&args, cx, actions::system(cx.inv, FT_SNAPSHOT))?;
            let name = snapshot_name(&args)?;
            let thumbnail = thumbnail_target(&args, cx)?;
            Ok::<_, ToolError>((destination, name, thumbnail))
        })();
        match checked {
            Ok((destination, name, thumbnail)) => {
                let max_texture = TEXTURE_DIMENSIONS[0];
                self.scan(call, Input::Picture { name, png }, destination, max_texture, thumbnail, now);
            }
            Err(e) => call.answer(Err(e)),
        }
    }

    /// A preview read: priced, kept, answered.
    fn plan_scanned(
        &mut self,
        (destination, rules, thumbnail): (Uuid, Rules, Option<Uuid>),
        scanned: Scanned,
        cx: &Ctx,
        decision: Decision,
        now: Instant,
    ) -> Result<Value, ToolError> {
        let mut files = Vec::with_capacity(scanned.files.len());
        for scan in scanned.files {
            let price = self.account.prices.cost(scan.kind, scan.size).filter(|p| *p >= 0).ok_or_else(|| {
                err(
                    ErrorCode::NotReady,
                    "this account's upload price is not known yet: try again once logged in",
                )
            })?;
            files.push(File { scan, price });
        }
        if !actions::destination(cx.inv, destination) || cx.protected.contains(&destination) {
            return Err(not_allowed("this folder does not take new items"));
        }
        let folders = folders_of(&files);
        if folders.len() > FOLDERS_MAX {
            return Err(invalid(format!(
                "the folder would create more than {FOLDERS_MAX} inventory folders"
            )));
        }
        let total = files
            .iter()
            .try_fold(0i32, |sum, f| sum.checked_add(f.price))
            .ok_or_else(|| invalid("the total is too large"))?;
        let plan = Plan {
            id: format!("up-{}", Uuid::new_v4().simple()),
            destination,
            root: scanned.root,
            files,
            folders,
            total,
            rules,
            thumbnail,
            created: now,
            used: None,
        };
        let answer = self.preview_answer(&plan, cx, decision);
        self.keep(plan, now);
        Ok(answer)
    }

    /// Keep `plan`: 4 per login, 128 MiB staged in all (the oldest unused go).
    fn keep(&mut self, plan: Plan, now: Instant) {
        self.plans.retain(|p| !p.expired(now));
        let bytes = plan.bytes();
        while self.plans.iter().filter(|p| p.used.is_none()).count() >= PLANS_MAX
            || self.plans.iter().filter(|p| p.used.is_none()).map(Plan::bytes).sum::<u64>() + bytes > PLAN_BYTES_MAX
        {
            let Some(index) = self.plans.iter().position(|p| p.used.is_none()) else {
                break;
            };
            self.plans.remove(index);
        }
        self.plans.push_back(plan);
    }

    fn preview_answer(&self, plan: &Plan, cx: &Ctx, decision: Decision) -> Value {
        let execution = match decision {
            Decision::Refuse => "never",
            _ if plan.total > 0 => "ask",
            Decision::Ask => "ask",
            Decision::Run => "allow",
        };
        let balance = self.account.balance;
        json!({
            "plan_id": plan.id,
            "expires_in": PLAN_SECONDS,
            "destination": {"folder_id": plan.destination, "path": folder_path(cx.inv, plan.destination)},
            "root_folder": plan.root,
            "files": plan.files.iter().enumerate().map(|(i, f)| f.json(i)).collect::<Vec<_>>(),
            "folders": plan.folders.iter().map(|f| f.join("/")).collect::<Vec<_>>(),
            "total": plan.total,
            "currency": "L$",
            "balance": balance,
            "balance_after": balance.map(|b| b - plan.total),
            "affordable": plan.total == 0 || balance.is_some_and(|b| b >= plan.total),
            "thumbnail_id": plan.thumbnail,
            "execution": execution,
            "always_asks": plan.total > 0,
            "next": "inventory_upload_execute with this plan_id and a new client_request_id; anything above L$0 asks the user in the viewer with the price",
        })
    }

    /// An execute just taken, once: its plan is checked and taken, and its
    /// arguments carry what the dialog and the class need.
    pub fn freeze(&mut self, call: &Pending, now: Instant) -> Result<Value, ToolError> {
        let args = known(&call.arguments, &["plan_id"])?;
        let id = args
            .get("plan_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("`plan_id` is required"))?;
        self.plans.retain(|p| !p.expired(now));
        let plan = self.plans.iter_mut().find(|p| p.id == id).ok_or_else(|| {
            err(
                ErrorCode::NotFound,
                "no such plan on this login, or it expired (120 seconds): preview again",
            )
        })?;
        if plan.used.is_some() {
            return Err(err(
                ErrorCode::NotFound,
                "this plan was already executed: read inventory_upload_status, never upload the same files again after an unclear answer",
            ));
        }
        if self.run.is_some() {
            return Err(err(
                ErrorCode::Busy,
                "another upload is under way: wait for it (inventory_upload_status)",
            ));
        }
        plan.used = Some(call.operation_id.clone().unwrap_or_default());
        Ok(json!({"plan_id": id, "frozen": {"total": plan.total, "thumbnail": plan.thumbnail.is_some()}}))
    }

    /// The plan an execute took (a frozen call's `plan_id`).
    fn taken(&self, call: &Pending) -> Option<&Plan> {
        let id = call.arguments["plan_id"].as_str()?;
        self.plans
            .iter()
            .find(|p| p.id == id && p.used.as_deref() == call.operation_id.as_deref().or(Some("")))
    }

    /// An execute or a mesh review planned (again just before it runs).
    pub fn plan(&self, call: &Pending, cx: &Ctx) -> Result<Planned, ToolError> {
        if call.tool == MESH_REVIEW {
            return mesh_plan(call, cx);
        }
        if call.tool == SNAPSHOT {
            return self.snapshot_plan(call, cx);
        }
        ready(cx)?;
        let plan = self.taken(call).ok_or_else(|| {
            err(
                ErrorCode::NotFound,
                "the plan is gone (the login changed or it was cancelled): preview again",
            )
        })?;
        if self.run.is_some() {
            return Err(err(
                ErrorCode::Busy,
                "another upload is under way: wait for it (inventory_upload_status)",
            ));
        }
        if !actions::destination(cx.inv, plan.destination) || cx.protected.contains(&plan.destination) {
            return Err(not_allowed("the destination folder no longer takes new items"));
        }
        if let Some(target) = plan.thumbnail {
            thumbnail_check(cx, target)?;
        }
        self.price_check(plan)?;
        let balance = self.account.balance;
        if plan.total > 0 && balance.is_none_or(|b| b < plan.total) {
            return Err(not_allowed(format!(
                "the balance (L${}) does not cover the total (L${}): nothing was sent",
                balance.map_or_else(|| "?".to_owned(), |b| b.to_string()),
                plan.total
            )));
        }
        let folder = folder_name(cx.inv, plan.destination);
        let count = plan.files.len();
        let summary = match &plan.root {
            Some(root) => aurora_i18n::tr!(
                "fs-mcp-ask-upload-folder",
                root = root.as_str(),
                count = count,
                folder = folder,
                total = plan.total
            )
            .to_owned(),
            None => aurora_i18n::tr!("fs-mcp-ask-upload-files", count = count, folder = folder, total = plan.total).to_owned(),
        };
        let reply = json!({"plan_id": plan.id, "total": plan.total, "files": count});
        Ok(Planned {
            work: Work::Upload,
            summary,
            reply,
            created: None,
        })
    }

    /// Whether a planned execute must ask whatever the levels: it costs L$.
    pub fn always_asks(call: &Pending) -> bool {
        call.tool == EXECUTE && call.arguments["frozen"]["total"].as_i64().is_some_and(|t| t > 0)
    }

    /// The Ask dialog's lines for an execute: files with their price, the
    /// folders, the balance before and after.
    pub fn dialog_lines(&self, call: &Pending) -> Vec<String> {
        let Some(plan) = (call.tool == EXECUTE).then(|| self.taken(call)).flatten() else {
            return Vec::new();
        };
        let mut lines: Vec<String> = plan
            .files
            .iter()
            .take(LINES_SHOWN)
            .map(|f| {
                let name = receipts::clean(&f.scan.name, 64);
                aurora_i18n::tr!("fs-mcp-upload-line", name = name, kind = kind_name(f.scan.kind), price = f.price).to_owned()
            })
            .collect();
        if plan.files.len() > LINES_SHOWN {
            lines.push(aurora_i18n::tr!("fs-mcp-upload-more", count = plan.files.len() - LINES_SHOWN).to_owned());
        }
        if !plan.folders.is_empty() {
            lines.push(aurora_i18n::tr!("fs-mcp-upload-folders", count = plan.folders.len()).to_owned());
        }
        if let Some(balance) = self.account.balance {
            lines.push(aurora_i18n::tr!("fs-mcp-upload-balance", before = balance, after = balance - plan.total).to_owned());
        }
        lines
    }

    /// Every file's price checked again: none higher than the plan's.
    fn price_check(&self, plan: &Plan) -> Result<(), ToolError> {
        for file in &plan.files {
            let now = self.account.prices.cost(file.scan.kind, file.scan.size);
            if now.is_none_or(|p| p > file.price) {
                return Err(err(
                    ErrorCode::Stale,
                    "the upload price went up (or is no longer known) since the preview: nothing was sent; preview again",
                ));
            }
        }
        Ok(())
    }

    /// The app runs a planned execute now: its run starts. The receipt's
    /// rows are the files.
    pub fn start(&mut self, call: Pending, ledger: &receipts::Ledger, now: Instant) {
        let operation = call.operation_id.clone().unwrap_or_default();
        let Some(index) = call.arguments["plan_id"].as_str().and_then(|id| {
            self.plans
                .iter()
                .position(|p| p.id == id && p.used.as_deref() == Some(operation.as_str()))
        }) else {
            return call.answer(Err(err(ErrorCode::NotFound, "the plan is gone: preview again")));
        };
        let Some(plan) = self.plans.remove(index) else { return };
        let rows: Vec<Row> = plan
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| Row::new(i.to_string(), &f.scan.file, &f.scan.name, None))
            .collect();
        ledger.with(|s| s.set_rows(&operation, rows));
        call.receipt(|r| r.plan_id = Some(plan.id.clone()));
        let picture = plan.thumbnail.map(|_| Picture::Waiting);
        let states = plan.files.iter().map(|_| FileState::Waiting).collect();
        self.run = Some(Run {
            call: Some(call),
            operation_id: operation,
            plan,
            folder_ids: HashMap::new(),
            folder_pending: None,
            next_folder: 0,
            states,
            asset: None,
            picture,
            spent: 0,
            stop: None,
        });
        let _ = now;
    }

    /// Once per frame: the previews read, the plans expired, the run moved
    /// on, each step decided again with the levels in force.
    pub fn tick(&mut self, cx: &Ctx, permissions: &Permissions, now: Instant) {
        let decision = permissions.decide_all([Class::Upload, Class::Read]);
        for wait in std::mem::take(&mut self.scans) {
            match wait.rx.try_recv() {
                Ok(Ok(scanned)) => {
                    let ScanWait {
                        call,
                        destination,
                        rules,
                        thumbnail,
                        ..
                    } = wait;
                    let result = self.plan_scanned((destination, rules, thumbnail), scanned, cx, decision, now);
                    call.answer(result);
                }
                Ok(Err(reason)) => wait.call.answer(Err(invalid(reason))),
                Err(TryRecvError::Empty) if now >= wait.deadline => {
                    wait.call.answer(Err(err(ErrorCode::Timeout, "the files took too long to read")));
                }
                Err(TryRecvError::Empty) => self.scans.push(wait),
                Err(TryRecvError::Disconnected) => wait.call.answer(Err(err(ErrorCode::Internal, "the preview's worker stopped"))),
            }
        }
        self.plans.retain(|p| !p.expired(now));
        self.step(cx.inv, decision, now);
    }

    /// The run's next step, if any may go now.
    fn step(&mut self, inv: &Inventory, decision: Decision, now: Instant) {
        let Some(run) = &mut self.run else { return };
        if decision == Decision::Refuse {
            run.halt(ErrorCode::PermissionDenied, PERMISSIONS_CHANGED);
        }
        // the encodings done
        for index in 0..run.states.len() {
            let FileState::Encoding(rx) = &run.states[index] else { continue };
            match rx.try_recv() {
                Ok(Ok(data)) => {
                    if run.stop.is_some() {
                        run.states[index] = if run.stop.as_ref().is_some_and(|s| s.code == ErrorCode::Cancelled) {
                            FileState::Cancelled
                        } else {
                            FileState::NotSent
                        };
                        continue;
                    }
                    let file = &run.plan.files[index];
                    let folder = folder_of(run, &file.scan.folder);
                    let request = Uuid::new_v4();
                    self.acts.push(Act::Upload {
                        request,
                        upload: upload::Request {
                            kind: file.scan.kind,
                            folder,
                            name: file.scan.name.clone(),
                            description: String::new(),
                            permissions: upload::Permissions::default(),
                            expected_cost: file.price,
                            data,
                        },
                    });
                    run.mark_sent();
                    run.states[index] = FileState::Sent(request);
                }
                Ok(Err(reason)) => {
                    run.states[index] = FileState::Failed(format!("not encoded: {reason}"));
                    run.halt(ErrorCode::Failed, "a file could not be encoded");
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    run.states[index] = FileState::Failed("the encoder stopped".into());
                    run.halt(ErrorCode::Failed, "a file could not be encoded");
                }
            }
        }
        if run.states.iter().any(FileState::busy) || run.folder_pending.is_some() {
            return self.picture_timeout(now);
        }
        // the folders, one at a time
        if run.stop.is_none() && run.next_folder < run.plan.folders.len() {
            let path = run.plan.folders[run.next_folder].clone();
            let parent = folder_of(run, &path[..path.len() - 1]);
            if !actions::destination(inv, parent) {
                run.halt(ErrorCode::NotAllowed, "the destination folder no longer takes new items");
            } else {
                match actions::create_folder(parent, &path[path.len() - 1]) {
                    Ok(change) => {
                        if let Some(Operation::Folder { id, .. }) = change.operations.first() {
                            let request = Uuid::new_v4();
                            run.folder_pending = Some((request, *id, path));
                            run.next_folder += 1;
                            run.mark_sent();
                            self.acts.push(Act::Folder { request, change });
                            return;
                        }
                        run.halt(ErrorCode::Internal, "the folder was not planned");
                    }
                    Err(reason) => run.halt(ErrorCode::NotAllowed, format!("a folder was refused: {reason}")),
                }
            }
        }
        // the next file
        if run.stop.is_none()
            && let Some(index) = run.states.iter().position(|s| matches!(s, FileState::Waiting))
        {
            let file = &run.plan.files[index];
            let folder = folder_of(run, &file.scan.folder);
            let price = self.account.prices.cost(file.scan.kind, file.scan.size);
            if !actions::destination(inv, folder) {
                run.halt(ErrorCode::NotAllowed, "the destination folder no longer takes new items");
            } else if price.is_none_or(|p| p > file.price) {
                run.halt(ErrorCode::Stale, "the upload price went up since the preview: preview again");
            } else if file.price > 0 && self.account.balance.is_none_or(|b| b < file.price) {
                run.halt(ErrorCode::NotAllowed, "the balance no longer covers the next file");
            } else {
                let source = file.scan.source.clone();
                let options = run.plan.rules.texture_options();
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                let spawned = std::thread::Builder::new().name("mcp-upload-encode".into()).spawn(move || {
                    let _ = tx.send(scan::encode(&source, options));
                });
                match spawned {
                    Ok(_) => run.states[index] = FileState::Encoding(rx),
                    Err(e) => run.halt(ErrorCode::Internal, format!("no worker to encode: {e}")),
                }
                return;
            }
        }
        // the picture, once the texture is uploaded
        if run.stop.is_none()
            && matches!(run.picture, Some(Picture::Waiting))
            && let (Some(asset), Some(target)) = (run.asset, run.plan.thumbnail)
        {
            let request = Uuid::new_v4();
            run.picture = Some(Picture::Sent(request, now));
            self.acts.push(Act::Thumbnail { request, target, asset });
            return;
        }
        if matches!(run.picture, Some(Picture::Sent(..))) {
            return self.picture_timeout(now);
        }
        if run.states.iter().all(|s| !matches!(s, FileState::Waiting)) || run.stop.is_some() {
            self.finish();
        }
    }

    fn picture_timeout(&mut self, now: Instant) {
        if let Some(run) = &mut self.run
            && let Some(Picture::Sent(_, since)) = run.picture
            && now.saturating_duration_since(since) >= THUMBNAIL_WAIT
        {
            run.picture = Some(Picture::Failed("the picture was not confirmed in time; it may still be set".into()));
        }
    }

    /// The run ended: its answer, its receipt's rows, its report.
    fn finish(&mut self) {
        let Some(mut run) = self.run.take() else { return };
        let done = run.states.iter().filter(|s| matches!(s, FileState::Done(_))).count();
        let sent = run.states.iter().any(FileState::sent);
        let unknown = run.states.iter().any(|s| matches!(s, FileState::Unknown(_)));
        let all = done == run.states.len();
        let state = if all {
            "done"
        } else if unknown {
            "unconfirmed"
        } else if sent && done > 0 {
            "stopped"
        } else if sent {
            "failed"
        } else {
            match run.stop.as_ref().map(|s| s.code) {
                Some(ErrorCode::Cancelled) => "cancelled",
                Some(ErrorCode::Stale) => "stale",
                Some(ErrorCode::Failed) => "failed",
                _ => "refused",
            }
        };
        let report = run.report(state);
        let reason = run.stop.as_ref().map(|s| s.reason.clone()).unwrap_or_default();
        let result = if all {
            let mut answer = report.clone();
            answer["done"] = json!(true);
            answer["item_ids"] = json!(run.item_ids());
            Ok(answer)
        } else if unknown {
            Err(err(
                ErrorCode::Timeout,
                format!(
                    "an upload's answer was lost or not verified: it may have landed. Never upload it again: look at the inventory ({reason})"
                ),
            ))
        } else if sent && done > 0 {
            Err(err(
                ErrorCode::Cancelled,
                format!("{done} of {} files uploaded, then stopped: {reason}", run.states.len()),
            ))
        } else if sent {
            Err(err(ErrorCode::Failed, format!("the server refused the upload: {reason}")))
        } else {
            let code = run.stop.as_ref().map_or(ErrorCode::Internal, |s| s.code);
            let code = if code == ErrorCode::NotAllowed && !run.folder_ids.is_empty() {
                ErrorCode::Failed
            } else {
                code
            };
            Err(err(code, format!("nothing was uploaded: {reason}")))
        };
        let outcomes: Vec<RowOutcome> = run
            .states
            .iter()
            .map(|s| match s {
                FileState::Done(_) => RowOutcome::Done,
                FileState::Failed(_) | FileState::Unknown(_) => RowOutcome::Unknown,
                _ => RowOutcome::NotSent,
            })
            .collect();
        let items = run.item_ids();
        if let Some(call) = run.call.take() {
            let tracked = call.tracked().cloned();
            call.answer(result);
            if let Some(tracked) = tracked {
                tracked.update(|r| {
                    for (row, outcome) in r.rows.iter_mut().zip(&outcomes) {
                        row.outcome = *outcome;
                    }
                    if r.result.is_empty() {
                        r.result = items.iter().map(Uuid::to_string).collect();
                    }
                });
            }
        }
        if self.ended.len() == ENDED_MAX {
            self.ended.pop_front();
        }
        self.ended.push_back(Ended {
            operation_id: run.operation_id,
            plan_id: run.plan.id,
            report,
        });
    }

    /// Whether `request` is a folder this run asked for.
    pub fn owns_folder(&self, request: Uuid) -> bool {
        self.run
            .as_ref()
            .is_some_and(|r| r.folder_pending.as_ref().is_some_and(|(id, ..)| *id == request))
    }

    /// The server's answer to a folder: the inventory learns it (as the
    /// uploader's `import_folder_result`); a refusal stops the run.
    pub fn folder_result(&mut self, request: Uuid, result: Result<Reply, String>, inventory: &mut Inventory) {
        self.record_folder(request, result, inventory);
        self.settle_stopped();
    }

    fn record_folder(&mut self, request: Uuid, result: Result<Reply, String>, inventory: &mut Inventory) {
        let Some(run) = &mut self.run else { return };
        let Some((_, asked, path)) = run.folder_pending.take_if(|(id, ..)| *id == request) else {
            return;
        };
        match result {
            Ok(reply) => {
                let id = reply.created.get(&asked).copied().unwrap_or(asked);
                inventory.remove(&reply.removed);
                inventory.apply(reply.contents);
                inventory.created_folders(reply.folders);
                if inventory.folders.contains_key(&id) {
                    run.folder_ids.insert(path, id);
                } else {
                    run.halt(ErrorCode::Failed, "a folder was not confirmed");
                }
            }
            Err(reason) => run.halt(
                ErrorCode::Failed,
                format!("a folder was refused: {}", receipts::clean(&reason, 120)),
            ),
        }
    }

    /// Whether `request` is an upload of this run.
    pub fn owns_upload(&self, request: Uuid) -> bool {
        self.run
            .as_ref()
            .is_some_and(|r| r.states.iter().any(|s| matches!(s, FileState::Sent(id) if *id == request)))
    }

    /// The server's answer to an upload: the new item, for the inventory.
    pub fn uploaded(&mut self, request: Uuid, result: Result<upload::Receipt, upload::Error>) -> Option<InvItem> {
        let item = self.record_upload(request, result);
        self.settle_stopped();
        item
    }

    /// A stopped run whose last send was answered ends now, even while
    /// access is off (its tick then does not run).
    fn settle_stopped(&mut self) {
        if self
            .run
            .as_ref()
            .is_some_and(|r| r.stop.is_some() && !r.states.iter().any(FileState::busy) && r.folder_pending.is_none())
        {
            self.finish();
        }
    }

    fn record_upload(&mut self, request: Uuid, result: Result<upload::Receipt, upload::Error>) -> Option<InvItem> {
        let run = self.run.as_mut()?;
        let index = run.states.iter().position(|s| matches!(s, FileState::Sent(id) if *id == request))?;
        match result {
            Ok(receipt) => {
                run.spent = run.spent.saturating_add(receipt.cost);
                run.states[index] = FileState::Done(receipt.item.id);
                if run.plan.thumbnail.is_some() {
                    run.asset = Some(receipt.item.asset_id);
                }
                Some(receipt.item)
            }
            Err(error) => {
                use upload::Error;
                let text = receipts::clean(&error.to_string(), 160);
                match error {
                    Error::PriceChanged(_) => {
                        run.states[index] = FileState::Failed(text);
                        run.halt(ErrorCode::Stale, "the server's price differs from the preview's: preview again");
                    }
                    Error::Unknown | Error::Uncertain(_) | Error::Verification => {
                        run.states[index] = FileState::Unknown(text);
                        run.halt(ErrorCode::Timeout, "an upload's outcome is not known");
                    }
                    _ => {
                        run.states[index] = FileState::Failed(text);
                        run.halt(ErrorCode::Failed, "the server refused a file");
                    }
                }
                None
            }
        }
    }

    /// Whether `request` is this run's picture (the Image window's path
    /// then runs without a window).
    pub fn image_job(&self, request: Uuid) -> bool {
        self.run
            .as_ref()
            .is_some_and(|r| matches!(r.picture, Some(Picture::Sent(id, _)) if id == request))
    }

    /// The Image window's path ended this run's picture.
    pub fn image_finished(&mut self, request: Uuid, error: Option<&str>) {
        if let Some(run) = &mut self.run
            && matches!(run.picture, Some(Picture::Sent(id, _)) if id == request)
        {
            run.picture = Some(match error {
                None => Picture::Done,
                Some(e) => Picture::Failed(receipts::clean(e, 160)),
            });
        }
    }

    /// `inventory_upload_status`, by plan or operation.
    pub fn status(&self, args: &Value, cx: &Ctx, decision: Decision) -> Result<Value, ToolError> {
        let args = known(args, &["plan_id", "operation_id"])?;
        let (key, id) = match (args.get("plan_id"), args.get("operation_id")) {
            (Some(id), None) => ("plan", id),
            (None, Some(id)) => ("operation", id),
            _ => return Err(invalid("give `plan_id` or `operation_id`")),
        };
        let id = id.as_str().ok_or_else(|| invalid("the id is a string"))?;
        let matches = |plan: &str, operation: &str| if key == "plan" { plan == id } else { operation == id };
        if let Some(run) = &self.run
            && matches(&run.plan.id, &run.operation_id)
        {
            return Ok(run.report("running"));
        }
        if let Some(ended) = self.ended.iter().rev().find(|e| matches(&e.plan_id, &e.operation_id)) {
            return Ok(ended.report.clone());
        }
        if let Some(plan) = self.plans.iter().find(|p| matches(&p.id, p.used.as_deref().unwrap_or_default())) {
            let mut answer = self.preview_answer(plan, cx, decision);
            answer["state"] = json!(if plan.used.is_some() { "asking" } else { "planned" });
            return Ok(answer);
        }
        Err(err(ErrorCode::NotFound, "no such upload on this login"))
    }

    /// Stop the run `operation_id`'s files not sent yet. None: not this
    /// run's.
    pub fn cancel(&mut self, operation_id: &str, by_user: bool) -> Option<Value> {
        let run = self.run.as_mut().filter(|r| r.operation_id == operation_id)?;
        let why = if by_user {
            "stopped by the user in Preferences"
        } else {
            "cancelled by the assistant"
        };
        run.halt(ErrorCode::Cancelled, why);
        let in_flight = run.states.iter().filter(|s| s.busy()).count() + usize::from(run.folder_pending.is_some());
        Some(json!({
            "operation_id": operation_id,
            "cancelled": true,
            "in_flight": in_flight,
            "message": "the files not sent yet will not be sent; an upload already sent runs to its answer, and accepted uploads and charges cannot be undone",
        }))
    }

    /// The login changed or access was turned off: plans and previews go,
    /// the run sends nothing more (its sent upload is still recorded).
    pub fn clear(&mut self, refusal: &ToolError) {
        for wait in std::mem::take(&mut self.scans) {
            wait.call.answer(Err(refusal.clone()));
        }
        self.plans.clear();
        if let Some(run) = &mut self.run {
            run.halt(refusal.code, refusal.message.clone());
            if !run.states.iter().any(FileState::busy) && run.folder_pending.is_none() {
                self.finish();
            }
        }
    }

    /// The previews waiting, refused (the dialog's queue is refused too).
    pub fn refuse_all(&mut self, refusal: &ToolError) {
        for wait in std::mem::take(&mut self.scans) {
            wait.call.answer(Err(refusal.clone()));
        }
    }

    /// Plans kept, for the tests.
    #[cfg(test)]
    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }
}

/// The folder a file of `run` goes to: its created folder, else the
/// destination.
fn folder_of(run: &Run, path: &[String]) -> Uuid {
    if path.is_empty() {
        return run.plan.destination;
    }
    run.folder_ids.get(path).copied().unwrap_or(run.plan.destination)
}

/// The destination: `folder_id` or `path`, else `fallback`; a folder that
/// takes new items.
fn destination(args: &Value, cx: &Ctx, fallback: Option<Uuid>) -> Result<Uuid, ToolError> {
    let folder = scope(args, cx.inv)?
        .or(fallback)
        .ok_or_else(|| invalid("give the destination as `folder_id` or `path`"))?;
    let marketplace = crate::world::marketplace::root(cx.inv).is_some_and(|root| actions::under(cx.inv, folder, root));
    if !actions::destination(cx.inv, folder) || cx.protected.contains(&folder) || marketplace {
        return Err(not_allowed(
            "this folder does not take new items (Library, Trash, Current Outfit, Marketplace or a locked folder)",
        ));
    }
    Ok(folder)
}

fn snapshot_name(args: &Value) -> Result<String, ToolError> {
    match &args["name"] {
        Value::Null => Ok(format!("Snapshot {}", chrono::Local::now().format("%Y-%m-%d %H:%M"))),
        Value::String(name) => scan::item_name(name).map_err(invalid),
        _ => Err(invalid("`name` is a string")),
    }
}

fn thumbnail_target(args: &Value, cx: &Ctx) -> Result<Option<Uuid>, ToolError> {
    let Some(target) = uuid_arg(args, "thumbnail_id")? else {
        return Ok(None);
    };
    thumbnail_check(cx, target)?;
    Ok(Some(target))
}

/// An entry whose picture the assistant may set (`inventory_set_thumbnail`'s
/// rule: not a link, the Library, the Current Outfit or Received Items, and
/// Modify for an item; never under the Marketplace listings).
fn thumbnail_check(cx: &Ctx, target: Uuid) -> Result<(), ToolError> {
    let inv = cx.inv;
    let item = inv.items.get(&target);
    if item.is_none() && !inv.folders.contains_key(&target) {
        return Err(err(ErrorCode::NotFound, "no inventory entry for `thumbnail_id`"));
    }
    if !actions::mutable(inv, target)
        || crate::world::marketplace::root(inv).is_some_and(|root| actions::under(inv, target, root))
        || item.is_some_and(|it| matches!(it.asset_type, 24 | 25) || actions::held(inv, it) & actions::MODIFY == 0)
    {
        return Err(not_allowed(
            "this entry's picture cannot be changed: links, Library entries, the Current Outfit, Received Items and items without Modify permission keep theirs",
        ));
    }
    Ok(())
}

fn folder_name(inv: &Inventory, folder: Uuid) -> String {
    let path = folder_path(inv, folder);
    if path.is_empty() {
        inv.folders.get(&folder).map(|f| f.info.name.clone()).unwrap_or_default()
    } else {
        path
    }
}

fn entry_name(inv: &Inventory, id: Uuid) -> String {
    inv.items
        .get(&id)
        .map(|i| i.name.clone())
        .or_else(|| inv.folders.get(&id).map(|f| f.info.name.clone()))
        .map(|n| receipts::clean(&n, 64))
        .unwrap_or_default()
}

/// `inventory_upload_mesh_review`'s path, checked: a local `.dae` file.
fn mesh_plan(call: &Pending, cx: &Ctx) -> Result<Planned, ToolError> {
    ready(cx)?;
    let args = known(&call.arguments, &["path"])?;
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`path` is required"))?;
    let path = scan::check_path(std::path::Path::new(path)).map_err(invalid)?;
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dae")) {
        return Err(invalid(
            "the mesh review opens a Collada (.dae) file; other files go to inventory_upload_preview",
        ));
    }
    if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
        return Err(invalid("`path` is not a regular file"));
    }
    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
    Ok(Planned {
        work: Work::MeshReview(path),
        summary: aurora_i18n::tr!("fs-mcp-ask-mesh-review", file = receipts::clean(&file, 64)).to_owned(),
        reply: json!({
            "opened": true,
            "file": file,
            "uploaded": false,
            "next": "the viewer's mesh uploader shows the file, its server fee and its Upload button: the user reviews and uploads by hand; nothing was uploaded or spent",
        }),
        created: None,
    })
}
