//! Folderstorm: guarded operations (folderstorm/plans/mcp-receipts.md,
//! section 5): an early answer only when asked and declared, polling
//! through `asking`, `queued`, `running` and `done`; cancel while asking
//! (the dialog closes), while queued, and once sent (not recalled); Stop
//! from Preferences; the live and dialog bounds; and end to end from the
//! sidecar, `inventory_execute_plan` with `wait: false`.

use super::*;
use folderstorm_mcp_bridge::protocol::assistant::{CANCEL, EXECUTE_PLAN, PREVIEW_MOVE};
use folderstorm_mcp_bridge::receipts;

fn take(desk: &mut Desk, inv: &Inventory, tool: &str, args: Value) -> Receiver<Result<Value, ToolError>> {
    let (c, answer) = call(tool, args, false);
    desk.take(c, &ctx(inv), Instant::now());
    answer
}

fn state(desk: &mut Desk, inv: &Inventory, operation: &str) -> String {
    let answer = take(desk, inv, "assistant_operation_status", json!({"operation_id": operation}));
    answered(&answer).expect("found")["receipt"]["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn move_plan(desk: &mut Desk, inv: &Inventory) -> String {
    let answer = take(
        desk,
        inv,
        PREVIEW_MOVE,
        json!({"ids": [TEXTURE.to_string()], "folder_id": SUBFOLDER.to_string()}),
    );
    answered(&answer).expect("previewed")["plan_id"]
        .as_str()
        .expect("a plan")
        .to_owned()
}

#[test]
fn an_early_call_answers_at_once_and_is_polled_to_its_end() {
    let mut inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let plan = move_plan(&mut desk, &inv);
    let (mut c, answer) = call(EXECUTE_PLAN, json!({"plan_id": plan, "wait": false}), true);
    c.early = true; // the listener sets it for a sidecar that declares `operations`
    desk.take(c, &ctx(&inv), now);
    let early = answered(&answer).expect("at once");
    assert_eq!((early["state"].as_str(), early["early"].as_bool()), (Some("asking"), Some(true)));
    let operation = early["operation_id"].as_str().expect("named").to_owned();
    let q = desk.question(now).expect("the dialog");
    desk.decide(q.number, true);
    let run = desk.tick(&IDLE, now).into_iter().next().expect("allowed");
    assert_eq!(state(&mut desk, &inv, &operation), "queued");
    let planned = plan_of(&inv, &run);
    let Work::Inventory(change) = planned.work.clone() else {
        panic!("an inventory change")
    };
    desk.begin(run, &planned, false, now);
    desk.sent(Some(Uuid::from_u128(60)), None);
    assert_eq!(state(&mut desk, &inv, &operation), "running");
    let reply = actions::demo_mutate(&mut inv, AGENT, change).expect("applied");
    desk.observe((Uuid::from_u128(60), false, Outcome::Done(reply.created)));
    assert_eq!(state(&mut desk, &inv, &operation), "done");
    assert!(answer.try_recv().is_err(), "answered once, early");
    // without `early` (an older sidecar, or wait omitted): the call waits
    let inv = demo_inventory();
    let plan = move_plan(&mut desk, &inv);
    let answer = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": plan, "wait": false}));
    assert!(answer.try_recv().is_err(), "no early answer");
    let answer = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": "pl-x", "wait": "no"}));
    assert_eq!(answered(&answer).expect_err("invalid").code, ErrorCode::InvalidArguments);
}

fn plan_of(inv: &Inventory, run: &Pending) -> Planned {
    plan(&run.tool, &run.arguments, &ctx(inv)).expect("planned")
}

#[test]
fn cancel_stops_what_was_not_sent() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let rename = |name: &str| json!({"id": TEXTURE.to_string(), "name": name});
    // in the dialog: it closes
    let (a, asked) = call("inventory_rename", rename("A"), true);
    desk.take(a, &ctx(&inv), now);
    let operation = desk.ledger().with(|s| s.recent(None, 1)[0].operation_id.clone());
    let cancel = take(&mut desk, &inv, CANCEL, json!({"operation_id": operation}));
    let answer = answered(&cancel).expect("cancelled");
    assert_eq!(
        (answer["cancelled"].as_bool(), answer["state"].as_str()),
        (Some(true), Some("cancelled"))
    );
    assert!(desk.question(now).is_none(), "the dialog closed");
    let e = answered(&asked).expect_err("the call ends");
    assert_eq!(e.code, ErrorCode::Cancelled);
    assert_eq!(state(&mut desk, &inv, &operation), "cancelled");
    // waiting for its turn
    desk.set_permissions(allow_all());
    let (b, queued) = call("inventory_rename", rename("B"), false);
    desk.take(b, &ctx(&inv), now);
    let operation = desk.ledger().with(|s| s.recent(None, 1)[0].operation_id.clone());
    answered(&take(&mut desk, &inv, CANCEL, json!({"operation_id": operation}))).expect("cancelled");
    assert!(desk.tick(&IDLE, now).is_empty(), "never sent");
    assert_eq!(answered(&queued).expect_err("ended").code, ErrorCode::Cancelled);
    // sent: not recalled
    let (c, _sent) = call("inventory_rename", rename("C"), false);
    desk.take(c, &ctx(&inv), now);
    let run = desk.tick(&IDLE, now).into_iter().next().expect("runs");
    let operation = run.operation_id.clone().expect("named");
    let planned = plan_of(&inv, &run);
    desk.begin(run, &planned, false, now);
    desk.sent(Some(Uuid::from_u128(61)), None);
    let answer = answered(&take(&mut desk, &inv, CANCEL, json!({"operation_id": operation}))).expect("answered");
    assert_eq!(
        (answer["cancelled"].as_bool(), answer["state"].as_str()),
        (Some(false), Some("running"))
    );
    assert!(answer["message"].as_str().is_some_and(|m| m.contains("cannot be recalled")));
    // another login's, or none: not found; bad arguments
    for bad in [json!({"operation_id": "op-0"}), json!({})] {
        let e = answered(&take(&mut desk, &inv, CANCEL, bad.clone())).expect_err("refused");
        assert!(matches!(e.code, ErrorCode::NotFound | ErrorCode::InvalidArguments), "{bad}");
    }
}

#[test]
fn stop_in_preferences_cancels_too() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let (a, asked) = call("inventory_trash", json!({"ids": [TEXTURE.to_string()]}), true);
    desk.take(a, &ctx(&inv), now);
    let operation = desk.ledger().with(|s| s.recent(None, 1)[0].operation_id.clone());
    let answer = desk.cancel(&operation, true).expect("stopped");
    assert_eq!(answer["cancelled"], true);
    let e = answered(&asked).expect_err("stopped");
    assert!(e.message.contains("Preferences"), "{e}");
    assert_eq!(state(&mut desk, &inv, &operation), "cancelled");
}

#[test]
fn live_operations_and_waiting_questions_are_bounded() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let mut answers = Vec::new();
    for n in 0..receipts::LIVE_MAX {
        let (c, answer) = call(
            "inventory_rename",
            json!({"id": TEXTURE.to_string(), "name": format!("N{n}"), "client_request_id": format!("r{n}")}),
            true,
        );
        desk.take(c, &ctx(&inv), now);
        answers.push(answer);
    }
    let (extra, answer) = call("inventory_rename", json!({"id": TEXTURE.to_string(), "name": "X"}), true);
    desk.take(extra, &ctx(&inv), now);
    assert_eq!(answered(&answer).expect_err("busy").code, ErrorCode::Busy);
    // a retry of one of them still answers its receipt
    let (again, answer) = call(
        "inventory_rename",
        json!({"id": TEXTURE.to_string(), "name": "N0", "client_request_id": "r0"}),
        true,
    );
    desk.take(again, &ctx(&inv), now);
    assert_eq!(answered(&answer).expect("its receipt")["duplicate"], true);
    assert!(answers.iter().all(|a| a.try_recv().is_err()), "the 8 still wait");
    assert!(
        crate::mcp_bridge::desk::ASKS_MAX >= receipts::LIVE_MAX,
        "the dialog's queue holds every live change"
    );
}

/// The sidecar → a real socket → the bridge → the desk:
/// `inventory_execute_plan` with `wait: false` answers at once with
/// `asking`; the assistant polls while the user reads, the user allows,
/// and the status ends `done`.
#[test]
fn an_early_plan_from_the_sidecar_is_polled_to_done() {
    use folderstorm_mcp::connect::Viewer;
    use folderstorm_mcp_bridge::Bridge;
    use folderstorm_mcp_bridge::server::Config;

    let dir = tempfile::tempdir().expect("temp dir");
    let (bridge, calls) = Bridge::start(Config {
        discovery_dir: Some(dir.path().to_path_buf()),
        pid: std::process::id(),
        version: "test".into(),
        permissions: Preset::AskBeforeChanges.permissions(),
        viewer_timeout: Duration::from_secs(10),
    })
    .expect("bridge started");
    bridge.set_login(1, true);
    let folder = dir.path().to_path_buf();
    let allow = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let allow_now = allow.clone();
    let assistant = std::thread::spawn(move || {
        let mut viewer = Viewer::new(Some(folder), None);
        let preview = viewer
            .call(
                PREVIEW_MOVE,
                json!({"ids": [TEXTURE.to_string()], "folder_id": SUBFOLDER.to_string()}),
            )
            .expect("previewed");
        let plan = preview["plan_id"].as_str().unwrap_or_default().to_owned();
        let early = viewer.call(EXECUTE_PLAN, json!({"plan_id": plan, "wait": false})).expect("at once");
        let operation = early["operation_id"].as_str().unwrap_or_default().to_owned();
        let mut seen = vec![early["state"].as_str().unwrap_or_default().to_owned()];
        allow_now.store(true, std::sync::atomic::Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let status = viewer
                .call("assistant_operation_status", json!({"operation_id": operation}))
                .expect("status");
            let state = status["receipt"]["state"].as_str().unwrap_or_default().to_owned();
            if seen.last() != Some(&state) {
                seen.push(state.clone());
            }
            if state == "done" {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        seen
    });
    let mut inv = demo_inventory();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let deadline = Instant::now() + Duration::from_secs(30);
    while !assistant.is_finished() && Instant::now() < deadline {
        let now = Instant::now();
        let session = bridge.viewer_session();
        desk.set_login(1, session.clone());
        let mut cx = ctx(&inv);
        cx.viewer_session = session;
        while let Ok(pending) = calls.try_recv() {
            desk.take(pending, &cx, now);
        }
        if allow.load(std::sync::atomic::Ordering::SeqCst)
            && let Some(q) = desk.question(now)
        {
            desk.decide(q.number, true);
        }
        for run in desk.tick(&IDLE, now) {
            let planned = plan_of(&inv, &run);
            let Work::Inventory(change) = planned.work.clone() else {
                panic!("an inventory change")
            };
            desk.begin(run, &planned, false, now);
            let request = Uuid::new_v4();
            let reply = actions::demo_mutate(&mut inv, AGENT, change).expect("applied");
            desk.observe((request, false, Outcome::Done(reply.created)));
            desk.sent(Some(request), None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let seen = assistant.join().expect("assistant");
    assert_eq!(seen.first().map(String::as_str), Some("asking"), "{seen:?}");
    assert_eq!(seen.last().map(String::as_str), Some("done"), "{seen:?}");
    assert_eq!(inv.items[&TEXTURE].parent, SUBFOLDER);
    bridge.stop();
}
