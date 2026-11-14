//! Folderstorm: the script testing tools
//! (folderstorm/plans/mcp-script-testing.md) on the demo plaza's cut box:
//! touch (always asked, range, RLVa, owned only), the box's dialogs
//! (listed and answered, L$ and public chat refused, closed dialogs stale,
//! permission questions never listed), channel lines (always asked, the
//! rate limit, @sendchannel), recipes (shape and bounds, one approval) and
//! runs (output windows and consumption, absence, runtime errors, lost
//! lines, dialogs, running state, a class on Never, Stop with permissions
//! revoked, the run's time), and end to end over a real socket.

use super::changes::{Planned, Work};
use super::desk::{Bake, Desk};
use super::objects::{Scene, Want};
use super::script_feed;
use super::script_testing::{self, Send, parse_recipe};
use super::tools::{Ctx, Session};
use crate::demo::DEMO_AGENT;
use crate::world::World;
use crate::world::inventory::actions;
use aurora_net::build::{ObjectProps, TaskItem};
use aurora_net::{ChatMessage, ChatSourceType, ChatType, NetCommand, NetEvent};
use folderstorm_mcp_bridge::protocol::objects::LIST;
use folderstorm_mcp_bridge::protocol::script_testing::{BEGIN, CHANNEL_SEND, DIALOG_LIST, DIALOG_REPLY, STATUS, STOP, TOUCH};
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
const IDLE: Bake = Bake {
    outfit_change_pending: false,
    cof_version: 3,
    appearance_cof_version: 3,
    server_bakes: true,
};

fn the_box() -> Uuid {
    Uuid::from_u128(0xA0E0_0000_0000_0000_0000_0000_0000_0000 | BOX_LOCAL as u128)
}

fn door() -> Uuid {
    Uuid::from_u128(0x7A5C_0001)
}

fn sealed() -> Uuid {
    Uuid::from_u128(0x7A5C_0002)
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
    let idx = world.objects.index_of_uuid(&the_box()).expect("the box");
    let at = world.objects.get(idx).map(|o| o.position).expect("placed");
    world.agent.position = at + glam::Vec3::new(2.0, 0.0, 0.0);
    world
}

fn ctx(world: &World) -> Ctx<'_> {
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
        content: super::content::Limits::of(&world.rlva),
        objects: Scene {
            world: Some(world),
            props: &[],
        },
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

fn answered(answer: &Receiver<Result<Value, ToolError>>) -> Result<Value, ToolError> {
    answer.try_recv().expect("answered")
}

fn refused_code(answer: &Receiver<Result<Value, ToolError>>) -> ErrorCode {
    answered(answer).expect_err("refused").code
}

fn take(desk: &mut Desk, world: &World, tool: &str, args: Value, now: Instant) -> Receiver<Result<Value, ToolError>> {
    let (c, answer) = Pending::detached(tool, args, false, Some("Claude Code"));
    desk.take(c, &ctx(world), now);
    answer
}

fn task_item(item: Uuid, name: &str, owner_mask: u32) -> TaskItem {
    TaskItem {
        item_id: item,
        parent_id: the_box(),
        owner_id: DEMO_AGENT,
        base_mask: 0x7fff_ffff,
        owner_mask,
        asset_id: Uuid::new_v4(),
        asset_type: 10,
        inv_type: 10,
        name: name.into(),
        ..Default::default()
    }
}

/// What the simulator answers: the box's owner and contents, and its
/// scripts' running state.
struct Sim {
    owner: Uuid,
    running: bool,
    /// Running queries answered.
    replies: bool,
}

impl Sim {
    fn new() -> Sim {
        Sim {
            owner: DEMO_AGENT,
            running: true,
            replies: true,
        }
    }

    fn serve(&self, desk: &mut Desk, world: &World, now: Instant) {
        for _ in 0..4 {
            let wants = desk.objects().wanted();
            if wants.is_empty() {
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
                    Want::Contents { object, .. } => NetEvent::TaskInventory {
                        object,
                        serial: Some(3),
                        result: Ok(vec![
                            task_item(door(), "Door", 0x7fff_ffff),
                            task_item(sealed(), "Sealed", 0x7fff_ffff & !actions::MODIFY),
                        ]),
                    },
                };
                desk.objects().observe(&event, now);
            }
            desk.tick_reads(world, now);
        }
    }
}

fn snapshot(desk: &mut Desk, world: &World, sim: &Sim, now: Instant) -> Result<String, ToolError> {
    let answer = take(desk, world, LIST, json!({"object_id": the_box().to_string()}), now);
    sim.serve(desk, world, now);
    answered(&answer).map(|page| page["snapshot_id"].as_str().expect("a snapshot").to_owned())
}

/// The change the desk lets run now, planned again as just before the send.
fn next_change(desk: &mut Desk, world: &World, now: Instant) -> Option<(Pending, Planned)> {
    let call = desk.tick(&IDLE, now).into_iter().next()?;
    let login = desk.login();
    let call = desk.recheck(call, login, None, &ctx(world), now)?;
    match desk.script_test_plan(&call, &ctx(world), now) {
        Ok(planned) => Some((call, planned)),
        Err(e) => {
            call.answer(Err(e));
            None
        }
    }
}

/// The box's own answers to a send (the demo's), and the app's part:
/// a dialog answered through the viewer's notifications.
fn box_answers(world: &mut World, send: &Send) -> Vec<NetCommand> {
    let mut commands = Vec::new();
    if let Send::Dialog {
        notification,
        button,
        label,
        ..
    } = send
    {
        let response = match button {
            Some(i) => crate::world::notifications::Response::Button(*i),
            None => {
                if let Some(n) = world.notifications.list.iter_mut().find(|n| n.id == *notification) {
                    n.text.clone_from(label);
                }
                crate::world::notifications::Response::SendText
            }
        };
        commands = world.respond_notification(*notification, response).0;
    }
    for event in script_testing::demo::answers(send, DEMO_AGENT) {
        match event {
            NetEvent::Chat(message) => script_feed::capture(&message, DEMO_AGENT),
            event => {
                let _ = world.apply(event);
            }
        }
    }
    commands
}

/// The app runs the change `planned` for `call`: what it sent.
fn app(desk: &mut Desk, world: &mut World, call: Pending, planned: Planned, now: Instant) -> Vec<NetCommand> {
    match planned.work {
        Work::Interact(interaction) => {
            let commands = box_answers(world, &interaction.send);
            desk.testing().interacted(call, &interaction, planned.reply, Ok(()), now);
            commands
        }
        Work::ScriptTest(recipe) => {
            desk.testing().start(call, *recipe, now);
            Vec::new()
        }
        _ => panic!("not a script testing change"),
    }
}

/// One frame of a run: the desk moves on, the app sends what it wants and
/// the box answers.
fn pump(desk: &mut Desk, world: &mut World, sim: &Sim, now: Instant) -> Vec<Send> {
    desk.tick_reads(world, now);
    let mut sent = Vec::new();
    for (token, send) in desk.testing().wanted() {
        if let Send::Query { object, item, .. } = &send {
            if sim.replies {
                desk.testing().observe(&NetEvent::ScriptRunning {
                    object_id: *object,
                    item_id: *item,
                    running: sim.running,
                    mono: Some(true),
                    luau: aurora_net::scripts::Luau::Off,
                });
            }
        } else {
            box_answers(world, &send);
        }
        desk.testing().sent(token, Ok(()), now);
        sent.push(send);
    }
    sent
}

fn says(chat_type: ChatType, text: &str) -> ChatMessage {
    ChatMessage {
        from_name: "Cut box".into(),
        source_id: the_box(),
        owner_id: DEMO_AGENT,
        source_type: ChatSourceType::Object,
        chat_type,
        position: glam::Vec3::ZERO,
        message: text.into(),
    }
}

fn dialog(message: &str, channel: i32, buttons: &[&str]) -> NetEvent {
    NetEvent::ScriptDialog {
        sender: std::net::SocketAddr::from(([127, 0, 0, 1], 13000)),
        object_id: the_box(),
        object_name: "Cut box".into(),
        owner_name: "Demo".into(),
        owner_id: DEMO_AGENT,
        message: message.into(),
        channel,
        buttons: buttons.iter().map(|b| (*b).to_owned()).collect(),
    }
}

fn object() -> String {
    the_box().to_string()
}

#[test]
fn a_touch_always_asks_and_is_sent_once_as_a_click() {
    let mut world = plaza();
    let sim = Sim::new();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    snapshot(&mut desk, &world, &sim, now).expect("listed");
    let answer = take(
        &mut desk,
        &world,
        TOUCH,
        json!({"object_id": object(), "client_request_id": "touch-1"}),
        now,
    );
    let q = desk.question(now).expect("a touch asks even on Allow");
    assert!(q.summary.contains("Cut box"), "{}", q.summary);
    desk.decide(q.number, true);
    let (call, planned) = next_change(&mut desk, &world, now).expect("runs");
    let Work::Interact(interaction) = &planned.work else {
        panic!("an interaction")
    };
    assert!(matches!(interaction.send, Send::Touch { object, local_id: BOX_LOCAL, .. } if object == the_box()));
    app(&mut desk, &mut world, call, planned, now);
    let reply = answered(&answer).expect("sent");
    assert_eq!(reply["sent"], true);
    assert_eq!(reply["handled_confirmed"], false, "a touch is never acknowledged");
    // the same request again answers its receipt, nothing is sent twice
    let again = take(
        &mut desk,
        &world,
        TOUCH,
        json!({"object_id": object(), "client_request_id": "touch-1"}),
        now,
    );
    assert_eq!(answered(&again).expect("receipt")["duplicate"], true);
    assert!(desk.question(now).is_none());
}

#[test]
fn a_touch_is_refused_far_away_on_someone_elses_prim_under_rlva_and_on_never() {
    let now = Instant::now();
    let touch = |n: u32| json!({"object_id": object(), "client_request_id": format!("touch-{n}")});
    // no client_request_id
    let world = plaza();
    let mut desk = desk_with(allow_all());
    snapshot(&mut desk, &world, &Sim::new(), now).expect("listed");
    let answer = take(&mut desk, &world, TOUCH, json!({"object_id": object()}), now);
    assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments);
    // too far
    let mut far = plaza();
    far.agent.position += glam::Vec3::new(100.0, 0.0, 0.0);
    let answer = take(&mut desk, &far, TOUCH, touch(1), now);
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    // RLVa @touchall
    let mut locked = plaza();
    locked.rlva.configure(true);
    locked.rlva_console_input("@touchall=n");
    let answer = take(&mut desk, &locked, TOUCH, touch(2), now);
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    // someone else's prim
    let mut desk = desk_with(allow_all());
    let stranger = Sim {
        owner: STRANGER,
        ..Sim::new()
    };
    let _ = snapshot(&mut desk, &world, &stranger, now);
    let answer = take(&mut desk, &world, TOUCH, touch(3), now);
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    // Interact on Never
    let mut never = allow_all();
    never.set(Class::Interact, Level::Never);
    let mut desk = desk_with(never);
    snapshot(&mut desk, &world, &Sim::new(), now).expect("listed");
    let answer = take(&mut desk, &world, TOUCH, touch(4), now);
    assert_eq!(refused_code(&answer), ErrorCode::PermissionDenied);
}

#[test]
fn the_boxs_dialogs_are_listed_and_answered_as_the_users_click() {
    let mut world = plaza();
    let sim = Sim::new();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    snapshot(&mut desk, &world, &sim, now).expect("listed");
    let _ = world.apply(dialog("Door menu", -7001, &["Open", "Close"]));
    // a permission question of the same box is never listed
    let _ = world.apply(NetEvent::ScriptQuestion {
        task_id: the_box(),
        item_id: door(),
        object_name: "Cut box".into(),
        owner_name: "Demo".into(),
        questions: 0x2, // PERMISSION_DEBIT
        experience: Uuid::nil(),
        region: world.main_region.unwrap_or_default(),
    });
    let listed = answered(&take(&mut desk, &world, DIALOG_LIST, json!({"object_id": object()}), now)).expect("listed");
    let dialogs = listed["dialogs"].as_array().expect("dialogs");
    assert_eq!(dialogs.len(), 1, "{listed}");
    let d = &dialogs[0];
    assert_eq!(
        (d["kind"].as_str(), d["channel"].as_i64(), d["answerable"].as_bool()),
        (Some("dialog"), Some(-7001), Some(true))
    );
    assert_eq!(d["buttons"][0]["label"], "Open");
    assert!(listed["note"].as_str().is_some_and(|n| n.contains("data")));
    // answered without a dialog (Interact on Allow), as the user's click
    let answer = take(
        &mut desk,
        &world,
        DIALOG_REPLY,
        json!({"object_id": object(), "dialog_id": d["dialog_id"], "button_index": 0, "client_request_id": "reply-1"}),
        now,
    );
    assert!(desk.question(now).is_none(), "a dialog answer follows the level");
    let (call, planned) = next_change(&mut desk, &world, now).expect("runs");
    let commands = app(&mut desk, &mut world, call, planned, now);
    assert!(
        matches!(&commands[..], [NetCommand::ScriptDialogReply { channel: -7001, index: 0, label, object_id, .. }] if label == "Open" && *object_id == the_box()),
        "{commands:?}"
    );
    assert_eq!(answered(&answer).expect("answered")["dialog_closed"], true);
    let listed = answered(&take(&mut desk, &world, DIALOG_LIST, json!({"object_id": object()}), now)).expect("listed");
    assert!(listed["dialogs"].as_array().is_some_and(Vec::is_empty), "{listed}");
    // Interact on Ask: the answer asks
    let mut ask = allow_all();
    ask.set(Class::Interact, Level::Ask);
    desk.set_permissions(ask);
    let _ = world.apply(dialog("Door menu", -7001, &["Open", "Close"]));
    let listed = answered(&take(&mut desk, &world, DIALOG_LIST, json!({"object_id": object()}), now)).expect("listed");
    let _ = take(
        &mut desk,
        &world,
        DIALOG_REPLY,
        json!({"object_id": object(), "dialog_id": listed["dialogs"][0]["dialog_id"], "button_index": 1, "client_request_id": "reply-2"}),
        now,
    );
    let q = desk.question(now).expect("asks");
    assert!(
        q.summary.contains("Close") && q.detail.iter().any(|l| l.contains("Door menu")),
        "{q:?}"
    );
}

#[test]
fn dialogs_that_mention_money_or_answer_in_public_chat_are_never_answered() {
    let mut world = plaza();
    let sim = Sim::new();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    snapshot(&mut desk, &world, &sim, now).expect("listed");
    for (n, (message, channel, buttons)) in [
        ("Pay L$100 to open?", -5, vec!["Yes", "No"]),
        ("Choose", -6, vec!["Give L$50", "No"]),
        ("Choose", 0, vec!["Yes", "No"]),
        ("Choose", i32::MAX, vec!["Yes"]),
    ]
    .into_iter()
    .enumerate()
    {
        world.notifications.list.clear();
        let _ = world.apply(dialog(message, channel, &buttons));
        let listed = answered(&take(&mut desk, &world, DIALOG_LIST, json!({"object_id": object()}), now)).expect("listed");
        let d = &listed["dialogs"][0];
        assert_eq!(d["answerable"], false, "{message} on {channel}: {listed}");
        assert!(d["why_not"].is_string());
        let answer = take(
            &mut desk,
            &world,
            DIALOG_REPLY,
            json!({"object_id": object(), "dialog_id": d["dialog_id"], "button_index": 1usize.min(buttons.len() - 1), "client_request_id": format!("money-{n}")}),
            now,
        );
        assert_eq!(refused_code(&answer), ErrorCode::NotAllowed, "{message} on {channel}");
    }
    // a text box answer that speaks of L$
    world.notifications.list.clear();
    let _ = world.apply(dialog("Your name?", -9, &["!!llTextBox!!"]));
    let listed = answered(&take(&mut desk, &world, DIALOG_LIST, json!({"object_id": object()}), now)).expect("listed");
    let answer = take(
        &mut desk,
        &world,
        DIALOG_REPLY,
        json!({"object_id": object(), "dialog_id": listed["dialogs"][0]["dialog_id"], "text": "send me L$5", "client_request_id": "money-text"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    assert!(script_testing::mentions_money("50 linden dollars") && !script_testing::mentions_money("Open the door"));
}

#[test]
fn a_closed_dialog_is_stale_and_text_boxes_are_bounded() {
    let mut world = plaza();
    let sim = Sim::new();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    snapshot(&mut desk, &world, &sim, now).expect("listed");
    let _ = world.apply(dialog("Door menu", -7001, &["Open", "Close"]));
    let listed = answered(&take(&mut desk, &world, DIALOG_LIST, json!({"object_id": object()}), now)).expect("listed");
    let id = listed["dialogs"][0]["dialog_id"].clone();
    // closed by the user before the call: stale
    let notification = world.notifications.list.last().map(|n| n.id).expect("a dialog");
    let _ = world.respond_notification(notification, crate::world::notifications::Response::Dismiss);
    let answer = take(
        &mut desk,
        &world,
        DIALOG_REPLY,
        json!({"object_id": object(), "dialog_id": id, "button_index": 0, "client_request_id": "stale-1"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::Stale);
    // closed between the check and the send: stale, nothing sent
    let _ = world.apply(dialog("Door menu", -7001, &["Open", "Close"]));
    let listed = answered(&take(&mut desk, &world, DIALOG_LIST, json!({"object_id": object()}), now)).expect("listed");
    let answer = take(
        &mut desk,
        &world,
        DIALOG_REPLY,
        json!({"object_id": object(), "dialog_id": listed["dialogs"][0]["dialog_id"], "button_index": 0, "client_request_id": "stale-2"}),
        now,
    );
    world.notifications.list.clear();
    assert!(next_change(&mut desk, &world, now).is_none());
    assert_eq!(refused_code(&answer), ErrorCode::Stale);
    // a text box: text only, 250 bytes at most, empty allowed
    let _ = world.apply(dialog("Your name?", -9, &["!!llTextBox!!"]));
    let listed = answered(&take(&mut desk, &world, DIALOG_LIST, json!({"object_id": object()}), now)).expect("listed");
    let d = &listed["dialogs"][0];
    assert_eq!(d["kind"], "text_box");
    for (n, bad) in [json!({"text": "é".repeat(126)}), json!({"button_index": 0})]
        .into_iter()
        .enumerate()
    {
        let mut args = json!({"object_id": object(), "dialog_id": d["dialog_id"], "client_request_id": format!("box-{n}")});
        args.as_object_mut()
            .expect("object")
            .extend(bad.as_object().expect("object").clone());
        assert_eq!(
            refused_code(&take(&mut desk, &world, DIALOG_REPLY, args, now)),
            ErrorCode::InvalidArguments
        );
    }
    let answer = take(
        &mut desk,
        &world,
        DIALOG_REPLY,
        json!({"object_id": object(), "dialog_id": d["dialog_id"], "text": "", "client_request_id": "box-ok"}),
        now,
    );
    let (call, planned) = next_change(&mut desk, &world, now).expect("runs");
    let commands = app(&mut desk, &mut world, call, planned, now);
    assert!(
        matches!(&commands[..], [NetCommand::ScriptDialogReply { channel: -9, label, .. }] if label.is_empty()),
        "{commands:?}"
    );
    assert!(answered(&answer).is_ok());
    // an llDialog takes a button index in range only
    let _ = world.apply(dialog("Door menu", -7001, &["Open"]));
    let listed = answered(&take(&mut desk, &world, DIALOG_LIST, json!({"object_id": object()}), now)).expect("listed");
    let answer = take(
        &mut desk,
        &world,
        DIALOG_REPLY,
        json!({"object_id": object(), "dialog_id": listed["dialogs"][0]["dialog_id"], "button_index": 3, "client_request_id": "range-1"}),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments);
}

/// A channel line asked, allowed and sent at `now`.
fn send_line(desk: &mut Desk, world: &mut World, n: u32, now: Instant) -> Result<Value, ToolError> {
    let answer = take(
        desk,
        world,
        CHANNEL_SEND,
        json!({"object_id": object(), "channel": 7, "text": "ping", "client_request_id": format!("line-{n}")}),
        now,
    );
    if let Ok(result) = answer.try_recv() {
        return result;
    }
    let q = desk.question(now).expect("a channel line always asks");
    assert!(q.summary.contains("20 m"), "the broadcast is said: {}", q.summary);
    desk.decide(q.number, true);
    let (call, planned) = next_change(desk, world, now).expect("runs");
    let Work::Interact(interaction) = &planned.work else {
        panic!("an interaction")
    };
    assert_eq!(
        interaction.send,
        Send::Channel {
            channel: 7,
            text: "ping".into()
        }
    );
    app(desk, world, call, planned, now);
    answered(&answer)
}

#[test]
fn channel_lines_always_ask_keep_to_the_rate_limit_and_respect_sendchannel() {
    let mut world = plaza();
    let sim = Sim::new();
    let start = Instant::now();
    let mut desk = desk_with(allow_all());
    snapshot(&mut desk, &world, &sim, start).expect("listed");
    for (n, (channel, text)) in [
        (json!(0), json!("ping")),
        (json!(i32::MAX), json!("ping")),
        (json!(7), json!("")),
        (json!(7), json!("x".repeat(1024))),
        (json!(1_i64 << 40), json!("ping")),
    ]
    .into_iter()
    .enumerate()
    {
        let answer = take(
            &mut desk,
            &world,
            CHANNEL_SEND,
            json!({"object_id": object(), "channel": channel, "text": text, "client_request_id": format!("bad-{n}")}),
            start,
        );
        assert_eq!(refused_code(&answer), ErrorCode::InvalidArguments, "{channel} {text}");
    }
    let reply = send_line(&mut desk, &mut world, 1, start).expect("sent");
    assert_eq!((reply["sent"].as_bool(), reply["broadcast"].as_bool()), (Some(true), Some(true)));
    // one a second
    assert_eq!(
        send_line(&mut desk, &mut world, 2, start).expect_err("too soon").code,
        ErrorCode::Busy
    );
    // six a minute
    for n in 3..8 {
        send_line(&mut desk, &mut world, n, start + Duration::from_secs(u64::from(n) * 2)).expect("sent");
    }
    let refused = send_line(&mut desk, &mut world, 9, start + Duration::from_secs(20)).expect_err("seventh in a minute");
    assert_eq!(refused.code, ErrorCode::Busy);
    send_line(&mut desk, &mut world, 10, start + Duration::from_secs(70)).expect("a minute later");
    // RLVa @sendchannel, and too far for the object to hear
    let mut locked = plaza();
    locked.rlva.configure(true);
    locked.rlva_console_input("@sendchannel=n");
    let answer = take(
        &mut desk,
        &locked,
        CHANNEL_SEND,
        json!({"object_id": object(), "channel": 7, "text": "ping", "client_request_id": "rlva-1"}),
        start + Duration::from_secs(200),
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    let mut far = plaza();
    far.agent.position += glam::Vec3::new(30.0, 0.0, 0.0);
    let answer = take(
        &mut desk,
        &far,
        CHANNEL_SEND,
        json!({"object_id": object(), "channel": 7, "text": "ping", "client_request_id": "far-1"}),
        start + Duration::from_secs(200),
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
}

fn recipe_args(snapshot: &str, steps: Value) -> Value {
    let _ = snapshot;
    json!({"object_ids": [object()], "steps": steps, "timeout_seconds": 60, "client_request_id": format!("run-{}", Uuid::new_v4())})
}

#[test]
fn recipes_are_checked_for_scope_fields_and_bounds() {
    let o = object();
    let good = json!({"object_ids": [o], "steps": [{"action": "touch", "object_id": o}], "timeout_seconds": 60});
    let recipe = parse_recipe(good.as_object().expect("object")).expect("a recipe");
    assert!(recipe.fail_on_errors, "errors fail by default");
    let bad = [
        json!({"object_ids": [o], "steps": [{"action": "touch", "object_id": Uuid::new_v4().to_string()}], "timeout_seconds": 60}),
        json!({"object_ids": [o], "steps": [{"action": "touch", "object_id": o, "text": "x"}], "timeout_seconds": 60}),
        json!({"object_ids": [o], "steps": [{"action": "eval", "object_id": o}], "timeout_seconds": 60}),
        json!({"object_ids": [o, o], "steps": [{"action": "touch", "object_id": o}], "timeout_seconds": 60}),
        json!({"object_ids": [o], "steps": [{"action": "touch", "object_id": o}], "timeout_seconds": 601}),
        json!({"object_ids": [o], "steps": [{"action": "touch", "object_id": o}], "timeout_seconds": 0}),
        json!({"object_ids": [o], "steps": [{"action": "wait_output", "object_id": o, "text": "x", "timeout_seconds": 5}], "timeout_seconds": 60}),
        json!({"object_ids": [o], "steps": [{"action": "wait_output", "object_id": o, "text": "x", "expected": true, "timeout_seconds": 121}], "timeout_seconds": 60}),
        json!({"object_ids": [o], "steps": [{"action": "delay", "seconds": 61}], "timeout_seconds": 60}),
        json!({"object_ids": [o], "steps": [{"action": "delay", "object_id": o, "seconds": 1}], "timeout_seconds": 60}),
        json!({"object_ids": [o], "steps": [{"action": "send_channel", "object_id": o, "channel": 0, "text": "x"}], "timeout_seconds": 60}),
        json!({"object_ids": [o], "steps": [{"action": "running", "object_id": o, "snapshot_id": "s", "item_id": door().to_string()}], "timeout_seconds": 60}),
        json!({"object_ids": [o], "steps": [], "timeout_seconds": 60}),
        json!({"object_ids": [o], "steps": vec![json!({"action": "touch", "object_id": o}); 65], "timeout_seconds": 60}),
        json!({"object_ids": (0..33).map(|_| Uuid::new_v4().to_string()).collect::<Vec<_>>(), "steps": [{"action": "delay", "seconds": 1}], "timeout_seconds": 60}),
    ];
    for args in bad {
        assert_eq!(
            parse_recipe(args.as_object().expect("object")).expect_err("refused").code,
            ErrorCode::InvalidArguments,
            "{args}"
        );
    }
}

/// A recipe begun and allowed: its run_id.
fn begin(desk: &mut Desk, world: &mut World, args: Value, now: Instant) -> Result<String, ToolError> {
    let answer = take(desk, world, BEGIN, args, now);
    if let Ok(result) = answer.try_recv() {
        return result.map(|v| v["run_id"].as_str().unwrap_or_default().to_owned());
    }
    let q = desk.question(now).expect("a recipe always asks");
    desk.decide(q.number, true);
    let (call, planned) = next_change(desk, world, now).expect("runs");
    app(desk, world, call, planned, now);
    answered(&answer).map(|v| v["run_id"].as_str().expect("a run id").to_owned())
}

fn status(desk: &mut Desk, world: &World, run: &str, now: Instant) -> Value {
    answered(&take(desk, world, STATUS, json!({"run_id": run}), now)).expect("status")
}

/// Pump frames from `start` every 100 ms until the run ends or `limit`.
fn run_until_done(desk: &mut Desk, world: &mut World, sim: &Sim, run: &str, start: Instant, limit: Duration) -> (Value, Vec<Send>) {
    let mut sent = Vec::new();
    let mut t = start;
    while t <= start + limit {
        sent.extend(pump(desk, world, sim, t));
        let s = status(desk, world, run, t);
        if s["state"] != "running" {
            return (s, sent);
        }
        t += Duration::from_millis(100);
    }
    (status(desk, world, run, t), sent)
}

#[test]
fn a_recipe_asks_once_and_its_steps_run_without_another_dialog() {
    script_feed::set_capturing(true);
    let mut world = plaza();
    let sim = Sim::new();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let snap = snapshot(&mut desk, &world, &sim, now).expect("listed");
    let o = object();
    let item = door().to_string();
    let steps = json!([
        {"action": "touch", "object_id": o},
        {"action": "wait_output", "object_id": o, "text": "Touched", "expected": true, "timeout_seconds": 5},
        {"action": "wait_dialog", "object_id": o, "text": "Door", "expected": true, "timeout_seconds": 5},
        {"action": "send_channel", "object_id": o, "channel": 7, "text": "ping"},
        {"action": "wait_output", "object_id": o, "text": "Heard", "expected": true, "timeout_seconds": 5},
        {"action": "reset", "object_id": o, "snapshot_id": snap, "item_id": item},
        {"action": "delay", "seconds": 1},
        {"action": "assert_running", "object_id": o, "snapshot_id": snap, "item_id": item, "expected": true, "timeout_seconds": 5}
    ]);
    let answer = take(&mut desk, &world, BEGIN, recipe_args(&snap, steps.clone()), now);
    let q = desk.question(now).expect("asks once");
    assert!(q.summary.contains('8'), "{}", q.summary);
    assert_eq!(q.detail.len(), 8, "the dialog lists the steps: {:?}", q.detail);
    assert!(q.detail[3].contains("ping") && q.detail[5].contains("Door"), "{:?}", q.detail);
    desk.decide(q.number, true);
    let (call, planned) = next_change(&mut desk, &world, now).expect("runs");
    app(&mut desk, &mut world, call, planned, now);
    let run = answered(&answer).expect("started")["run_id"].as_str().expect("run id").to_owned();
    let (s, sent) = run_until_done(&mut desk, &mut world, &sim, &run, now, Duration::from_secs(30));
    assert_eq!(s["state"], "passed", "{s}");
    assert_eq!(s["assertions"]["passed"], 4, "{s}");
    assert!(desk.question(now).is_none(), "no step asked again");
    assert_eq!(sent.iter().filter(|s| !matches!(s, Send::Query { .. })).count(), 3, "{sent:?}");
    assert!(sent.iter().any(|s| matches!(s, Send::Reset { item, .. } if *item == door())));
    assert_eq!(s["steps"][1]["matched"], "Touched by the demo assistant.");
    // one run at a time; a second one starts once it ended
    let again = begin(
        &mut desk,
        &mut world,
        recipe_args(&snap, json!([{"action": "delay", "seconds": 1}])),
        now,
    )
    .expect("a second run");
    let (s, _) = run_until_done(&mut desk, &mut world, &sim, &again, now, Duration::from_secs(5));
    assert_eq!(s["state"], "completed_unverified", "no assertion: {s}");
}

#[test]
fn output_windows_consume_lines_and_absence_covers_the_last_action() {
    script_feed::set_capturing(true);
    let mut world = plaza();
    let sim = Sim::new();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let snap = snapshot(&mut desk, &world, &sim, now).expect("listed");
    let o = object();
    // a line said before the touch does not count
    script_feed::capture(&says(ChatType::OwnerSay, "Touched before"), DEMO_AGENT);
    let steps = json!([
        {"action": "touch", "object_id": o},
        {"action": "wait_output", "object_id": o, "text": "Touched", "expected": true, "timeout_seconds": 2},
        {"action": "wait_output", "object_id": o, "text": "Touched", "expected": true, "timeout_seconds": 2}
    ]);
    let run = begin(&mut desk, &mut world, recipe_args(&snap, steps), now).expect("run");
    let (s, _) = run_until_done(&mut desk, &mut world, &sim, &run, now, Duration::from_secs(10));
    assert_eq!(s["state"], "failed", "a second wait does not reuse the first line: {s}");
    assert_eq!(s["steps"][1]["state"], "passed");
    assert_eq!(s["steps"][1]["matched"], "Touched by the demo assistant.");
    assert_eq!(s["steps"][2]["state"], "failed");
    // absence: nothing matching within the window passes
    let steps = json!([
        {"action": "touch", "object_id": o},
        {"action": "wait_output", "object_id": o, "text": "boom", "expected": false, "timeout_seconds": 2},
        {"action": "wait_output", "object_id": o, "text": "Touched", "match": "equals", "expected": false, "timeout_seconds": 1}
    ]);
    let run = begin(&mut desk, &mut world, recipe_args(&snap, steps), now).expect("run");
    let (s, _) = run_until_done(&mut desk, &mut world, &sim, &run, now, Duration::from_secs(10));
    assert_eq!(s["state"], "passed", "{s}");
    // and a matching line fails it
    let steps = json!([
        {"action": "touch", "object_id": o},
        {"action": "wait_output", "object_id": o, "text": "Touched", "expected": false, "timeout_seconds": 2}
    ]);
    let run = begin(&mut desk, &mut world, recipe_args(&snap, steps), now).expect("run");
    let (s, _) = run_until_done(&mut desk, &mut world, &sim, &run, now, Duration::from_secs(10));
    assert_eq!(s["state"], "failed", "{s}");
}

#[test]
fn a_runtime_error_fails_the_run_unless_allowed_and_lost_lines_are_inconclusive() {
    script_feed::set_capturing(true);
    let mut world = plaza();
    let sim = Sim::new();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let snap = snapshot(&mut desk, &world, &sim, now).expect("listed");
    let o = object();
    let steps = json!([
        {"action": "delay", "seconds": 1},
        {"action": "wait_output", "object_id": o, "text": "never", "expected": true, "timeout_seconds": 5}
    ]);
    let run = begin(&mut desk, &mut world, recipe_args(&snap, steps.clone()), now).expect("run");
    pump(&mut desk, &mut world, &sim, now);
    script_feed::capture(&says(ChatType::Debug, "Math error"), DEMO_AGENT);
    let (s, _) = run_until_done(&mut desk, &mut world, &sim, &run, now, Duration::from_secs(10));
    assert_eq!(s["state"], "failed", "{s}");
    assert_eq!(s["runtime_errors"][0], "Math error");
    assert_eq!(s["steps"][0]["state"], "cancelled", "the delay under way: {s}");
    // errors allowed: the run goes on (and fails here only on its wait)
    let mut args = recipe_args(
        &snap,
        json!([{"action": "delay", "seconds": 1}, {"action": "wait_output", "object_id": o, "text": "Math", "expected": true, "timeout_seconds": 5}]),
    );
    args["fail_on_runtime_errors"] = json!(false);
    let answer = take(&mut desk, &world, BEGIN, args, now);
    let q = desk.question(now).expect("asks");
    assert!(q.detail.last().is_some_and(|l| l.contains("errors")), "{:?}", q.detail);
    desk.decide(q.number, true);
    let (call, planned) = next_change(&mut desk, &world, now).expect("runs");
    app(&mut desk, &mut world, call, planned, now);
    let run = answered(&answer).expect("run")["run_id"].as_str().expect("id").to_owned();
    pump(&mut desk, &mut world, &sim, now);
    script_feed::capture(&says(ChatType::Debug, "Math error"), DEMO_AGENT);
    let (s, _) = run_until_done(&mut desk, &mut world, &sim, &run, now, Duration::from_secs(10));
    assert_eq!(s["state"], "passed", "{s}");
    // lines lost between two frames: a wait that sees nothing cannot tell
    let run = begin(&mut desk, &mut world, recipe_args(&snap, steps), now).expect("run");
    pump(&mut desk, &mut world, &sim, now);
    for i in 0..1100 {
        script_feed::capture(&says(ChatType::OwnerSay, &format!("line {i}")), DEMO_AGENT);
    }
    let (s, _) = run_until_done(&mut desk, &mut world, &sim, &run, now, Duration::from_secs(10));
    assert_eq!((s["state"].as_str(), s["gap"].as_bool()), (Some("inconclusive"), Some(true)), "{s}");
}

#[test]
fn a_class_turned_to_never_stops_the_run_and_stop_needs_no_permission() {
    script_feed::set_capturing(true);
    let mut world = plaza();
    let sim = Sim::new();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let snap = snapshot(&mut desk, &world, &sim, now).expect("listed");
    let o = object();
    let steps = json!([{"action": "delay", "seconds": 5}, {"action": "touch", "object_id": o}]);
    let run = begin(&mut desk, &mut world, recipe_args(&snap, steps.clone()), now).expect("run");
    pump(&mut desk, &mut world, &sim, now);
    let mut never = allow_all();
    never.set(Class::Interact, Level::Never);
    desk.set_permissions(never);
    let sent = pump(&mut desk, &mut world, &sim, now + Duration::from_secs(6));
    assert!(sent.is_empty(), "nothing more leaves: {sent:?}");
    let s = status(&mut desk, &world, &run, now);
    assert_eq!(s["state"], "stopped", "{s}");
    assert_eq!(s["steps"][1]["state"], "cancelled");
    // Stop works with every permission revoked; nothing is sent after it
    desk.set_permissions(allow_all());
    let run = begin(&mut desk, &mut world, recipe_args(&snap, steps), now).expect("run");
    pump(&mut desk, &mut world, &sim, now);
    let mut none = Preset::ReadOnly.permissions();
    none.set(Class::Read, Level::Never);
    desk.set_permissions(none);
    let stopped = answered(&take(&mut desk, &world, STOP, json!({"run_id": run}), now)).expect("stopped");
    assert_eq!(stopped["state"], "stopped");
    assert_eq!(stopped["cancelled_steps"], 2, "the delay under way and the touch: {stopped}");
    assert!(pump(&mut desk, &mut world, &sim, now + Duration::from_secs(6)).is_empty());
    let again = answered(&take(&mut desk, &world, STOP, json!({"run_id": run}), now)).expect("answered");
    assert_eq!(again["already_ended"], true);
    assert_eq!(
        refused_code(&take(&mut desk, &world, STOP, json!({"run_id": "nope"}), now)),
        ErrorCode::NotFound
    );
    // status needs Read
    assert_eq!(
        refused_code(&take(&mut desk, &world, STATUS, json!({"run_id": run}), now)),
        ErrorCode::PermissionDenied
    );
}

#[test]
fn the_runs_time_ends_it_and_assert_running_reads_the_simulator() {
    script_feed::set_capturing(true);
    let mut world = plaza();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let snap = snapshot(&mut desk, &world, &Sim::new(), now).expect("listed");
    let o = object();
    let item = door().to_string();
    // the run's time over
    let mut args = recipe_args(
        &snap,
        json!([{"action": "wait_output", "object_id": o, "text": "never", "expected": true, "timeout_seconds": 100}]),
    );
    args["timeout_seconds"] = json!(3);
    let run = begin(&mut desk, &mut world, args, now).expect("run");
    let (s, _) = run_until_done(&mut desk, &mut world, &Sim::new(), &run, now, Duration::from_secs(5));
    assert_eq!(s["state"], "inconclusive", "{s}");
    assert!(s["reason"].as_str().is_some_and(|r| r.contains("time")), "{s}");
    // the other running state reported: failed, with what was seen
    let stopped_check = json!([{"action": "assert_running", "object_id": o, "snapshot_id": snap, "item_id": item, "expected": false, "timeout_seconds": 3}]);
    let run = begin(&mut desk, &mut world, recipe_args(&snap, stopped_check.clone()), now).expect("run");
    let (s, sent) = run_until_done(&mut desk, &mut world, &Sim::new(), &run, now, Duration::from_secs(5));
    assert_eq!(s["state"], "failed", "{s}");
    assert_eq!(s["steps"][0]["observed_running"], true);
    assert!(
        sent.iter().filter(|s| matches!(s, Send::Query { .. })).count() >= 2,
        "asked again: {sent:?}"
    );
    // no reply at all: inconclusive
    let silent = Sim {
        replies: false,
        ..Sim::new()
    };
    let run = begin(&mut desk, &mut world, recipe_args(&snap, stopped_check), now).expect("run");
    let (s, _) = run_until_done(&mut desk, &mut world, &silent, &run, now, Duration::from_secs(5));
    assert_eq!(s["state"], "inconclusive", "{s}");
    // a reset of a script without Modify is refused at the begin
    let answer = take(
        &mut desk,
        &world,
        BEGIN,
        recipe_args(
            &snap,
            json!([{"action": "reset", "object_id": o, "snapshot_id": snap, "item_id": sealed().to_string()}]),
        ),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::NotAllowed);
    // and a recipe needs Scripts
    let mut no_scripts = allow_all();
    no_scripts.set(Class::Scripts, Level::Never);
    desk.set_permissions(no_scripts);
    let answer = take(
        &mut desk,
        &world,
        BEGIN,
        recipe_args(&snap, json!([{"action": "delay", "seconds": 1}])),
        now,
    );
    assert_eq!(refused_code(&answer), ErrorCode::PermissionDenied);
}

/// End to end: the sidecar's `Viewer` over a real socket to the bridge and
/// the desk: list, touch (allowed in the dialog), the dialog listed and
/// answered, a recipe begun (allowed once) and read until it ends.
#[test]
fn an_assistant_tests_a_script_on_an_owned_prim_end_to_end() {
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

    script_feed::set_capturing(true);
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
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"Claude Code"}}}"#,
            )
            .expect("initialized");
        let mut ask = |id: u32, name: &str, arguments: Value| -> Value {
            let line = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}});
            let answer: Value = serde_json::from_str(&server.handle_line(&line.to_string()).expect("answer")).expect("json");
            answer["result"]["structuredContent"].clone()
        };
        let o = object.to_string();
        let listed = ask(2, "object_contents_list", json!({"object_id": o}));
        let snapshot = listed["snapshot_id"].clone();
        let item = listed["items"][0]["item_id"].clone();
        let touched = ask(3, "object_touch", json!({"object_id": o, "client_request_id": "e2e-touch"}));
        let dialogs = ask(4, "object_dialog_list", json!({"object_id": o}));
        let replied = ask(
            5,
            "object_dialog_reply",
            json!({"object_id": o, "dialog_id": dialogs["dialogs"][0]["dialog_id"], "button_index": 0, "client_request_id": "e2e-reply"}),
        );
        let begun = ask(
            6,
            "script_test_begin",
            json!({
                "object_ids": [o],
                "steps": [
                    {"action": "touch", "object_id": o},
                    {"action": "wait_output", "object_id": o, "text": "Touched", "expected": true, "timeout_seconds": 10},
                    {"action": "assert_running", "object_id": o, "snapshot_id": snapshot, "item_id": item, "expected": true, "timeout_seconds": 10}
                ],
                "timeout_seconds": 30,
                "client_request_id": "e2e-run"
            }),
        );
        let run = begun["run_id"].clone();
        let mut status = Value::Null;
        for n in 0..400 {
            status = ask(10 + n, "script_test_status", json!({"run_id": run}));
            if status["state"] != "running" {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        (touched, dialogs, replied, begun, status)
    });
    let mut world = plaza();
    let sim = Sim::new();
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
        if let Some((call, planned)) = next_change(&mut desk, &world, now) {
            app(&mut desk, &mut world, call, planned, now);
        }
        sim.serve(&mut desk, &world, now);
        pump(&mut desk, &mut world, &sim, now);
        std::thread::sleep(Duration::from_millis(5));
    }
    let (touched, dialogs, replied, begun, status) = assistant.join().expect("assistant");
    assert_eq!(touched["sent"], true, "{touched}");
    assert_eq!(dialogs["dialogs"][0]["buttons"][0]["label"], "Open", "{dialogs}");
    assert_eq!(replied["dialog_closed"], true, "{replied}");
    assert!(begun["run_id"].is_string(), "{begun}");
    assert_eq!(status["state"], "passed", "{status}");
    assert_eq!(questions.len(), 3, "touch, the answer, the recipe: {questions:?}");
    bridge.stop();
}
