//! Frame tests of checkpoint 6: a workspace opens its windows and closes
//! the others, with its layout or on the one in use; *Previous*; the
//! "modified" mark; save and update; the requests; the switcher and World
//! › Layouts with both kinds. The same pretend viewer as the others.

use super::*;
use crate::folderstorm::workspaces::menu::{self, Request};
use crate::folderstorm::workspaces::switcher;
use crate::folderstorm::workspaces::words::report_text;
use aurora_i18n::tr;

const CONVERSATIONS: &str = "conversations";
const PLACES: &str = "places";

impl Viewer {
    fn save_workspace(&mut self, name: &str, with_layout: bool) -> Result<usize, StoreError> {
        let ctx = self.ctx.clone();
        apply::save_new_workspace(&ctx, &mut self.live(), name, with_layout)
    }

    fn apply_workspace(&mut self, name: &str) -> apply::Report {
        let ctx = self.ctx.clone();
        let (index, _) = self.store.find_workspace(name).expect("in the list");
        apply::apply_workspace(&ctx, &mut self.live(), index).expect("applied")
    }

    /// The windows open, but Preferences (not a window of the allowlist).
    fn open_now(&self) -> Vec<&'static str> {
        self.open.iter().copied().filter(|id| *id != PREFERENCES).collect()
    }

    fn set_open(&mut self, open: &[&'static str]) {
        self.open = open.iter().copied().chain([PREFERENCES]).collect();
        self.frames(3);
    }

    fn workspace_modified(&self) -> bool {
        apply::workspace_modified(&self.store, &apply::open_now(&self.ctx))
    }
}

/// [`two_layouts`] ("Single" in use) and two workspaces: "Exploring" (the
/// maps and Places, on any layout) and "Sorting" (Inventory and People,
/// with the layout "Single"), saved with Conversations open. As left:
/// Inventory, People, Places and Conversations open, "Sorting" in use.
fn two_workspaces() -> Viewer {
    let mut v = two_layouts();
    v.set_open(&[MAP, MINIMAP, PLACES]);
    v.save_workspace("Exploring", false).expect("saved");
    v.set_open(&[INVENTORY, PEOPLE, CONVERSATIONS]);
    v.save_workspace("Sorting", true).expect("saved");
    v.set_open(&[INVENTORY, PEOPLE, PLACES, CONVERSATIONS]);
    v.settle();
    v.saved = false;
    v
}

fn report(v: &Viewer) -> apply::Report {
    apply::report(&v.ctx).expect("an apply was made")
}

#[test]
fn a_workspace_records_the_open_windows_and_never_conversations() {
    let v = two_workspaces();
    let open = |name: &str| -> Vec<&str> {
        let (_, workspace) = v.store.find_workspace(name).expect("saved");
        workspace.open.iter().map(String::as_str).collect()
    };
    assert_eq!(open("Exploring"), [MINIMAP, PLACES, MAP]);
    assert_eq!(open("Sorting"), [INVENTORY, PEOPLE], "not Conversations, not Preferences");
    let sorting = v.store.find_workspace("Sorting").expect("saved").1;
    assert_eq!(sorting.layout.as_deref(), Some("Single"), "saved with the layout in use");
    assert_eq!(v.store.find_workspace("Exploring").expect("saved").1.layout, None);
    assert_eq!(v.store.active_workspace.as_deref(), Some("Sorting"), "the one saved last is in use");
    let saved = serde_json::to_value(&v.store).expect("saved");
    assert_eq!(saved["workspaces"][1]["open"], serde_json::json!([INVENTORY, PEOPLE]));
}

#[test]
fn a_workspace_opens_its_windows_and_closes_the_others() {
    let mut v = two_workspaces();
    let places = [v.rect(INVENTORY), v.rect(PEOPLE)];
    v.apply_workspace("Exploring");
    v.settle();
    assert_eq!(
        v.open_now(),
        [CONVERSATIONS, MINIMAP, PLACES, MAP],
        "Conversations is left as it is"
    );
    assert!(v.open.contains(PREFERENCES), "not a window a workspace switches");
    let done = report(&v);
    assert_eq!((done.opened, done.closed, done.blocked), (2, 2, 0));
    assert_eq!((done.workspace.as_deref(), done.name.as_deref()), (Some("Exploring"), None));
    let text = report_text(&done, 0);
    assert!(text.starts_with(&tr!("fs-workspaces-applied", name = "Exploring")), "{text}");
    assert!(text.contains(&tr!("fs-workspaces-applied-opened", count = 2)), "{text}");
    assert!(text.contains(&tr!("fs-workspaces-applied-closed", count = 2)), "{text}");
    assert_eq!(v.store.active_workspace.as_deref(), Some("Exploring"));
    assert_eq!(v.active_layout(), Some("Single"), "it names no layout: the one in use stays");
    assert!(!v.workspace_modified());
    assert!(!v.modified(), "closing windows does not change the layout");
    // and back: the windows open where they were
    v.apply_workspace("Sorting");
    v.settle();
    assert_eq!(v.open_now(), [CONVERSATIONS, INVENTORY, PEOPLE]);
    assert!(close(v.rect(INVENTORY), places[0]) && close(v.rect(PEOPLE), places[1]));
    // nothing to do the second time
    v.apply_workspace("Sorting");
    v.settle();
    let again = report(&v);
    assert_eq!((again.opened, again.closed), (0, 0));
}

#[test]
fn conversations_closed_stays_closed() {
    let mut v = two_workspaces();
    v.set_open(&[MAP]);
    v.apply_workspace("Sorting");
    v.settle();
    assert_eq!(v.open_now(), [INVENTORY, PEOPLE], "saved with Conversations open: not opened");
}

#[test]
fn a_window_that_cannot_be_opened_is_counted_and_the_rest_is_done() {
    let mut v = two_workspaces();
    v.apply_workspace("Exploring");
    v.settle();
    v.blocked.insert(INVENTORY);
    v.apply_workspace("Sorting");
    v.settle();
    assert_eq!(v.open_now(), [CONVERSATIONS, PEOPLE]);
    let done = report(&v);
    assert_eq!((done.opened, done.blocked, done.closed), (1, 1, 3));
    assert!(report_text(&done, 0).contains(&tr!("fs-workspaces-applied-blocked", count = 1)));
    assert!(v.workspace_modified(), "what is open is not its list");
}

#[test]
fn a_workspace_with_a_layout_applies_it_and_its_list_wins() {
    let mut v = two_workspaces();
    v.apply("Wide");
    v.settle();
    assert!(v.world.enabled);
    // "Single" was saved with the map and the mini-map open: "Sorting"
    // does not list them
    v.set_open(&[MAP, MINIMAP]);
    let asked = v.apply_workspace("Sorting");
    assert_eq!(asked.name.as_deref(), Some("Single"));
    v.settle();
    assert_eq!(v.active_layout(), Some("Single"));
    assert!(!v.world.enabled, "the layout's world");
    assert_eq!(v.open_now(), [INVENTORY, PEOPLE], "the map of the layout is not opened");
    assert!(
        close(v.rect(INVENTORY), Rect::from_min_size(pos2(1200.0, 60.0), vec2(380.0, 500.0))),
        "{:?}",
        v.rect(INVENTORY)
    );
    assert!(close(v.rect(PEOPLE), Rect::from_min_size(pos2(20.0, 60.0), vec2(300.0, 420.0))));
    let done = report(&v);
    assert_eq!((done.opened, done.closed, done.blocked), (2, 2, 0));
    let text = report_text(&done, v.store.pending.len());
    assert!(text.starts_with(&tr!("fs-workspaces-applied", name = "Sorting")), "{text}");
    assert!(text.contains("“Single”"), "{text}");
    assert!(!v.modified() && !v.workspace_modified());
}

#[test]
fn a_layout_alone_keeps_the_workspace_in_use_and_its_windows() {
    let mut v = two_workspaces();
    v.apply_workspace("Exploring");
    v.settle();
    saved_closed(&mut v, "Wide", INVENTORY);
    saved_closed(&mut v, "Wide", PEOPLE);
    v.apply("Wide");
    v.settle();
    assert_eq!(v.store.active_workspace.as_deref(), Some("Exploring"));
    assert_eq!(v.open_now(), [CONVERSATIONS, MINIMAP, PLACES, MAP], "a layout closes nothing");
    assert!(!v.workspace_modified());
}

#[test]
fn previous_puts_the_windows_back_open_or_closed_and_comes_back() {
    let mut v = two_workspaces();
    let before = v.open_now();
    v.apply_workspace("Exploring");
    v.settle();
    assert!(v.previous());
    v.settle();
    assert_eq!(v.open_now(), before, "as before the workspace");
    assert_eq!(v.store.active_workspace.as_deref(), Some("Sorting"));
    let done = report(&v);
    assert_eq!((done.workspace.as_deref(), done.name.as_deref()), (None, None));
    assert!(report_text(&done, 0).contains(&tr!("fs-workspaces-applied-closed", count = 2)));
    // again: the workspace
    assert!(v.previous());
    v.settle();
    assert_eq!(v.open_now(), [CONVERSATIONS, MINIMAP, PLACES, MAP]);
    assert_eq!(v.store.active_workspace.as_deref(), Some("Exploring"));
}

#[test]
fn opening_or_closing_a_window_marks_the_workspace_modified_until_an_update() {
    let mut v = two_workspaces();
    assert!(v.workspace_modified(), "Places was opened since it was saved");
    v.apply_workspace("Sorting");
    v.settle();
    assert!(!v.workspace_modified());
    v.set_open(&[INVENTORY, CONVERSATIONS]);
    assert!(v.workspace_modified(), "People was closed");
    v.set_open(&[INVENTORY, PEOPLE]);
    assert!(!v.workspace_modified(), "Conversations does not count");
    v.set_open(&[INVENTORY, PEOPLE, MAP]);
    let ctx = v.ctx.clone();
    apply::update_workspace(&ctx, &mut v.live(), 1).expect("updated");
    assert!(!v.workspace_modified());
    let sorting = v.store.find_workspace("Sorting").expect("saved").1;
    assert!(sorting.open.contains(MAP));
    assert_eq!(sorting.layout.as_deref(), Some("Single"), "its layout choice is kept");
}

#[test]
fn a_workspace_whose_layout_cannot_be_used_keeps_the_layout_in_use() {
    let mut v = two_workspaces();
    // "Single" as a newer version would have written it
    let mut file = serde_json::to_value(&v.store).expect("saved");
    file["layouts"][1]["schema"] = serde_json::json!(9);
    v.store = serde_json::from_value(file).expect("read");
    assert_eq!(v.store.find_workspace("Sorting").expect("read").1.layout.as_deref(), Some("Single"));
    v.set_open(&[MAP]);
    let asked = v.apply_workspace("Sorting");
    assert!(asked.no_layout && asked.name.is_none());
    v.settle();
    assert_eq!(v.open_now(), [INVENTORY, PEOPLE], "its windows all the same");
    assert!(report_text(&report(&v), 0).contains(tr!("fs-workspaces-applied-no-layout")));
}

#[test]
fn the_requests_apply_star_update_and_save_a_workspace() {
    let mut v = two_workspaces();
    v.ask(Request::ApplyWorkspace("exploring".to_owned()));
    v.settle();
    assert_eq!(v.store.active_workspace.as_deref(), Some("Exploring"));
    assert!(v.saved && !v.open.contains(INVENTORY));
    v.ask(Request::StarWorkspace("Exploring".to_owned(), true));
    assert!(v.store.is_favorite_workspace("Exploring") && !v.store.is_favorite("Exploring"));
    v.set_open(&[MAP]);
    v.ask(Request::UpdateWorkspace("Exploring".to_owned()));
    assert_eq!(v.store.find_workspace("Exploring").expect("saved").1.open.len(), 1);
    v.ask(Request::SaveWorkspace("Map only".to_owned(), true));
    let (_, saved) = v.store.find_workspace("Map only").expect("saved");
    assert_eq!(saved.layout.as_deref(), v.active_layout());
    // a name taken is refused, and said
    v.ask(Request::SaveWorkspace("map ONLY".to_owned(), false));
    assert_eq!(v.store.workspaces.len(), 3);
    assert!(apply::refused(&v.ctx).is_some());
    // one that is gone: nothing
    v.saved = false;
    v.ask(Request::ApplyWorkspace("Gone".to_owned()));
    assert!(!v.saved);
}

#[test]
fn the_switcher_lists_workspaces_then_layouts_and_enter_applies() {
    let mut v = two_workspaces();
    v.open_switcher();
    assert!(v.shows(tr!("fs-switcher-title")));
    assert!(v.shows(&tr!("fs-switcher-workspaces").to_uppercase()));
    assert!(v.shows(&tr!("fs-switcher-layouts").to_uppercase()));
    assert!(!v.shows(&tr!("fs-switcher-favorites").to_uppercase()));
    let top = |v: &Viewer, text: &str| v.text_rect(text).top();
    assert!(top(&v, "Exploring") < top(&v, "Sorting") && top(&v, "Sorting") < top(&v, "Wide"));
    // the first that is not in use is "Exploring": what it does is said
    let exploring = v.store.find_workspace("Exploring").expect("saved").1.clone();
    assert!(v.shows(&crate::folderstorm::workspaces::words::workspace_holds(&exploring)));
    assert!(v.shows(tr!("fs-workspaces-closes")));
    v.key(egui::Key::Enter);
    v.settle();
    assert!(!switcher::is_open(&v.ctx), "Enter closes");
    assert_eq!(v.store.active_workspace.as_deref(), Some("Exploring"));
    assert_eq!(v.open_now(), [CONVERSATIONS, MINIMAP, PLACES, MAP]);
    // typed: "sort" finds the workspace, Enter applies it with its layout
    v.apply("Wide");
    v.settle();
    v.open_switcher();
    v.type_text("sort");
    v.key(egui::Key::Enter);
    v.settle();
    assert_eq!(v.store.active_workspace.as_deref(), Some("Sorting"));
    assert_eq!(v.active_layout(), Some("Single"));
    assert_eq!(v.open_now(), [CONVERSATIONS, INVENTORY, PEOPLE]);
}

#[test]
fn the_switcher_stars_updates_and_saves_a_workspace() {
    let mut v = two_workspaces();
    v.open_switcher();
    v.click_star("Exploring");
    assert!(v.store.is_favorite_workspace("Exploring"));
    assert!(v.shows(&tr!("fs-switcher-favorites").to_uppercase()));
    // "Sorting" selected, in use and modified (Places is open): Update is its
    v.click_at(v.text_rect("Sorting").center());
    assert!(v.shows(tr!("fs-layouts-modified")));
    v.click(&tr!("fs-switcher-update", name = "Sorting"));
    assert!(v.shows(&tr!("fs-workspaces-ask-update", name = "Sorting")));
    v.click(tr!("fs-layouts-update"));
    v.frames(3);
    assert!(v.store.find_workspace("Sorting").expect("saved").1.open.contains(PLACES));
    assert!(!v.workspace_modified());
    // Save new…: a workspace is offered, a workspace being selected
    v.click(tr!("fs-switcher-save-new"));
    assert!(v.shows(tr!("fs-workspaces-save-help")));
    assert!(v.shows(&tr!("fs-workspaces-with-layout", name = "Single")));
    v.type_text("sorting");
    v.key(egui::Key::Enter);
    assert!(v.shows(tr!("fs-workspaces-error-taken")), "refused in the window");
    v.click(tr!("common-cancel"));
    v.click(tr!("fs-switcher-save-new"));
    v.type_text("sorting 2");
    v.key(egui::Key::Enter);
    v.frames(3);
    let (_, saved) = v.store.find_workspace("sorting 2").expect("saved");
    assert_eq!(saved.layout, None, "the box was not ticked");
    assert_eq!(v.store.layouts.len(), 2, "not a layout");
    // the other kind, from the same form
    v.click(tr!("fs-switcher-save-new"));
    v.click(tr!("fs-workspaces-save-layout"));
    assert!(v.shows(tr!("fs-layouts-part-windows")));
}

#[test]
fn world_layouts_lists_the_workspaces_too() {
    let mut v = two_workspaces();
    v.menu = true;
    v.frames(3);
    for name in ["Wide", "Single", "Exploring", "Sorting"] {
        assert!(v.shows(name), "{name}");
    }
    v.click("Exploring");
    v.settle();
    assert_eq!(v.store.active_workspace.as_deref(), Some("Exploring"));
    assert!(!v.open.contains(INVENTORY));
    // with a favorite, the favorites only
    v.ask(Request::StarWorkspace("Sorting".to_owned(), true));
    assert!(v.shows("Sorting") && !v.shows("Exploring") && !v.shows("Wide"));
    let _ = menu::view(&v.ctx);
}
