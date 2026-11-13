//! Receipts (folderstorm/plans/mcp-receipts.md, section 4): a bounded,
//! in-memory record of each change an assistant asked for, so that a lost
//! or late answer can be checked instead of retried blindly.
//!
//! - **Ids**: an `operation_id` (`op-` and 32 hex digits from the OS random
//!   source) made by the listener when a change arrives, so that even its
//!   own timeout can carry it; a `client_request_id` the assistant may give
//!   (1 to 64 printable ASCII characters): the same id with the same
//!   arguments on the same login answers the first receipt and runs
//!   nothing, with other arguments (or once retired) it answers `conflict`.
//! - **What a receipt holds** never goes beyond what its Ask dialog showed
//!   and its tool answered: the dialog's sentence, the inventory ids it
//!   touches and those its answer gives, plan rows (names before and
//!   after), the decision, what the server said. Never the arguments as
//!   given (only a hash, to tell a reused `client_request_id`), asset ids,
//!   pictures, text bodies, paths, tokens or session ids. Names go to an
//!   assistant only when Read is Allow ([`Receipt::to_json`]).
//! - **Bounds**: 100 receipts per login, 2,000 plan rows in all; a finished
//!   receipt leaves after 2 hours, or when 100 are kept (oldest finished
//!   first); live receipts are never evicted, and with 100 live a new
//!   change answers `busy`. The last 1,024 retired `client_request_id`s are
//!   kept as hashes, so an evicted id is never taken for a new request.
//!
//! Kept by the viewer's frame thread only (a [`Ledger`] each call it takes
//! carries, [`Tracked`]), never written to disk and never logged; cleared at
//! a login change. Pure: no thread, no viewer, so the tests drive it with
//! their own clock.

use crate::protocol::{ErrorCode, ToolError};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The argument every change takes, and `assistant_operation_status` too.
pub const CLIENT_REQUEST_ID: &str = "client_request_id";
pub const CLIENT_REQUEST_ID_MAX: usize = 64;
/// Receipts kept per login.
pub const RECEIPTS_MAX: usize = 100;
/// Plan rows kept, all receipts together.
pub const ROWS_MAX: usize = 2_000;
/// The longest summary, and the longest name in a row, in characters.
pub const SUMMARY_MAX: usize = 300;
pub const NAME_MAX: usize = 128;
/// Ids a receipt lists as its targets, and as its result.
pub const IDS_MAX: usize = 50;
/// How long a finished receipt is kept.
pub const RETENTION: Duration = Duration::from_secs(2 * 60 * 60);
/// Retired `client_request_id`s remembered (as hashes).
pub const RETIRED_MAX: usize = 1_024;
/// How long a change sent but not confirmed keeps listening for its answer,
/// and how many do at most.
pub const LATE_WAIT: Duration = Duration::from_secs(10 * 60);
pub const LATE_MAX: usize = 16;
/// The receipts `assistant_operation_status` gives without arguments.
pub const STATUS_RECENT: usize = 20;

/// An operation's lifecycle (section 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Taken by the frame thread, before its dialog or its turn.
    Waiting,
    /// In the Ask dialog.
    Asking,
    /// Allowed, waiting for its turn.
    Queued,
    /// Something was sent; waiting for the server, the camera, the photo.
    Running,
    /// Confirmed (or nothing needed sending).
    Done,
    /// The server refused it.
    Failed,
    /// By the user, the permissions or the viewer's rules.
    Refused,
    /// Stopped before anything was sent.
    Cancelled,
    /// Stopped after some steps were sent.
    Stopped,
    /// A guard found that the world changed before sending.
    Stale,
    /// Sent, no answer in its time: it may still land.
    Unconfirmed,
}

impl State {
    /// Still going: never evicted, counted against the live bound.
    pub fn live(self) -> bool {
        matches!(self, State::Waiting | State::Asking | State::Queued | State::Running)
    }

    /// The wire name (`waiting`, `done`…).
    pub fn name(self) -> &'static str {
        match self {
            State::Waiting => "waiting",
            State::Asking => "asking",
            State::Queued => "queued",
            State::Running => "running",
            State::Done => "done",
            State::Failed => "failed",
            State::Refused => "refused",
            State::Cancelled => "cancelled",
            State::Stopped => "stopped",
            State::Stale => "stale",
            State::Unconfirmed => "unconfirmed",
        }
    }
}

/// What decided the operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// The levels allowed it without a question.
    Allowed,
    /// The user clicked Allow.
    Approved,
    RefusedByUser,
    AskTimedOut,
    /// A class it needs went to Never, or access was turned off.
    RefusedPermissionsChanged,
    /// The viewer's rules (`not_allowed`), or the call itself was wrong.
    RefusedByViewer,
    /// The login changed while it waited.
    RefusedSessionChanged,
    Stale,
}

/// What the server said of what was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Server {
    Confirmed,
    /// A grid without AIS accepted the moves for later.
    Queued,
    Refused,
    NoAnswer,
}

/// A plan row's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowOutcome {
    /// Not sent yet.
    Pending,
    /// Left out by the plan (its reason in `skip`).
    Skipped,
    Done,
    /// Accepted by the server for later.
    Queued,
    /// The change was refused or not confirmed: it may have landed in part.
    Unknown,
    /// Nothing was sent.
    NotSent,
}

/// One row of a plan: an entry, its name before and after, or why it is
/// left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub before: String,
    pub after: String,
    pub skip: Option<String>,
    pub outcome: RowOutcome,
}

impl Row {
    /// A row as a receipt keeps it: names cleaned and cut.
    pub fn new(id: impl Into<String>, before: &str, after: &str, skip: Option<&str>) -> Row {
        Row {
            id: id.into(),
            before: clean(before, NAME_MAX),
            after: clean(after, NAME_MAX),
            skip: skip.map(|s| clean(s, NAME_MAX)),
            outcome: if skip.is_some() { RowOutcome::Skipped } else { RowOutcome::Pending },
        }
    }

    fn to_json(&self, names: bool) -> Value {
        let mut row = json!({"id": self.id, "outcome": self.outcome});
        if names {
            row["before"] = json!(self.before);
            match &self.skip {
                Some(reason) => row["skip"] = json!(reason),
                None => row["after"] = json!(self.after),
            }
        }
        row
    }
}

/// One change's record.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    pub operation_id: String,
    pub client_request_id: Option<String>,
    pub tool: String,
    /// The name the program gives itself.
    pub client: Option<String>,
    pub viewer_session: Option<String>,
    pub asked: SystemTime,
    pub decided: Option<SystemTime>,
    pub sent: Option<SystemTime>,
    pub finished: Option<SystemTime>,
    pub decision: Option<Decision>,
    /// The Ask dialog's sentence, in the user's language.
    pub summary: String,
    /// The inventory ids it touches, never asset ids.
    pub targets: Vec<String>,
    /// Plans only.
    pub rows: Vec<Row>,
    pub server: Option<Server>,
    /// The server's reason for a refusal.
    pub server_reason: Option<String>,
    /// The error code it ended with, if any.
    pub error: Option<ErrorCode>,
    /// The ids the tool's answer gave.
    pub result: Vec<String>,
    pub state: State,
    pub plan_id: Option<String>,
    hash: u64,
    finished_at: Option<Instant>,
}

impl Receipt {
    /// Something was sent (the time it was).
    pub fn mark_sent(&mut self) {
        self.sent.get_or_insert_with(SystemTime::now);
        self.state = State::Running;
    }

    /// The decision, and when it was made.
    pub fn decide(&mut self, decision: Decision) {
        self.decision = Some(decision);
        self.decided = Some(SystemTime::now());
    }

    /// The dialog's sentence, cleaned and cut.
    pub fn set_summary(&mut self, summary: &str) {
        self.summary = clean(summary, SUMMARY_MAX);
    }

    /// The inventory ids it touches (50 at most).
    pub fn set_targets(&mut self, targets: impl IntoIterator<Item = String>) {
        self.targets = targets.into_iter().take(IDS_MAX).collect();
    }

    /// The receipt as `assistant_operation_status` gives it; `names` (Read
    /// is Allow): with the summary, the rows' names and the server's reason.
    pub fn to_json(&self, names: bool) -> Value {
        let mut times = Map::new();
        for (key, at) in [
            ("asked", Some(self.asked)),
            ("decided", self.decided),
            ("sent", self.sent),
            ("finished", self.finished),
        ] {
            if let Some(at) = at {
                times.insert(key.to_owned(), json!(unix_ms(at)));
            }
        }
        let mut out = json!({
            "operation_id": self.operation_id,
            "tool": self.tool,
            "state": self.state,
            "times_ms": times,
            "targets": self.targets,
            "result": self.result,
        });
        let mut put = |key: &str, value: Value| {
            out[key] = value;
        };
        if let Some(id) = &self.client_request_id {
            put(CLIENT_REQUEST_ID, json!(id));
        }
        if let Some(client) = &self.client {
            put("client", json!(client));
        }
        if let Some(session) = &self.viewer_session {
            put("viewer_session", json!(session));
        }
        if let Some(decision) = self.decision {
            put("decision", json!(decision));
        }
        if let Some(server) = self.server {
            put("server", json!(server));
        }
        if let Some(error) = self.error {
            put("error", json!(error));
        }
        if let Some(plan) = &self.plan_id {
            put("plan_id", json!(plan));
        }
        if names {
            if !self.summary.is_empty() {
                put("summary", json!(self.summary));
            }
            if let Some(reason) = &self.server_reason {
                put("server_reason", json!(reason));
            }
        }
        if !self.rows.is_empty() {
            put("rows", Value::Array(self.rows.iter().map(|r| r.to_json(names)).collect()));
            put("row_counts", row_counts(&self.rows));
        }
        out
    }
}

/// The rows' counts by outcome.
pub fn row_counts(rows: &[Row]) -> Value {
    let count = |outcome: RowOutcome| rows.iter().filter(|r| r.outcome == outcome).count();
    let mut counts = json!({"total": rows.len()});
    for outcome in [
        RowOutcome::Pending,
        RowOutcome::Skipped,
        RowOutcome::Done,
        RowOutcome::Queued,
        RowOutcome::Unknown,
        RowOutcome::NotSent,
    ] {
        let n = count(outcome);
        if n > 0
            && let Ok(Value::String(key)) = serde_json::to_value(outcome)
        {
            counts[key] = json!(n);
        }
    }
    counts
}

/// A late answer of the server, for a change that answered `unconfirmed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Late {
    Confirmed,
    Queued,
    Refused(String),
}

/// A new change taken by the frame thread.
pub struct Request<'a> {
    pub operation_id: String,
    pub client_request_id: Option<String>,
    pub tool: &'a str,
    pub client: Option<String>,
    pub viewer_session: Option<String>,
    /// Its arguments, `client_request_id` taken out: only their hash is kept.
    pub arguments: &'a Value,
}

/// What [`Store::admit`] says of a new change.
#[derive(Debug, PartialEq)]
pub enum Admission {
    /// A receipt was made: run it.
    New,
    /// The same `client_request_id` and arguments: answer this receipt and
    /// run nothing.
    Duplicate(Box<Receipt>),
    /// The id was used with other arguments, or is retired.
    Conflict,
    /// 100 receipts are live.
    Busy,
}

/// The receipts of one login.
#[derive(Debug, Default)]
pub struct Store {
    /// Oldest first.
    receipts: VecDeque<Receipt>,
    retired: VecDeque<u64>,
}

impl Store {
    /// Admit a new change: a receipt `waiting`, unless its
    /// `client_request_id` was seen.
    pub fn admit(&mut self, request: Request, now: Instant) -> Admission {
        self.prune(now);
        let hash = arguments_hash(request.tool, request.arguments);
        let session = request.viewer_session.as_deref();
        if let Some(id) = request.client_request_id.as_deref() {
            if let Some(seen) = self.find_request(session, id) {
                return if seen.hash == hash {
                    Admission::Duplicate(Box::new(seen.clone()))
                } else {
                    Admission::Conflict
                };
            }
            if self.retired.contains(&retired_key(session, id)) {
                return Admission::Conflict;
            }
        }
        if self.receipts.iter().filter(|r| r.state.live()).count() >= RECEIPTS_MAX {
            return Admission::Busy;
        }
        while self.receipts.len() >= RECEIPTS_MAX && self.evict_one() {}
        self.receipts.push_back(Receipt {
            operation_id: request.operation_id,
            client_request_id: request.client_request_id,
            tool: clean(request.tool, NAME_MAX),
            client: request.client.map(|c| clean(&c, NAME_MAX)),
            viewer_session: request.viewer_session,
            asked: SystemTime::now(),
            decided: None,
            sent: None,
            finished: None,
            decision: None,
            summary: String::new(),
            targets: Vec::new(),
            rows: Vec::new(),
            server: None,
            server_reason: None,
            error: None,
            result: Vec::new(),
            state: State::Waiting,
            plan_id: None,
            hash,
            finished_at: None,
        });
        Admission::New
    }

    /// The receipt of `operation_id` on login `session`; another login's
    /// is not found.
    pub fn find(&self, session: Option<&str>, operation_id: &str) -> Option<&Receipt> {
        self.receipts
            .iter()
            .find(|r| r.operation_id == operation_id && r.viewer_session.as_deref() == session)
    }

    /// The receipt of the assistant's own `client_request_id` on `session`.
    pub fn find_request(&self, session: Option<&str>, client_request_id: &str) -> Option<&Receipt> {
        self.receipts
            .iter()
            .find(|r| r.client_request_id.as_deref() == Some(client_request_id) && r.viewer_session.as_deref() == session)
    }

    /// The newest `count` receipts of `session`, newest first.
    pub fn recent(&self, session: Option<&str>, count: usize) -> Vec<&Receipt> {
        self.receipts
            .iter()
            .rev()
            .filter(|r| r.viewer_session.as_deref() == session)
            .take(count)
            .collect()
    }

    /// Every receipt, oldest first (Preferences).
    pub fn all(&self) -> impl Iterator<Item = &Receipt> {
        self.receipts.iter()
    }

    /// Change the receipt of `operation_id` (any login: the frame thread
    /// holds the call itself).
    pub fn update<R>(&mut self, operation_id: &str, f: impl FnOnce(&mut Receipt) -> R) -> Option<R> {
        self.receipts.iter_mut().find(|r| r.operation_id == operation_id).map(f)
    }

    /// A plan's rows. False: no room for them among the 2,000 (every other
    /// receipt with rows is live), and none were kept.
    pub fn set_rows(&mut self, operation_id: &str, rows: Vec<Row>) -> bool {
        let others = |store: &Store| -> usize {
            store
                .receipts
                .iter()
                .filter(|r| r.operation_id != operation_id)
                .map(|r| r.rows.len())
                .sum()
        };
        while others(self) + rows.len() > ROWS_MAX {
            let oldest = self
                .receipts
                .iter()
                .position(|r| !r.state.live() && !r.rows.is_empty() && r.operation_id != operation_id);
            match oldest.and_then(|i| self.receipts.remove(i)) {
                Some(gone) => self.retire(&gone),
                None => return false,
            }
        }
        self.update(operation_id, |r| r.rows = rows).is_some()
    }

    /// The call of `operation_id` answered `result`: its end, from the
    /// answer and what the receipt knows. A receipt already finished keeps
    /// its end. Returns its state.
    pub fn finish(&mut self, operation_id: &str, result: &Result<Value, ToolError>, now: Instant) -> Option<State> {
        self.update(operation_id, |r| {
            if r.state.live() {
                end(r, result);
                r.finished = Some(SystemTime::now());
                r.finished_at = Some(now);
            }
            r.state
        })
    }

    /// A server's answer arriving after the call answered `unconfirmed`.
    pub fn settle(&mut self, operation_id: &str, late: Late, now: Instant) -> Option<State> {
        self.update(operation_id, |r| {
            if r.state == State::Unconfirmed && r.server == Some(Server::NoAnswer) {
                let (state, server, rows) = match &late {
                    Late::Confirmed => (State::Done, Server::Confirmed, RowOutcome::Done),
                    Late::Queued => (State::Unconfirmed, Server::Queued, RowOutcome::Queued),
                    Late::Refused(reason) => {
                        r.server_reason = Some(clean(reason, SUMMARY_MAX));
                        r.error = Some(ErrorCode::Failed);
                        (State::Failed, Server::Refused, RowOutcome::Unknown)
                    }
                };
                r.state = state;
                r.server = Some(server);
                if state == State::Done {
                    r.error = None;
                }
                set_row_outcomes(r, rows);
                r.finished = Some(SystemTime::now());
                r.finished_at = Some(now);
            }
            r.state
        })
    }

    /// Finished receipts older than [`RETENTION`] leave.
    pub fn prune(&mut self, now: Instant) {
        let (keep, gone): (Vec<Receipt>, Vec<Receipt>) = std::mem::take(&mut self.receipts)
            .into_iter()
            .partition(|r| r.finished_at.is_none_or(|at| now.saturating_duration_since(at) < RETENTION));
        self.receipts = keep.into();
        for receipt in &gone {
            self.retire(receipt);
        }
    }

    /// Another login: every receipt and retired id goes.
    pub fn clear(&mut self) {
        *self = Store::default();
    }

    /// The oldest finished receipt leaves. False: every receipt is live.
    fn evict_one(&mut self) -> bool {
        match self.receipts.iter().position(|r| !r.state.live()) {
            Some(index) => {
                if let Some(gone) = self.receipts.remove(index) {
                    self.retire(&gone);
                }
                true
            }
            None => false,
        }
    }

    fn retire(&mut self, receipt: &Receipt) {
        if let Some(id) = &receipt.client_request_id {
            if self.retired.len() == RETIRED_MAX {
                self.retired.pop_front();
            }
            self.retired.push_back(retired_key(receipt.viewer_session.as_deref(), id));
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.receipts.len()
    }
}

/// How a receipt ends, from its call's answer.
fn end(r: &mut Receipt, result: &Result<Value, ToolError>) {
    let sent = r.sent.is_some();
    match result {
        Ok(value) => {
            r.result = result_ids(value);
            if r.decision.is_none() {
                r.decide(Decision::Allowed);
            }
            if value.get("queued").and_then(Value::as_bool) == Some(true) {
                r.state = State::Unconfirmed;
                r.server = Some(Server::Queued);
                set_row_outcomes(r, RowOutcome::Queued);
            } else {
                r.state = State::Done;
                if sent {
                    r.server = Some(Server::Confirmed);
                }
                set_row_outcomes(r, RowOutcome::Done);
            }
        }
        Err(e) => {
            r.error = Some(e.code);
            let (state, decision, rows) = match e.code {
                ErrorCode::Timeout | ErrorCode::Unavailable if sent => {
                    r.server = Some(Server::NoAnswer);
                    (State::Unconfirmed, None, RowOutcome::Unknown)
                }
                ErrorCode::Failed => {
                    r.server = Some(Server::Refused);
                    r.server_reason = Some(clean(&e.message, SUMMARY_MAX));
                    (State::Failed, None, RowOutcome::Unknown)
                }
                ErrorCode::PermissionDenied => {
                    let kept = matches!(r.decision, Some(Decision::RefusedByUser | Decision::AskTimedOut));
                    (
                        State::Refused,
                        (!kept).then_some(Decision::RefusedPermissionsChanged),
                        RowOutcome::NotSent,
                    )
                }
                ErrorCode::SessionChanged => (State::Refused, Some(Decision::RefusedSessionChanged), RowOutcome::NotSent),
                ErrorCode::Stale => (State::Stale, Some(Decision::Stale), RowOutcome::NotSent),
                _ if sent => (State::Unconfirmed, None, RowOutcome::Unknown),
                _ => (State::Refused, Some(Decision::RefusedByViewer), RowOutcome::NotSent),
            };
            r.state = state;
            if let Some(decision) = decision {
                r.decide(decision);
            }
            set_row_outcomes(r, rows);
        }
    }
}

fn set_row_outcomes(r: &mut Receipt, outcome: RowOutcome) {
    for row in &mut r.rows {
        if row.skip.is_none() {
            row.outcome = outcome;
        }
    }
}

/// The answer keys whose UUIDs a receipt keeps as its result: what the
/// change made or touched, never an asset id.
const RESULT_KEYS: &[&str] = &[
    "id",
    "ids",
    "folder_id",
    "item_id",
    "item_ids",
    "parent_id",
    "outfit_id",
    "moved",
    "copied",
    "linked",
    "trashed",
    "restored",
    "renamed",
];

/// The ids an answer gives under [`RESULT_KEYS`], 50 at most.
pub fn result_ids(value: &Value) -> Vec<String> {
    fn collect(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(s) if is_uuid(s) && !out.contains(s) => out.push(s.clone()),
            Value::Array(list) => list.iter().for_each(|v| collect(v, out)),
            // `restored` and `renamed` list objects: their ids only
            Value::Object(map) => {
                for key in ["id", "folder_id"] {
                    if let Some(v) = map.get(key) {
                        collect(v, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Some(map) = value.as_object() {
        for key in RESULT_KEYS {
            if let Some(v) = map.get(*key) {
                collect(v, &mut out);
            }
        }
    }
    out.truncate(IDS_MAX);
    out
}

/// Whether `s` is a UUID in its hyphenated form.
fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// A fresh `operation_id`.
pub fn new_operation_id() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(format!("no random source: {e}")))?;
    Ok(format!("op-{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

/// Whether `value` has the form of an `operation_id`.
pub fn is_operation_id(value: &str) -> bool {
    value.len() == 35 && value.starts_with("op-") && value[3..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Take the `client_request_id` out of a change's arguments, checked: the
/// tool itself then reads the arguments as before.
pub fn take_client_request_id(arguments: &mut Value) -> Result<Option<String>, ToolError> {
    let Some(value) = arguments.as_object_mut().and_then(|a| a.remove(CLIENT_REQUEST_ID)) else {
        return Ok(None);
    };
    match value.as_str() {
        Some(id) if valid_client_request_id(id) => Ok(Some(id.to_owned())),
        _ => Err(ToolError::new(
            ErrorCode::InvalidArguments,
            format!("`{CLIENT_REQUEST_ID}` must be 1 to {CLIENT_REQUEST_ID_MAX} printable ASCII characters"),
        )),
    }
}

/// 1 to 64 printable ASCII characters (spaces inside allowed).
pub fn valid_client_request_id(id: &str) -> bool {
    (1..=CLIENT_REQUEST_ID_MAX).contains(&id.len()) && id.bytes().all(|b| (0x20..0x7f).contains(&b)) && id.trim() == id
}

/// Text as a receipt keeps it: no control characters, `max` characters at
/// most (cut on a character boundary).
pub fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

fn unix_ms(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// FNV-1a, 64 bits: enough to tell two argument sets apart in memory (not
/// a security boundary: the token is).
fn fnv(bytes: impl IntoIterator<Item = u8>, mut hash: u64) -> u64 {
    for b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

/// The hash of a tool and its arguments, keys in order whatever the JSON
/// map keeps.
fn arguments_hash(tool: &str, arguments: &Value) -> u64 {
    fn canonical(value: &Value, out: &mut String) {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for key in keys {
                    out.push_str(&Value::String(key.clone()).to_string());
                    out.push(':');
                    canonical(&map[key], out);
                    out.push(',');
                }
                out.push('}');
            }
            Value::Array(list) => {
                out.push('[');
                for v in list {
                    canonical(v, out);
                    out.push(',');
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut text = String::new();
    canonical(arguments, &mut text);
    fnv(text.bytes(), fnv(tool.bytes().chain([0]), FNV_START))
}

fn retired_key(session: Option<&str>, id: &str) -> u64 {
    fnv(id.bytes(), fnv(session.unwrap_or_default().bytes().chain([0]), FNV_START))
}

/// The frame thread's receipts, shared by the calls it took: each call's
/// answer ends its own receipt ([`Tracked`]).
#[derive(Clone, Debug, Default)]
pub struct Ledger(Arc<Mutex<Store>>);

impl Ledger {
    pub fn with<R>(&self, f: impl FnOnce(&mut Store) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// A call's receipt: its ledger and its `operation_id`.
#[derive(Clone, Debug)]
pub struct Tracked {
    pub ledger: Ledger,
    pub operation_id: String,
}

impl Tracked {
    pub fn update<R>(&self, f: impl FnOnce(&mut Receipt) -> R) -> Option<R> {
        self.ledger.with(|store| store.update(&self.operation_id, f))
    }

    /// The call answers `result`: its receipt ends, and the answer says
    /// which operation it was and its state.
    pub fn decorate(&self, result: Result<Value, ToolError>) -> Result<Value, ToolError> {
        let state = self.ledger.with(|store| store.finish(&self.operation_id, &result, Instant::now()));
        match result {
            Ok(mut value) => {
                if let Some(object) = value.as_object_mut() {
                    object.entry("operation_id").or_insert_with(|| json!(self.operation_id));
                    if let Some(state) = state {
                        object.entry("state").or_insert_with(|| json!(state));
                    }
                }
                Ok(value)
            }
            Err(mut e) => {
                e.operation_id = Some(self.operation_id.clone());
                Err(e)
            }
        }
    }

    /// A late server answer.
    pub fn settle(&self, late: Late) -> Option<State> {
        self.ledger.with(|store| store.settle(&self.operation_id, late, Instant::now()))
    }
}

#[cfg(test)]
mod tests;
