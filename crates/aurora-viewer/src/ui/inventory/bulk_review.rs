//! Folderstorm: "Review bulk changes…", a previewed rename or move of 1 to 50
//! selected inventory objects.
//!
//! Port of the native review of Folderstorm's FSFloaterInventoryBulk
//! (fsfloaterinventorybulk.cpp, Folderstorm contributors, originally LGPL
//! 2.1): the selection is frozen when the window opens; a rename uses a
//! pattern where `{name}` is the current name and `{n}` the object's number in
//! the selection; a move goes to a folder found by its name or its parent's
//! (the list of "Go to folder…", `folder_jump.rs`). Preview lists every row
//! before and after, and the skipped ones with their reason; Execute is
//! available only for the preview on screen, and checks the plan again
//! against the current inventory before anything is sent. Folderstorm's
//! session history and undo (FEATURES.md 2.5) are not part of it.
//!
//! The rules are Aurora's own (`rules::renameable`, `clean_name`, `patch`,
//! `move_selection`): a bulk change is never allowed what a single rename or
//! move would refuse.

use super::*;
use crate::ui::widgets::{Floater, flat_button, primary_button_sized, radio_value, search_field};

/// The most objects one review takes (Folderstorm's limit).
pub(super) const MAX: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    Rename,
    Move,
}

/// One object of a plan: its name before, and after (the new name, or the
/// destination's path), or why it is left out.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub(super) id: Uuid,
    pub(super) before: String,
    pub(super) after: String,
    pub(super) skip: Option<String>,
}

/// The request a preview was made for: Execute needs the same.
#[derive(Clone, Debug, PartialEq)]
struct Request {
    kind: Kind,
    pattern: String,
    to: Option<Uuid>,
}

pub(super) struct BulkReview {
    ids: Vec<Uuid>,
    kind: Kind,
    pattern: String,
    to: Option<Uuid>,
    query: String,
    folders: Option<(u64, Vec<super::folder_jump::Entry>)>,
    preview: Option<(Request, Vec<Row>)>,
    message: String,
}

impl BulkReview {
    pub(super) fn new(ids: Vec<Uuid>) -> Self {
        Self {
            ids,
            kind: Kind::Rename,
            pattern: "{name}".into(),
            to: None,
            query: String::new(),
            folders: None,
            preview: None,
            message: String::new(),
        }
    }

    fn request(&self) -> Request {
        Request {
            kind: self.kind,
            pattern: self.pattern.clone(),
            to: self.to,
        }
    }

    /// The preview still matches what the window asks for.
    fn current(&self) -> Option<&Vec<Row>> {
        self.preview.as_ref().filter(|(r, _)| *r == self.request()).map(|(_, rows)| rows)
    }
}

/// A rename pattern applied to one object: `{n}` first (so a name holding
/// "{n}" stays as it is), then `{name}`.
pub(super) fn apply_pattern(pattern: &str, number: usize, name: &str) -> String {
    pattern.replace("{n}", &number.to_string()).replace("{name}", name)
}

pub(super) fn plan_rename(inv: &Inventory, ids: &[Uuid], pattern: &str) -> Vec<Row> {
    ids.iter()
        .enumerate()
        .map(|(i, &id)| {
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
                match rules::clean_name(&apply_pattern(pattern, i + 1, &before)) {
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
        })
        .collect()
}

pub(super) fn plan_move(inv: &Inventory, ids: &[Uuid], to: Uuid, protected: &HashSet<Uuid>, worn: &HashSet<Uuid>) -> Vec<Row> {
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
pub(super) fn mutation(
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

fn plan(inv: &Inventory, review: &BulkReview, prefs: &InventoryPreferences, facts: &Facts) -> Option<Vec<Row>> {
    match review.kind {
        Kind::Rename => Some(plan_rename(inv, &review.ids, &review.pattern)),
        Kind::Move => review.to.map(|to| plan_move(inv, &review.ids, to, &prefs.protected, &facts.worn)),
    }
}

/// Opens the review for a selection of 1 to [`MAX`] objects.
pub(super) fn open(st: &mut InventoryUi, ids: Vec<Uuid>) {
    if (1..=MAX).contains(&ids.len()) {
        st.bulk_review = Some(BulkReview::new(ids));
    }
}

pub(super) fn show(
    ctx: &egui::Context,
    p: &Palette,
    inv: &Inventory,
    st: &mut InventoryUi,
    prefs: &InventoryPreferences,
    facts: &Facts,
    actions: &mut Vec<InvAction>,
) {
    let Some(mut review) = st.bulk_review.take() else { return };
    let mut open = true;
    let mut close = false;
    let busy = st.pending.is_some();
    let window_id = format!("inventory_bulk_review_{}", st.ui_id);
    Floater::new(
        &window_id,
        aurora_i18n::tr!("inventory-bulk-review-title"),
        ctx.content_rect().center() - egui::vec2(290.0, 260.0),
        Vec2::new(580.0, 520.0),
    )
    .show(ctx, p, &mut open, |ui| {
        ui.label(aurora_i18n::tr!("inventory-bulk-review-frozen", count = review.ids.len()));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            radio_value(ui, p, &mut review.kind, Kind::Rename, aurora_i18n::tr!("common-rename"));
            radio_value(ui, p, &mut review.kind, Kind::Move, aurora_i18n::tr!("inventory-bulk-review-move"));
        });
        ui.add_space(4.0);
        match review.kind {
            Kind::Rename => {
                ui.horizontal(|ui| {
                    ui.label(aurora_i18n::tr!("inventory-bulk-review-pattern"));
                    ui.add(egui::TextEdit::singleline(&mut review.pattern).desired_width(f32::INFINITY));
                });
                ui.label(
                    RichText::new(aurora_i18n::tr!("inventory-bulk-review-pattern-help"))
                        .size(11.0)
                        .color(p.muted),
                );
            }
            Kind::Move => destination(ui, p, inv, &mut review),
        }
        ui.add_space(6.0);
        let can_plan = review.kind == Kind::Rename || review.to.is_some();
        ui.add_enabled_ui(can_plan, |ui| {
            if flat_button(ui, p, aurora_i18n::tr!("inventory-bulk-review-preview")).clicked() {
                review.message.clear();
                review.preview = plan(inv, &review, prefs, facts).map(|rows| (review.request(), rows));
            }
        });
        let stale = review.preview.is_some() && review.current().is_none();
        if stale {
            ui.label(RichText::new(aurora_i18n::tr!("inventory-bulk-review-stale")).color(p.warn));
        }
        if let Some(rows) = review.current() {
            let eligible = rows.iter().filter(|r| r.skip.is_none()).count();
            ui.label(aurora_i18n::tr!(
                "inventory-bulk-review-counts",
                count = rows.len(),
                eligible = eligible,
                skipped = rows.len() - eligible
            ));
            egui::Frame::new().fill(p.field).show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("inventory_bulk_review_rows", st.ui_id))
                    .max_height((ui.available_height() - 60.0).max(80.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for row in rows {
                            preview_row(ui, p, row);
                        }
                    });
            });
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let ready = review.current().is_some_and(|rows| rows.iter().any(|r| r.skip.is_none()));
            ui.add_enabled_ui(ready && !busy, |ui| {
                if primary_button_sized(ui, p, aurora_i18n::tr!("inventory-bulk-review-execute"), Vec2::new(110.0, 26.0)).clicked() {
                    execute(inv, &mut review, prefs, facts, actions, &mut close);
                }
            });
            if flat_button(ui, p, aurora_i18n::tr!("common-cancel")).clicked() {
                close = true;
            }
        });
        if busy {
            ui.label(RichText::new(aurora_i18n::tr!("ui-inventory-wait-for-the-current-change-to-finish")).color(p.muted));
        }
        if !review.message.is_empty() {
            ui.label(RichText::new(&review.message).color(p.warn));
        }
    });
    if open && !close {
        st.bulk_review = Some(review);
    }
}

/// Plans again against the current inventory: the change leaves only when
/// it is still the one on screen.
fn execute(
    inv: &Inventory,
    review: &mut BulkReview,
    prefs: &InventoryPreferences,
    facts: &Facts,
    actions: &mut Vec<InvAction>,
    close: &mut bool,
) {
    let Some(shown) = review.current().cloned() else { return };
    let Some(now) = plan(inv, review, prefs, facts) else { return };
    if now != shown {
        review.message = aurora_i18n::tr!("inventory-bulk-review-changed").into();
        review.preview = Some((review.request(), now));
        return;
    }
    match mutation(inv, review.kind, &now, review.to, &prefs.protected, &facts.worn) {
        Ok(change) if !change.operations.is_empty() => {
            actions.push(InvAction::Edit(change));
            *close = true;
        }
        Ok(_) => review.message = aurora_i18n::tr!("inventory-bulk-review-nothing").into(),
        Err(reason) => review.message = reason,
    }
}

fn preview_row(ui: &mut egui::Ui, p: &Palette, row: &Row) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(RichText::new(&row.before).color(p.ink)).truncate());
        match &row.skip {
            None => {
                ui.label(RichText::new("→").color(p.muted));
                ui.add(egui::Label::new(RichText::new(&row.after).color(p.violet_light)).truncate());
            }
            Some(reason) => {
                ui.add(
                    egui::Label::new(
                        RichText::new(aurora_i18n::tr!("inventory-bulk-review-skipped", reason = reason.as_str()))
                            .size(11.0)
                            .color(p.muted),
                    )
                    .truncate(),
                );
            }
        }
    });
}

/// The destination: a folder search (that of "Go to folder…") and the chosen
/// folder's path.
fn destination(ui: &mut egui::Ui, p: &Palette, inv: &Inventory, review: &mut BulkReview) {
    if review.folders.as_ref().is_none_or(|(generation, _)| *generation != inv.generation) {
        review.folders = Some((inv.generation, super::folder_jump::index(inv)));
    }
    let width = ui.available_width();
    search_field(ui, &mut review.query, aurora_i18n::tr!("inventory-folder-jump-filter"), width);
    let Some((_, folders)) = &review.folders else { return };
    let (results, _) = super::folder_jump::search(folders, &review.query.trim().to_lowercase());
    egui::Frame::new().fill(p.field).show(ui, |ui| {
        egui::ScrollArea::vertical()
            .id_salt("inventory_bulk_review_folders")
            .max_height(120.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for i in results {
                    let entry = &folders[i];
                    let label = if entry.parent_path.is_empty() {
                        entry.name.clone()
                    } else {
                        format!("{} — {}", entry.name, entry.parent_path)
                    };
                    let chosen = review.to == Some(entry.id);
                    if radio_value(ui, p, &mut review.to, Some(entry.id), label).clicked() && !chosen {
                        review.message.clear();
                    }
                }
            });
    });
    let path = review
        .to
        .filter(|id| inv.folders.contains_key(id))
        .map(|id| inventory_path(inv, id));
    ui.label(match path {
        Some(path) => aurora_i18n::tr!("inventory-bulk-review-destination", path = path.as_str()),
        None => aurora_i18n::tr!("inventory-bulk-review-choose").to_owned(),
    });
}

/// `AURORA_DEMO_INVENTORY=bulk-review|aurora-bulk-review`: the review open on
/// three demo items with the pattern "Sorted {n} - {name}", previewed.
pub fn demo(st: &mut InventoryUi, inv: &Inventory) {
    let ids: Vec<Uuid> = [8100, 8102, 8103].into_iter().map(Uuid::from_u128).collect();
    let mut review = BulkReview::new(ids);
    review.pattern = "Sorted {n} - {name}".into();
    let rows = plan_rename(inv, &review.ids, &review.pattern);
    review.preview = Some((review.request(), rows));
    st.bulk_review = Some(review);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::inventory::Folder;
    use aurora_net::inventory::{InvFolder, operations::Operation};

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn fixture() -> Inventory {
        let mut inv = Inventory {
            root: id(1),
            ..Default::default()
        };
        inv.folders.insert(
            inv.root,
            Folder {
                info: InvFolder {
                    id: inv.root,
                    type_default: 8,
                    ..Default::default()
                },
                children: Vec::new(),
                items: Vec::new(),
                state: FetchState::Fetched,
                library: false,
            },
        );
        crate::world::inventory::demo::seed(&mut inv, id(2));
        inv
    }

    #[test]
    fn patterns_number_and_keep_the_name() {
        assert_eq!(apply_pattern("Sorted {n} - {name}", 3, "Hat"), "Sorted 3 - Hat");
        assert_eq!(
            apply_pattern("{name}", 1, "Odd {n} name"),
            "Odd {n} name",
            "a name keeps its own braces"
        );
    }

    #[test]
    fn rename_plan_numbers_eligible_rows_and_skips_with_reasons() {
        let inv = fixture();
        let ids: Vec<Uuid> = [8100, 8102].into_iter().map(id).chain([id(999_999)]).collect();
        let rows = plan_rename(&inv, &ids, "Box {n} {name}");
        assert_eq!(rows.len(), 3);
        for (i, row) in rows.iter().take(2).enumerate() {
            if rules::renameable(&inv, row.id) {
                assert_eq!(row.after, format!("Box {} {}", i + 1, row.before));
                assert!(row.skip.is_none());
            } else {
                assert!(row.skip.is_some());
            }
        }
        assert!(rows[2].skip.is_some(), "a missing object is skipped");
        // the same name is no change
        let same = plan_rename(&inv, &ids[..1], "{name}");
        assert!(same[0].skip.is_some());
    }

    #[test]
    fn rename_mutation_patches_each_eligible_row_once() {
        let inv = fixture();
        let ids: Vec<Uuid> = [8100, 8102, 8103].into_iter().map(id).collect();
        let rows = plan_rename(&inv, &ids, "New {n}");
        let eligible = rows.iter().filter(|r| r.skip.is_none()).count();
        let change = mutation(&inv, Kind::Rename, &rows, None, &HashSet::new(), &HashSet::new()).expect("a change");
        assert_eq!(change.operations.len(), eligible);
        assert!(change.operations.iter().all(|op| matches!(op, Operation::Patch { .. })));
        let mut refresh = change.refresh.clone();
        refresh.dedup();
        assert_eq!(refresh.len(), change.refresh.len(), "each folder refreshed once");
    }

    #[test]
    fn move_plan_follows_the_single_move_rules() {
        let inv = fixture();
        let ids: Vec<Uuid> = [8100, 8102].into_iter().map(id).collect();
        let to = id(8001);
        let rows = plan_move(&inv, &ids, to, &HashSet::new(), &HashSet::new());
        for row in &rows {
            let single = rules::move_selection(&inv, &[row.id], to, &HashSet::new(), &HashSet::new());
            let expected = single.is_ok_and(|c| !c.operations.is_empty());
            assert_eq!(row.skip.is_none(), expected, "{row:?}");
        }
        // a protected (locked) source is refused like a single move
        let locked = plan_move(&inv, &ids, to, &HashSet::from([id(8000)]), &HashSet::new());
        let single = rules::move_selection(&inv, &ids[..1], to, &HashSet::from([id(8000)]), &HashSet::new());
        assert_eq!(locked[0].skip.is_none(), single.is_ok_and(|c| !c.operations.is_empty()));
    }

    #[test]
    fn execute_refuses_a_preview_the_inventory_no_longer_matches() {
        let mut inv = fixture();
        let mut st = InventoryUi::default();
        demo(&mut st, &inv);
        let mut review = st.bulk_review.take().expect("open");
        // the first item renamed elsewhere in the meantime
        if let Some(item) = inv.items.get_mut(&id(8100)) {
            item.name = "Renamed meanwhile".into();
        }
        let (mut actions, mut close) = (Vec::new(), false);
        execute(
            &inv,
            &mut review,
            &InventoryPreferences::default(),
            &Facts::default(),
            &mut actions,
            &mut close,
        );
        assert!(actions.is_empty() && !close, "nothing leaves");
        assert!(!review.message.is_empty());
        assert!(review.current().is_some(), "the new preview is shown");
        // previewed again, it goes
        execute(
            &inv,
            &mut review,
            &InventoryPreferences::default(),
            &Facts::default(),
            &mut actions,
            &mut close,
        );
        assert_eq!(actions.len(), 1);
        assert!(close);
    }

    #[test]
    fn a_changed_request_needs_a_new_preview() {
        let inv = fixture();
        let mut st = InventoryUi::default();
        demo(&mut st, &inv);
        let mut review = st.bulk_review.take().expect("open");
        assert!(review.current().is_some());
        review.pattern.push('!');
        assert!(review.current().is_none());
        review.pattern.pop();
        review.kind = Kind::Move;
        assert!(review.current().is_none());
    }

    #[test]
    fn only_one_to_fifty_objects_open_a_review() {
        let mut st = InventoryUi::default();
        open(&mut st, Vec::new());
        assert!(st.bulk_review.is_none());
        open(&mut st, (0..51).map(|n| id(10_000 + n)).collect());
        assert!(st.bulk_review.is_none());
        open(&mut st, vec![id(8100)]);
        assert!(st.bulk_review.is_some());
    }
}
