//! Folderstorm: `inventory_changes` (folderstorm/plans/mcp.md, "Phase 3,
//! part 2"): what changed in My Inventory since a generation, after
//! Folderstorm (C++)'s change cursor (`tools/fs-mcp/README.md`: "Poll it
//! with the generation"). The viewer keeps no list of changed entries, so
//! the journal compares a fingerprint of every entry of My Inventory with
//! the one it saw last, and only while an assistant polls: the first call
//! starts it, ten minutes without a call stop it and free its memory. A
//! comparison runs at most once a second, and only when the inventory's
//! own generation moved.
//!
//! Each change carries the entry's id, kind, name and folder: never an
//! asset or thumbnail id. The Library does not change and is left out.

use crate::world::inventory::Inventory;
use folderstorm_mcp_bridge::protocol::inventory::CHANGES_MAX;
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::{Value, json};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// The journal stops when no call came for this long.
pub const IDLE: Duration = Duration::from_secs(600);
/// How often the inventory is compared at most while it changes.
pub const SCAN_EVERY: Duration = Duration::from_secs(1);
/// Changes kept; older ones are forgotten (a cursor before them is reset).
pub const KEPT: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Added,
    Changed,
    Removed,
}

impl Change {
    fn name(self) -> &'static str {
        match self {
            Change::Added => "added",
            Change::Changed => "changed",
            Change::Removed => "removed",
        }
    }
}

struct Entry {
    generation: u64,
    id: Uuid,
    folder: bool,
    change: Change,
}

struct Following {
    root: Uuid,
    until: Instant,
    seen: u64,
    scanned: Instant,
    prints: HashMap<Uuid, (u64, bool)>,
    log: VecDeque<Entry>,
    /// The generation before the oldest change kept: a cursor older than it
    /// has lost changes.
    floor: u64,
}

#[derive(Default)]
pub struct Journal {
    /// The generation of the last change seen, never reset (a cursor from
    /// an earlier journal stays older than the next one's changes).
    generation: u64,
    following: Option<Following>,
}

/// What an entry looks like now, for the comparison: where it is, its
/// name and text, its type and rights, its favorite and picture.
fn prints(inv: &Inventory) -> HashMap<Uuid, (u64, bool)> {
    let mut out = HashMap::with_capacity(inv.folders.len() + inv.items.len());
    for (id, f) in &inv.folders {
        if f.library || *id == inv.lib_root {
            continue;
        }
        let mut h = DefaultHasher::new();
        (f.info.parent, &f.info.name, f.info.type_default, f.info.favorite, f.info.thumbnail).hash(&mut h);
        out.insert(*id, (h.finish(), true));
    }
    for it in inv.items.values() {
        if inv.folders.get(&it.parent).is_some_and(|f| f.library) {
            continue;
        }
        let mut h = DefaultHasher::new();
        (it.parent, &it.name, &it.desc, it.asset_type, it.inv_type, it.flags, it.asset_id).hash(&mut h);
        (it.favorite, it.thumbnail, it.owner, it.group_owned, it.owner_mask, it.everyone_mask).hash(&mut h);
        (it.group_mask, it.next_owner_mask).hash(&mut h);
        out.insert(it.id, (h.finish(), false));
    }
    out
}

impl Journal {
    /// Once per frame: compare the inventory when it moved (a second at
    /// most between two comparisons); stop after [`IDLE`] without a call.
    pub fn follow(&mut self, inv: &Inventory, now: Instant) {
        let Some(f) = &self.following else { return };
        if now >= f.until || inv.root.is_nil() {
            self.following = None;
            return;
        }
        if f.root != inv.root {
            // another login: what came before says nothing of this one
            self.following = None;
            return;
        }
        if f.seen != inv.generation && now.duration_since(f.scanned) >= SCAN_EVERY {
            self.scan(inv, now);
        }
    }

    fn scan(&mut self, inv: &Inventory, now: Instant) {
        let Some(f) = self.following.as_mut() else { return };
        let fresh = prints(inv);
        let mut changes: Vec<(Uuid, bool, Change)> = Vec::new();
        for (id, (print, folder)) in &fresh {
            match f.prints.get(id) {
                None => changes.push((*id, *folder, Change::Added)),
                Some((old, _)) if old != print => changes.push((*id, *folder, Change::Changed)),
                Some(_) => {}
            }
        }
        for (id, (_, folder)) in &f.prints {
            if !fresh.contains_key(id) {
                changes.push((*id, *folder, Change::Removed));
            }
        }
        // a stable order inside one comparison
        changes.sort_by_key(|(id, folder, _)| (!*folder, *id));
        for (id, folder, change) in changes {
            self.generation += 1;
            f.log.push_back(Entry {
                generation: self.generation,
                id,
                folder,
                change,
            });
        }
        while f.log.len() > KEPT {
            if let Some(old) = f.log.pop_front() {
                f.floor = old.generation;
            }
        }
        f.prints = fresh;
        f.seen = inv.generation;
        f.scanned = now;
    }

    /// `inventory_changes`: without `since`, the generation to start from
    /// (the journal starts if needed); with it, what changed after it.
    pub fn answer(&mut self, args: &Value, inv: &Inventory, now: Instant) -> Result<Value, ToolError> {
        if args.as_object().is_none_or(|a| a.keys().any(|k| k != "since")) {
            return Err(ToolError::new(ErrorCode::InvalidArguments, "unknown argument"));
        }
        let since = match &args["since"] {
            Value::Null => None,
            v => Some(
                v.as_u64()
                    .ok_or_else(|| ToolError::new(ErrorCode::InvalidArguments, "`since` must be a generation (a whole number)"))?,
            ),
        };
        let started = self.following.is_none();
        if started {
            self.following = Some(Following {
                root: inv.root,
                until: now + IDLE,
                seen: inv.generation,
                scanned: now,
                prints: prints(inv),
                log: VecDeque::new(),
                floor: self.generation,
            });
        } else if self.following.as_ref().is_some_and(|f| f.seen != inv.generation) {
            self.scan(inv, now);
        }
        let generation = self.generation;
        let Some(f) = self.following.as_mut() else {
            return Err(ToolError::new(ErrorCode::Internal, "the journal did not start"));
        };
        f.until = now + IDLE;
        let Some(since) = since else {
            return Ok(json!({"generation": generation, "changes": [], "more": false}));
        };
        if since > generation {
            return Err(ToolError::new(
                ErrorCode::InvalidArguments,
                "`since` is ahead of the viewer: start again without it",
            ));
        }
        if started || since < f.floor {
            return Ok(json!({
                "generation": generation,
                "changes": [],
                "more": false,
                "reset": true,
                "next": "the changes before this generation are not known: read again what you need, then poll from this generation",
            }));
        }
        // one line per entry: added then changed is added, added then
        // removed is nothing to report, anything else is the last change
        let mut order: Vec<Uuid> = Vec::new();
        let mut first_last: HashMap<Uuid, (Change, Change, bool)> = HashMap::new();
        let mut reached = generation;
        let mut more = false;
        for entry in f.log.iter().filter(|e| e.generation > since) {
            if !first_last.contains_key(&entry.id) && order.len() == CHANGES_MAX {
                more = true;
                reached = entry.generation - 1;
                break;
            }
            first_last
                .entry(entry.id)
                .and_modify(|(_, last, _)| *last = entry.change)
                .or_insert_with(|| {
                    order.push(entry.id);
                    (entry.change, entry.change, entry.folder)
                });
        }
        let changes: Vec<Value> = order
            .iter()
            .filter_map(|id| {
                let (first, last, folder) = first_last[id];
                let change = match (first, last) {
                    (Change::Added, Change::Removed) => return None,
                    (Change::Added, _) => Change::Added,
                    (_, last) => last,
                };
                let mut out = json!({"id": id, "kind": if folder { "folder" } else { "item" }, "change": change.name()});
                if change != Change::Removed {
                    let (name, parent) = if folder {
                        let f = inv.folders.get(id);
                        (f.map(|f| f.info.name.as_str()), f.map(|f| f.info.parent))
                    } else {
                        let it = inv.items.get(id);
                        (it.map(|it| it.name.as_str()), it.map(|it| it.parent))
                    };
                    out["name"] = json!(name);
                    out["folder_id"] = json!(parent);
                }
                Some(out)
            })
            .collect();
        Ok(json!({"generation": reached, "changes": changes, "more": more}))
    }

    /// Whether the journal is following the inventory, for the tests.
    #[cfg(test)]
    pub fn following(&self) -> bool {
        self.following.is_some()
    }
}
