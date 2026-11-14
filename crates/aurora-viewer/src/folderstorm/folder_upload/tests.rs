use super::plan::{self, Preview, Receipt, Routing, Row, RowState, Skipped, Tracker};
use super::ui::{self, Context, Tone};
use super::*;
use crate::imports::folders::Layout;
use aurora_net::inventory::upload::Kind;
use std::path::Path;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fs-folder-upload-{name}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn touch(root: &Path, relative: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    std::fs::write(path, b"x").expect("file");
}

fn row(name: &str, kind: Kind, path: &[&str], state: RowState) -> Row {
    Row {
        id: Uuid::new_v4(),
        name: name.into(),
        kind: Some(kind),
        upload_path: Some(path.iter().map(|part| part.to_string()).collect()),
        selected: true,
        state,
    }
}

// --- file types and the scan ---

#[test]
fn supported_types_follow_the_uploader_s_folder_scan_in_any_case() {
    for name in [
        "a.png", "a.PNG", "b.Jpeg", "c.tga", "d.wav", "e.anim", "f.bvh", "g.dae", "h.glb", "i.gltf", "j.j2c",
    ] {
        assert!(plan::supported(Path::new(name)), "{name}");
    }
    for name in ["a.txt", "b.psd", "c.mp3", "d", "e.png.bak", "f.blend"] {
        assert!(!plan::supported(Path::new(name)), "{name}");
    }
}

#[test]
fn the_scan_lists_unsupported_files_and_folders_that_will_not_be_created() {
    let root = temp("scan");
    touch(&root, "Cover.png");
    touch(&root, "Fabrics/Wood.png");
    touch(&root, "Fabrics/notes.TXT");
    touch(&root, "Deep/Deeper/Chime.wav");
    touch(&root, "Notes/Readme.txt");
    touch(&root, "Notes/Inner/b.psd");
    touch(&root, "Thumbs.db");
    touch(&root, ".DS_Store");
    std::fs::create_dir_all(root.join("Empty")).expect("empty");
    let skipped = plan::scan(&root, plan::SCAN_LIMIT);
    assert_eq!(skipped.files, ["Fabrics/notes.TXT", "Notes/Inner/b.psd", "Notes/Readme.txt"]);
    // Notes, Notes/Inner and Empty hold nothing to upload; Deep holds a file
    // two levels down, so it is created.
    assert_eq!(skipped.empty_folders, 3);
    assert_eq!(skipped.links, 0);
    assert!(!skipped.truncated);
    std::fs::remove_dir_all(root).expect("clean");
}

#[test]
fn the_scan_stops_at_its_limit_and_says_so() {
    let root = temp("limit");
    for i in 0..20 {
        touch(&root, &format!("file{i}.txt"));
    }
    let skipped = plan::scan(&root, 5);
    assert!(skipped.truncated);
    assert!(skipped.files.len() <= 5);
    std::fs::remove_dir_all(root).expect("clean");
}

#[cfg(unix)]
#[test]
fn the_scan_never_follows_links() {
    let root = temp("links");
    let outside = temp("outside");
    touch(&outside, "secret.txt");
    touch(&root, "a.png");
    std::os::unix::fs::symlink(&outside, root.join("linked")).expect("link");
    let skipped = plan::scan(&root, plan::SCAN_LIMIT);
    assert_eq!(skipped.links, 1);
    assert!(skipped.files.is_empty());
    std::fs::remove_dir_all(root).expect("clean");
    std::fs::remove_dir_all(outside).expect("clean");
}

#[test]
fn the_demo_folder_shows_every_case_of_the_preview() {
    let base = temp("demo");
    let folder = demo::fixtures(&base).expect("fixtures");
    assert_eq!(folder.file_name().and_then(|n| n.to_str()), Some(demo::FOLDER));
    let skipped = plan::scan(&folder, plan::SCAN_LIMIT);
    assert_eq!(skipped.files, ["Notes/Readme.txt"]);
    assert_eq!(skipped.empty_folders, 2);
    assert_eq!(demo::Mode::parse("confirm"), Some(demo::Mode::Confirm));
    assert_eq!(demo::Mode::parse("1"), Some(demo::Mode::Preview));
    assert_eq!(demo::Mode::parse("nope"), None);
    std::fs::remove_dir_all(base).expect("clean");
}

// --- routing ---

#[test]
fn routing_is_a_mirror_into_the_folder_and_goes_back_unless_the_user_changed_it() {
    let destination = Uuid::new_v4();
    let mut prefs = crate::imports::Preferences::default();
    prefs.use_default_folders = true;
    prefs.include_source_folder = false;
    prefs.folder_layout = Some(Layout::Type);
    let saved = Routing::read(&prefs);
    let applied = Routing::mirror_into(destination);
    applied.write(&mut prefs);
    assert!(!prefs.use_default_folders && prefs.include_source_folder);
    assert_eq!((prefs.batch_folder, prefs.folder_layout), (destination, Some(Layout::Mirror)));
    assert!(plan::restore(&mut prefs, &saved, &applied));
    assert_eq!(Routing::read(&prefs), saved);

    applied.write(&mut prefs);
    prefs.folder_layout = Some(Layout::Flat); // the user's own choice in the uploader
    assert!(!plan::restore(&mut prefs, &saved, &applied));
    assert_eq!(prefs.folder_layout, Some(Layout::Flat));
}

// --- preview ---

#[test]
fn the_preview_counts_files_types_and_new_folders_of_the_selected_ready_drafts() {
    let mut unselected = row("Hidden", Kind::Texture, &["Kit", "Other"], RowState::Ready);
    unselected.selected = false;
    let rows = vec![
        row("Cover", Kind::Texture, &["Kit"], RowState::Ready),
        row("Wood", Kind::Texture, &["Kit", "Fabrics"], RowState::Ready),
        row("Chime", Kind::Sound, &["Kit", "Sounds", "Bells"], RowState::Ready),
        row("Broken", Kind::Animation, &["Kit"], RowState::Error("bad".into())),
        unselected,
    ];
    let preview = Preview::build(&rows, &[], Some(30), Some(100));
    assert_eq!(preview.files, 3);
    assert_eq!(preview.by_kind, [(Kind::Texture, 2), (Kind::Sound, 1)]);
    // Kit, Kit/Fabrics, Kit/Sounds, Kit/Sounds/Bells
    assert_eq!(preview.new_folders, 4);
    assert_eq!(preview.errors, 1);
    assert_eq!(preview.balance_after(), Some(70));
    assert!(preview.affordable());
}

#[test]
fn name_collisions_are_reported_not_merged() {
    let rows = vec![
        row("Wood", Kind::Texture, &["Kit", "Fabrics"], RowState::Ready),
        row("wood", Kind::Texture, &["Kit", "Fabrics"], RowState::Ready),
        row("Wood", Kind::Texture, &["Kit", "Other"], RowState::Ready),
        row("Linen", Kind::Texture, &["Kit", "Fabrics"], RowState::Ready),
    ];
    let preview = Preview::build(&rows, &["kit".into(), "Objects".into()], Some(40), Some(1000));
    assert_eq!(preview.existing_names, ["Kit"]);
    assert_eq!(preview.shared_names, ["Wood"]);
    let none = Preview::build(&rows[2..], &["Objects".into()], Some(20), Some(1000));
    assert!(none.existing_names.is_empty() && none.shared_names.is_empty());
}

#[test]
fn the_balance_decides_whether_the_upload_is_affordable() {
    let rows = vec![row("Cover", Kind::Texture, &["Kit"], RowState::Ready)];
    assert!(!Preview::build(&rows, &[], Some(50), Some(40)).affordable());
    assert!(!Preview::build(&rows, &[], Some(50), None).affordable());
    assert!(!Preview::build(&rows, &[], None, Some(400)).affordable());
    assert!(Preview::build(&rows, &[], Some(0), None).affordable());
    assert_eq!(Preview::build(&rows, &[], Some(50), Some(50)).balance_after(), Some(0));
}

// --- receipt ---

#[test]
fn the_receipt_reports_each_file_and_counts_retired_drafts_as_uploaded() {
    let mut rows = vec![
        row("A", Kind::Texture, &["Kit"], RowState::Waiting),
        row("B", Kind::Texture, &["Kit"], RowState::Waiting),
        row("C", Kind::Sound, &["Kit"], RowState::Waiting),
        row("D", Kind::Sound, &["Kit"], RowState::Waiting),
        row("Untouched", Kind::Sound, &["Kit"], RowState::Ready),
    ];
    let mut tracker = Tracker::begin(&rows);
    assert!(!tracker.is_empty());
    rows[0].state = RowState::Done;
    rows[2].state = RowState::Error("refused".into());
    rows[3].state = RowState::Cancelled;
    tracker.observe(&rows);
    // The uploader drops acknowledged drafts from its list: B went from
    // waiting to gone, A from done to gone.
    rows.retain(|row| row.name != "A" && row.name != "B");
    tracker.observe(&rows);
    let receipt = tracker.receipt(20);
    assert_eq!(
        receipt,
        Receipt {
            uploaded: vec!["A".into(), "B".into()],
            failed: vec![("C".into(), "refused".into())],
            cancelled: vec!["D".into()],
            spent: 20,
        }
    );
    assert_eq!(receipt.files(), 4);
}

// --- the run, step by step ---

fn snapshot(agent: Uuid) -> Snapshot {
    Snapshot {
        agent,
        open: true,
        ..Default::default()
    }
}

#[test]
fn a_run_loads_the_chosen_folder_tracks_the_upload_and_restores_the_routing() {
    let (agent, destination) = (Uuid::new_v4(), Uuid::new_v4());
    let mut prefs = crate::imports::Preferences::default();
    let saved = Routing::read(&prefs);
    let mut state = State::default();
    assert_eq!(state.begin(destination, agent, false, &mut prefs), Err(Refusal::Busy));
    assert_eq!(Routing::read(&prefs), saved, "a refused run leaves the settings alone");
    state.begin(destination, agent, true, &mut prefs).expect("begin");
    assert_eq!(Routing::read(&prefs), Routing::mirror_into(destination));
    assert_eq!(state.begin(destination, agent, true, &mut prefs), Err(Refusal::Busy));

    let mut snap = snapshot(agent);
    assert!(state.step(&snap).is_empty(), "waiting for the dialog");
    let folder = PathBuf::from("/disk/Kit");
    let skipped = Skipped {
        files: vec!["a.txt".into()],
        ..Default::default()
    };
    state
        .sender()
        .send(Picked {
            path: Some(folder.clone()),
            skipped: skipped.clone(),
        })
        .expect("send");
    assert_eq!(state.step(&snap), [Effect::Load(folder.clone())]);
    assert!(state.loading(true).is_empty());
    snap.loading = true;
    assert!(state.step(&snap).is_empty());
    assert!(state.loaded().is_none());
    snap.loading = false;
    snap.rows = vec![row("A", Kind::Texture, &["Kit"], RowState::Ready)];
    assert!(state.step(&snap).is_empty());
    let run = state.loaded().expect("loaded");
    assert_eq!((run.destination, run.skipped.clone()), (destination, skipped));

    // Confirmed: the queue runs, then goes idle.
    snap.busy = true;
    snap.rows[0].state = RowState::Waiting;
    assert!(state.step(&snap).is_empty());
    snap.rows.clear();
    snap.busy = false;
    snap.spent = 10;
    let effects = state.step(&snap);
    let [Effect::Receipt { folder: name, receipt }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(name, "Kit");
    assert_eq!((receipt.uploaded.clone(), receipt.spent), (vec!["A".to_string()], 10));

    // Closing the uploader ends the run and puts the routing back.
    snap.open = false;
    let effects = state.step(&snap);
    let [Effect::Restore { saved: s, applied }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert!(plan::restore(&mut prefs, s, applied));
    assert_eq!(Routing::read(&prefs), saved);
    assert!(state.run().is_none());
}

#[test]
fn a_cancelled_dialog_a_refused_load_or_another_account_end_the_run() {
    let (agent, destination) = (Uuid::new_v4(), Uuid::new_v4());
    let mut prefs = crate::imports::Preferences::default();
    let mut state = State::default();

    state.begin(destination, agent, true, &mut prefs).expect("begin");
    state
        .sender()
        .send(Picked {
            path: None,
            skipped: Skipped::default(),
        })
        .expect("send");
    assert!(matches!(state.step(&snapshot(agent)).as_slice(), [Effect::Restore { .. }]));

    state.begin(destination, agent, true, &mut prefs).expect("begin");
    state
        .sender()
        .send(Picked {
            path: Some("/disk/Kit".into()),
            skipped: Skipped::default(),
        })
        .expect("send");
    assert!(matches!(state.step(&snapshot(agent)).as_slice(), [Effect::Load(_)]));
    assert!(matches!(state.loading(false).as_slice(), [Effect::Restore { .. }]));

    state.begin(destination, agent, true, &mut prefs).expect("begin");
    assert!(matches!(state.step(&snapshot(Uuid::new_v4())).as_slice(), [Effect::Restore { .. }]));
    assert!(state.run().is_none());
}

#[test]
fn an_old_dialog_answer_never_starts_a_new_run() {
    let (agent, destination) = (Uuid::new_v4(), Uuid::new_v4());
    let mut prefs = crate::imports::Preferences::default();
    let mut state = State::default();
    state
        .sender()
        .send(Picked {
            path: Some("/disk/Old".into()),
            skipped: Skipped::default(),
        })
        .expect("send");
    state.begin(destination, agent, true, &mut prefs).expect("begin");
    assert!(state.step(&snapshot(agent)).is_empty());
}

// --- texts ---

#[test]
fn long_lists_are_cut_with_a_count_of_the_rest() {
    let names: Vec<String> = (1..=8).map(|i| format!("f{i}")).collect();
    let text = ui::list(&names);
    assert!(text.starts_with("f1, f2, f3, f4, f5"));
    assert!(!text.contains("f6"));
    assert!(text.contains('3'));
    assert_eq!(ui::list(&names[..2]), "f1, f2");
}

#[test]
fn the_confirmation_lines_warn_about_skipped_files_names_and_a_short_balance() {
    let context = Context {
        destination: Uuid::new_v4(),
        folder: "Kit".into(),
        skipped: Skipped {
            files: vec!["Notes/Readme.txt".into()],
            empty_folders: 2,
            links: 1,
            truncated: true,
        },
    };
    let rows = vec![
        row("Wood", Kind::Texture, &["Kit"], RowState::Ready),
        row("Wood", Kind::Texture, &["Kit"], RowState::Ready),
    ];
    let preview = Preview::build(&rows, &["Kit".into()], Some(20), Some(10));
    let lines = ui::lines(&context, &preview, "My Inventory");
    let find = |needle: &str| lines.iter().find(|(text, _)| text.contains(needle)).map(|(_, tone)| *tone);
    assert_eq!(lines[0].1, Tone::Strong);
    assert!(lines[0].0.contains("Kit") && lines[0].0.contains("My Inventory"));
    assert_eq!(find("Notes/Readme.txt"), Some(Tone::Warn));
    assert_eq!(
        lines.iter().filter(|(_, tone)| *tone == Tone::Danger).count(),
        1,
        "the short balance: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|(text, tone)| *tone == Tone::Warn && text.contains("My Inventory"))
    );

    let affordable = Preview::build(&rows[..1], &[], Some(10), Some(100));
    let lines = ui::lines(&context, &affordable, "My Inventory");
    assert!(lines.iter().all(|(_, tone)| *tone != Tone::Danger));
    assert!(lines.iter().any(|(text, _)| text.contains("90")));
}

#[test]
fn the_receipt_notice_names_the_failed_files() {
    let receipt = Receipt {
        uploaded: vec!["A".into()],
        failed: vec![("Broken".into(), "refused".into())],
        cancelled: Vec::new(),
        spent: 10,
    };
    let text = ui::receipt_text("Kit", &receipt);
    assert!(text.contains("Kit") && text.contains("Broken") && text.contains("10"));
    let clean = ui::receipt_text("Kit", &Receipt::default());
    assert!(!clean.contains("Broken"));
}

// --- the uploader's confirmation dialog, drawn ---

fn texture(path: &str, folders: &[&str]) -> crate::imports::Entry {
    let mut entry = crate::imports::Entry::new(
        path.into(),
        Ok(crate::imports::Source::Texture(std::sync::Arc::new(aurora_assets::DecodedImage {
            width: 64,
            height: 64,
            components: 3,
            data: vec![128; 64 * 64 * 3],
            discard: 0,
        }))),
        Default::default(),
    );
    entry.source_folders = folders.iter().map(|part| part.to_string()).collect();
    entry.upload_path = Some(entry.source_folders.clone());
    entry
}

fn confirm_texts(published: Option<Context>) -> Vec<String> {
    let ctx = egui::Context::default();
    let theme = crate::theme::Theme::default();
    theme.apply(&ctx, 1.0);
    let mut world = crate::world::World::new(std::sync::Arc::new(crate::scene::avatar::AvatarLibrary::load()));
    world.balance = Some(1000);
    let mut imports = crate::imports::Imports {
        entries: vec![texture("Cover.png", &["Kit"]), texture("Wood.png", &["Kit", "Fabrics"])],
        ..Default::default()
    };
    imports.demo_started = true;
    imports.confirm = true;
    let mut prefs = crate::imports::Preferences::default();
    let mut texts = Vec::new();
    for _ in 0..4 {
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 1000.0))),
                ..Default::default()
            },
            |ui| {
                ui::publish(ui.ctx(), published.clone());
                crate::imports::ui::show(ui.ctx(), &theme.palette(), &mut imports, &mut prefs, &world, &Default::default());
            },
        );
        texts = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
    }
    texts
}

#[test]
fn the_uploader_s_confirmation_shows_the_folder_upload_block_only_for_a_folder_upload() {
    let context = Context {
        destination: Uuid::new_v4(),
        folder: "Kit".into(),
        skipped: Skipped {
            files: vec!["Notes/Readme.txt".into()],
            ..Default::default()
        },
    };
    let texts = confirm_texts(Some(context));
    let price = aurora_i18n::tr!("import-confirm-price", cost = 20);
    assert!(texts.iter().any(|text| *text == price), "the uploader's total stays: {texts:?}");
    let files = aurora_i18n::tr!("fs-folder-upload-files", files = 2, folders = 2);
    assert!(texts.iter().any(|text| *text == files), "{texts:?}");
    assert!(texts.iter().any(|text| text.contains("Notes/Readme.txt")));
    let balance = aurora_i18n::tr!("fs-folder-upload-balance", balance = 1000, after = 980);
    assert!(texts.iter().any(|text| *text == balance), "{texts:?}");

    let plain = confirm_texts(None);
    assert!(plain.iter().any(|text| *text == price));
    assert!(!plain.iter().any(|text| *text == files || text.contains("Notes/Readme.txt")));
}
