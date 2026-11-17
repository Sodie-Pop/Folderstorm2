//! The account backup without a window: the file, every section's round
//! trip, Merge and Replace, the safety backup, another account, a newer
//! file, and the guard against secrets.

use super::apply::{self, Plan};
use super::file::{Backup, Entry, Kind, ReadError};
use super::section::{Env, Mode, Target};
use super::{Account, folder, secrets, sections};
use crate::folderstorm::desktop::store::Target as Opens;
use crate::folderstorm::workspaces::context::{Hud, Huds};
use crate::folderstorm::workspaces::store::Layout;
use crate::folderstorm::workspaces::workspace::Workspace;
use crate::settings::Settings;
use crate::ui::inventory::custom_tabs;
use crate::ui::inventory::window_mode::{Key, Mode as WindowMode};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const ME: Uuid = Uuid::from_u128(0xa11ce);
const ALT: Uuid = Uuid::from_u128(0xb0b);
const FOLDER: Uuid = Uuid::from_u128(0xf01d);
const ITEM: Uuid = Uuid::from_u128(0x17e3);

fn me() -> Account {
    Account {
        id: ME,
        name: "Alice Resident".into(),
    }
}

fn alt() -> Account {
    Account {
        id: ALT,
        name: "Bob Resident".into(),
    }
}

/// A folder of its own for one test.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("folderstorm-backup-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("scratch folder");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A picture file in `config`'s backgrounds folder, named as the viewer
/// names its copies; its name.
fn picture(config: &Path, bytes: &[u8]) -> String {
    let name = format!("{:016x}.png", crate::folderstorm::backgrounds::import::hash(bytes));
    let dir = config.join("folderstorm_backgrounds");
    std::fs::create_dir_all(&dir).expect("backgrounds folder");
    std::fs::write(dir.join(&name), bytes).expect("picture");
    name
}

fn session_host(main: &Path, slots: Value) {
    std::fs::write(
        main.join(folderstorm_brand::SESSION_HOST_FILE),
        serde_json::to_vec(&json!({ "slots": slots, "chat_sound": true })).expect("json"),
    )
    .expect("session host file");
}

fn host_slots(main: &Path) -> Vec<Value> {
    let bytes = std::fs::read(main.join(folderstorm_brand::SESSION_HOST_FILE)).expect("session host file");
    let value: Value = serde_json::from_slice(&bytes).expect("json");
    value["slots"].as_array().cloned().unwrap_or_default()
}

/// A profile with something in every section, for `account`; the
/// background picture in `config`.
fn full(config: &Path, account: Uuid) -> Settings {
    use crate::folderstorm::backgrounds::record::{Kind as Look, Picture};
    let mut s = Settings::default();
    let store = &mut s.folderstorm_workspaces;
    store.add(Layout::new("Desk", None, None, Vec::new())).expect("layout");
    let mut wall = Layout::new("Wall", None, None, Vec::new());
    let file = picture(config, b"the wall's picture");
    wall.background = Some(crate::folderstorm::backgrounds::Background {
        kind: Look::Picture,
        picture: Some(Picture {
            file: file.clone(),
            name: "wall.png".into(),
        }),
        ..Default::default()
    });
    store.add(wall).expect("layout");
    let mut work = Workspace::new("Work", Some("Desk"), ["inventory".to_owned()]);
    work.huds = Some(Huds {
        account,
        items: vec![Hud { item: ITEM, point: 33 }],
    });
    store.add_workspace(work).expect("workspace");
    store.background.kind = Look::Colour;
    store.background.colour = [10, 20, 30, 255];
    let desktop = &mut s.folderstorm_desktop;
    desktop.shortcuts_on = true;
    desktop.add(Opens::command("people"), 0, (0, 0)).expect("shortcut");
    desktop
        .add(Opens::Folder { folder: FOLDER, account }, 1, (2, 1))
        .expect("shortcut");
    let tabs = &mut s.inventory.custom_tabs;
    let id = custom_tabs::add(tabs, "Hair", Default::default(), "wig".into()).expect("tab");
    tabs[0].folders.insert(account, vec![FOLDER]);
    tabs[0].items.insert(account, vec![ITEM]);
    s.inventory.window_modes.set(Key::Tab(id), WindowMode { pinned: true, quiet: false });
    s.inventory
        .window_modes
        .set(Key::Folder(FOLDER), WindowMode { pinned: false, quiet: true });
    let boards = s.folderstorm_gesture_boards.accounts.entry(account).or_default();
    let board = boards.new_board("Hello").expect("board");
    boards.add_items(board, &[ITEM], None).expect("tile");
    s.folderstorm_viewport.enabled = true;
    s.folderstorm_viewport.left = 640;
    s.media.autoplay_huds = false;
    s.media.max_instances = 3;
    s
}

fn capture(s: &Settings, account: Option<Account>, main: &Path, config: &Path) -> Backup {
    let env = Env {
        settings: s,
        account: account.as_ref().map(|a| a.id),
        main_config: main,
    };
    let mut backup = Backup::capture(&env, Kind::Manual, account);
    backup.read_files(config);
    backup
}

/// The backup written to a file and read back.
fn through_file(backup: &Backup) -> Backup {
    Backup::from_zip(&backup.to_zip().expect("written")).expect("read back")
}

fn import(plan: &Plan, s: &mut Settings, main: &Path) -> apply::Report {
    let mut target = Target { settings: s, main_config: main };
    apply::apply(plan, &mut target)
}

fn with_mode(mut plan: Plan, mode: Mode) -> Plan {
    for choice in &mut plan.choices {
        choice.mode = mode;
    }
    plan
}

#[test]
fn every_section_has_a_plain_unique_id_and_a_label() {
    let mut seen = std::collections::BTreeSet::new();
    for section in sections::SECTIONS {
        assert!(super::file::valid_id(section.id()), "{}", section.id());
        assert!(seen.insert(section.id()), "{} twice", section.id());
        assert!(!section.label().is_empty());
        assert!(sections::find(section.id()).is_some());
    }
}

#[test]
fn every_section_comes_back_as_it_was_through_a_file() {
    let (config, main, elsewhere) = (Scratch::new(), Scratch::new(), Scratch::new());
    session_host(&main.0, json!([{ "slot": 0, "profile": "alice", "alias": "Al", "colour": "teal" }, { "slot": 2, "profile": "bob" }]));
    let original = full(&config.0, ME);
    let backup = through_file(&capture(&original, Some(me()), &main.0, &config.0));
    let ids: Vec<&str> = backup.manifest.sections.iter().map(|e| e.id.as_str()).collect();
    let all: Vec<&str> = sections::SECTIONS.iter().map(|s| s.id()).collect();
    assert_eq!(ids, all, "every section is written");
    assert_eq!(backup.files.len(), 1, "the wall's picture goes with it");
    // into an empty profile on another PC, logged in as the same account
    let plan = with_mode(Plan::new(backup, Some(me()), false), Mode::Replace);
    assert!(!plan.other_account() && !plan.skip_inventory);
    let other_config = Scratch::new();
    let safety = capture(&Settings::default(), Some(me()), &elsewhere.0, &other_config.0);
    apply::prepare(safety, &plan, &other_config.0).expect("prepared");
    let mut restored = Settings::default();
    let report = import(&plan, &mut restored, &elsewhere.0);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(report.done.len(), all.len());
    let ws = (&restored.folderstorm_workspaces, &original.folderstorm_workspaces);
    assert_eq!(ws.0.layouts, ws.1.layouts);
    assert_eq!(ws.0.workspaces, ws.1.workspaces);
    assert_eq!(ws.0.background, ws.1.background);
    assert_eq!(restored.folderstorm_desktop, original.folderstorm_desktop);
    assert_eq!(restored.inventory.custom_tabs, original.inventory.custom_tabs);
    assert_eq!(restored.inventory.window_modes, original.inventory.window_modes);
    assert_eq!(restored.folderstorm_gesture_boards, original.folderstorm_gesture_boards);
    assert_eq!(restored.folderstorm_viewport, original.folderstorm_viewport);
    assert_eq!(restored.media.autoplay_huds, false);
    assert_eq!(restored.media.max_instances, 3);
    let slots = host_slots(&elsewhere.0);
    assert_eq!(slots.len(), 2);
    assert_eq!((slots[0]["profile"].as_str(), slots[0]["alias"].as_str()), (Some("alice"), Some("Al")));
    // the picture is in the other PC's folder, under its own name
    let name = format!("{:016x}.png", crate::folderstorm::backgrounds::import::hash(b"the wall's picture"));
    assert!(other_config.0.join("folderstorm_backgrounds").join(name).exists());
}

#[test]
fn merge_adds_and_replaces_by_name_replace_takes_the_file() {
    let (config, main) = (Scratch::new(), Scratch::new());
    let mut file_side = Settings::default();
    file_side.folderstorm_workspaces.add(Layout::new("B", None, None, Vec::new())).expect("layout");
    file_side.folderstorm_workspaces.add(Layout::new("C", None, None, Vec::new())).expect("layout");
    file_side.inventory.custom_tabs.push(custom_tabs::CustomTab {
        id: 1,
        name: "Hair".into(),
        search: "from the file".into(),
        ..Default::default()
    });
    let boards = file_side.folderstorm_gesture_boards.accounts.entry(ME).or_default();
    let board = boards.new_board("Hello").expect("board");
    boards.add_items(board, &[ITEM, Uuid::from_u128(5)], None).expect("tiles");
    let backup = capture(&file_side, Some(me()), &main.0, &config.0);

    let mine = || {
        let mut s = Settings::default();
        s.folderstorm_workspaces.add(Layout::new("A", None, None, Vec::new())).expect("layout");
        s.folderstorm_workspaces.add(Layout::new("b", None, None, Vec::new())).expect("layout");
        s.inventory.custom_tabs.push(custom_tabs::CustomTab {
            id: 7,
            name: "hair".into(),
            search: "mine".into(),
            folders: [(ALT, vec![FOLDER])].into_iter().collect(),
            ..Default::default()
        });
        let boards = s.folderstorm_gesture_boards.accounts.entry(ME).or_default();
        let board = boards.new_board("Mine").expect("board");
        boards.add_items(board, &[Uuid::from_u128(9)], None).expect("tile");
        s
    };
    let names = |s: &Settings| -> Vec<String> {
        s.folderstorm_workspaces
            .layouts
            .iter()
            .filter_map(|r| r.layout().map(|l| l.name.clone()))
            .collect()
    };

    let mut merged = mine();
    import(&with_mode(Plan::new(backup.clone(), Some(me()), false), Mode::Merge), &mut merged, &main.0);
    // "b" is replaced by the file's "B" in its place; "C" is added
    assert_eq!(names(&merged), ["A", "B", "C"]);
    let tabs = &merged.inventory.custom_tabs;
    assert_eq!(tabs.len(), 1);
    assert_eq!((tabs[0].id, tabs[0].search.as_str()), (7, "from the file"), "the tab keeps its id");
    assert_eq!(tabs[0].folders.get(&ALT), Some(&vec![FOLDER]), "another account's folders stay");
    let boards = &merged.folderstorm_gesture_boards.accounts[&ME];
    assert_eq!(boards.boards.len(), 2);
    let mut tile_ids: Vec<u32> = boards.boards.iter().flat_map(|b| b.tiles.iter().map(|t| t.id)).collect();
    let count = tile_ids.len();
    tile_ids.sort();
    tile_ids.dedup();
    assert_eq!((count, tile_ids.len()), (3, 3), "tile ids stay unique");

    let mut replaced = mine();
    import(&with_mode(Plan::new(backup, Some(me()), false), Mode::Replace), &mut replaced, &main.0);
    assert_eq!(names(&replaced), ["B", "C"]);
    assert_eq!(replaced.inventory.custom_tabs.len(), 1);
    assert_eq!(replaced.inventory.custom_tabs[0].search, "from the file");
    assert!(replaced.inventory.custom_tabs[0].folders.is_empty());
    assert_eq!(replaced.folderstorm_gesture_boards.accounts[&ME].boards.len(), 1);
}

#[test]
fn an_import_makes_a_safety_backup_first_that_undoes_it() {
    let (config, main) = (Scratch::new(), Scratch::new());
    let before = full(&config.0, ME);
    let mut file_side = Settings::default();
    file_side.folderstorm_viewport.enabled = false;
    file_side.folderstorm_workspaces.add(Layout::new("Other", None, None, Vec::new())).expect("layout");
    let backup = capture(&file_side, Some(me()), &main.0, &config.0);
    let plan = with_mode(Plan::new(backup, Some(me()), false), Mode::Replace);

    let safety = capture(&before, Some(me()), &main.0, &config.0);
    let path = apply::prepare(safety, &plan, &config.0).expect("safety backup written");
    let listed = folder::list(&config.0);
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].kind, &listed[0].path), (Kind::BeforeImport, &path));
    assert_eq!(listed[0].account.as_deref(), Some("Alice Resident"));

    let mut now = before.clone();
    import(&plan, &mut now, &main.0);
    assert!(!now.folderstorm_viewport.enabled);
    assert_ne!(now.folderstorm_workspaces.layouts, before.folderstorm_workspaces.layouts);

    // Restore of the safety backup: as it was
    let bytes = std::fs::read(&path).expect("safety file");
    let restore = Plan::new(Backup::from_zip(&bytes).expect("readable"), Some(me()), true);
    assert!(restore.choices.iter().all(|c| c.mode == Mode::Replace), "a restore replaces");
    let report = import(&restore, &mut now, &main.0);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(now.folderstorm_viewport, before.folderstorm_viewport);
    assert_eq!(now.folderstorm_workspaces.layouts, before.folderstorm_workspaces.layouts);
    assert_eq!(now.folderstorm_desktop, before.folderstorm_desktop);
    assert_eq!(now.inventory.custom_tabs, before.inventory.custom_tabs);
}

#[test]
fn no_safety_backup_no_import() {
    let (config, main) = (Scratch::new(), Scratch::new());
    // the settings folder is a file: the backups folder cannot be made
    let blocked = config.0.join("blocked");
    std::fs::write(&blocked, b"a file").expect("file");
    let backup = capture(&Settings::default(), Some(me()), &main.0, &config.0);
    let plan = Plan::new(backup.clone(), Some(me()), false);
    assert!(apply::prepare(backup, &plan, &blocked).is_err());
}

#[test]
fn the_safety_backups_and_the_daily_ones_are_pruned_per_account() {
    let config = Scratch::new();
    for (n, account) in [(1, ME), (2, ME), (3, ME), (4, ALT)] {
        let stamp = format!("2026-11-0{n}_120000");
        folder::write(&config.0, Kind::Daily, &stamp, Some(account), b"zip").expect("written");
    }
    assert!(folder::has_daily(&config.0, Some(ME), "2026-11-03"));
    assert!(!folder::has_daily(&config.0, Some(ALT), "2026-11-03"));
    folder::prune(&config.0, Kind::Daily, Some(ME), 2);
    let mut left: Vec<String> = std::fs::read_dir(folder::dir(&config.0))
        .expect("folder")
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    left.sort();
    assert_eq!(left.len(), 3, "{left:?}");
    assert!(!folder::has_daily(&config.0, Some(ME), "2026-11-01"), "the oldest is gone");
    assert!(folder::has_daily(&config.0, Some(ALT), "2026-11-04"), "another account's stays");
    // only names of the folder's own shape count
    assert!(folder::parse_name("daily_2026-11-01_120000_00000000.zip").is_some());
    assert!(folder::parse_name("../daily_2026-11-01_120000_00000000.zip").is_none());
    assert!(folder::parse_name("daily_2026-11-01_120000_0000000z.zip").is_none());
    assert!(folder::parse_name("notes.zip").is_none());
}

#[test]
fn another_account_s_inventory_is_left_out_or_kept_for_that_account() {
    let (config, main) = (Scratch::new(), Scratch::new());
    let backup = capture(&full(&config.0, ALT), Some(alt()), &main.0, &config.0);
    let plan = Plan::new(backup, Some(me()), false);
    assert!(plan.other_account());
    assert!(plan.skip_inventory, "left out by default");
    // a HUD, a folder shortcut, a tab's folder and item, a folder window,
    // a board's tile
    assert_eq!(plan.inventory_refs(), 6);

    let mut s = Settings::default();
    let report = import(&plan, &mut s, &main.0);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(!report.notes.is_empty(), "what was left out is said");
    let ws = s.folderstorm_workspaces.workspaces.iter().filter_map(|r| r.workspace()).next().expect("the workspace");
    assert!(ws.huds.is_none(), "the other account's HUDs are left out");
    assert_eq!(s.folderstorm_desktop.shortcuts.len(), 1, "the command shortcut only");
    assert!(s.inventory.custom_tabs[0].folders.is_empty() && s.inventory.custom_tabs[0].items.is_empty());
    assert_eq!(s.inventory.window_modes.get(Key::Folder(FOLDER)), WindowMode::default());
    assert!(s.folderstorm_gesture_boards.accounts.is_empty(), "no boards for anyone");

    // kept: tied to the account they belong to, never to the one logged in
    let mut plan = plan;
    plan.skip_inventory = false;
    let mut s = Settings::default();
    import(&plan, &mut s, &main.0);
    assert!(matches!(
        s.folderstorm_desktop.shortcuts[1].target,
        Opens::Folder { account, .. } if account == ALT
    ));
    assert!(s.inventory.custom_tabs[0].folders.contains_key(&ALT));
    assert!(s.folderstorm_gesture_boards.accounts.contains_key(&ALT));
    assert!(!s.folderstorm_gesture_boards.accounts.contains_key(&ME));
}

#[test]
fn a_newer_file_is_read_for_what_this_version_knows() {
    let (config, main) = (Scratch::new(), Scratch::new());
    let mut backup = capture(&full(&config.0, ME), Some(me()), &main.0, &config.0);
    backup.manifest.version = 99;
    backup.manifest.sections.push(Entry {
        id: "desk_icons".into(),
        label: "Desktop icon styles".into(),
        files: Vec::new(),
    });
    backup.values.insert("desk_icons".into(), json!({ "style": "glass" }));
    if let Some(Value::Object(desktop)) = backup.values.get_mut("desktop") {
        desktop.insert("a_newer_switch".into(), json!(true));
    }
    let read = through_file(&backup);
    assert!(read.newer());
    assert_eq!(read.unknown().iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["desk_icons"]);
    let plan = Plan::new(read, Some(me()), false);
    assert!(plan.choices.iter().all(|c| c.id != "desk_icons"), "an unknown section is not offered");
    let mut s = Settings::default();
    let report = import(&plan, &mut s, &main.0);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(s.folderstorm_desktop.shortcuts_on);
}

#[test]
fn what_is_not_a_backup_is_refused_and_names_are_never_paths() {
    assert_eq!(Backup::from_zip(b"not a zip").err(), Some(ReadError::NotZip));
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("manifest.json", options).expect("entry");
    std::io::Write::write_all(&mut zip, br#"{"format":"aurora-settings-backup","version":1}"#).expect("written");
    let other = zip.finish().expect("zip").into_inner();
    assert_eq!(Backup::from_zip(&other).err(), Some(ReadError::NotABackup));

    let (config, main) = (Scratch::new(), Scratch::new());
    let mut backup = capture(&full(&config.0, ME), Some(me()), &main.0, &config.0);
    backup.manifest.sections.push(Entry {
        id: "../escape".into(),
        label: String::new(),
        files: vec!["backgrounds/../../escape.png".into(), "elsewhere/x.png".into()],
    });
    // a picture whose bytes are not the ones its name says
    let forged = format!("backgrounds/{:016x}.png", 1u64);
    backup.files.insert(forged.clone(), b"something else".to_vec());
    if let Some(entry) = backup.manifest.sections.iter_mut().find(|e| e.id == "background") {
        entry.files.push(forged.clone());
    }
    let read = through_file(&backup);
    assert!(read.manifest.sections.iter().all(|e| super::file::valid_id(&e.id)));
    assert!(!read.files.contains_key(&forged));
    assert!(read.files.keys().all(|f| super::file::split(f).is_some()));
}

/// Settings full of what must never leave the PC.
fn full_of_secrets() -> (Settings, Vec<&'static str>) {
    let mut s = full(&Scratch::new().0, ME);
    s.username = "secret-user-name".into();
    s.last_login = Some(("secret-first".into(), "secret-last".into()));
    s.remember_password = true;
    s.custom_login_uri = "https://secret-grid.example/login".into();
    s.mfa_hash = "secret-mfa-hash".into();
    s.gifs.giphy_key = "secret-giphy-key".into();
    s.gifs.klipy_key = "secret-klipy-key".into();
    s.gifs.customer_id = "secret-customer-id".into();
    s.local_assistant.enabled = true;
    s.folderstorm_chat_upload.host = "https://secret-upload-host.example".into();
    s.folderstorm_chat_upload
        .folders
        .insert("https://secret-upload-host.example".into(), "secret-upload-folder".into());
    s.media.plugin_dir = "C:\\secret-plugins".into();
    let sentinels = vec![
        "secret-user-name",
        "secret-first",
        "secret-last",
        "secret-grid",
        "secret-mfa-hash",
        "secret-giphy-key",
        "secret-klipy-key",
        "secret-customer-id",
        "secret-upload-host",
        "secret-upload-folder",
        "secret-plugins",
        "secret-host-token",
        "/cap/",
    ];
    (s, sentinels)
}

#[test]
fn no_secret_ever_goes_into_a_backup() {
    let (config, main) = (Scratch::new(), Scratch::new());
    // the session host's file may hold more than names and roles
    session_host(
        &main.0,
        json!([{ "slot": 0, "profile": "alice", "controller": true, "monitor_window": { "token": "secret-host-token" } }]),
    );
    let (s, sentinels) = full_of_secrets();
    let env = Env {
        settings: &s,
        account: Some(ME),
        main_config: &main.0,
    };
    // each section as it writes itself, before the guard
    for section in sections::SECTIONS {
        let Some(exported) = section.export(&env) else { continue };
        let found = secrets::findings(&exported.value);
        assert!(found.is_empty(), "section {} writes private keys: {found:?}", section.id());
        let text = exported.value.to_string();
        for sentinel in &sentinels {
            assert!(!text.contains(sentinel), "section {} writes {sentinel}", section.id());
        }
        assert!(!text.contains("controller"), "section {} writes the controller access", section.id());
    }
    // and the whole file
    let mut backup = Backup::capture(&env, Kind::Manual, Some(me()));
    backup.read_files(&config.0);
    let bytes = backup.to_zip().expect("written");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).expect("entry");
        let mut text = String::new();
        if std::io::Read::read_to_string(&mut entry, &mut text).is_ok() {
            for sentinel in &sentinels {
                assert!(!text.contains(sentinel), "entry {index} holds {sentinel}");
            }
        }
    }
}

#[test]
fn the_guard_removes_secrets_written_and_read() {
    let mut value = json!({
        "name": "Desk",
        "api_token": "x",
        "Password": "x",
        "nested": { "seed_capability": "x", "url": "https://sim.example:12043/cap/0b9d", "kept": 1 },
        "list": ["fine", "https://user:pw@host.example/"],
        "email_address": "x"
    });
    assert_eq!(secrets::scrub(&mut value), 6);
    assert_eq!(value, json!({ "name": "Desk", "nested": { "kept": 1 }, "list": ["fine"] }));
    // a file someone made with a secret in a section: never read in
    let (config, main) = (Scratch::new(), Scratch::new());
    let mut backup = capture(&Settings::default(), Some(me()), &main.0, &config.0);
    if let Some(Value::Object(desktop)) = backup.values.get_mut("desktop") {
        desktop.insert("auth_token".into(), json!("planted"));
    }
    let read = through_file(&backup);
    assert!(read.values["desktop"].get("auth_token").is_none());
}
