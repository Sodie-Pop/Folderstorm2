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

use super::changes::{self, Planned, Work};
use super::tools::{self, Ctx};
use super::{journal::Journal, landmark}; // Folderstorm: phase 3, part 2
use folderstorm_mcp_bridge::protocol::{self, ASK_TIMEOUT, BAKE_WAIT_DEFAULT, BAKE_WAIT_MAX, CHANGE_WAIT};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{Decision, ErrorCode, Permissions, ToolError, binding};
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

/// What the server said of a change.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Done, but the copies it holds are still on their way (the same
    /// request answers again once they are made).
    Copying,
    /// Done; the folders it created, temporary id → the server's.
    Done(HashMap<Uuid, Uuid>),
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
}

fn refused(message: &str) -> ToolError {
    ToolError::new(ErrorCode::PermissionDenied, message)
}

/// A class the call needs was set to Never while it waited.
pub const PERMISSIONS_CHANGED: &str =
    "the user changed the permissions in Preferences › Privacy › Local assistant while the call waited: refused, nothing was sent";
/// The viewer logged out, or in again, while the call waited.
pub const LOGIN_CHANGED: &str =
    "the viewer's login changed while the call waited: refused, nothing was sent. Call viewer_status and look again before retrying";
/// The user turned local assistant access off while the call waited.
pub const ACCESS_OFF: &str = "local assistant access was turned off: refused, nothing was sent";

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
        // the levels may have changed while the call was queued
        match decision(&self.permissions, &call.tool) {
            Decision::Refuse => return call.answer(Err(refused(PERMISSIONS_CHANGED))),
            Decision::Ask => call.ask = true,
            Decision::Run => {}
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
        if !spec.changes() {
            let result = tools::answer(&call.tool, &call.arguments, cx);
            call.answer(result);
            return;
        }
        match changes::plan(&call.tool, &call.arguments, cx) {
            Err(e) => call.answer(Err(e)),
            Ok(Planned {
                work: Work::Nothing,
                reply,
                ..
            }) => call.answer(Ok(reply)),
            Ok(planned) if call.ask => self.ask(call, planned.summary, now),
            Ok(planned) => self.ready.push(Ready {
                call,
                summary: planned.summary,
            }),
        }
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
    }

    /// Access is turned off: every call still waiting is refused, the
    /// changes already sent answer that they may still land, and the desk
    /// starts empty.
    pub fn close(&mut self) {
        let mut old = std::mem::take(self);
        old.refuse_waiting(&refused(ACCESS_OFF));
        for flight in old.sending.take().into_iter().chain(old.sent) {
            flight.call.answer(Err(ToolError::new(
                ErrorCode::Timeout,
                "the change was sent before local assistant access was turned off; it may still land: look again before retrying",
            )));
        }
    }

    /// Just before the app sends `call`'s change: decided again with the
    /// levels and the login in force. Some: send it now. None: it was
    /// refused, or it goes back to the dialog (its class asks now).
    pub fn recheck(&mut self, call: Pending, login: u64, session: Option<String>, cx: &Ctx, now: Instant) -> Option<Pending> {
        let moved = login != self.login;
        self.set_login(login, session);
        if moved {
            call.answer(Err(self.login_changed()));
            return None;
        }
        match decision(&self.permissions, &call.tool) {
            Decision::Refuse => {
                call.answer(Err(refused(PERMISSIONS_CHANGED)));
                None
            }
            Decision::Ask if !call.ask => {
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
        })
    }

    /// The user's answer to question `number` (Allow, or Refuse / Escape /
    /// a click outside).
    pub fn decide(&mut self, number: u64, allow: bool) {
        let Some(index) = self.asks.iter().position(|a| a.number == number && a.decision.is_none()) else {
            return;
        };
        // an Allow for a class set to Never meanwhile refuses at once
        if allow && decision(&self.permissions, &self.asks[index].call.tool) == Decision::Refuse {
            if let Some(ask) = self.asks.remove(index) {
                ask.call.answer(Err(refused(PERMISSIONS_CHANGED)));
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
            if decision(&self.permissions, &ask.call.tool) == Decision::Refuse {
                // the dialog closes with it
                ask.call.answer(Err(refused(PERMISSIONS_CHANGED)));
                continue;
            }
            match ask.decision {
                Some(true) => self.ready.push(Ready {
                    call: ask.call,
                    summary: ask.summary,
                }),
                Some(false) => ask.call.answer(Err(refused("the user refused this change"))),
                None if now >= ask.deadline => ask.call.answer(Err(refused(&format!(
                    "the user did not answer within {} seconds: refused",
                    ASK_TIMEOUT.as_secs()
                )))),
                None => self.asks.push_back(ask),
            }
        }
        for flight in std::mem::take(&mut self.sent) {
            if now >= flight.deadline {
                flight.call.answer(Err(ToolError::new(
                    ErrorCode::Timeout,
                    "the change was sent but the server did not confirm it in time; it may still land: look again before retrying",
                )));
            } else {
                self.sent.push(flight);
            }
        }
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
            match decision(&self.permissions, &ready.call.tool) {
                Decision::Refuse => ready.call.answer(Err(refused(PERMISSIONS_CHANGED))),
                // not asked yet, and its class asks now
                Decision::Ask if !ready.call.ask => self.ask(ready.call, ready.summary, now),
                _ => runs.push(ready.call),
            }
        }
        runs
    }

    fn ask(&mut self, mut call: Pending, summary: String, now: Instant) {
        call.ask = true;
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
        }
    }

    /// Folderstorm: once per frame, the reads that wait or follow the
    /// inventory (phase 3).
    pub fn tick_reads(&mut self, world: &crate::world::World, now: Instant) {
        self.journal.follow(&world.inventory, now);
        landmark::tick(&mut self.landmarks, world, now);
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
