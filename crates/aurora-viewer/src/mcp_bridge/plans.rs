//! Folderstorm: batch plans, preview then execute
//! (folderstorm/plans/mcp-receipts.md, section 6).
//!
//! - `inventory_preview_batch_rename` / `inventory_preview_batch_move` are
//!   reads (Read): they plan 1 to 50 entries with the native review's
//!   planner (`folderstorm::bulk_plan`), change nothing and grant nothing,
//!   and keep the plan for 5 minutes under a `plan_id` (8 open plans per
//!   login at most, the oldest unused one going first).
//! - `inventory_execute_plan` needs the plan's own class (Edit for a rename,
//!   Move for a move), decided by the viewer from the plan. The plan is
//!   consumed when its execute is taken: executing it again answers the
//!   same operation, nothing is sent twice. Its rows are planned again from
//!   the same request and must equal the preview exactly (the native
//!   review's rule), when the execute is taken and again just before the
//!   send (`changes::plan`, through the frozen plan the call carries);
//!   otherwise it answers `stale` and nothing is sent.
//!
//! Stricter than the native review: entries under the Marketplace Listings
//! folder are left out, a destination there or in the Trash is refused
//! (`inventory_move`'s rule), duplicate ids are refused, and RLVa's
//! @showinv refuses every plan.

use super::changes::{Planned, Work};
use super::tools::{Ctx, err, ready, scope};
use crate::folderstorm::bulk_plan::{self, Kind, Row};
use crate::world::inventory::actions;
use folderstorm_mcp_bridge::protocol::assistant::{EXECUTE_PLAN, PREVIEW_MOVE, PREVIEW_RENAME};
use folderstorm_mcp_bridge::receipts;
use folderstorm_mcp_bridge::{Class, ErrorCode, ToolError};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// How long a plan may wait for its execute.
pub const PLAN_TTL: Duration = Duration::from_secs(5 * 60);
/// Open plans per login.
pub const PLANS_MAX: usize = 8;
/// Rows the Ask dialog lists before "and N more".
pub const DIALOG_ROWS: usize = 5;
/// LLFolderType::FT_TRASH.
const FT_TRASH: i32 = 14;

/// Whether `tool` is one of the previews.
pub fn previews(tool: &str) -> bool {
    tool == PREVIEW_RENAME || tool == PREVIEW_MOVE
}

/// One plan kept for its execute.
struct Plan {
    id: String,
    /// The preview's tool and arguments: the request it plans again.
    tool: String,
    arguments: Value,
    rows: Vec<Row>,
    created: Instant,
    /// The operation of its execute, once taken.
    used: Option<String>,
}

/// The plans of one login (the desk drops them at a login change and when
/// access is turned off).
#[derive(Default)]
pub struct Plans {
    plans: Vec<Plan>,
}

/// What [`Plans::take`] found for an execute.
pub enum Found {
    /// Not there, expired, or of another login.
    Missing,
    /// Already executed by this operation.
    Used,
    /// Consumed now by the execute: its request, frozen.
    Fresh { tool: String, arguments: Value, rows: Vec<Row> },
}

impl Plans {
    /// Keep a new plan; returns its id.
    fn keep(&mut self, tool: &str, arguments: Value, rows: Vec<Row>, now: Instant) -> String {
        self.prune(now);
        while self.plans.len() >= PLANS_MAX {
            let oldest = self.plans.iter().position(|p| p.used.is_none()).unwrap_or(0);
            self.plans.remove(oldest);
        }
        let id = format!("pl-{}", Uuid::new_v4().simple());
        self.plans.push(Plan {
            id: id.clone(),
            tool: tool.to_owned(),
            arguments,
            rows,
            created: now,
            used: None,
        });
        id
    }

    /// Whether plan `id` was already executed, and by which operation.
    pub fn used(&self, id: &str, now: Instant) -> Option<String> {
        self.plans
            .iter()
            .find(|p| p.id == id && now.saturating_duration_since(p.created) < PLAN_TTL)
            .and_then(|p| p.used.clone())
    }

    /// The execute of operation `operation` takes plan `id`.
    pub fn take(&mut self, id: &str, operation: &str, now: Instant) -> Found {
        self.prune(now);
        let Some(plan) = self.plans.iter_mut().find(|p| p.id == id) else {
            return Found::Missing;
        };
        if plan.used.is_some() {
            return Found::Used;
        }
        plan.used = Some(operation.to_owned());
        Found::Fresh {
            tool: plan.tool.clone(),
            arguments: plan.arguments.clone(),
            rows: plan.rows.clone(),
        }
    }

    fn prune(&mut self, now: Instant) {
        self.plans.retain(|p| now.saturating_duration_since(p.created) < PLAN_TTL);
    }
}

/// The class a plan's execute needs: Edit for a rename, Move for a move.
pub fn class(tool: &str) -> Class {
    if tool == PREVIEW_MOVE { Class::Move } else { Class::Edit }
}

fn invalid(message: impl Into<String>) -> ToolError {
    err(ErrorCode::InvalidArguments, message)
}

fn known(args: &Value, keys: &[&str]) -> Result<(), ToolError> {
    match args.as_object() {
        Some(a) if a.keys().all(|k| keys.contains(&k.as_str())) => Ok(()),
        _ => Err(invalid("unknown argument")),
    }
}

/// 1 to 50 ids, each once, each an inventory entry.
fn ids(list: &[Value], cx: &Ctx) -> Result<Vec<Uuid>, ToolError> {
    if list.is_empty() || list.len() > bulk_plan::MAX {
        return Err(invalid(format!("a plan takes 1 to {} entries", bulk_plan::MAX)));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(list.len());
    for value in list {
        let id = value
            .as_str()
            .and_then(|s| Uuid::parse_str(s.trim()).ok())
            .ok_or_else(|| invalid("an id is not a UUID"))?;
        if !seen.insert(id) {
            return Err(invalid(format!("{id} is given twice")));
        }
        if !cx.inv.items.contains_key(&id) && !cx.inv.folders.contains_key(&id) {
            return Err(err(ErrorCode::NotFound, format!("no inventory entry {id}")));
        }
        out.push(id);
    }
    Ok(out)
}

/// The rows of a preview's request, from the shared planner and the
/// bridge's own rules.
pub fn rows(tool: &str, args: &Value, cx: &Ctx) -> Result<Vec<Row>, ToolError> {
    ready(cx)?;
    let inv = cx.inv;
    let mut rows = match tool {
        PREVIEW_RENAME => {
            known(args, &["items", "ids", "pattern"])?;
            match (&args["items"], &args["ids"], &args["pattern"]) {
                (Value::Array(items), Value::Null, Value::Null) => {
                    let ids = ids(&items.iter().map(|i| i["id"].clone()).collect::<Vec<_>>(), cx)?;
                    let mut named = Vec::with_capacity(ids.len());
                    for (id, item) in ids.into_iter().zip(items) {
                        if item.as_object().is_none_or(|o| o.keys().any(|k| k != "id" && k != "name")) {
                            return Err(invalid("each item is {\"id\", \"name\"}"));
                        }
                        let name = item["name"].as_str().ok_or_else(|| invalid("each item needs a `name`"))?;
                        named.push((id, name.to_owned()));
                    }
                    bulk_plan::plan_rename_to(inv, &named)
                }
                (Value::Null, Value::Array(list), Value::String(pattern)) => {
                    if !pattern.contains("{name}") && !pattern.contains("{n}") && list.len() > 1 {
                        return Err(invalid("a pattern for several entries needs `{name}` or `{n}`"));
                    }
                    bulk_plan::plan_rename(inv, &ids(list, cx)?, pattern)
                }
                _ => return Err(invalid("give `items` ({id, name} each), or `ids` with a `pattern`")),
            }
        }
        PREVIEW_MOVE => {
            known(args, &["ids", "folder_id", "path"])?;
            let list = args["ids"].as_array().ok_or_else(|| invalid("`ids` is required"))?;
            let ids = ids(list, cx)?;
            let to = scope(args, inv)?.ok_or_else(|| invalid("give the destination as `folder_id` or `path`"))?;
            if Some(to) == actions::system(inv, FT_TRASH) || actions::in_type(inv, to, FT_TRASH) {
                return Err(invalid("to put something in the Trash, use inventory_trash"));
            }
            if crate::world::marketplace::root(inv).is_some_and(|root| actions::under(inv, to, root)) {
                return Err(err(
                    ErrorCode::NotAllowed,
                    "the Marketplace Listings folder is not changed by an assistant",
                ));
            }
            bulk_plan::plan_move(inv, &ids, to, &cx.protected, &cx.worn)
        }
        _ => return Err(err(ErrorCode::UnknownTool, "unknown tool")),
    };
    // the Marketplace Listings folder stays out of reach
    if let Some(root) = crate::world::marketplace::root(inv) {
        for row in rows.iter_mut().filter(|r| r.skip.is_none() && actions::under(inv, r.id, root)) {
            row.after.clear();
            row.skip = Some("the Marketplace Listings folder is not changed by an assistant".into());
        }
    }
    Ok(rows)
}

/// The destination of a move preview.
fn destination(args: &Value, cx: &Ctx) -> Option<Uuid> {
    scope(args, cx.inv).ok().flatten()
}

fn rows_json(rows: &[Row]) -> Value {
    Value::Array(
        rows.iter()
            .map(|r| match &r.skip {
                None => json!({"id": r.id, "before": r.before, "after": r.after}),
                Some(reason) => json!({"id": r.id, "before": r.before, "skip": reason}),
            })
            .collect(),
    )
}

fn counts(rows: &[Row]) -> Value {
    let eligible = rows.iter().filter(|r| r.skip.is_none()).count();
    json!({"total": rows.len(), "eligible": eligible, "skipped": rows.len() - eligible})
}

/// A preview: plan, keep, answer. `execution` is what the plan's class
/// says now (`allow`, `ask` or `never`): a preview shows the plan even when
/// its execute would be refused.
pub fn preview(
    plans: &mut Plans,
    tool: &str,
    args: &Value,
    cx: &Ctx,
    execution: folderstorm_mcp_bridge::Decision,
    now: Instant,
) -> Result<Value, ToolError> {
    let rows = rows(tool, args, cx)?;
    let answer_rows = rows_json(&rows);
    let counts = counts(&rows);
    let id = plans.keep(tool, args.clone(), rows, now);
    Ok(json!({
        "plan_id": id,
        "expires_in": PLAN_TTL.as_secs(),
        "rows": answer_rows,
        "counts": counts,
        "execution": match execution {
            folderstorm_mcp_bridge::Decision::Run => "allow",
            folderstorm_mcp_bridge::Decision::Ask => "ask",
            folderstorm_mcp_bridge::Decision::Refuse => "never",
        },
        "viewer_session": cx.viewer_session,
        "next": format!("call {EXECUTE_PLAN} with this plan_id within {} seconds to run it", PLAN_TTL.as_secs()),
    }))
}

/// The arguments an execute carries once its plan is taken: the plan
/// frozen, so that the send plans it again ([`plan`]).
pub fn frozen(plan_id: &str, tool: &str, arguments: Value, rows: &[Row]) -> Value {
    json!({"plan_id": plan_id, "frozen": {"tool": tool, "arguments": arguments, "rows": rows_json(rows)}})
}

/// A stale plan: the inventory or the rules changed since the preview.
pub fn stale(now: &[Row], shown: &Value) -> ToolError {
    let shown = shown.as_array().map_or(0, Vec::len);
    let differ = now.len().max(shown);
    ToolError::new(
        ErrorCode::Stale,
        format!(
            "the inventory changed since the preview ({differ} rows planned, not all as shown): nothing was sent and the plan is gone. Preview again"
        ),
    )
}

/// The receipt rows of a plan.
pub fn receipt_rows(rows: &[Row]) -> Vec<receipts::Row> {
    rows.iter()
        .map(|r| receipts::Row::new(r.id.to_string(), &r.before, &r.after, r.skip.as_deref()))
        .collect()
}

/// `inventory_execute_plan` with its frozen plan: planned again, it must
/// equal the preview; then one inventory change for the eligible rows.
pub fn plan(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    let frozen = &args["frozen"];
    let (Some(tool), Some(plan_id)) = (frozen["tool"].as_str(), args["plan_id"].as_str()) else {
        return Err(err(ErrorCode::Internal, "the plan was not taken"));
    };
    let preview_args = &frozen["arguments"];
    let now = rows(tool, preview_args, cx)?;
    if rows_json(&now) != frozen["rows"] {
        return Err(stale(&now, &frozen["rows"]));
    }
    let kind = if tool == PREVIEW_MOVE { Kind::Move } else { Kind::Rename };
    let to = destination(preview_args, cx);
    let change = bulk_plan::mutation(cx.inv, kind, &now, to, &cx.protected, &cx.worn).map_err(|e| err(ErrorCode::NotAllowed, e))?;
    let eligible: Vec<&Row> = now.iter().filter(|r| r.skip.is_none()).collect();
    if change.operations.is_empty() || eligible.is_empty() {
        return Err(err(ErrorCode::NotAllowed, "every row of this plan is left out: nothing to send"));
    }
    let summary = match (kind, to) {
        (Kind::Move, Some(to)) => aurora_i18n::tr!(
            "fs-mcp-ask-plan-move",
            count = eligible.len(),
            folder = super::tools::path(cx.inv, to)
        ),
        _ => aurora_i18n::tr!("fs-mcp-ask-plan-rename", count = eligible.len()),
    };
    // the rows' outcomes; names stay in the preview and in the receipt
    // (Read-gated)
    let reply_rows: Vec<Value> = now
        .iter()
        .map(|r| json!({"id": r.id, "outcome": if r.skip.is_some() { "skipped" } else { "done" }}))
        .collect();
    let reply = json!({"plan_id": plan_id, "rows": reply_rows, "counts": counts(&now)});
    Ok(Planned {
        work: Work::Inventory(change),
        summary,
        reply,
        created: None,
    })
}

/// The lines the Ask dialog lists under a plan's sentence: at most 5 rows,
/// "and N more", and the entries left out. Empty for any other call.
pub fn dialog_lines(args: &Value) -> Vec<String> {
    let Some(rows) = args["frozen"]["rows"].as_array() else {
        return Vec::new();
    };
    let rename = args["frozen"]["tool"].as_str() == Some(PREVIEW_RENAME);
    let eligible: Vec<&Value> = rows.iter().filter(|r| r.get("skip").is_none()).collect();
    let text = |v: &Value, key: &str| v[key].as_str().unwrap_or_default().to_owned();
    let mut lines: Vec<String> = eligible
        .iter()
        .take(DIALOG_ROWS)
        .map(|row| {
            if rename {
                aurora_i18n::tr!(
                    "fs-mcp-ask-plan-row-rename",
                    before = text(row, "before"),
                    after = text(row, "after")
                )
            } else {
                aurora_i18n::tr!("fs-mcp-ask-plan-row-move", name = text(row, "before"))
            }
        })
        .collect();
    if eligible.len() > DIALOG_ROWS {
        lines.push(aurora_i18n::tr!("fs-mcp-ask-plan-more", count = eligible.len() - DIALOG_ROWS));
    }
    let skipped = rows.len() - eligible.len();
    if skipped > 0 {
        lines.push(aurora_i18n::tr!("fs-mcp-ask-plan-skipped", count = skipped));
    }
    lines
}
