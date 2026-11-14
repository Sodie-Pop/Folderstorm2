//! Folderstorm: where the local assistant's calls wait on the frame thread
//! (folderstorm/plans/mcp.md, "Phase 2"): for the user in the Ask dialog,
//! for the app to run them, for the server's confirmation, for the avatar's
//! bake. Polled once per frame with the time given, so the tests drive it
//! without a clock or a window.
//!
//! The listener thread decided Allow / Ask / Never when the call arrived;
//! the desk is the authority after that. It decides again with the levels
//! in force (given each frame) when it takes a call, on every frame while
//! the call waits, when Allow is clicked and just before the app sends it
//! ([`Desk::recheck`]): any class now on Never refuses, a call not asked yet
//! whose class now asks goes back to asking, and an Allow never answers a
//! question already asked. A login change or access turned off refuses
//! every call still waiting. A change already sent is never undone.
//!
//! Login binding (folderstorm/plans/mcp-receipts.md, section 2): a bound
//! tool pinned by its sidecar to another `viewer_session` than the login's
//! now is refused with `session_changed` when the desk takes it; once taken,
//! a call belongs to the login generation it was taken under, and a login
//! change refuses it with `session_changed` too.
//!
//! Receipts (folderstorm/plans/mcp-receipts.md, section 4): every change the
//! desk takes gets a receipt in its [`Ledger`], whatever its end; the call's
//! answer ends it ([`Pending::answer`]) and carries its `operation_id` and
//! `state`. A `client_request_id` seen before answers the first receipt and
//! runs nothing. A change the server does not confirm in time answers
//! `unconfirmed` and keeps listening for its answer ([`LateWait`]).
//!
//! Operations (section 5): 8 live per login and 16 questions waiting for
//! the dialog at most (`busy` beyond); a call asked to answer early does so
//! once the desk took it ([`Pending::answer_early`]); the assistant's
//! `assistant_operation_cancel` and Preferences' Stop cancel what has not
//! been sent ([`Desk::cancel`]); just before the send, a call whose time ran
//! out is cancelled.

use super::changes::{self, Planned, Work};
use super::objects::{self, Objects}; // Folderstorm: object tools (mcp-objects.md)
use super::plans::{self, Plans}; // Folderstorm: batch plans
use super::tools::{self, Ctx};
use super::{journal::Journal, landmark}; // Folderstorm: phase 3, part 2
use folderstorm_mcp_bridge::protocol::assistant::{CANCEL, EXECUTE_PLAN};
use folderstorm_mcp_bridge::protocol::objects as object_tool; // Folderstorm: object tools
use folderstorm_mcp_bridge::protocol::{self, ASK_TIMEOUT, BAKE_WAIT_DEFAULT, BAKE_WAIT_MAX, CHANGE_WAIT};
use folderstorm_mcp_bridge::receipts::{self, Admission, Late, Ledger, Receipt, State, Tracked};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::server::VIEWER_TIMEOUT;
use folderstorm_mcp_bridge::{Class, Decision, ErrorCode, Permissions, ToolError, binding};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// A call waiting for the user's answer in the Ask dialog.
struct Asking {
    /// Tells the dialog's answer from a later dialog's.
    number: u64,
    call: Pending,
    summary: String,
    deadline: Instant,
    decision: Option<bool>,
}

/// A change sent, waiting for the server.
struct InFlight {
    call: Pending,
    outfit: bool,
    reply: Value,
    created: Option<Uuid>,
    request: Option<Uuid>,
    outcome: Option<Outcome>,
    deadline: Instant,
    /// A new item: the server's answer is the item arriving in `parent`,
    /// of that asset type and name (phase 3).
    expect: Option<(Uuid, i32, Option<String>)>,
    /// A picture from a texture: the Image window's path ends it, or says
    /// why it failed (phase 3).
    image: bool,
}

struct BakeWait {
    call: Pending,
    deadline: Instant,
}

/// Folderstorm: a change that answered `unconfirmed`, still listening for
/// the server's answer to update its receipt (receipts).
struct LateWait {
    tracked: Tracked,
    outfit: bool,
    request: Option<Uuid>,
    expect: Option<(Uuid, i32, Option<String>)>,
    image: bool,
    deadline: Instant,
}

/// What the server said of a change.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Done, but the copies it holds are still on their way (the same
    /// request answers again once they are made).
    Copying,
    /// Done; the folders it created, temporary id → the server's.
    Done(HashMap<Uuid, Uuid>),
    /// Folderstorm: a grid without AIS accepted the moves for later
    /// (`Reply::moves_queued`): sent, not confirmed.
    Queued,
    /// The new item arrived (phase 3).
    Created(Uuid),
    Failed(String),
}

/// Where the last outfit change stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bake {
    pub outfit_change_pending: bool,
    pub cof_version: i32,
    pub appearance_cof_version: i32,
    /// The region bakes the avatar (not the demo, not a region that leaves
    /// it to the viewer).
    pub server_bakes: bool,
}

impl Bake {
    /// LLAppearanceMgr's view of a finished change: no outfit write or
    /// detach pending, and the server's appearance made for this Current
    /// Outfit version (mLastUpdateReceivedCOFVersion against the folder's).
    pub fn settled(&self) -> bool {
        !self.outfit_change_pending && (!self.server_bakes || self.cof_version < 0 || self.appearance_cof_version >= self.cof_version)
    }

    fn answer(&self) -> Value {
        json!({
            "settled": self.settled(),
            "outfit_change_pending": self.outfit_change_pending,
            "cof_version": self.cof_version,
            "appearance_cof_version": self.appearance_cof_version,
        })
    }
}

/// The Ask dialog's content for the oldest request.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub number: u64,
    pub client: Option<String>,
    pub summary: String,
    pub seconds_left: u64,
    /// Other requests waiting behind this one.
    pub waiting: usize,
    /// Folderstorm: a plan's rows under its sentence (at most 5, "and N
    /// more", the entries left out); empty for any other change.
    pub detail: Vec<String>,
}

/// A change allowed, to run on this frame.
struct Ready {
    call: Pending,
    /// For the dialog, should the levels ask first after all.
    summary: String,
}

#[derive(Default)]
pub struct Desk {
    asks: VecDeque<Asking>,
    asked: u64,
    /// Changes to run on this frame (allowed, or needing no question).
    ready: Vec<Ready>,
    sent: Vec<InFlight>,
    /// The change the app is sending now: answers arriving meanwhile are
    /// its own (the demo answers at once, inside the send).
    sending: Option<InFlight>,
    bakes: Vec<BakeWait>,
    /// `inventory_changes`' journal (phase 3).
    journal: Journal,
    /// `inventory_landmark` calls waiting for their landmark (phase 3).
    landmarks: Vec<landmark::Wait>,
    /// The user's levels in force ([`Desk::set_permissions`]); Read only
    /// until the first frame gives them.
    permissions: Permissions,
    /// The login generation the waiting calls were taken under
    /// ([`Desk::set_login`]).
    login: u64,
    /// That login's `viewer_session` (None while logged out), which a
    /// bound call must be pinned to, when pinned at all.
    session: Option<String>,
    /// Folderstorm: the receipts of this login (kept when access is turned
    /// off, cleared at a login change).
    receipts: Ledger,
    /// Folderstorm: changes that answered `unconfirmed`, listening on.
    late: Vec<LateWait>,
    /// Folderstorm: this login's batch plans.
    plans: Plans,
    /// Folderstorm: this login's object snapshots, listings and watches.
    objects: Objects,
}

fn refused(message: &str) -> ToolError {
    ToolError::new(ErrorCode::PermissionDenied, message)
}

/// Folderstorm: refuse `call`, its receipt saying why first (the user, or
/// the 60 seconds).
fn refuse_as(call: Pending, decision: receipts::Decision, message: &str) {
    call.receipt(|r| r.decide(decision));
    call.answer(Err(refused(message)));
}

/// Folderstorm: the inventory ids a change names (never asset ids: only
/// what the inventory holds), for its receipt.
fn targets(args: &Value, inv: &crate::world::inventory::Inventory) -> Vec<String> {
    let mut out = Vec::new();
    let mut add = |value: &Value| {
        if let Some(id) = value.as_str().and_then(|s| Uuid::parse_str(s.trim()).ok())
            && (inv.items.contains_key(&id) || inv.folders.contains_key(&id))
            && !out.contains(&id.to_string())
        {
            out.push(id.to_string());
        }
    };
    for key in ["id", "item_id", "outfit_id", "folder_id"] {
        add(&args[key]);
    }
    for key in ["ids", "item_ids"] {
        args[key].as_array().into_iter().flatten().for_each(&mut add);
    }
    args["items"].as_array().into_iter().flatten().for_each(|item| add(&item["id"]));
    out
}

/// A class the call needs was set to Never while it waited.
pub const PERMISSIONS_CHANGED: &str =
    "the user changed the permissions in Preferences › Privacy › Local assistant while the call waited: refused, nothing was sent";
/// The viewer logged out, or in again, while the call waited.
pub const LOGIN_CHANGED: &str =
    "the viewer's login changed while the call waited: refused, nothing was sent. Call viewer_status and look again before retrying";
/// The user turned local assistant access off while the call waited.
pub const ACCESS_OFF: &str = "local assistant access was turned off: refused, nothing was sent";
/// Folderstorm: questions waiting for the dialog at most (operations).
pub const ASKS_MAX: usize = 16;
// the dialog's queue holds every live change of a login
const _: () = assert!(ASKS_MAX >= receipts::LIVE_MAX);
/// Folderstorm: the assistant cancelled it.
pub const CANCELLED: &str = "cancelled by the assistant before it was sent: nothing was sent";
/// Folderstorm: the user stopped it in Preferences.
pub const STOPPED: &str = "stopped by the user in Preferences › Privacy › Local assistant before it was sent: nothing was sent";
/// Folderstorm: its time ran out before it was sent.
pub const OUT_OF_TIME: &str = "its time ran out before it was sent: nothing was sent";

/// Folderstorm: what [`Desk::review`] says of a call waiting outside the
/// desk.
#[derive(Debug)]
pub enum Review {
    /// It may go on.
    Run,
    /// Its classes ask now: a call not asked yet goes back to the desk
    /// ([`Desk::take`]) before it acts.
    Ask,
    /// Refused (`session_changed`, or a class on Never): the refusal the
    /// desk gives its own waiting calls.
    Refuse(ToolError),
}

/// What the levels say of `call` now, for every class it needs
/// (`ToolSpec::classes`: a thumbnail from a picture needs Edit and Camera):
/// any class on Never refuses, else any on Ask asks (once), else it runs.
/// An unknown tool is refused.
pub fn decision(permissions: &Permissions, tool: &str) -> Decision {
    let Some(spec) = protocol::tool(tool) else {
        return Decision::Refuse;
    };
    permissions.decide_all(spec.classes())
}

/// Folderstorm: what the levels say of `call`, a plan's execute by its
/// plan's class.
pub fn call_decision(permissions: &Permissions, call: &Pending) -> Decision {
    match call.plan_class {
        Some(class) => permissions.decide(class),
        None => decision(permissions, &call.tool),
    }
}

impl Desk {
    /// The login changed under `call`: refused, with the login now.
    fn login_changed(&self) -> ToolError {
        ToolError {
            viewer_session: self.session.clone(),
            ..ToolError::new(ErrorCode::SessionChanged, LOGIN_CHANGED)
        }
    }

    /// A new call: a read is answered now; a change is checked, then asks
    /// the user or waits to run; `wait_for_bake` waits.
    pub fn take(&mut self, mut call: Pending, cx: &Ctx, now: Instant) {
        // Folderstorm: seen for the first time (not back from the dialog's
        // checks), for the object tools' required id below
        let fresh = call.tracked().is_none();
        let Some(spec) = protocol::tool(&call.tool) else {
            call.answer(Err(ToolError::new(ErrorCode::UnknownTool, "unknown tool")));
            return;
        };
        // pinned by its sidecar to another login than this one
        if spec.bound()
            && let Some(pinned) = &call.session
            && self.session.as_deref() != Some(pinned.as_str())
        {
            let refusal = binding::session_changed(self.session.as_deref());
            return call.answer(Err(refusal));
        }
        // Folderstorm: a plan already executed answers its operation
        if call.tool == EXECUTE_PLAN
            && call.tracked().is_none()
            && let Some(operation) = call.arguments["plan_id"].as_str().and_then(|id| self.plans.used(id, now))
        {
            let receipt = self.receipts.with(|s| s.find(self.session.as_deref(), &operation).cloned());
            return call.answer(match receipt {
                Some(receipt) => Ok(self.duplicate(&receipt)),
                None => Err(ToolError::new(
                    ErrorCode::NotFound,
                    "this plan was executed and its receipt is gone",
                )),
            });
        }
        // Folderstorm: an object change needs its client_request_id
        // (mcp-objects.md; decision 3 of mcp-receipts.md)
        if fresh && object_tool::needs_client_request_id(&call.tool) && call.arguments.get(receipts::CLIENT_REQUEST_ID).is_none() {
            return call.answer(Err(ToolError::new(
                ErrorCode::InvalidArguments,
                "`client_request_id` is required for an object change: choose a new one for each change, and send the same one again only to ask for that change's receipt",
            )));
        }
        // Folderstorm: a change gets its receipt (once: a call that comes
        // back from the dialog's checks has it already)
        if spec.changes() && call.tracked().is_none() {
            match self.admit_receipt(call, now) {
                Some(admitted) => call = admitted,
                None => return,
            }
        }
        if call.tool == "assistant_operation_status" {
            let result = self.status(&call.arguments);
            return call.answer(result);
        }
        if call.tool == CANCEL {
            let result = match call.arguments.as_object() {
                Some(a) if a.len() == 1 && a.contains_key("operation_id") => match a["operation_id"].as_str() {
                    Some(id) => self.cancel(id, false),
                    None => Err(ToolError::new(ErrorCode::InvalidArguments, "`operation_id` is a string")),
                },
                _ => Err(ToolError::new(ErrorCode::InvalidArguments, "give `operation_id` only")),
            };
            return call.answer(result);
        }
        // Folderstorm: a plan's execute takes its plan (once)
        if call.tool == EXECUTE_PLAN && call.plan_class.is_none() {
            match self.take_plan(call, now) {
                Some(taken) => call = taken,
                None => return,
            }
        }
        // the levels may have changed while the call was queued
        match call_decision(&self.permissions, &call) {
            Decision::Refuse => return call.answer(Err(refused(PERMISSIONS_CHANGED))),
            Decision::Ask => call.ask = true,
            Decision::Run => {}
        }
        // Folderstorm: removing from an object and taking one always ask
        if object_tool::always_asks(&call.tool) {
            call.ask = true;
        }
        if call.tool == "wait_for_bake" {
            match bake_timeout(&call.arguments) {
                Ok(wait) => self.bakes.push(BakeWait {
                    call,
                    deadline: now + wait,
                }),
                Err(e) => call.answer(Err(e)),
            }
            return;
        }
        // Folderstorm: phase 3 reads that keep state or wait
        if call.tool == "inventory_changes" {
            let result = tools::ready(cx).and_then(|()| self.journal.answer(&call.arguments, cx.inv, now));
            call.answer(result);
            return;
        }
        if call.tool == "inventory_landmark" {
            self.landmarks.extend(landmark::start(call, cx, now));
            return;
        }
        // Folderstorm: batch plans
        if plans::previews(&call.tool) {
            let execution = self.permissions.decide(plans::class(&call.tool));
            let result = plans::preview(&mut self.plans, &call.tool, &call.arguments, cx, execution, now);
            return call.answer(result);
        }
        // Folderstorm: the object tools' reads (mcp-objects.md)
        if call.tool == object_tool::LIST {
            self.objects.list(call, cx, now);
            return;
        }
        if call.tool == object_tool::STATUS {
            let result = self
                .objects
                .status(&call.arguments, &self.receipts, self.session.as_deref(), self.names());
            return call.answer(result);
        }
        if !spec.changes() {
            let result = tools::answer(&call.tool, &call.arguments, cx);
            call.answer(result);
            return;
        }
        // Folderstorm: an object change is planned against its snapshot
        let planned = if objects::handles(&call.tool) {
            self.objects.plan(&call, cx, now, false)
        } else {
            changes::plan(&call.tool, &call.arguments, cx)
        };
        match planned {
            Err(e) => call.answer(Err(e)),
            Ok(Planned {
                work: Work::Nothing,
                reply,
                ..
            }) => call.answer(Ok(reply)),
            Ok(planned) => {
                call.receipt(|r| {
                    r.set_summary(&planned.summary);
                    if r.targets.is_empty() {
                        r.set_targets(targets(&call.arguments, cx.inv));
                    }
                });
                if call.ask {
                    self.ask(call, planned.summary, now);
                } else {
                    self.queue(call, planned.summary);
                }
            }
        }
    }

    /// Folderstorm: an allowed change waits for its turn.
    fn queue(&mut self, mut call: Pending, summary: String) {
        call.receipt(|r| {
            if r.decision.is_none() {
                r.decide(receipts::Decision::Allowed);
            }
            r.state = State::Queued;
        });
        call.answer_early();
        self.ready.push(Ready { call, summary });
    }

    /// Folderstorm: stop operation `operation_id` before it is sent (the
    /// assistant's `assistant_operation_cancel` on this login, or the
    /// user's Stop, `by_user`). In the dialog or waiting for its turn it is
    /// cancelled (the dialog closes); sent or under way, it cannot be
    /// recalled, and the answer says so.
    pub fn cancel(&mut self, operation_id: &str, by_user: bool) -> Result<Value, ToolError> {
        let session = self.session.clone();
        let state = self.receipts.with(|s| {
            if by_user {
                s.all().find(|r| r.operation_id == operation_id).map(|r| r.state)
            } else {
                s.find(session.as_deref(), operation_id).map(|r| r.state)
            }
        });
        let Some(state) = state else {
            return Err(ToolError::new(ErrorCode::NotFound, "no such operation on this login"));
        };
        let mine = |call: &Pending| call.operation_id.as_deref() == Some(operation_id);
        let waiting = if let Some(index) = self.asks.iter().position(|a| mine(&a.call)) {
            self.asks.remove(index).map(|a| a.call)
        } else {
            self.ready
                .iter()
                .position(|r| mine(&r.call))
                .map(|index| self.ready.remove(index).call)
        };
        if let Some(call) = waiting {
            let why = if by_user { STOPPED } else { CANCELLED };
            call.answer(Err(ToolError::new(ErrorCode::Cancelled, why)));
            return Ok(json!({"operation_id": operation_id, "cancelled": true, "state": State::Cancelled}));
        }
        let message = if state.live() {
            "already sent or under way: a change sent cannot be recalled. assistant_operation_status gives its end"
        } else {
            "it had already ended"
        };
        Ok(json!({"operation_id": operation_id, "cancelled": false, "state": state, "message": message}))
    }

    /// Folderstorm: the receipt of a change just taken, its
    /// `client_request_id` taken out of its arguments. None: the call was
    /// answered (the same request seen before, a conflict, too many live).
    fn admit_receipt(&mut self, mut call: Pending, now: Instant) -> Option<Pending> {
        let client_request_id = match receipts::take_client_request_id(&mut call.arguments) {
            Ok(id) => id,
            Err(e) => {
                call.answer(Err(e));
                return None;
            }
        };
        if call.operation_id.is_none() {
            // a call made in the process (the demo, the tests)
            call.operation_id = receipts::new_operation_id().ok();
        }
        // 8 live operations per login (a request seen before still answers)
        let session = self.session.clone();
        let busy = self.receipts.with(|s| {
            s.live(session.as_deref()) >= receipts::LIVE_MAX
                && client_request_id
                    .as_deref()
                    .is_none_or(|id| s.find_request(session.as_deref(), id).is_none())
        });
        if busy {
            call.answer(Err(ToolError::new(
                ErrorCode::Busy,
                format!("{} changes are still under way: try again once one ends", receipts::LIVE_MAX),
            )));
            return None;
        }
        let Some(operation_id) = call.operation_id.clone() else {
            return Some(call);
        };
        let admission = self.receipts.with(|store| {
            store.admit(
                receipts::Request {
                    operation_id,
                    client_request_id,
                    tool: &call.tool,
                    client: call.client.clone(),
                    viewer_session: self.session.clone(),
                    arguments: &call.arguments,
                },
                now,
            )
        });
        let answer = match admission {
            Admission::New => {
                call.track(&self.receipts);
                return Some(call);
            }
            Admission::Duplicate(receipt) => Ok(self.duplicate(&receipt)),
            Admission::Conflict => Err(ToolError::new(
                ErrorCode::Conflict,
                "this client_request_id was already used on this login with other arguments (or is too old to tell): nothing was run. Use a new id, or ask assistant_operation_status",
            )),
            Admission::Busy => Err(ToolError::new(
                ErrorCode::Busy,
                "too many changes are still under way: try again once they end",
            )),
        };
        call.answer(answer);
        None
    }

    /// Folderstorm: the answer of a request seen before: its receipt.
    fn duplicate(&self, receipt: &Receipt) -> Value {
        json!({
            "operation_id": receipt.operation_id,
            "state": receipt.state,
            "duplicate": true,
            "receipt": receipt.to_json(self.names()),
        })
    }

    /// Folderstorm: a plan's execute takes its plan: consumed now, its rows
    /// kept by the receipt, the plan's class decides, and the call carries
    /// the plan frozen so that the send plans it again. None: answered.
    fn take_plan(&mut self, mut call: Pending, now: Instant) -> Option<Pending> {
        let invalid = |m: &str| ToolError::new(ErrorCode::InvalidArguments, m);
        if call
            .arguments
            .as_object()
            .is_none_or(|a| a.keys().any(|k| k != "plan_id" && k != "wait"))
        {
            call.answer(Err(invalid("unknown argument")));
            return None;
        }
        if !call.arguments["wait"].is_null() && !call.arguments["wait"].is_boolean() {
            call.answer(Err(invalid("`wait` is true or false")));
            return None;
        }
        let Some(id) = call.arguments["plan_id"].as_str().map(str::to_owned) else {
            call.answer(Err(invalid("`plan_id` is required")));
            return None;
        };
        let operation = call.operation_id.clone().unwrap_or_default();
        match self.plans.take(&id, &operation, now) {
            plans::Found::Missing => {
                call.answer(Err(ToolError::new(
                    ErrorCode::NotFound,
                    "no such plan on this login, or it expired (5 minutes): preview again",
                )));
                None
            }
            plans::Found::Used => {
                call.answer(Err(ToolError::new(ErrorCode::NotFound, "this plan was already executed")));
                None
            }
            plans::Found::Fresh { tool, arguments, rows } => {
                let kept = self.receipts.with(|s| s.set_rows(&operation, plans::receipt_rows(&rows)));
                if !kept && call.tracked().is_some() {
                    call.answer(Err(ToolError::new(ErrorCode::Busy, "too many plans are still under way")));
                    return None;
                }
                call.receipt(|r| {
                    r.plan_id = Some(id.clone());
                    r.set_targets(rows.iter().map(|row| row.id.to_string()));
                });
                call.plan_class = Some(plans::class(&tool));
                call.arguments = plans::frozen(&id, &tool, arguments, &rows);
                Some(call)
            }
        }
    }

    /// Folderstorm: whether receipts give names to the assistant (Read is
    /// Allow; decision 1).
    fn names(&self) -> bool {
        self.permissions.check(Class::Read).is_ok()
    }

    /// Folderstorm: `assistant_operation_status`, on this login only.
    fn status(&self, args: &Value) -> Result<Value, ToolError> {
        let invalid = |m: &str| ToolError::new(ErrorCode::InvalidArguments, m);
        let Some(args) = args.as_object() else {
            return Err(invalid("unknown argument"));
        };
        if args.keys().any(|k| k != "operation_id" && k != receipts::CLIENT_REQUEST_ID) {
            return Err(invalid("unknown argument"));
        }
        let names = self.names();
        let session = self.session.as_deref();
        let not_found = || ToolError::new(ErrorCode::NotFound, "no such operation on this login");
        self.receipts.with(|store| {
            let found = match (args.get("operation_id"), args.get(receipts::CLIENT_REQUEST_ID)) {
                (Some(_), Some(_)) => return Err(invalid("give `operation_id` or `client_request_id`, not both")),
                (Some(id), None) => store.find(session, id.as_str().ok_or_else(|| invalid("`operation_id` is a string"))?),
                (None, Some(id)) => store.find_request(session, id.as_str().ok_or_else(|| invalid("`client_request_id` is a string"))?),
                (None, None) => {
                    let list: Vec<Value> = store
                        .recent(session, receipts::STATUS_RECENT)
                        .into_iter()
                        .map(|r| r.to_json(names))
                        .collect();
                    return Ok(json!({"receipts": list, "viewer_session": session}));
                }
            };
            found.map(|r| json!({"receipt": r.to_json(names)})).ok_or_else(not_found)
        })
    }

    /// Folderstorm: this login's receipts (Preferences).
    pub fn ledger(&self) -> &Ledger {
        &self.receipts
    }

    /// Folderstorm: a call answered outside the desk (the world reads, which
    /// `mcp_bridge::frame` answers at once) is checked first as [`Desk::take`]
    /// checks every call: pinned to another login, or needing a class now on
    /// Never, it is refused. Some: answer it now.
    pub fn admit(&self, call: Pending) -> Option<Pending> {
        let Some(spec) = protocol::tool(&call.tool) else {
            call.answer(Err(ToolError::new(ErrorCode::UnknownTool, "unknown tool")));
            return None;
        };
        if spec.bound()
            && let Some(pinned) = &call.session
            && self.session.as_deref() != Some(pinned.as_str())
        {
            call.answer(Err(binding::session_changed(self.session.as_deref())));
            return None;
        }
        if decision(&self.permissions, &call.tool) == Decision::Refuse {
            call.answer(Err(refused(PERMISSIONS_CHANGED)));
            return None;
        }
        Some(call)
    }

    /// Folderstorm: the login generation the desk binds its calls to now
    /// (the studio binds its camera moves, pictures and thumbnail to it).
    pub fn login(&self) -> u64 {
        self.login
    }

    /// Folderstorm: a call that waits outside the desk (the studio's camera
    /// moves, pictures and thumbnail, #21), taken under login generation
    /// `login`, decided again by the desk's own rules: under another login
    /// than the one now it is refused with `session_changed`; needing a
    /// class now on Never it is refused; else what the levels say now.
    pub fn review(&self, tool: &str, login: u64) -> Review {
        self.review_as(login, decision(&self.permissions, tool))
    }

    /// Folderstorm: [`Desk::review`] with the levels' decision given.
    fn review_as(&self, login: u64, decided: Decision) -> Review {
        if login != self.login {
            return Review::Refuse(self.login_changed());
        }
        match decided {
            Decision::Refuse => Review::Refuse(refused(PERMISSIONS_CHANGED)),
            Decision::Ask => Review::Ask,
            Decision::Run => Review::Run,
        }
    }

    /// The user's levels in force, given each frame before the calls: the
    /// calls waiting are decided again with them ([`Desk::tick`]).
    pub fn set_permissions(&mut self, permissions: Permissions) {
        self.permissions = permissions;
    }

    /// The login generation now and its `viewer_session`, given each frame
    /// before the calls: when the generation moved, every call still
    /// waiting (question, change not sent, bake wait) is refused with
    /// `session_changed`. Changes already sent wait for their answer.
    pub fn set_login(&mut self, generation: u64, session: Option<String>) {
        self.session = session;
        if generation == self.login {
            return;
        }
        self.login = generation;
        let refusal = self.login_changed();
        self.refuse_waiting(&refusal);
        // Folderstorm: the old login's receipts and plans go (Activity
        // lines stay)
        self.plans = Plans::default();
        self.late.clear();
        self.receipts.with(receipts::Store::clear);
        self.objects.clear(&refusal); // Folderstorm: object tools
    }

    fn refuse_waiting(&mut self, refusal: &ToolError) {
        for ask in std::mem::take(&mut self.asks) {
            ask.call.answer(Err(refusal.clone()));
        }
        for ready in std::mem::take(&mut self.ready) {
            ready.call.answer(Err(refusal.clone()));
        }
        for wait in std::mem::take(&mut self.bakes) {
            wait.call.answer(Err(refusal.clone()));
        }
        // Folderstorm: phase 3's landmark reads wait too
        landmark::refuse_all(&mut self.landmarks, refusal);
        self.objects.refuse_all(refusal); // Folderstorm: object listings
    }

    /// Access is turned off: every call still waiting is refused, the
    /// changes already sent answer that they may still land, and the desk
    /// starts empty.
    pub fn close(&mut self) {
        let mut old = std::mem::take(self);
        old.refuse_waiting(&refused(ACCESS_OFF));
        let now = Instant::now();
        for flight in old.sending.take().into_iter().chain(std::mem::take(&mut old.sent)) {
            old.listen_late(flight, now);
        }
        // Folderstorm: the receipts stay with the login, and a change sent
        // keeps listening for its answer
        self.receipts = old.receipts;
        self.late = old.late;
        self.login = old.login;
        // Folderstorm: the object operations' facts stay, the snapshots go
        old.objects.close(&refused(ACCESS_OFF));
        self.objects = old.objects;
    }

    /// Folderstorm: `flight` answers that it may still land, and listens on
    /// for the server's answer to update its receipt.
    fn listen_late(&mut self, flight: InFlight, now: Instant) {
        let tracked = flight.call.tracked().cloned();
        flight.call.answer(Err(ToolError::new(
            ErrorCode::Timeout,
            "the change was sent but the server did not confirm it in time; it may still land: ask assistant_operation_status or look again before retrying",
        )));
        if let Some(tracked) = tracked {
            if self.late.len() == receipts::LATE_MAX {
                self.late.remove(0);
            }
            self.late.push(LateWait {
                tracked,
                outfit: flight.outfit,
                request: flight.request,
                expect: flight.expect,
                image: flight.image,
                deadline: now + receipts::LATE_WAIT,
            });
        }
    }

    /// Just before the app sends `call`'s change: decided again with the
    /// levels and the login in force. Some: send it now. None: it was
    /// refused, or it goes back to the dialog (its class asks now).
    pub fn recheck(&mut self, call: Pending, login: u64, session: Option<String>, cx: &Ctx, now: Instant) -> Option<Pending> {
        // Folderstorm: its deadline, the listener's whole wait (operations)
        let wait = protocol::tool(&call.tool).map_or(Duration::ZERO, |spec| spec.wait);
        let age = call.receipt(|r| r.asked.elapsed().unwrap_or_default());
        if age.is_some_and(|age| age > ASK_TIMEOUT + VIEWER_TIMEOUT + wait) {
            call.answer(Err(ToolError::new(ErrorCode::Cancelled, OUT_OF_TIME)));
            return None;
        }
        let taken = self.login;
        self.set_login(login, session);
        match self.review_as(taken, call_decision(&self.permissions, &call)) {
            Review::Refuse(refusal) => {
                call.answer(Err(refusal));
                None
            }
            Review::Ask if !call.ask => {
                self.take(call, cx, now);
                None
            }
            _ => Some(call),
        }
    }

    /// What the dialog shows now: the oldest question still open.
    pub fn question(&self, now: Instant) -> Option<Question> {
        let open: Vec<&Asking> = self.asks.iter().filter(|a| a.decision.is_none()).collect();
        let first = open.first()?;
        Some(Question {
            number: first.number,
            client: first.call.client.clone(),
            summary: first.summary.clone(),
            seconds_left: first.deadline.saturating_duration_since(now).as_secs_f32().ceil() as u64,
            waiting: open.len() - 1,
            detail: plans::dialog_lines(&first.call.arguments),
        })
    }

    /// The user's answer to question `number` (Allow, or Refuse / Escape /
    /// a click outside).
    pub fn decide(&mut self, number: u64, allow: bool) {
        let Some(index) = self.asks.iter().position(|a| a.number == number && a.decision.is_none()) else {
            return;
        };
        // an Allow for a class set to Never meanwhile refuses at once
        if allow && call_decision(&self.permissions, &self.asks[index].call) == Decision::Refuse {
            if let Some(ask) = self.asks.remove(index) {
                refuse_as(ask.call, receipts::Decision::RefusedPermissionsChanged, PERMISSIONS_CHANGED);
            }
            return;
        }
        self.asks[index].decision = Some(allow);
    }

    /// Once per frame: settle the questions (answered or timed out), the
    /// changes the server never confirmed and the bake waits; returns the
    /// changes the app runs now.
    pub fn tick(&mut self, bake: &Bake, now: Instant) -> Vec<Pending> {
        for ask in std::mem::take(&mut self.asks) {
            if call_decision(&self.permissions, &ask.call) == Decision::Refuse {
                // the dialog closes with it
                ask.call.answer(Err(refused(PERMISSIONS_CHANGED)));
                continue;
            }
            match ask.decision {
                Some(true) => {
                    ask.call.receipt(|r| r.decide(receipts::Decision::Approved));
                    self.queue(ask.call, ask.summary);
                }
                Some(false) => refuse_as(ask.call, receipts::Decision::RefusedByUser, "the user refused this change"),
                None if now >= ask.deadline => refuse_as(
                    ask.call,
                    receipts::Decision::AskTimedOut,
                    &format!("the user did not answer within {} seconds: refused", ASK_TIMEOUT.as_secs()),
                ),
                None => self.asks.push_back(ask),
            }
        }
        for flight in std::mem::take(&mut self.sent) {
            if now >= flight.deadline {
                self.listen_late(flight, now);
            } else {
                self.sent.push(flight);
            }
        }
        self.late.retain(|late| now < late.deadline);
        for wait in std::mem::take(&mut self.bakes) {
            if decision(&self.permissions, &wait.call.tool) == Decision::Refuse {
                wait.call.answer(Err(refused(PERMISSIONS_CHANGED)));
            } else if bake.settled() || now >= wait.deadline {
                wait.call.answer(Ok(bake.answer()));
            } else {
                self.bakes.push(wait);
            }
        }
        let mut runs = Vec::new();
        for ready in std::mem::take(&mut self.ready) {
            match call_decision(&self.permissions, &ready.call) {
                Decision::Refuse => ready.call.answer(Err(refused(PERMISSIONS_CHANGED))),
                // not asked yet, and its class asks now
                Decision::Ask if !ready.call.ask => self.ask(ready.call, ready.summary, now),
                _ => runs.push(ready.call),
            }
        }
        runs
    }

    fn ask(&mut self, mut call: Pending, summary: String, now: Instant) {
        // Folderstorm: 16 questions waiting at most (operations)
        if self.asks.len() >= ASKS_MAX {
            call.answer(Err(ToolError::new(
                ErrorCode::Busy,
                "too many requests are waiting for the user's answer: try again later",
            )));
            return;
        }
        call.ask = true;
        call.receipt(|r| r.state = State::Asking);
        call.answer_early();
        self.asked += 1;
        self.asks.push_back(Asking {
            number: self.asked,
            call,
            summary,
            deadline: now + ASK_TIMEOUT,
            decision: None,
        });
    }

    /// The app starts sending `call`'s change.
    pub fn begin(&mut self, call: Pending, planned: &Planned, outfit: bool, now: Instant) {
        if let Some(earlier) = self.sending.take() {
            self.sent_one(earlier, None, None);
        }
        let expect = match &planned.work {
            Work::Create(c) => Some((c.parent, c.asset_type(), c.name.clone())),
            _ => None,
        };
        call.receipt(Receipt::mark_sent);
        self.sending = Some(InFlight {
            call,
            outfit,
            reply: planned.reply.clone(),
            created: planned.created,
            request: None,
            outcome: None,
            deadline: now + CHANGE_WAIT,
            expect,
            image: matches!(planned.work, Work::Image { .. }),
        });
    }

    /// The app sent it, as `request` (None: it did not start, `failure`
    /// saying why).
    pub fn sent(&mut self, request: Option<Uuid>, failure: Option<String>) {
        if let Some(flight) = self.sending.take() {
            self.sent_one(flight, request, failure);
        }
    }

    fn sent_one(&mut self, mut flight: InFlight, request: Option<Uuid>, failure: Option<String>) {
        if let Some(outcome) = flight.outcome.take() {
            finish(flight, outcome);
            return;
        }
        flight.request = flight.request.or(request);
        if flight.request.is_some() || (flight.expect.is_some() && failure.is_none()) {
            self.sent.push(flight);
            return;
        }
        let message = failure
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| "the viewer did not apply the change (an RLV restriction may block it)".to_owned());
        flight.call.answer(Err(ToolError::new(ErrorCode::NotAllowed, message)));
    }

    /// The server's answer to an inventory (`outfit` false) or outfit
    /// change.
    pub fn observe(&mut self, (request, outfit, outcome): (Uuid, bool, Outcome)) {
        if let Some(flight) = self
            .sending
            .as_mut()
            .filter(|f| f.outfit == outfit && f.expect.is_none() && f.request.is_none_or(|r| r == request))
        {
            flight.request = Some(request);
            if outcome != Outcome::Copying {
                flight.outcome = Some(outcome);
            }
            return;
        }
        if outcome == Outcome::Copying {
            return;
        }
        if let Some(index) = self.sent.iter().position(|f| f.outfit == outfit && f.request == Some(request)) {
            finish(self.sent.swap_remove(index), outcome);
        } else if let Some(index) = self
            .late
            .iter()
            .position(|l| l.outfit == outfit && l.expect.is_none() && l.request == Some(request))
        {
            // Folderstorm: the answer of a change that answered `unconfirmed`
            let late = self.late.swap_remove(index);
            late.tracked.settle(match outcome {
                Outcome::Failed(reason) => Late::Refused(reason),
                Outcome::Queued => Late::Queued,
                _ => Late::Confirmed,
            });
        }
    }

    /// Folderstorm: a new item arrived; the creation waiting for it ends.
    /// True when it was the local assistant's.
    pub fn created(&mut self, item: &aurora_net::inventory::InvItem) -> bool {
        let matches = |f: &InFlight| {
            f.expect.as_ref().is_some_and(|(parent, asset_type, name)| {
                *parent == item.parent && *asset_type == item.asset_type && name.as_ref().is_none_or(|n| *n == item.name)
            })
        };
        if let Some(flight) = self.sending.as_mut().filter(|f| matches(f) && f.outcome.is_none()) {
            flight.outcome = Some(Outcome::Created(item.id));
            return true;
        }
        if let Some(index) = self.sent.iter().position(matches) {
            finish(self.sent.swap_remove(index), Outcome::Created(item.id));
            return true;
        }
        let late = |l: &LateWait| {
            l.expect.as_ref().is_some_and(|(parent, asset_type, name)| {
                *parent == item.parent && *asset_type == item.asset_type && name.as_ref().is_none_or(|n| *n == item.name)
            })
        };
        if let Some(index) = self.late.iter().position(late) {
            self.late.swap_remove(index).tracked.settle(Late::Confirmed);
            return true;
        }
        false
    }

    /// Folderstorm: whether `request` is a picture this desk waits for (the
    /// Image window's path then runs without a window).
    pub fn image_job(&self, request: Uuid) -> bool {
        self.sent.iter().any(|f| f.image && f.request == Some(request))
    }

    /// Folderstorm: the Image window's path ended `request` with an error
    /// before the server was asked (or the server refused it).
    pub fn image_failed(&mut self, request: Uuid, error: &str) {
        if let Some(index) = self.sent.iter().position(|f| f.image && f.request == Some(request)) {
            let flight = self.sent.swap_remove(index);
            flight
                .call
                .answer(Err(ToolError::new(ErrorCode::Failed, format!("the picture was not set: {error}"))));
        } else if let Some(index) = self.late.iter().position(|l| l.image && l.request == Some(request)) {
            self.late
                .swap_remove(index)
                .tracked
                .settle(Late::Refused(format!("the picture was not set: {error}")));
        }
    }

    /// Folderstorm: changes listening for a late answer, for the tests.
    #[cfg(test)]
    pub fn late_count(&self) -> usize {
        self.late.len()
    }

    /// Folderstorm: once per frame, the reads that wait or follow the
    /// inventory (phase 3).
    pub fn tick_reads(&mut self, world: &crate::world::World, now: Instant) {
        self.journal.follow(&world.inventory, now);
        landmark::tick(&mut self.landmarks, world, now);
        self.objects.tick(world, now); // Folderstorm: object tools
    }

    /// Folderstorm: the object tools' state (mcp-objects.md).
    pub fn objects(&mut self) -> &mut Objects {
        &mut self.objects
    }

    /// Folderstorm: the landmark assets the waiting calls need.
    pub fn wanted_landmarks(&self) -> Vec<Uuid> {
        landmark::wanted(&self.landmarks)
    }

    /// Calls in each state, for the tests: asking, ready, sent, bake waits.
    #[cfg(test)]
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        (self.asks.len(), self.ready.len(), self.sent.len(), self.bakes.len())
    }
}

fn finish(flight: InFlight, outcome: Outcome) {
    match outcome {
        Outcome::Done(created) => {
            let mut reply = flight.reply;
            if let Some(temporary) = flight.created {
                reply["folder_id"] = json!(created.get(&temporary).copied().unwrap_or(temporary));
            }
            reply["done"] = json!(true);
            flight.call.answer(Ok(reply));
        }
        Outcome::Created(item) => {
            let mut reply = flight.reply;
            reply["item_id"] = json!(item);
            reply["done"] = json!(true);
            flight.call.answer(Ok(reply));
        }
        Outcome::Failed(reason) => flight.call.answer(Err(ToolError::new(
            ErrorCode::Failed,
            format!("the server refused the change: {reason}"),
        ))),
        // Folderstorm: accepted for later, never "done" (receipts)
        Outcome::Queued => {
            let mut reply = flight.reply;
            // a plan's rows too
            for row in reply["rows"].as_array_mut().into_iter().flatten() {
                if row["outcome"] == "done" {
                    row["outcome"] = json!("queued");
                }
            }
            reply["done"] = json!(false);
            reply["queued"] = json!(true);
            reply["next"] = json!("the grid accepted the moves for later: look again in a moment before relying on them");
            flight.call.answer(Ok(reply));
        }
        Outcome::Copying => flight
            .call
            .answer(Err(ToolError::new(ErrorCode::Internal, "the change did not end"))),
    }
}

fn bake_timeout(args: &Value) -> Result<Duration, ToolError> {
    let invalid = || {
        ToolError::new(
            ErrorCode::InvalidArguments,
            format!("`timeout_seconds` must be 1 to {BAKE_WAIT_MAX}"),
        )
    };
    if args.as_object().is_none_or(|a| a.keys().any(|k| k != "timeout_seconds")) {
        return Err(ToolError::new(ErrorCode::InvalidArguments, "unknown argument"));
    }
    let seconds = match &args["timeout_seconds"] {
        Value::Null => BAKE_WAIT_DEFAULT,
        v => v.as_u64().filter(|n| (1..=BAKE_WAIT_MAX).contains(n)).ok_or_else(invalid)?,
    };
    Ok(Duration::from_secs(seconds))
}

/// The server's answer in an event, for [`Desk::observe`].
pub fn outcome(event: &aurora_net::NetEvent) -> Option<(Uuid, bool, Outcome)> {
    use aurora_net::NetEvent;
    match event {
        NetEvent::InventoryEdited { request, result } => Some((
            *request,
            false,
            match result {
                Ok(reply) if !reply.copies.is_empty() => Outcome::Copying,
                Ok(reply) if reply.moves_queued => Outcome::Queued, // Folderstorm: receipts
                Ok(reply) => Outcome::Done(reply.created.clone()),
                Err(reason) => Outcome::Failed(reason.clone()),
            },
        )),
        NetEvent::OutfitUpdated { request, result } => Some((
            *request,
            true,
            match result {
                Ok(_) => Outcome::Done(HashMap::new()),
                Err(reason) => Outcome::Failed(reason.clone()),
            },
        )),
        _ => None,
    }
}
