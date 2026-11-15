//! Folderstorm: the camera read and file upload tools
//! (folderstorm/plans/mcp-uploads.md): the local paths and the scan
//! (absolute, no links, supported types only, bounds, hashes, name order),
//! prices and plans (expiry, used once, the bounds, a login change), the
//! execute (always asks above L$0, Allow at L$0, the balance, a higher
//! price, folders then files, failures and unclear answers stopping the
//! rest, cancel, Never mid-run, the receipt), the snapshot plan, the mesh
//! review, `camera_get`, no asset id or local path in any answer, and end
//! to end over a real socket.

use super::camera::{self, Pose, Stage};
use super::change_tests::{ctx, demo_inventory};
use super::changes::Work;
use super::desk::{Bake, Desk};
use super::uploads::scan::{self, Input, Rules};
use super::uploads::{self, Account, Act};
use crate::world::inventory::{Inventory, actions};
use aurora_net::inventory::InvItem;
use aurora_net::inventory::upload::{self, Prices};
use folderstorm_mcp_bridge::protocol::uploads::{CANCEL, EXECUTE, MESH_REVIEW, PREVIEW, SNAPSHOT, STATUS};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{Class, ErrorCode, Level, Permissions, Preset, ToolError};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use uuid::Uuid;

const AGENT: Uuid = Uuid::from_u128(2);
const DEMO_FOLDER: Uuid = Uuid::from_u128(8000);
const IDLE: Bake = Bake {
    outfit_change_pending: false,
    cof_version: 3,
    appearance_cof_version: 3,
    server_bakes: true,
};

fn prices(each: i32) -> Prices {
    Prices {
        texture: Some(each),
        large_texture: Some(each * 4),
        sound: Some(each),
        animation: Some(each),
    }
}

fn account(each: i32, balance: Option<i32>) -> Account {
    Account {
        prices: prices(each),
        balance,
        sound_limit: 30,
        texture: Default::default(),
    }
}

fn rules() -> Rules {
    Rules {
        max_texture: 1024,
        sound_limit: 30,
        texture: Default::default(),
    }
}

fn call(tool: &str, args: Value, ask: bool) -> (Pending, Receiver<Result<Value, ToolError>>) {
    Pending::detached(tool, args, ask, Some("Claude Code"))
}

fn desk_with(permissions: Permissions, account: Account) -> Desk {
    let mut desk = Desk::default();
    desk.set_permissions(permissions);
    desk.uploads().set_account(account);
    desk
}

fn allow_all() -> Permissions {
    let mut p = Permissions::default();
    for class in Class::ALL {
        p.set(class, Level::Allow);
    }
    p
}

/// The demo's folder: two textures in `Textures/` and a sound.
fn fixtures() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = uploads::demo::fixtures(dir.path()).expect("fixtures");
    (dir, folder)
}

fn chime(folder: &Path) -> Value {
    json!({"files": [folder.join("Chime.wav").display().to_string()], "folder_id": DEMO_FOLDER.to_string()})
}

fn whole(folder: &Path) -> Value {
    json!({"directory": folder.display().to_string(), "folder_id": DEMO_FOLDER.to_string()})
}

/// Wait for a preview's answer, ticking the desk.
fn preview(desk: &mut Desk, inv: &Inventory, args: Value) -> Result<Value, ToolError> {
    let (c, answer) = call(PREVIEW, args, false);
    desk.take(c, &ctx(inv), Instant::now());
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(result) = answer.try_recv() {
            return result;
        }
        assert!(Instant::now() < deadline, "the preview answers");
        desk.tick_uploads(&ctx(inv), Instant::now());
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn plan_id(desk: &mut Desk, inv: &Inventory, args: Value) -> String {
    preview(desk, inv, args).expect("previewed")["plan_id"]
        .as_str()
        .expect("id")
        .to_owned()
}

/// A call taken and answered at once.
fn at_once(desk: &mut Desk, inv: &Inventory, tool: &str, args: Value) -> Result<Value, ToolError> {
    let (c, answer) = call(tool, args, false);
    desk.take(c, &ctx(inv), Instant::now());
    answer.try_recv().expect("answered at once")
}

/// What the fake server does with each upload.
#[derive(Clone, Copy, PartialEq)]
enum Server {
    Accept,
    Refuse,
    Lose,
}

fn receipt_for(upload: &upload::Request) -> upload::Receipt {
    upload::Receipt {
        item: InvItem {
            id: Uuid::new_v4(),
            asset_id: Uuid::new_v4(),
            parent: upload.folder,
            name: upload.name.clone(),
            asset_type: upload.kind.asset_type(),
            owner: AGENT,
            ..Default::default()
        },
        cost: upload.expected_cost,
    }
}

/// The app's part of an execute: runs started, folders created on the
/// demo inventory, uploads answered by `server` (by order), until the
/// execute answers, or `stop_after` uploads were answered (None then).
fn drive(
    desk: &mut Desk,
    inv: &mut Inventory,
    answer: &Receiver<Result<Value, ToolError>>,
    server: &dyn Fn(usize) -> Server,
    stop_after: Option<usize>,
) -> Option<Result<Value, ToolError>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut answered = 0usize;
    loop {
        if let Ok(result) = answer.try_recv() {
            return Some(result);
        }
        assert!(Instant::now() < deadline, "the execute ends");
        let now = Instant::now();
        for run in desk.tick(&IDLE, now) {
            let planned = desk.upload_plan(&run, &ctx(inv)).expect("still allowed");
            assert!(matches!(planned.work, Work::Upload));
            desk.upload_start(run, now);
        }
        desk.tick_uploads(&ctx(inv), now);
        for act in desk.uploads().take_acts() {
            match act {
                Act::Folder { request, change } => {
                    let reply = actions::demo_mutate(inv, AGENT, change);
                    desk.uploads().folder_result(request, reply, inv);
                }
                Act::Upload { request, upload } => {
                    assert!(!upload.data.is_empty(), "encoded");
                    let result = match server(answered) {
                        Server::Accept => Ok(receipt_for(&upload)),
                        Server::Refuse => Err(upload::Error::Rejected),
                        Server::Lose => Err(upload::Error::Unknown),
                    };
                    answered += 1;
                    if let Some(item) = desk.uploads().uploaded(request, result) {
                        inv.add_items(vec![item]);
                    }
                    if stop_after == Some(answered) {
                        return None;
                    }
                }
                Act::Thumbnail { request, .. } => desk.uploads().image_finished(request, None),
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

// --- the scan ---

#[test]
fn paths_are_absolute_local_and_never_linked() {
    let (dir, folder) = fixtures();
    assert!(scan::check_path(Path::new("relative/file.png")).is_err());
    assert!(scan::check_path(Path::new("//server/share/file.png")).is_err());
    assert!(scan::check_path(&folder.join("..").join("x")).is_err());
    assert!(scan::check_path(&dir.path().join("missing.png")).is_err());
    assert!(scan::check_path(&folder.join("Chime.wav")).is_ok());
    #[cfg(unix)]
    {
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&folder, &link).expect("link");
        let e = scan::check_path(&link.join("Chime.wav")).expect_err("a linked parent");
        assert!(e.contains("symbolic links"), "{e}");
        let inside = folder.join("Textures").join("again.png");
        std::os::unix::fs::symlink(folder.join("Textures").join("Red square.png"), &inside).expect("link");
        let e = scan::run(Input::Directory(folder.clone()), &rules()).err().expect("a link inside");
        assert!(e.contains("links"), "{e}");
    }
}

#[test]
fn a_folder_is_read_in_name_order_hashed_and_decoded() {
    let (_dir, folder) = fixtures();
    let scanned = scan::run(Input::Directory(folder.clone()), &rules()).expect("scanned");
    assert_eq!(scanned.root.as_deref(), Some(uploads::demo::FOLDER));
    let names: Vec<(&str, String)> = scanned.files.iter().map(|f| (f.name.as_str(), f.folder.join("/"))).collect();
    assert_eq!(
        names,
        vec![
            ("Chime", "Assistant upload demo".to_owned()),
            ("Blue square", "Assistant upload demo/Textures".to_owned()),
            ("Red square", "Assistant upload demo/Textures".to_owned()),
        ]
    );
    let first = &scanned.files[0];
    let bytes = std::fs::read(folder.join("Chime.wav")).expect("read");
    use sha2::Digest as _;
    let expected: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(first.sha256, expected);
    assert_eq!(first.bytes, bytes.len() as u64);
    assert!(first.seconds.is_some_and(|s| (s - 0.5).abs() < 0.01));
    assert_eq!(scanned.files[1].size, (64, 64));
    // what is uploaded is encoded from the bytes read then
    for file in &scanned.files {
        assert!(!scan::encode(&file.source, rules().texture_options()).expect("encoded").is_empty());
    }
}

#[test]
fn unsupported_long_deep_or_too_many_files_refuse_the_whole_preview() {
    let (dir, folder) = fixtures();
    std::fs::write(folder.join("notes.txt"), b"hello").expect("write");
    let e = scan::run(Input::Directory(folder.clone()), &rules()).err().expect("a text file");
    assert!(e.contains("notes.txt") && e.contains("unsupported"), "{e}");
    std::fs::remove_file(folder.join("notes.txt")).expect("removed");
    let dae = dir.path().join("chair.dae");
    std::fs::write(&dae, b"<COLLADA/>").expect("write");
    let e = scan::run(Input::Files(vec![dae]), &rules()).err().expect("a Collada file");
    assert!(e.contains("inventory_upload_mesh_review"), "{e}");
    let short = Rules { sound_limit: 0, ..rules() };
    let e = scan::run(Input::Files(vec![folder.join("Chime.wav")]), &short)
        .err()
        .expect("too long");
    assert!(e.contains("longer than"), "{e}");
    let mut deep = dir.path().join("deep");
    for level in 0..9 {
        deep = deep.join(format!("level{level}"));
    }
    std::fs::create_dir_all(&deep).expect("deep");
    std::fs::write(deep.join("Chime.wav"), uploads::demo::wav_bytes()).expect("write");
    let e = scan::run(Input::Directory(dir.path().join("deep")), &rules())
        .err()
        .expect("too deep");
    assert!(e.contains("deeper"), "{e}");
    let many = dir.path().join("many");
    std::fs::create_dir_all(&many).expect("many");
    for n in 0..33 {
        std::fs::write(many.join(format!("s{n}.wav")), uploads::demo::wav_bytes()).expect("write");
    }
    let e = scan::run(Input::Directory(many), &rules()).err().expect("too many");
    assert!(e.contains("32"), "{e}");
    let twice = folder.join("Chime.wav");
    assert!(scan::run(Input::Files(vec![twice.clone(), twice]), &rules()).is_err());
    assert!(scan::item_name("a|b").is_err() && scan::item_name("").is_err());
}

// --- plans ---

#[test]
fn a_preview_prices_the_files_and_keeps_a_plan_without_paths() {
    let (_dir, folder) = fixtures();
    let inv = demo_inventory();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions(), account(10, Some(500)));
    let answer = preview(&mut desk, &inv, whole(&folder)).expect("previewed");
    assert_eq!(answer["total"], 30);
    assert_eq!(answer["balance_after"], 470);
    assert_eq!(answer["execution"], "ask");
    assert_eq!(answer["always_asks"], true);
    assert_eq!(
        answer["folders"],
        json!(["Assistant upload demo", "Assistant upload demo/Textures"])
    );
    assert_eq!(answer["files"].as_array().map(Vec::len), Some(3));
    assert_eq!(answer["expires_in"], 120);
    let text = answer.to_string();
    assert!(!text.contains(&folder.display().to_string()), "no local path: {text}");
    // read only: previewed, never executed
    let mut read_only = desk_with(Preset::ReadOnly.permissions(), account(10, Some(500)));
    let answer = preview(&mut read_only, &inv, chime(&folder)).expect("previewed");
    assert_eq!(answer["execution"], "never");
    // an unknown price is never free
    let mut unknown = desk_with(
        allow_all(),
        Account {
            prices: Prices::default(),
            ..account(0, Some(0))
        },
    );
    assert_eq!(
        preview(&mut unknown, &inv, chime(&folder)).expect_err("no price").code,
        ErrorCode::NotReady
    );
    // the Trash is refused before anything is read
    let trash = actions::system(&inv, 14).expect("trash");
    let args = json!({"files": [folder.join("Chime.wav").display().to_string()], "folder_id": trash.to_string()});
    assert_eq!(preview(&mut desk, &inv, args).expect_err("trash").code, ErrorCode::NotAllowed);
    // 2048: the large texture price; 1024 by default
    let big = folder.join("big.png");
    image::RgbImage::from_pixel(2048, 2048, image::Rgb([1, 2, 3]))
        .save(&big)
        .expect("big");
    let args = json!({"files": [big.display().to_string()], "folder_id": DEMO_FOLDER.to_string(), "max_texture_dimension": 2048});
    assert_eq!(preview(&mut desk, &inv, args).expect("previewed")["total"], 40);
    let args = json!({"files": [big.display().to_string()], "folder_id": DEMO_FOLDER.to_string()});
    assert_eq!(preview(&mut desk, &inv, args).expect("previewed")["total"], 10);
}

#[test]
fn plans_expire_are_bounded_and_end_with_the_login() {
    let (_dir, folder) = fixtures();
    let inv = demo_inventory();
    let mut desk = desk_with(allow_all(), account(0, Some(0)));
    let ids: Vec<String> = (0..5).map(|_| plan_id(&mut desk, &inv, chime(&folder))).collect();
    assert_eq!(desk.uploads().plan_count(), 4, "the oldest went");
    let gone = at_once(&mut desk, &inv, STATUS, json!({"plan_id": ids[0]}));
    assert_eq!(gone.expect_err("gone").code, ErrorCode::NotFound);
    let kept = at_once(&mut desk, &inv, STATUS, json!({"plan_id": ids[4]}));
    assert_eq!(kept.expect("kept")["state"], "planned");
    desk.tick_uploads(&ctx(&inv), Instant::now() + Duration::from_secs(121));
    assert_eq!(desk.uploads().plan_count(), 0, "120 seconds");
    let id = plan_id(&mut desk, &inv, chime(&folder));
    desk.set_login(7, Some("vs-other".into()));
    assert_eq!(desk.uploads().plan_count(), 0, "a login change ends the plans");
    let e = at_once(&mut desk, &inv, STATUS, json!({"plan_id": id})).expect_err("gone");
    assert_eq!(e.code, ErrorCode::NotFound);
}

// --- the execute ---

#[test]
fn a_paid_upload_always_asks_with_its_price_then_runs_folders_then_files() {
    let (_dir, folder) = fixtures();
    let mut inv = demo_inventory();
    let mut desk = desk_with(allow_all(), account(10, Some(500)));
    let id = plan_id(&mut desk, &inv, whole(&folder));
    let (c, answer) = call(EXECUTE, json!({"plan_id": id, "client_request_id": "up-1"}), false);
    desk.take(c, &ctx(&inv), Instant::now());
    let question = desk.question(Instant::now()).expect("asked although Upload is on Allow");
    assert!(question.summary.contains("30"), "{}", question.summary);
    assert!(
        question.detail.iter().any(|l| l.contains("Chime") && l.contains("10")),
        "{:?}",
        question.detail
    );
    assert!(
        question.detail.iter().any(|l| l.contains("500") && l.contains("470")),
        "{:?}",
        question.detail
    );
    desk.decide(question.number, true);
    let result = drive(&mut desk, &mut inv, &answer, &|_| Server::Accept, None)
        .expect("ended")
        .expect("done");
    assert_eq!(result["done"], true);
    assert_eq!(result["spent"], 30);
    assert_eq!(result["folders_created"], 2);
    assert_eq!(result["item_ids"].as_array().map(Vec::len), Some(3));
    let root = inv
        .folders
        .values()
        .find(|f| f.info.name == uploads::demo::FOLDER && f.info.parent == DEMO_FOLDER)
        .expect("root folder")
        .info
        .id;
    let sub = inv
        .folders
        .values()
        .find(|f| f.info.name == "Textures" && f.info.parent == root)
        .expect("sub folder")
        .info
        .id;
    assert_eq!(inv.items.values().find(|i| i.name == "Red square").expect("red").parent, sub);
    assert_eq!(inv.items.values().find(|i| i.name == "Chime").expect("chime").parent, root);
    // the receipt: done, a row per file, never an asset id or a path
    let receipt =
        at_once(&mut desk, &inv, "assistant_operation_status", json!({"client_request_id": "up-1"})).expect("found")["receipt"].clone();
    assert_eq!(receipt["state"], "done", "{receipt}");
    let rows = receipt["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r["outcome"] == "done"));
    let assets: Vec<String> = inv
        .items
        .values()
        .filter(|i| i.owner == AGENT)
        .map(|i| i.asset_id.to_string())
        .collect();
    for text in [receipt.to_string(), result.to_string()] {
        assert!(!assets.iter().any(|a| text.contains(a.as_str())), "no asset id: {text}");
        assert!(!text.contains(&folder.display().to_string()), "no path: {text}");
    }
    let report = at_once(&mut desk, &inv, STATUS, json!({"plan_id": id})).expect("found");
    assert_eq!(report["state"], "done");
    // used once
    let again = at_once(&mut desk, &inv, EXECUTE, json!({"plan_id": id, "client_request_id": "up-2"}));
    assert_eq!(again.expect_err("used").code, ErrorCode::NotFound);
}

#[test]
fn a_free_upload_on_allow_runs_without_a_question_and_ask_still_asks() {
    let (_dir, folder) = fixtures();
    let mut inv = demo_inventory();
    let mut desk = desk_with(allow_all(), account(0, Some(0)));
    let id = plan_id(&mut desk, &inv, chime(&folder));
    let (c, answer) = call(EXECUTE, json!({"plan_id": id, "client_request_id": "free-1"}), false);
    desk.take(c, &ctx(&inv), Instant::now());
    assert!(desk.question(Instant::now()).is_none(), "L$0 on Allow: no dialog");
    let result = drive(&mut desk, &mut inv, &answer, &|_| Server::Accept, None)
        .expect("ended")
        .expect("done");
    assert_eq!(result["spent"], 0);
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions(), account(0, Some(0)));
    let id = plan_id(&mut desk, &inv, chime(&folder));
    let (c, answer) = call(EXECUTE, json!({"plan_id": id, "client_request_id": "free-2"}), true);
    desk.take(c, &ctx(&inv), Instant::now());
    let q = desk.question(Instant::now()).expect("asked");
    desk.decide(q.number, false);
    desk.tick(&IDLE, Instant::now());
    assert_eq!(
        answer.try_recv().expect("answered").expect_err("refused").code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn the_balance_a_higher_price_and_never_refuse_before_anything_is_sent() {
    let (_dir, folder) = fixtures();
    let inv = demo_inventory();
    let mut desk = desk_with(allow_all(), account(10, Some(5)));
    let id = plan_id(&mut desk, &inv, chime(&folder));
    let poor = at_once(&mut desk, &inv, EXECUTE, json!({"plan_id": id, "client_request_id": "poor"}));
    assert_eq!(poor.expect_err("poor").code, ErrorCode::NotAllowed);
    let mut desk = desk_with(allow_all(), account(10, Some(500)));
    let id = plan_id(&mut desk, &inv, chime(&folder));
    desk.uploads().set_account(account(15, Some(500)));
    let dearer = at_once(&mut desk, &inv, EXECUTE, json!({"plan_id": id, "client_request_id": "dearer"}));
    assert_eq!(dearer.expect_err("stale").code, ErrorCode::Stale);
    let mut never = allow_all();
    never.set(Class::Upload, Level::Never);
    let mut desk = desk_with(never, account(0, Some(0)));
    let id = plan_id(&mut desk, &inv, chime(&folder));
    let refused = at_once(&mut desk, &inv, EXECUTE, json!({"plan_id": id, "client_request_id": "never"}));
    assert_eq!(refused.expect_err("never").code, ErrorCode::PermissionDenied);
    let no_id = at_once(&mut desk, &inv, EXECUTE, json!({"plan_id": "up-x"}));
    assert_eq!(no_id.expect_err("no id").code, ErrorCode::InvalidArguments);
}

#[test]
fn a_refusal_or_an_unclear_answer_stops_the_files_not_sent() {
    let (_dir, folder) = fixtures();
    for (server, code, second) in [
        (Server::Refuse, ErrorCode::Cancelled, "failed"),
        (Server::Lose, ErrorCode::Timeout, "unknown"),
    ] {
        let mut inv = demo_inventory();
        let mut desk = desk_with(allow_all(), account(0, Some(0)));
        let id = plan_id(&mut desk, &inv, whole(&folder));
        let (c, answer) = call(EXECUTE, json!({"plan_id": id, "client_request_id": "stop"}), false);
        desk.take(c, &ctx(&inv), Instant::now());
        let behaviour = move |n: usize| if n == 1 { server } else { Server::Accept };
        let e = drive(&mut desk, &mut inv, &answer, &behaviour, None)
            .expect("ended")
            .expect_err("stopped");
        assert_eq!(e.code, code, "{e}");
        let report = at_once(&mut desk, &inv, STATUS, json!({"plan_id": id})).expect("found");
        let outcomes: Vec<&str> = report["files"]
            .as_array()
            .expect("files")
            .iter()
            .filter_map(|f| f["outcome"].as_str())
            .collect();
        assert_eq!(outcomes, vec!["done", second, "not_sent"], "{report}");
    }
}

#[test]
fn cancel_and_never_stop_what_is_not_sent_yet() {
    let (_dir, folder) = fixtures();
    let mut inv = demo_inventory();
    let mut desk = desk_with(allow_all(), account(0, Some(0)));
    let id = plan_id(&mut desk, &inv, whole(&folder));
    let (c, answer) = call(EXECUTE, json!({"plan_id": id, "client_request_id": "cancel"}), false);
    desk.take(c, &ctx(&inv), Instant::now());
    assert!(drive(&mut desk, &mut inv, &answer, &|_| Server::Accept, Some(1)).is_none());
    let report = at_once(&mut desk, &inv, STATUS, json!({"plan_id": id})).expect("found");
    let operation = report["operation_id"].as_str().expect("op").to_owned();
    let cancelled = at_once(&mut desk, &inv, CANCEL, json!({"operation_id": operation})).expect("cancelled");
    assert_eq!(cancelled["cancelled"], true);
    let e = drive(&mut desk, &mut inv, &answer, &|_| Server::Accept, None)
        .expect("ended")
        .expect_err("stopped");
    assert_eq!(e.code, ErrorCode::Cancelled);
    let receipt = at_once(&mut desk, &inv, "assistant_operation_status", json!({"operation_id": operation})).expect("found");
    assert_eq!(receipt["receipt"]["state"], "stopped");
    let report = at_once(&mut desk, &inv, STATUS, json!({"operation_id": operation})).expect("found");
    assert!(
        report["files"]
            .as_array()
            .expect("files")
            .iter()
            .any(|f| f["outcome"] == "cancelled"),
        "{report}"
    );
    // Upload set to Never during the run
    let mut inv = demo_inventory();
    let mut desk = desk_with(allow_all(), account(0, Some(0)));
    let id = plan_id(&mut desk, &inv, whole(&folder));
    let (c, answer) = call(EXECUTE, json!({"plan_id": id, "client_request_id": "never"}), false);
    desk.take(c, &ctx(&inv), Instant::now());
    assert!(drive(&mut desk, &mut inv, &answer, &|_| Server::Accept, Some(1)).is_none());
    let mut never = allow_all();
    never.set(Class::Upload, Level::Never);
    desk.set_permissions(never);
    let result = drive(&mut desk, &mut inv, &answer, &|_| Server::Accept, None).expect("ended");
    assert!(result.is_err(), "{result:?}");
    let report = at_once(&mut desk, &inv, STATUS, json!({"plan_id": id})).expect("found");
    assert!(
        report["files"]
            .as_array()
            .expect("files")
            .iter()
            .any(|f| f["outcome"] == "not_sent"),
        "{report}"
    );
}

// --- the snapshot, the mesh review, camera_get ---

#[test]
fn a_snapshot_becomes_a_texture_plan_and_its_picture_needs_edit() {
    let inv = demo_inventory();
    let mut desk = desk_with(allow_all(), account(10, Some(100)));
    let args = json!({"folder_id": DEMO_FOLDER.to_string(), "name": "Shop front", "thumbnail_id": DEMO_FOLDER.to_string(), "width": 256, "height": 256});
    let (c, _) = call(SNAPSHOT, args.clone(), false);
    let planned = desk.uploads().snapshot_plan(&c, &ctx(&inv)).expect("planned");
    let Work::Snapshot(request) = planned.work else {
        panic!("a picture")
    };
    assert!(!request.interface && !request.save);
    assert_eq!(request.format, super::picture::Format::Png);
    let mut png = Vec::new();
    image::RgbImage::from_pixel(256, 256, image::Rgb([9, 9, 9]))
        .write_with_encoder(image::codecs::png::PngEncoder::new(&mut png))
        .expect("png");
    let (c, answer) = call(SNAPSHOT, args, false);
    desk.uploads().stage_snapshot(c, Ok(png), &ctx(&inv), Instant::now());
    let deadline = Instant::now() + Duration::from_secs(10);
    let plan = loop {
        if let Ok(result) = answer.try_recv() {
            break result.expect("planned");
        }
        assert!(Instant::now() < deadline);
        desk.tick_uploads(&ctx(&inv), Instant::now());
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(plan["total"], 10);
    assert_eq!(plan["files"][0]["kind"], "snapshot");
    assert_eq!(plan["files"][0]["name"], "Shop front");
    assert_eq!(plan["thumbnail_id"], DEMO_FOLDER.to_string());
    assert_eq!(
        uploads::extra_classes(EXECUTE, &json!({"frozen": {"thumbnail": true}})),
        vec![Class::Edit]
    );
    assert!(uploads::extra_classes(EXECUTE, &json!({"frozen": {"thumbnail": false}})).is_empty());
    // the picture without Edit: the execute is refused
    let mut no_edit = allow_all();
    no_edit.set(Class::Edit, Level::Never);
    desk.set_permissions(no_edit);
    let id = plan["plan_id"].as_str().expect("id");
    let refused = at_once(&mut desk, &inv, EXECUTE, json!({"plan_id": id, "client_request_id": "snap"}));
    assert_eq!(refused.expect_err("no edit").code, ErrorCode::PermissionDenied);
    // a link's picture is never set; never the interface
    let link = inv.items.values().find(|i| i.asset_type == 24).map(|i| i.id).expect("a link");
    let (c, _) = call(
        SNAPSHOT,
        json!({"folder_id": DEMO_FOLDER.to_string(), "thumbnail_id": link.to_string()}),
        false,
    );
    assert_eq!(
        desk.uploads().snapshot_plan(&c, &ctx(&inv)).err().map(|e| e.code),
        Some(ErrorCode::NotAllowed)
    );
    let (c, _) = call(SNAPSHOT, json!({"interface": true}), false);
    assert!(desk.uploads().snapshot_plan(&c, &ctx(&inv)).is_err(), "never the interface");
}

#[test]
fn the_mesh_review_takes_a_local_collada_file_only() {
    let inv = demo_inventory();
    let dir = tempfile::tempdir().expect("temp dir");
    let dae = dir.path().join("chair.dae");
    std::fs::write(&dae, b"<COLLADA/>").expect("write");
    let png = dir.path().join("chair.png");
    std::fs::write(&png, b"x").expect("write");
    let plan = |path: &str| {
        let (c, _) = call(MESH_REVIEW, json!({"path": path}), false);
        Desk::default().upload_plan(&c, &ctx(&inv))
    };
    let planned = plan(&dae.display().to_string()).expect("planned");
    assert!(matches!(planned.work, Work::MeshReview(ref p) if p.ends_with("chair.dae")));
    assert_eq!(planned.reply["uploaded"], false);
    assert!(
        !planned.reply.to_string().contains(&dir.path().display().to_string()),
        "no path in the answer"
    );
    assert_eq!(
        plan(&png.display().to_string()).err().map(|e| e.code),
        Some(ErrorCode::InvalidArguments)
    );
    assert_eq!(plan("chair.dae").err().map(|e| e.code), Some(ErrorCode::InvalidArguments));
}

#[test]
fn camera_get_reads_where_the_camera_is() {
    let pose = Pose {
        position: glam::Vec3::new(10.0, 20.0, 30.0),
        target: glam::Vec3::new(13.0, 24.0, 30.0),
        fov: 1.0,
    };
    let stage = Stage {
        in_world: true,
        avatar: Some(glam::Vec3::new(12.0, 22.0, 29.0)),
        ..Default::default()
    };
    let answer = camera::get(&pose, &stage).expect("read");
    assert_eq!(answer["camera"], json!([10.0, 20.0, 30.0]));
    assert_eq!(answer["focus"], json!([13.0, 24.0, 30.0]));
    assert_eq!(answer["distance"], 5.0);
    assert_eq!(answer["avatar"], json!([12.0, 22.0, 29.0]));
    let away = Stage { in_world: false, ..stage };
    assert_eq!(camera::get(&pose, &away).expect_err("not in world").code, ErrorCode::NotReady);
}

// --- end to end ---

#[test]
fn an_assistant_previews_and_uploads_a_folder_end_to_end() {
    use folderstorm_mcp::connect::Viewer;
    use folderstorm_mcp::jsonrpc::{Backend, Server as Rpc};
    use folderstorm_mcp_bridge::Bridge;
    use folderstorm_mcp_bridge::server::Config;

    struct Live(Viewer);
    impl Backend for Live {
        fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
            self.0.call(tool, arguments)
        }
        fn client_name(&mut self, name: &str) {
            self.0.set_client_name(name);
        }
    }

    let (_files, folder) = fixtures();
    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = Bridge::start(Config {
        discovery_dir: Some(dir.path().to_path_buf()),
        pid: std::process::id(),
        version: "test".into(),
        permissions: Preset::AskBeforeChanges.permissions(),
        viewer_timeout: Duration::from_secs(20),
    })
    .expect("bridge started");
    let discovery = dir.path().to_path_buf();
    let arguments = whole(&folder);
    let assistant = std::thread::spawn(move || {
        let mut server = Rpc::new(Live(Viewer::new(Some(discovery), None)));
        let mut ask = |line: String| -> Value { serde_json::from_str(&server.handle_line(&line).expect("answer")).expect("json") };
        ask(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code"}}}"#.into());
        let tool = |id: u32, name: &str, arguments: Value| {
            json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}}).to_string()
        };
        let text = |answer: &Value| -> Value {
            serde_json::from_str(answer["result"]["content"][0]["text"].as_str().unwrap_or("null")).unwrap_or(Value::Null)
        };
        let plan = text(&ask(tool(2, PREVIEW, arguments)));
        let early = text(&ask(tool(
            3,
            EXECUTE,
            json!({"plan_id": plan["plan_id"], "client_request_id": "e2e-1"}),
        )));
        let mut status = Value::Null;
        for n in 0..400 {
            status = text(&ask(tool(10 + n, STATUS, json!({"plan_id": plan["plan_id"]}))));
            if status["state"] != "running" && status["state"] != "asking" {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        (plan, early, status)
    });
    let mut inv = demo_inventory();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions(), account(10, Some(500)));
    let mut questions = 0;
    let deadline = Instant::now() + Duration::from_secs(60);
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        while let Ok(pending) = calls.try_recv() {
            desk.take(pending, &ctx(&inv), now);
        }
        if let Some(q) = desk.question(now) {
            questions += 1;
            desk.decide(q.number, true);
        }
        for run in desk.tick(&IDLE, now) {
            assert!(matches!(desk.upload_plan(&run, &ctx(&inv)).expect("planned").work, Work::Upload));
            desk.upload_start(run, now);
        }
        desk.tick_uploads(&ctx(&inv), now);
        for act in desk.uploads().take_acts() {
            match act {
                Act::Folder { request, change } => {
                    let reply = actions::demo_mutate(&mut inv, AGENT, change);
                    desk.uploads().folder_result(request, reply, &mut inv);
                }
                Act::Upload { request, upload } => {
                    if let Some(item) = desk.uploads().uploaded(request, Ok(receipt_for(&upload))) {
                        inv.add_items(vec![item]);
                    }
                }
                Act::Thumbnail { .. } => {}
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let (plan, early, status) = assistant.join().expect("assistant");
    assert_eq!(plan["total"], 30, "{plan}");
    assert_eq!(early["early"], true, "{early}");
    assert_eq!(status["state"], "done", "{status}");
    assert_eq!(status["spent"], 30);
    assert_eq!(questions, 1, "one dialog, with the price");
    let names = ["Red square", "Blue square", "Chime"];
    assert_eq!(inv.items.values().filter(|i| names.contains(&i.name.as_str())).count(), 3);
    bridge.stop();
}
