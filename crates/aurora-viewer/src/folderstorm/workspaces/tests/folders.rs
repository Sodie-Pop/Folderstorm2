//! Frame tests of checkpoint 6, part 2: a workspace that remembers
//! Inventory folders reopens their windows at their places, for the
//! account that saved them; a deleted folder, another account and RLVa
//! skip with a note. The same pretend viewer as the others.

use super::*;
use crate::folderstorm::workspaces::words::{report_text, workspace_holds};
use aurora_i18n::tr;
use uuid::Uuid;

const ME: Uuid = Uuid::from_u128(7);
const OUTFITS: Uuid = Uuid::from_u128(8000);
const OBJECTS: Uuid = Uuid::from_u128(8001);
const NOTES: Uuid = Uuid::from_u128(8002);

impl Viewer {
    /// A window opened on `folder` by hand, at a place.
    fn open_folder(&mut self, folder: Uuid, x: f32, y: f32) -> String {
        let window = self.inventory.open(folder);
        widgets::place_floater(&self.ctx, &window, pos2(x, y), vec2(420.0, 440.0));
        self.frames(3);
        window
    }

    /// The folders that have a window, in the windows' order.
    fn folders_open(&self) -> Vec<Uuid> {
        self.inventory.windows.iter().map(|(_, folder)| *folder).collect()
    }

    fn folder_rect(&self, folder: Uuid) -> Rect {
        let (window, _) = self.inventory.windows.iter().find(|(_, of)| *of == folder).expect("a window on it");
        self.rect(window)
    }

    fn close_folders(&mut self) {
        self.inventory.windows.clear();
        self.frames(3);
    }

    fn save_with_folders(&mut self, name: &str) -> Result<usize, StoreError> {
        let ctx = self.ctx.clone();
        let keep = apply::Keep {
            layout: false,
            folders: true,
        };
        apply::save_new_workspace(&ctx, &mut self.live(), name, keep)
    }

    fn apply_named(&mut self, name: &str) -> apply::Report {
        let ctx = self.ctx.clone();
        let (index, _) = self.store.find_workspace(name).expect("in the list");
        apply::apply_workspace(&ctx, &mut self.live(), index).expect("applied");
        self.settle();
        apply::report(&self.ctx).expect("an apply was made")
    }
}

fn at(x: f32, y: f32) -> Rect {
    Rect::from_min_size(pos2(x, y), vec2(420.0, 440.0))
}

/// [`two_layouts`], logged in, with windows on Outfits and Objects saved
/// in the workspace "Sorting" (*Remember Inventory folders*), and a
/// workspace "Plain" that does not remember folders.
fn with_folders() -> Viewer {
    let mut v = two_layouts();
    v.inventory.account = Some(ME);
    v.inventory.known = BTreeSet::from([OUTFITS, OBJECTS, NOTES]);
    v.frames(2);
    let keep = apply::Keep::default();
    let ctx = v.ctx.clone();
    apply::save_new_workspace(&ctx, &mut v.live(), "Plain", keep).expect("saved");
    v.open_folder(OUTFITS, 40.0, 80.0);
    v.open_folder(OBJECTS, 500.0, 120.0);
    v.save_with_folders("Sorting").expect("saved");
    v.settle();
    v
}

#[test]
fn remember_inventory_folders_records_each_window_s_folder_and_place() {
    let v = with_folders();
    let sorting = v.store.find_workspace("Sorting").expect("saved").1;
    let folders = sorting.folders.as_ref().expect("remembered");
    assert_eq!(folders.account, ME);
    assert_eq!(folders.windows.iter().map(|w| w.folder).collect::<Vec<_>>(), [OUTFITS, OBJECTS]);
    let first = folders.windows[0].place;
    assert_eq!((first.x, first.y, first.width, first.height), (40.0, 80.0, 420.0, 440.0));
    assert_eq!(v.store.find_workspace("Plain").expect("saved").1.folders, None, "only when asked");
    assert!(workspace_holds(sorting).contains(&tr!("fs-workspaces-holds-folders", count = 2)));
    // not logged in: nothing to remember, and said
    let mut out = two_layouts();
    assert_eq!(out.save_with_folders("Nobody"), Err(StoreError::Invalid));
}

#[test]
fn applying_reopens_the_folder_windows_at_their_places_and_closes_the_others() {
    let mut v = with_folders();
    v.close_folders();
    // one already open on Objects, elsewhere, and one the workspace does
    // not have
    let kept = v.open_folder(OBJECTS, 900.0, 300.0);
    v.open_folder(NOTES, 300.0, 300.0);
    let done = v.apply_named("Sorting");
    assert_eq!(v.folders_open(), [OBJECTS, OUTFITS]);
    assert!(
        v.inventory.windows.iter().any(|(window, _)| *window == kept),
        "the open one is reused"
    );
    assert!(close(v.folder_rect(OUTFITS), at(40.0, 80.0)), "{:?}", v.folder_rect(OUTFITS));
    assert!(close(v.folder_rect(OBJECTS), at(500.0, 120.0)), "{:?}", v.folder_rect(OBJECTS));
    assert_eq!(
        (done.folders_missing, done.folders_blocked, done.folders_other_account),
        (0, 0, false)
    );
    assert!(done.opened >= 1 && done.closed >= 1, "{done:?}");
    // nothing to do the second time
    let again = v.apply_named("Sorting");
    assert_eq!((again.opened, again.closed), (0, 0));
    assert_eq!(v.inventory.opened, 5, "no window opened again");
}

#[test]
fn a_workspace_that_does_not_remember_folders_leaves_their_windows_alone() {
    let mut v = with_folders();
    let before = v.folders_open();
    let done = v.apply_named("Plain");
    assert_eq!(v.folders_open(), before);
    assert!(!done.folders_other_account && done.folders_missing == 0);
}

#[test]
fn a_deleted_folder_is_skipped_with_a_note_and_the_rest_is_restored() {
    let mut v = with_folders();
    v.close_folders();
    v.inventory.known.remove(&OUTFITS);
    let done = v.apply_named("Sorting");
    assert_eq!(v.folders_open(), [OBJECTS]);
    assert_eq!(done.folders_missing, 1);
    assert!(report_text(&done, 0).contains(&tr!("fs-workspaces-folders-missing", count = 1)));
    assert_eq!(
        v.store
            .find_workspace("Sorting")
            .expect("saved")
            .1
            .folders
            .as_ref()
            .map(|f| f.windows.len()),
        Some(2),
        "the workspace keeps it: the Inventory may only not be loaded yet"
    );
}

#[test]
fn another_account_skips_the_folders_with_a_note() {
    let mut v = with_folders();
    v.close_folders();
    v.inventory.account = Some(Uuid::from_u128(9));
    v.open_folder(NOTES, 300.0, 300.0);
    let done = v.apply_named("Sorting");
    assert_eq!(v.folders_open(), [NOTES], "none opened, none closed");
    assert!(done.folders_other_account);
    assert!(report_text(&done, 0).contains(tr!("fs-workspaces-folders-other-account")));
    // its windows are switched all the same
    assert_eq!(v.store.active_workspace.as_deref(), Some("Sorting"));
}

#[test]
fn rlva_hiding_the_inventory_blocks_the_folder_windows() {
    let mut v = with_folders();
    v.close_folders();
    v.blocked.insert(INVENTORY);
    let done = v.apply_named("Sorting");
    assert!(v.folders_open().is_empty());
    assert_eq!(done.folders_blocked, 2);
    assert!(report_text(&done, 0).contains(tr!("fs-workspaces-folders-blocked")));
}

#[test]
fn previous_puts_the_folder_windows_back() {
    let mut v = with_folders();
    v.close_folders();
    v.open_folder(NOTES, 300.0, 300.0);
    v.apply_named("Sorting");
    assert_eq!(v.folders_open(), [OUTFITS, OBJECTS]);
    assert!(v.previous());
    v.settle();
    assert_eq!(v.folders_open(), [NOTES]);
    assert!(close(v.folder_rect(NOTES), at(300.0, 300.0)));
    assert!(v.previous());
    v.settle();
    assert_eq!(v.folders_open().len(), 2);
}

#[test]
fn update_takes_the_folder_windows_of_now_and_the_box_forgets_them() {
    let mut v = with_folders();
    v.close_folders();
    v.open_folder(NOTES, 300.0, 300.0);
    let ctx = v.ctx.clone();
    let (index, _) = v.store.find_workspace("Sorting").expect("saved");
    apply::update_workspace(&ctx, &mut v.live(), index).expect("updated");
    let folders = |v: &Viewer| -> Option<Vec<Uuid>> {
        let saved = v.store.find_workspace("Sorting").expect("saved").1;
        saved.folders.as_ref().map(|f| f.windows.iter().map(|w| w.folder).collect())
    };
    assert_eq!(folders(&v), Some(vec![NOTES]));
    assert_eq!(apply::folder_windows_now(&ctx), 1);
    assert!(apply::remember_folders(&ctx, &mut v.live(), index, false));
    assert_eq!(folders(&v), None);
    // a workspace that does not remember them is not given any by Update
    apply::update_workspace(&ctx, &mut v.live(), index).expect("updated");
    assert_eq!(folders(&v), None);
    assert!(apply::remember_folders(&ctx, &mut v.live(), index, true));
    assert_eq!(folders(&v), Some(vec![NOTES]));
}

#[test]
fn at_most_sixteen_folder_windows_are_remembered() {
    let mut v = with_folders();
    for n in 0..20u128 {
        let folder = Uuid::from_u128(9000 + n);
        v.inventory.known.insert(folder);
        v.inventory.open(folder);
    }
    v.frames(3);
    v.save_with_folders("Many").expect("saved");
    let many = v.store.find_workspace("Many").expect("saved").1;
    assert_eq!(many.folders.as_ref().map(|f| f.windows.len()), Some(16));
}
