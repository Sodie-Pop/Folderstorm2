//! Folderstorm: the planner of a bulk rename or move of 1 to 50 inventory
//! entries, shared by the inventory's "Review bulk changes…" window
//! (`ui/inventory/bulk_review.rs`) and the local assistant's batch plans
//! (`mcp_bridge/plans.rs`, folderstorm/plans/mcp-receipts.md, section 6), so
//! that a plan is never allowed what the native review, a single rename or a
//! single move refuses.
//!
//! Port of the native review of Folderstorm's FSFloaterInventoryBulk
//! (fsfloaterinventorybulk.cpp, Folderstorm contributors, originally LGPL
//! 2.1): a rename uses a pattern where `{name}` is the current name and
//! `{n}` the entry's number in the selection (or, for the assistant, an
//! explicit name per entry); a move goes to one folder. Each row says the
//! name before and after, or why it is left out. The rules are Aurora's own
//! (`rules::renameable`, `clean_name`, `patch`, `move_selection`).

use crate::ui::inventory::inventory_path;
use crate::world::inventory::{Inventory, actions as rules};
use aurora_net::inventory::operations::Mutation;
use std::collections::HashSet;
use uuid::Uuid;

/// The most entries one plan takes (Folderstorm's limit).
pub(crate) const MAX: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Rename,
    Move,
}

/// One entry of a plan: its name before, and after (the new name, or the
/// destination's path), or why it is left out.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub(crate) id: Uuid,
    pub(crate) before: String,
    pub(crate) after: String,
    pub(crate) skip: Option<String>,
}

/// A rename pattern applied to one entry: `{n}` first (so a name holding
/// "{n}" stays as it is), then `{name}`.
pub(crate) fn apply_pattern(pattern: &str, number: usize, name: &str) -> String {
    pattern.replace("{n}", &number.to_string()).replace("{name}", name)
}

/// The row of renaming `id` to `wanted` (cleaned as the window cleans it).
fn rename_row(inv: &Inventory, id: Uuid, wanted: impl FnOnce(&str) -> String) -> Row {
    let before = rules::name(inv, id);
    let mut row = Row {
        id,
        before: before.clone(),
        after: String::new(),
        skip: None,
    };
    let exists = inv.items.contains_key(&id) || inv.folders.contains_key(&id);
    row.skip = if !exists {
        Some(aurora_i18n::tr!("common-item-not-found").into())
    } else if !rules::renameable(inv, id) {
        Some(aurora_i18n::tr!("inventory-bulk-review-skip-rename").into())
    } else {
        match rules::clean_name(&wanted(&before)) {
            Err(reason) => Some(reason),
            Ok(name) if name == before => Some(aurora_i18n::tr!("inventory-bulk-review-skip-unchanged").into()),
            Ok(name) => match rules::patch(inv, id, aurora_llsd::llsd_map! { "name" => name.clone() }) {
                Ok(_) => {
                    row.after = name;
                    None
                }
                Err(reason) => Some(reason),
            },
        }
    };
    row
}

pub(crate) fn plan_rename(inv: &Inventory, ids: &[Uuid], pattern: &str) -> Vec<Row> {
    ids.iter()
        .enumerate()
        .map(|(i, &id)| rename_row(inv, id, |before| apply_pattern(pattern, i + 1, before)))
        .collect()
}

/// A rename to an explicit name per entry (the assistant's `items`).
pub(crate) fn plan_rename_to(inv: &Inventory, items: &[(Uuid, String)]) -> Vec<Row> {
    items.iter().map(|(id, name)| rename_row(inv, *id, |_| name.clone())).collect()
}

pub(crate) fn plan_move(inv: &Inventory, ids: &[Uuid], to: Uuid, protected: &HashSet<Uuid>, worn: &HashSet<Uuid>) -> Vec<Row> {
    let path = inventory_path(inv, to);
    ids.iter()
        .map(|&id| {
            let skip = match rules::move_selection(inv, &[id], to, protected, worn) {
                Err(reason) => Some(reason),
                Ok(change) if change.operations.is_empty() => Some(aurora_i18n::tr!("inventory-bulk-review-skip-there").into()),
                Ok(_) => None,
            };
            Row {
                id,
                before: rules::name(inv, id),
                after: if skip.is_none() { path.clone() } else { String::new() },
                skip,
            }
        })
        .collect()
}

/// The change for the eligible rows of a plan: one mutation.
pub(crate) fn mutation(
    inv: &Inventory,
    kind: Kind,
    rows: &[Row],
    to: Option<Uuid>,
    protected: &HashSet<Uuid>,
    worn: &HashSet<Uuid>,
) -> Result<Mutation, String> {
    let eligible: Vec<&Row> = rows.iter().filter(|r| r.skip.is_none()).collect();
    match kind {
        Kind::Rename => {
            let mut all = Mutation::default();
            for row in eligible {
                let change = rules::patch(inv, row.id, aurora_llsd::llsd_map! { "name" => row.after.clone() })?;
                all.operations.extend(change.operations);
                for folder in change.refresh {
                    if !all.refresh.contains(&folder) {
                        all.refresh.push(folder);
                    }
                }
            }
            Ok(all)
        }
        Kind::Move => {
            let to = to.ok_or_else(|| aurora_i18n::tr!("inventory-bulk-review-choose").to_owned())?;
            let ids: Vec<Uuid> = eligible.iter().map(|r| r.id).collect();
            rules::move_selection(inv, &ids, to, protected, worn)
        }
    }
}
