//! Frame tests of row 1.7: a workspace's context is handed to the app once,
//! after its windows; what the app did goes in the report; *Previous
//! arrangement* hands back what it changed from, and comes back; a layout
//! alone never touches it; what Preferences shows of HUDs. The app's own
//! side (selecting a profile, moving the camera, wearing) is pretended
//! here: `app/folderstorm_workspaces/context.rs` does it with the viewer.

use super::*;
use crate::camera::CameraPreset;
use crate::folderstorm::workspaces::context::{
    self, Ask, Before, Camera, CameraView, Context, Done, GraphicsBefore, Hud, HudCounts, Huds, Part, Told,
};
use crate::folderstorm::workspaces::words::report_text;
use crate::folderstorm::workspaces::workspace::Workspace;
use aurora_i18n::tr;
use uuid::Uuid;

const ME: Uuid = Uuid::from_u128(7);

fn hud(n: u128, point: u8) -> Hud {
    Hud {
        item: Uuid::from_u128(9000 + n),
        point,
    }
}

fn photos_context() -> Context {
    Context {
        graphics: Some("My photo profile".to_owned()),
        camera: Some(Camera::View(CameraPreset::Front)),
        huds: Some(Huds {
            account: ME,
            items: vec![hud(1, 35), hud(2, 31)],
        }),
    }
}

/// [`two_layouts`] ("Single" in use) and two workspaces: "Photos" (the
/// world map, the layout "Wide", a graphics profile, the Front view and
/// two HUDs) and "Plain" (Inventory, no context). Logged in as `ME`.
fn with_context() -> Viewer {
    let mut v = two_layouts();
    v.inventory.account = Some(ME);
    let mut photos = Workspace::new("Photos", Some("Wide"), [MAP.to_owned()]);
    let context = photos_context();
    photos.graphics = context.graphics;
    photos.camera = context.camera;
    photos.huds = context.huds;
    v.store.add_workspace(photos).expect("added");
    v.store
        .add_workspace(Workspace::new("Plain", None, [INVENTORY.to_owned()]))
        .expect("added");
    v.store.active_workspace = None;
    v.settle();
    v.asked.clear();
    v
}

/// What the app would say after doing `ask`, changing from `before`.
fn done(told: Told, before: Before) -> Done {
    Done { told, before }
}

fn graphics_before(name: &str) -> GraphicsBefore {
    GraphicsBefore {
        values: Default::default(),
        selected: Some(name.to_owned()),
    }
}

impl Viewer {
    fn apply_workspace_named(&mut self, name: &str) -> apply::Report {
        let ctx = self.ctx.clone();
        let (index, _) = self.store.find_workspace(name).expect("in the list");
        apply::apply_workspace(&ctx, &mut self.live(), index).expect("applied")
    }

    /// The app says what it did.
    fn tell(&mut self, done: Done) {
        apply::context_done(&self.ctx, done);
    }
}

#[test]
fn a_workspace_s_context_is_handed_to_the_app_once() {
    let mut v = with_context();
    let report = v.apply_workspace_named("Photos");
    assert_eq!(report.context, None, "the app has not said anything yet");
    v.frames(1);
    assert_eq!(v.asked, [Ask::Apply(photos_context())], "in the next frame");
    v.settle();
    assert_eq!(v.asked.len(), 1, "once");
    // its windows and its layout as before
    assert_eq!(v.store.active_layout.as_deref(), Some("Wide"));
    assert!(v.open.contains(MAP) && !v.open.contains(INVENTORY));
    // a workspace without context hands nothing
    v.asked.clear();
    v.apply_workspace_named("Plain");
    v.settle();
    assert!(v.asked.is_empty());
}

#[test]
fn a_workspace_applied_from_the_requests_hands_its_context_in_the_same_frame() {
    let mut v = with_context();
    crate::folderstorm::workspaces::menu::ask(
        &v.ctx,
        crate::folderstorm::workspaces::menu::Request::ApplyWorkspace("photos".to_owned()),
    );
    v.frames(1);
    assert_eq!(v.asked, [Ask::Apply(photos_context())]);
}

#[test]
fn the_report_says_what_the_app_did() {
    let mut v = with_context();
    v.apply_workspace_named("Photos");
    v.frames(1);
    v.tell(done(
        Told {
            graphics: Some(("My photo profile".to_owned(), Part::Done)),
            camera: Some((Camera::View(CameraPreset::Front), Part::Locked)),
            huds: Some(HudCounts {
                asked: 1,
                worn: 1,
                ..Default::default()
            }),
            restored: false,
        },
        Before::default(),
    ));
    let report = apply::report(&v.ctx).expect("applied");
    let text = report_text(&report, 0);
    for sentence in [
        tr!("fs-workspaces-applied", name = "Photos"),
        tr!("fs-context-graphics-done", name = "My photo profile"),
        tr!("fs-context-camera-locked").to_owned(),
        tr!("fs-context-huds-asked", count = 1),
        tr!("fs-context-huds-worn", count = 1),
    ] {
        assert!(text.contains(&sentence), "“{sentence}” in “{text}”");
    }
    assert!(!text.contains(&tr!("fs-context-restored").to_owned()));
}

#[test]
fn previous_hands_back_what_the_context_changed_from_and_comes_back() {
    let mut v = with_context();
    v.apply_workspace_named("Photos");
    v.frames(1);
    let view = CameraView::of(&Default::default());
    let before = Before {
        graphics: Some(graphics_before("Everyday")),
        camera: Some(view),
    };
    v.tell(done(
        Told {
            graphics: Some(("My photo profile".to_owned(), Part::Done)),
            ..Default::default()
        },
        before.clone(),
    ));
    v.settle();
    v.asked.clear();
    assert!(v.previous());
    v.frames(1);
    assert_eq!(v.asked, [Ask::Restore(before.clone())], "the graphics and the camera of then");
    // the app puts them back, and says what it changed from
    let again = Before {
        graphics: Some(graphics_before("My photo profile")),
        camera: None,
    };
    v.tell(done(
        Told {
            restored: true,
            ..Default::default()
        },
        again.clone(),
    ));
    let text = report_text(&apply::report(&v.ctx).expect("a report"), 0);
    assert!(text.contains(tr!("fs-context-restored")), "{text}");
    v.settle();
    assert!(v.open.contains(INVENTORY) && v.store.active_layout.as_deref() == Some("Single"));
    v.asked.clear();
    // and Previous again comes back to the workspace's
    assert!(v.previous());
    v.frames(1);
    assert_eq!(v.asked, [Ask::Restore(again)]);
}

#[test]
fn a_context_that_changed_nothing_leaves_previous_to_the_windows() {
    let mut v = with_context();
    v.apply_workspace_named("Photos");
    v.frames(1);
    // the profile was gone, the camera locked: nothing to put back
    v.tell(done(
        Told {
            graphics: Some(("My photo profile".to_owned(), Part::Missing)),
            camera: Some((Camera::View(CameraPreset::Front), Part::Mouselook)),
            ..Default::default()
        },
        Before::default(),
    ));
    let text = report_text(&apply::report(&v.ctx).expect("a report"), 0);
    assert!(
        text.contains(&tr!("fs-context-graphics-missing", name = "My photo profile")),
        "{text}"
    );
    assert!(text.contains(tr!("fs-context-camera-mouselook")), "{text}");
    v.settle();
    v.asked.clear();
    assert!(v.previous());
    v.settle();
    assert!(v.asked.is_empty(), "no context to put back");
    assert!(v.open.contains(INVENTORY), "the windows are back");
}

#[test]
fn a_layout_alone_never_touches_the_context() {
    let mut v = with_context();
    v.apply_workspace_named("Photos");
    v.frames(1);
    v.tell(done(
        Told::default(),
        Before {
            graphics: Some(graphics_before("Everyday")),
            camera: None,
        },
    ));
    v.settle();
    v.asked.clear();
    v.apply("Single");
    v.settle();
    assert!(v.asked.is_empty());
    // its Previous goes back to "Wide" alone
    assert!(v.previous());
    v.settle();
    assert!(v.asked.is_empty());
}

#[test]
fn a_new_apply_cancels_a_context_not_handed_yet() {
    let mut v = with_context();
    v.apply_workspace_named("Photos");
    // in the same frame, a layout: the workspace's context is not done
    v.apply("Single");
    v.settle();
    assert!(v.asked.is_empty());
}

#[test]
fn preferences_are_told_the_huds_worn_and_the_names_of_this_account_s_items() {
    let mut v = with_context();
    v.huds = vec![hud(3, 33)];
    v.names.insert(hud(1, 35).item, "Radar".to_owned());
    v.names.insert(hud(3, 33).item, "Camera HUD".to_owned());
    v.frames(1);
    let view = context::view(&v.ctx);
    assert_eq!(view.account, Some(ME));
    assert_eq!(view.worn, [(hud(3, 33), "Camera HUD".to_owned())]);
    assert_eq!(
        view.names.iter().collect::<Vec<_>>(),
        [(&hud(1, 35).item, &"Radar".to_owned())],
        "the workspaces' items the Inventory has"
    );
    // another account: its items are not looked up
    v.inventory.account = Some(Uuid::from_u128(8));
    v.frames(1);
    let view = context::view(&v.ctx);
    assert!(view.names.is_empty());
    assert_eq!(view.worn.len(), 1);
}

#[test]
fn update_keeps_the_context_as_chosen() {
    let mut v = with_context();
    v.apply_workspace_named("Photos");
    v.settle();
    v.open.insert(PEOPLE);
    v.frames(3);
    let ctx = v.ctx.clone();
    let (index, _) = v.store.find_workspace("Photos").expect("listed");
    apply::update_workspace(&ctx, &mut v.live(), index).expect("updated");
    let photos = v.store.find_workspace("Photos").expect("listed").1;
    assert!(photos.open.contains(PEOPLE));
    assert_eq!(photos.context(), photos_context());
}
