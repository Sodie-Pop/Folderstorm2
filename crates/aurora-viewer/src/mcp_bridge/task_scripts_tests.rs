//! Folderstorm: the task script tools (folderstorm/plans/mcp-task-scripts.md)
//! on the demo plaza's cut box, with a small simulator for its contents,
//! its scripts' sources, Experience associations and running states: the
//! permissions matrix, RLVa, the snapshot and its stale checks (the
//! upload's asset winning over a lagging listing), saves (compiled, with
//! errors, failed, unconfirmed then late), the extra classes, start, stop
//! and reset, status, the feed through the desk, and end to end over a
//! real socket.

use super::changes::Work;
use super::desk::{Bake, Desk};
use super::objects::{Scene, Want};
use super::task_scripts::{Action, Send};
use super::tools::{Ctx, Session};
use crate::demo::DEMO_AGENT;
use crate::world::World;
use crate::world::inventory::{FetchState, Folder, actions};
use aurora_net::build::{ObjectProps, TaskItem};
use aurora_net::inventory::InvFolder;
use aurora_net::scripts::experiences::ScriptExperiences;
use aurora_net::scripts::{ScriptSaved, ScriptTarget};
use aurora_net::{ChatMessage, ChatSourceType, ChatType, NetEvent};
use folderstorm_mcp_bridge::protocol::objects::{LIST, STATUS as OPERATION_STATUS};
use folderstorm_mcp_bridge::protocol::task_scripts::{DEBUG_POLL, GET, PREVIEW, RESET, SAVE, SET_RUNNING, STATUS};
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{Class, ErrorCode, Level, Permissions, Preset, ToolError};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use uuid::Uuid;

const BOX_LOCAL: u32 = 118;
const STRANGER: Uuid = Uuid::from_u128(0x5747);
const EXPERIENCE: Uuid = Uuid::from_u128(0xE4E4);
const IDLE: Bake = Bake {
    outfit_change_pending: false,
    cof_version: 3,
    appearance_cof_version: 3,
    server_bakes: true,
};
const SOURCE: &str = "default\n{\n    state_entry()\n    {\n        llOwnerSay(\"ready\");\n    }\n}\n";

fn prim_id(local: u32) -> Uuid {
    Uuid::from_u128(0xA0E0_0000_0000_0000_0000_0000_0000_0000 | local as u128)
}

fn the_box() -> Uuid {
    prim_id(BOX_LOCAL)
}

fn script_id(n: u128) -> Uuid {
    Uuid::from_u128(0x7A5C_0000 + n)
}

/// The demo region and its prims, the avatar next to the cut box.
fn plaza() -> World {
    let mut world = World::new(Arc::new(crate::scene::avatar::AvatarLibrary::load()));
    world.agent_id = DEMO_AGENT;
    for ev in crate::demo::events() {
        if matches!(
            ev,
            NetEvent::MainRegionChanged { .. } | NetEvent::RegionHandshake(_) | NetEvent::ObjectUpdates { .. }
        ) {
            let _ = world.apply(ev);
        }
    }
    let root = Uuid::from_u128(80);
    world.inventory.root = root;
    world.inventory.owner = DEMO_AGENT;
    world.inventory.folders.insert(
        root,
        Folder {
            info: InvFolder {
                id: root,
                name: "My Inventory".into(),
                type_default: 8,
                ..Default::default()
            },
            children: vec![],
            items: vec![],
            state: FetchState::Fetched,
            library: false,
        },
    );
    let idx = world.objects.index_of_uuid(&the_box()).expect("the box");
    let at = world.objects.get(idx).map(|o| o.position).expect("placed");
    world.agent.position = at + glam::Vec3::new(2.0, 0.0, 0.0);
    world
}

fn ctx(world: &World) -> Ctx<'_> {
    ctx_with(world, HashSet::new())
}

fn ctx_with(world: &World, open: HashSet<Uuid>) -> Ctx<'_> {
    Ctx {
        inv: &world.inventory,
        session: Session::Demo,
        worn: HashSet::new(),
        attachment_points: HashMap::new(),
        inventory_locked: false,
        protected: HashSet::new(),
        outfit_points: HashMap::new(),
        stage: Default::default(),
        viewer_session: None,
        identity: Default::default(),
        nav: Default::default(),
        content: super::content::Limits {
            open,
            ..super::content::Limits::of(&world.rlva)
        },
        objects: Scene {
            world: Some(world),
            props: &[],
        },
        avatar: Default::default(),
    }
}

fn allow_all() -> Permissions {
    let mut p = Permissions::default();
    for class in Class::ALL {
        p.set(class, Level::Allow);
    }
    p
}

fn desk_with(permissions: Permissions) -> Desk {
    let mut desk = Desk::default();
    desk.set_permissions(permissions);
    desk
}

fn call(tool: &str, args: Value) -> (Pending, Receiver<Result<Value, ToolError>>) {
    Pending::detached(tool, args, false, Some("Claude Code"))
}

fn answered(answer: &Receiver<Result<Value, ToolError>>) -> Result<Value, ToolError> {
    answer.try_recv().expect("answered")
}

fn refused_code(answer: &Receiver<Result<Value, ToolError>>) -> ErrorCode {
    answered(answer).expect_err("refused").code
}

fn task_item(n: u128, name: &str, asset_type: i8, owner_mask: u32) -> TaskItem {
    TaskItem {
        item_id: script_id(n),
        parent_id: the_box(),
        owner_id: DEMO_AGENT,
        base_mask: 0x7fff_ffff,
        owner_mask,
        asset_id: Uuid::from_u128(0xA55E_0000 + n),
        asset_type,
        inv_type: asset_type,
        name: name.into(),
        ..Default::default()
    }
}

/// What the simulator holds and answers.
struct Sim {
    owner: Uuid,
    serial: i16,
    items: Vec<TaskItem>,
    sources: HashMap<Uuid, String>,
    experiences: HashMap<Uuid, Result<Uuid, String>>,
    running: HashMap<Uuid, bool>,
    /// Save answers given in place of a normal save.
    save_answer: Option<Result<ScriptSaved, String>>,
    /// Saves are never answered.
    silent: bool,
    /// The running state does not follow SetScriptRunning.
    stuck: bool,
}

impl Sim {
    /// The cut box: a full script (with an Experience), a no-modify script,
    /// a no-copy script, an SLua script, a notecard.
    fn plaza() -> Sim {
        let mut lua = task_item(4, "Lua", 10, 0x7fff_ffff);
        lua.flags = 1;
        let items = vec![
            task_item(1, "Door", 10, 0x7fff_ffff),
            task_item(2, "Sealed", 10, 0x7fff_ffff & !actions::MODIFY),
            task_item(3, "Only here", 10, 0x7fff_ffff & !actions::COPY),
            lua,
            task_item(5, "Read me", 7, 0x7fff_ffff),
        ];
        let mut sources = HashMap::new();
        let mut experiences = HashMap::new();
        for item in &items {
            sources.insert(item.item_id, SOURCE.to_owned());
            experiences.insert(item.item_id, Ok(Uuid::nil()));
        }
        experiences.insert(script_id(1), Ok(EXPERIENCE));
        Sim {
            owner: DEMO_AGENT,
            serial: 5,
            items,
            sources,
            experiences,
            running: HashMap::new(),
            save_answer: None,
            silent: false,
            stuck: false,
        }
    }

    fn listing(&self) -> NetEvent {
        NetEvent::TaskInventory {
            object: the_box(),
            serial: Some(self.serial),
            result: Ok(self.items.clone()),
        }
    }

    /// Answer what the tools asked for, then let the desk move on.
    fn serve(&mut self, desk: &mut Desk, world: &World, now: Instant) {
        for _ in 0..4 {
            let wants = desk.objects().wanted();
            let fetches = desk.task_scripts().wanted();
            let queries = desk.task_scripts().wanted_queries();
            if wants.is_empty() && fetches.is_empty() && queries.is_empty() {
                break;
            }
            for want in wants {
                let event = match want {
                    Want::Properties { object, .. } => NetEvent::ObjectProperties(vec![ObjectProps {
                        object_id: object,
                        owner_id: self.owner,
                        name: "Cut box".into(),
                        ..Default::default()
                    }]),
                    Want::Contents { .. } => self.listing(),
                };
                desk.task_script_observe(&event, now);
                desk.objects().observe(&event, now);
            }
            for fetch in fetches {
                let target = ScriptTarget {
                    object: fetch.object,
                    item: fetch.item,
                };
                desk.task_script_observe(
                    &NetEvent::ScriptExperiences {
                        request: fetch.experience_request,
                        target,
                        result: ScriptExperiences {
                            associated: self.experiences.get(&fetch.item).cloned().unwrap_or(Ok(Uuid::nil())),
                            available: Ok(vec![]),
                        },
                    },
                    now,
                );
                desk.task_script_observe(
                    &NetEvent::ScriptLoaded {
                        request: fetch.source_request,
                        target,
                        result: self.sources.get(&fetch.item).cloned().ok_or_else(|| "missing".to_owned()),
                    },
                    now,
                );
            }
            for query in queries {
                let running = self.running.get(&query.item).copied().unwrap_or(true);
                desk.task_script_observe(
                    &NetEvent::ScriptRunning {
                        object_id: query.object,
                        item_id: query.item,
                        running,
                        mono: Some(true),
                        luau: aurora_net::scripts::Luau::Off,
                    },
                    now,
                );
            }
            desk.tick_reads(world, now);
        }
    }

    /// The simulator receives a planned change (the upload's answer comes
    /// back as `ScriptSaved`).
    fn apply(&mut self, desk: &mut Desk, send: &Send, request: Option<Uuid>, now: Instant) {
        match &send.action {
            Action::Save {
                source,
                running,
                experience,
                ..
            } => {
                assert_eq!(
                    *experience,
                    self.experiences[&send.item].clone().unwrap_or_default(),
                    "the association kept"
                );
                if self.silent {
                    return;
                }
                let result = self.save_answer.clone().unwrap_or_else(|| {
                    let compiled = !source.contains("SYNTAX_ERROR");
                    let asset = Uuid::new_v4();
                    if let Some(item) = self.items.iter_mut().find(|i| i.item_id == send.item) {
                        item.asset_id = asset;
                    }
                    self.serial += 1;
                    self.sources.insert(send.item, source.clone());
                    self.running.insert(send.item, compiled && *running);
                    Ok(ScriptSaved {
                        asset,
                        compiled,
                        checked: true,
                        errors: if compiled {
                            vec![]
                        } else {
                            vec!["(3, 8) : ERROR : Syntax error".into()]
                        },
                    })
                });
                desk.task_script_observe(
                    &NetEvent::ScriptSaved {
                        request: request.expect("a save's request"),
                        target: ScriptTarget {
                            object: send.object,
                            item: send.item,
                        },
                        result,
                    },
                    now,
                );
            }
            Action::Running(running) => {
                if !self.stuck {
                    self.running.insert(send.item, *running);
                }
                // the app asks the state right after, as the editor does
                self.reply_running(desk, send.item, now);
            }
            Action::Reset => self.reply_running(desk, send.item, now),
        }
    }

    fn reply_running(&self, desk: &mut Desk, item: Uuid, now: Instant) {
        desk.task_script_observe(
            &NetEvent::ScriptRunning {
                object_id: the_box(),
                item_id: item,
                running: self.running.get(&item).copied().unwrap_or(true),
                mono: Some(true),
                luau: aurora_net::scripts::Luau::Off,
            },
            now,
        );
    }
}

fn take(desk: &mut Desk, world: &World, tool: &str, args: Value, now: Instant) -> Receiver<Result<Value, ToolError>> {
    let (c, answer) = call(tool, args);
    desk.take(c, &ctx(world), now);
    answer
}

fn snapshot(desk: &mut Desk, world: &World, sim: &mut Sim, now: Instant) -> String {
    let answer = take(desk, world, LIST, json!({"object_id": the_box().to_string()}), now);
    sim.serve(desk, world, now);
    let page = answered(&answer).expect("listed");
    page["snapshot_id"].as_str().expect("a snapshot").to_owned()
}

fn get(desk: &mut Desk, world: &World, sim: &mut Sim, item: Uuid, now: Instant) -> Result<Value, ToolError> {
    let snap = snapshot(desk, world, sim, now);
    let answer = take(
        desk,
        world,
        GET,
        json!({"object_id": the_box().to_string(), "snapshot_id": snap, "item_id": item.to_string()}),
        now,
    );
    sim.serve(desk, world, now);
    desk.tick_reads(world, now);
    answered(&answer)
}

fn save_args(read: &Value, text: &str, request: &str) -> Value {
    json!({
        "object_id": the_box().to_string(),
        "item_id": read["item_id"],
        "snapshot_hash": read["snapshot_hash"],
        "text": text,
        "client_request_id": request,
    })
}

/// The change the desk lets run now, planned again as just before the
/// send, and sent (to the simulator).
fn run(desk: &mut Desk, world: &World, sim: &mut Sim, now: Instant) -> Option<Send> {
    let call = desk.tick(&IDLE, now).into_iter().next()?;
    let login = desk.login();
    let call = desk.recheck(call, login, None, &ctx(world), now)?;
    let planned = match desk.task_script_plan(&call, &ctx(world), now) {
        Ok(planned) => planned,
        Err(e) => {
            call.answer(Err(e));
            return None;
        }
    };
    let Work::TaskScript(send) = planned.work else {
        panic!("not a task script change");
    };
    let request = desk.task_scripts().sending(call, &send, planned.reply, now);
    sim.apply(desk, &send, request, now);
    Some(*send)
}

fn edited() -> String {
    SOURCE.replace("ready", "ready again")
}

#[test]
fn a_read_gives_the_source_and_a_hash_never_an_asset_id() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    let now = Instant::now();
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    assert_eq!(read["text"], SOURCE);
    assert_eq!(read["editable"], true, "{read}");
    assert_eq!(read["complete"], true);
    assert_eq!(read["experience"], "kept");
    assert!(read["snapshot_hash"].as_str().is_some_and(|h| h.len() == 64));
    assert!(read["note"].as_str().is_some_and(|n| n.contains("data")));
    let text = read.to_string();
    for item in &sim.items {
        assert!(!text.contains(&item.asset_id.to_string()), "no asset id: {text}");
    }
    assert!(!text.contains(&EXPERIENCE.to_string()), "no association id: {text}");
}

#[test]
fn reads_follow_the_script_editor_rights_and_the_prim_rules() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    let now = Instant::now();
    // no Modify, no Copy: never read
    for item in [script_id(2), script_id(3)] {
        let e = get(&mut desk, &world, &mut sim, item, now).expect_err("refused");
        assert_eq!(e.code, ErrorCode::NotAllowed, "{e}");
        assert!(e.message.contains("Copy and Modify"), "{e}");
    }
    // a notecard is not a script
    let e = get(&mut desk, &world, &mut sim, script_id(5), now).expect_err("refused");
    assert_eq!(e.code, ErrorCode::InvalidArguments);
    // SLua and an unknown association: readable, not editable
    let lua = get(&mut desk, &world, &mut sim, script_id(4), now).expect("read");
    assert_eq!(lua["editable"], false);
    assert!(lua["not_editable"].as_str().is_some_and(|w| w.contains("SLua")));
    assert!(lua["snapshot_hash"].is_null());
    sim.experiences.insert(script_id(1), Err("no capability".into()));
    let unknown = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    assert_eq!(unknown["editable"], false);
    assert_eq!(unknown["experience"], "unknown");
    assert!(unknown["not_editable"].as_str().is_some_and(|w| w.contains("Experience")));
    // someone else's prim: refused before anything is asked
    let mut stranger = Sim::plaza();
    stranger.owner = STRANGER;
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    let answer = take(&mut desk, &world, LIST, json!({"object_id": the_box().to_string()}), now);
    stranger.serve(&mut desk, &world, now);
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
}

#[test]
fn viewscript_refuses_reads_and_saves_and_edit_refuses_saves() {
    let mut world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    for (rule, tool, args) in [
        (
            "viewscript",
            GET,
            json!({"object_id": the_box().to_string(), "snapshot_id": snap, "item_id": script_id(1).to_string()}),
        ),
        ("viewscript", SAVE, save_args(&read, &edited(), "rlva-1")),
        ("edit", SAVE, save_args(&read, &edited(), "rlva-2")),
    ] {
        let mut cx = ctx(&world);
        match rule {
            "viewscript" => cx.content.viewscript = true,
            _ => cx.content.edit = true,
        }
        let (c, answer) = call(tool, args);
        desk.take(c, &cx, now);
        let e = answered(&answer).expect_err("refused");
        assert_eq!(e.code, ErrorCode::NotAllowed, "{rule} {tool}");
        assert!(e.message.contains(&format!("@{rule}")), "{rule}: {e}");
    }
    // the viewer's own @edit refuses the prim itself
    world.rlva.configure(true);
    world.rlva_console_input("@edit=n");
    let answer = take(&mut desk, &world, LIST, json!({"object_id": the_box().to_string()}), now);
    sim.serve(&mut desk, &world, now);
    let e = answered(&answer).expect_err("refused");
    assert!(e.message.contains("@edit"), "{e}");
    world.rlva_console_input("@clear");
}

#[test]
fn a_save_asks_keeps_the_experience_and_answers_with_the_uploads_snapshot() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let now = Instant::now();
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    let answer = take(&mut desk, &world, SAVE, save_args(&read, &edited(), "save-1"), now);
    let q = desk.question(now).expect("asks");
    assert!(q.summary.contains("Door") && q.summary.contains("Cut box"), "{}", q.summary);
    assert!(q.summary.contains("stopped"), "{}", q.summary);
    assert!(q.detail.iter().any(|l| l.contains("ready again")), "{:?}", q.detail);
    desk.decide(q.number, true);
    let send = run(&mut desk, &world, &mut sim, now).expect("sent");
    assert!(matches!(
        send.action,
        Action::Save {
            running: false,
            experience: EXPERIENCE,
            ..
        }
    ));
    let reply = answered(&answer).expect("saved");
    assert_eq!(reply["source_saved"], true, "{reply}");
    assert_eq!(reply["compiled"], true);
    assert_eq!(reply["requested_running"], false);
    assert!(reply["errors"].as_array().is_some_and(Vec::is_empty));
    let hash = reply["snapshot_hash"].as_str().expect("a new snapshot").to_owned();
    assert_ne!(json!(hash), read["snapshot_hash"]);
    // the old snapshot ended with the upload
    let again = take(&mut desk, &world, SAVE, save_args(&read, "default {}", "save-2"), now);
    assert_eq!(refused_code(&again), ErrorCode::Stale);
    // the new one previews against the saved text
    let preview = take(
        &mut desk,
        &world,
        PREVIEW,
        json!({"object_id": the_box().to_string(), "item_id": read["item_id"], "snapshot_hash": hash, "text": edited()}),
        now,
    );
    let preview = answered(&preview).expect("preview");
    assert_eq!(preview["unchanged"], true, "{preview}");
    assert_eq!(preview["save"], "ask");
    // the Contents tab learns the upload's asset; the prim is listed again
    let updates = desk.task_scripts().take_updates();
    assert_eq!(updates.len(), 1);
    assert_eq!((updates[0].0, updates[0].1), (the_box(), script_id(1)));
    desk.tick_reads(&world, now);
    assert!(desk.objects().wanted().iter().any(|w| matches!(w, Want::Contents { .. })));
}

#[test]
fn the_uploads_asset_wins_over_a_lagging_listing_but_not_a_third_asset() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let before = sim.items[0].clone();
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    let answer = take(&mut desk, &world, SAVE, save_args(&read, &edited(), "lag-1"), now);
    run(&mut desk, &world, &mut sim, now).expect("sent");
    let hash = answered(&answer).expect("saved")["snapshot_hash"].clone();
    let preview = |desk: &mut Desk| {
        let a = take(
            desk,
            &world,
            PREVIEW,
            json!({"object_id": the_box().to_string(), "item_id": read["item_id"], "snapshot_hash": hash, "text": "x"}),
            now,
        );
        answered(&a)
    };
    // a listing that still shows the asset from before the save
    let lagging = NetEvent::TaskInventory {
        object: the_box(),
        serial: Some(5),
        result: Ok(vec![before.clone()]),
    };
    desk.task_script_observe(&lagging, now);
    assert!(preview(&mut desk).is_ok(), "a lagging listing is ignored");
    // the listing catches up
    desk.task_script_observe(&sim.listing(), now);
    desk.task_script_observe(&lagging, now);
    assert_eq!(
        preview(&mut desk).expect_err("stale").code,
        ErrorCode::Stale,
        "once caught up, the old asset is another change"
    );
    // a fresh read, then a third asset (saved elsewhere)
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    let mut elsewhere = sim.items[0].clone();
    elsewhere.asset_id = Uuid::from_u128(0xE15E);
    desk.task_script_observe(
        &NetEvent::TaskInventory {
            object: the_box(),
            serial: Some(9),
            result: Ok(vec![elsewhere]),
        },
        now,
    );
    let answer = take(&mut desk, &world, SAVE, save_args(&read, &edited(), "lag-2"), now);
    assert_eq!(refused_code(&answer), ErrorCode::Stale);
}

#[test]
fn a_compile_failure_is_a_saved_source_and_a_failed_upload_saved_nothing() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    let answer = take(&mut desk, &world, SAVE, save_args(&read, "default { SYNTAX_ERROR }", "bad-1"), now);
    run(&mut desk, &world, &mut sim, now).expect("sent");
    let reply = answered(&answer).expect("saved");
    assert_eq!(
        (reply["source_saved"].as_bool(), reply["compiled"].as_bool()),
        (Some(true), Some(false))
    );
    assert_eq!(reply["errors"][0]["line"], 4);
    assert_eq!(reply["errors"][0]["column"], 9);
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    sim.save_answer = Some(Err("the server refused".into()));
    let answer = take(&mut desk, &world, SAVE, save_args(&read, &edited(), "bad-2"), now);
    run(&mut desk, &world, &mut sim, now).expect("sent");
    assert_eq!(refused_code(&answer), ErrorCode::Failed);
    let status = take(&mut desk, &world, OPERATION_STATUS, json!({"client_request_id": "bad-2"}), now);
    let status = answered(&status).expect("status");
    assert_eq!(status["script"]["source_saved"], false, "{status}");
    let status = take(&mut desk, &world, OPERATION_STATUS, json!({"client_request_id": "bad-1"}), now);
    let status = answered(&status).expect("status");
    assert_eq!(status["script"]["compiled"], false, "{status}");
}

#[test]
fn an_unanswered_save_is_unconfirmed_and_a_late_answer_settles_it() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    sim.silent = true;
    let answer = take(&mut desk, &world, SAVE, save_args(&read, &edited(), "late-1"), now);
    let send = run(&mut desk, &world, &mut sim, now).expect("sent");
    assert!(answer.try_recv().is_err(), "waits for the upload");
    let later = now + Duration::from_secs(31);
    desk.tick_reads(&world, later);
    assert_eq!(refused_code(&answer), ErrorCode::Timeout);
    let status = take(&mut desk, &world, OPERATION_STATUS, json!({"client_request_id": "late-1"}), later);
    let status = answered(&status).expect("status");
    assert_eq!(status["receipt"]["state"], "unconfirmed", "{status}");
    assert!(status["script"]["source_saved"].is_null(), "{status}");
    // the same id again answers the receipt and sends nothing
    let again = take(&mut desk, &world, SAVE, save_args(&read, &edited(), "late-1"), later);
    let again = answered(&again);
    assert!(desk.tick(&IDLE, later).is_empty(), "never sent twice: {again:?}");
    // the late answer
    sim.silent = false;
    let request = desk.task_scripts().late_request_for_tests();
    sim.apply(&mut desk, &send, request, later);
    let status = take(&mut desk, &world, OPERATION_STATUS, json!({"client_request_id": "late-1"}), later);
    let status = answered(&status).expect("status");
    assert_eq!(status["script"]["source_saved"], true, "{status}");
    assert_ne!(status["receipt"]["state"], "unconfirmed", "{status}");
}

#[test]
fn a_save_needs_read_and_running_needs_scripts() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let now = Instant::now();
    let mut levels = allow_all();
    levels.set(Class::Scripts, Level::Never);
    let mut desk = desk_with(levels);
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    let mut args = save_args(&read, &edited(), "run-1");
    args["running"] = json!(true);
    let answer = take(&mut desk, &world, SAVE, args.clone(), now);
    assert_eq!(refused_code(&answer), ErrorCode::PermissionDenied);
    // Scripts on Ask: the save asks, although Content and Objects allow
    levels.set(Class::Scripts, Level::Ask);
    desk.set_permissions(levels);
    args["client_request_id"] = json!("run-2");
    let answer = take(&mut desk, &world, SAVE, args.clone(), now);
    let q = desk.question(now).expect("asks");
    assert!(q.summary.contains("running"), "{}", q.summary);
    desk.decide(q.number, true);
    let send = run(&mut desk, &world, &mut sim, now).expect("sent");
    assert!(matches!(send.action, Action::Save { running: true, .. }));
    assert_eq!(answered(&answer).expect("saved")["requested_running"], true);
    // stopped: no Scripts needed; Read on Never refuses the save
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    levels.set(Class::Scripts, Level::Never);
    levels.set(Class::Read, Level::Never);
    desk.set_permissions(levels);
    let answer = take(&mut desk, &world, SAVE, save_args(&read, &edited(), "run-3"), now);
    assert_eq!(refused_code(&answer), ErrorCode::PermissionDenied);
}

#[test]
fn an_open_editor_makes_a_save_busy_and_text_is_bounded() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    let (c, answer) = call(SAVE, save_args(&read, &edited(), "open-1"));
    desk.take(c, &ctx_with(&world, HashSet::from([script_id(1)])), now);
    assert_eq!(refused_code(&answer), ErrorCode::Busy);
    let long = "x".repeat(folderstorm_mcp_bridge::protocol::content::TEXT_MAX + 1);
    let answer = take(&mut desk, &world, SAVE, save_args(&read, &long, "long-1"), now);
    assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments);
    let answer = take(&mut desk, &world, SAVE, save_args(&read, "a\0b", "nul-1"), now);
    assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments);
    let mut args = save_args(&read, &edited(), "x");
    if let Some(a) = args.as_object_mut() {
        a.remove("client_request_id");
    }
    let answer = take(&mut desk, &world, SAVE, args, now);
    assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments);
    let mut args = save_args(&read, &edited(), "frozen-1");
    args["frozen_content"] = json!({"kind": "task_script"});
    let answer = take(&mut desk, &world, SAVE, args, now);
    assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments, "only the desk freezes");
}

#[test]
fn start_stop_and_reset_report_what_the_simulator_says() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let args = |item: Uuid, running: Option<bool>, request: &str| {
        let mut a =
            json!({"object_id": the_box().to_string(), "snapshot_id": snap, "item_id": item.to_string(), "client_request_id": request});
        if let Some(r) = running {
            a["running"] = json!(r);
        }
        a
    };
    let answer = take(&mut desk, &world, SET_RUNNING, args(script_id(1), Some(false), "stop-1"), now);
    let send = run(&mut desk, &world, &mut sim, now).expect("sent");
    assert_eq!(send.action, Action::Running(false));
    let reply = answered(&answer).expect("stopped");
    assert_eq!(
        (reply["done"].as_bool(), reply["observed_running"].as_bool()),
        (Some(true), Some(false))
    );
    // the simulator does not follow: unconfirmed with what it reports
    sim.stuck = true;
    let answer = take(&mut desk, &world, SET_RUNNING, args(script_id(1), Some(true), "start-1"), now);
    run(&mut desk, &world, &mut sim, now).expect("sent");
    assert!(answer.try_recv().is_err(), "waits for the state asked");
    // asked again after 2 s
    desk.tick_reads(&world, now + Duration::from_secs(2));
    assert_eq!(desk.task_scripts().wanted_queries().len(), 1);
    desk.tick_reads(&world, now + Duration::from_secs(31));
    let reply = answered(&answer).expect("answered");
    assert_eq!(reply["unconfirmed"], true, "{reply}");
    assert_eq!(reply["observed_running"], false);
    // a reset: never acknowledged as such
    let answer = take(&mut desk, &world, RESET, args(script_id(1), None, "reset-1"), now);
    let send = run(&mut desk, &world, &mut sim, now).expect("sent");
    assert_eq!(send.action, Action::Reset);
    let reply = answered(&answer).expect("reset");
    assert_eq!(
        (reply["done"].as_bool(), reply["reset_confirmed"].as_bool()),
        (Some(true), Some(false))
    );
    // Modify needed (a no-copy script may still be run), not a notecard
    let answer = take(&mut desk, &world, RESET, args(script_id(2), None, "reset-2"), now);
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    let answer = take(&mut desk, &world, RESET, args(script_id(3), None, "reset-3"), now);
    run(&mut desk, &world, &mut sim, now).expect("sent");
    assert!(answered(&answer).is_ok());
    let answer = take(&mut desk, &world, RESET, args(script_id(5), None, "reset-4"), now);
    assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments);
    // the id is required; Scripts on Never refuses
    let mut a = args(script_id(1), None, "x");
    if let Some(a) = a.as_object_mut() {
        a.remove("client_request_id");
    }
    let answer = take(&mut desk, &world, RESET, a, now);
    assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments);
    let mut levels = allow_all();
    levels.set(Class::Scripts, Level::Never);
    desk.set_permissions(levels);
    let answer = take(&mut desk, &world, RESET, args(script_id(1), None, "reset-5"), now);
    assert_eq!(refused_code(&answer), ErrorCode::PermissionDenied);
}

#[test]
fn start_stop_and_reset_never_always_ask_but_ask_on_ask() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let now = Instant::now();
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let _answer = take(
        &mut desk,
        &world,
        RESET,
        json!({"object_id": the_box().to_string(), "snapshot_id": snap, "item_id": script_id(1).to_string(), "client_request_id": "ask-1"}),
        now,
    );
    let q = desk.question(now).expect("asks");
    assert!(q.summary.contains("Reset") && q.summary.contains("Door"), "{}", q.summary);
    desk.decide(q.number, false);
    desk.tick(&IDLE, now);
    let mut desk = desk_with(allow_all());
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    let _answer = take(
        &mut desk,
        &world,
        RESET,
        json!({"object_id": the_box().to_string(), "snapshot_id": snap, "item_id": script_id(1).to_string(), "client_request_id": "ask-2"}),
        now,
    );
    assert!(desk.question(now).is_none(), "Allow runs without a dialog");
}

#[test]
fn status_reads_the_simulators_reply() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    let now = Instant::now();
    let snap = snapshot(&mut desk, &world, &mut sim, now);
    sim.running.insert(script_id(1), false);
    let answer = take(
        &mut desk,
        &world,
        STATUS,
        json!({"object_id": the_box().to_string(), "snapshot_id": snap, "item_id": script_id(1).to_string()}),
        now,
    );
    sim.serve(&mut desk, &world, now);
    let reply = answered(&answer).expect("status");
    assert_eq!((reply["running"].as_bool(), reply["mono"].as_bool()), (Some(false), Some(true)));
    // no reply: timeout
    let answer = take(
        &mut desk,
        &world,
        STATUS,
        json!({"object_id": the_box().to_string(), "snapshot_id": snap, "item_id": script_id(1).to_string()}),
        now,
    );
    desk.task_scripts().wanted_queries();
    desk.tick_reads(&world, now + Duration::from_secs(21));
    assert_eq!(refused_code(&answer), ErrorCode::Timeout);
}

#[test]
fn the_feed_is_polled_through_the_desk_and_cleared_at_a_login_change() {
    let world = plaza();
    let mut desk = desk_with(Preset::ReadOnly.permissions());
    let now = Instant::now();
    super::script_feed::set_capturing(true);
    super::script_feed::clear();
    let line = |owner: Uuid, text: &str| ChatMessage {
        from_name: "Cut box".into(),
        source_id: the_box(),
        owner_id: owner,
        source_type: ChatSourceType::Object,
        chat_type: ChatType::OwnerSay,
        position: glam::Vec3::ZERO,
        message: text.into(),
    };
    let first = answered(&take(&mut desk, &world, DEBUG_POLL, json!({"start": "now"}), now)).expect("polled");
    super::script_feed::capture(&line(DEMO_AGENT, "ready"), DEMO_AGENT);
    super::script_feed::capture(&line(STRANGER, "not mine"), DEMO_AGENT);
    let page = answered(&take(&mut desk, &world, DEBUG_POLL, json!({"cursor": first["next_cursor"]}), now)).expect("polled");
    assert_eq!(page["events"].as_array().map(Vec::len), Some(1), "{page}");
    assert_eq!(page["events"][0]["text"], "ready");
    assert_eq!(page["events"][0]["object_id"], json!(the_box()));
    desk.set_login(desk.login() + 1, None);
    let stale = take(&mut desk, &world, DEBUG_POLL, json!({"cursor": page["next_cursor"]}), now);
    assert_eq!(refused_code(&stale), ErrorCode::Stale);
    // Read on Never refuses it
    let mut levels = Permissions::default();
    levels.set(Class::Read, Level::Never);
    desk.set_permissions(levels);
    let answer = take(&mut desk, &world, DEBUG_POLL, json!({"start": "retained"}), now);
    assert_eq!(refused_code(&answer), ErrorCode::PermissionDenied);
    super::script_feed::set_capturing(false);
}

#[test]
fn a_login_change_and_access_off_end_the_snapshots() {
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(allow_all());
    let now = Instant::now();
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    desk.close();
    desk.set_permissions(allow_all());
    let answer = take(&mut desk, &world, SAVE, save_args(&read, &edited(), "off-1"), now);
    assert_eq!(refused_code(&answer), ErrorCode::Stale);
    let read = get(&mut desk, &world, &mut sim, script_id(1), now).expect("read");
    desk.set_login(desk.login() + 1, None);
    let answer = take(&mut desk, &world, SAVE, save_args(&read, &edited(), "login-1"), now);
    assert_eq!(refused_code(&answer), ErrorCode::Stale);
}

/// End to end: the sidecar's `Viewer` over a real socket to the bridge and
/// the desk: list, read, save (allowed in the dialog), status of the
/// operation.
#[test]
fn an_assistant_reads_and_saves_a_script_in_an_owned_prim_end_to_end() {
    use folderstorm_mcp::connect::Viewer;
    use folderstorm_mcp::jsonrpc::{Backend, Server};
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

    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = Bridge::start(Config {
        discovery_dir: Some(dir.path().to_path_buf()),
        pid: std::process::id(),
        version: "test".into(),
        permissions: Preset::AskBeforeChanges.permissions(),
        viewer_timeout: Duration::from_secs(10),
    })
    .expect("bridge started");
    let folder = dir.path().to_path_buf();
    let object = the_box();
    let item = script_id(1);
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code"}}}"#,
            )
            .expect("initialized");
        let mut ask = |id: u32, name: &str, arguments: Value| -> Value {
            let line = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}});
            serde_json::from_str(&server.handle_line(&line.to_string()).expect("answer")).expect("json")
        };
        let listed = ask(2, "object_contents_list", json!({"object_id": object.to_string()}));
        let snapshot = listed["result"]["structuredContent"]["snapshot_id"].clone();
        let read = ask(
            3,
            "object_script_get",
            json!({"object_id": object.to_string(), "snapshot_id": snapshot, "item_id": item.to_string()}),
        );
        let hash = read["result"]["structuredContent"]["snapshot_hash"].clone();
        let saved = ask(
            4,
            "object_script_save_compile",
            json!({"object_id": object.to_string(), "item_id": item.to_string(), "snapshot_hash": hash, "text": SOURCE.replace("ready", "ready again"), "client_request_id": "e2e-save-1"}),
        );
        let status = ask(5, "object_operation_status", json!({"client_request_id": "e2e-save-1"}));
        (read, saved, status)
    });
    let world = plaza();
    let mut sim = Sim::plaza();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let mut questions = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        while let Ok(pending) = calls.try_recv() {
            desk.take(pending, &ctx(&world), now);
        }
        if let Some(q) = desk.question(now) {
            questions.push(q.summary.clone());
            desk.decide(q.number, true);
        }
        run(&mut desk, &world, &mut sim, now);
        sim.serve(&mut desk, &world, now);
        desk.tick_reads(&world, now);
        std::thread::sleep(Duration::from_millis(5));
    }
    let (read, saved, status) = assistant.join().expect("assistant");
    assert_eq!(read["result"]["isError"], false, "{read}");
    assert_eq!(read["result"]["structuredContent"]["editable"], true, "{read}");
    assert_eq!(saved["result"]["isError"], false, "{saved}");
    let reply = &saved["result"]["structuredContent"];
    assert_eq!(
        (reply["source_saved"].as_bool(), reply["compiled"].as_bool()),
        (Some(true), Some(true)),
        "{reply}"
    );
    assert_eq!(questions.len(), 1, "{questions:?}");
    assert!(questions[0].contains("Door"), "{questions:?}");
    assert_eq!(status["result"]["structuredContent"]["script"]["source_saved"], true, "{status}");
    assert!(sim.sources[&item].contains("ready again"));
    bridge.stop();
}
