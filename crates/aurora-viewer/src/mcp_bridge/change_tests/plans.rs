//! Folderstorm: batch plans, preview then execute
//! (folderstorm/plans/mcp-receipts.md, section 6): the preview's rows are
//! the native review's; execute needs the plan's own class, asks once for
//! the whole plan, plans again and sends nothing when the rows changed, runs
//! once, and ends with its login; the outcomes reach the receipt's rows;
//! and end to end from the sidecar on the demo inventory.

use super::*;
use crate::folderstorm::bulk_plan;
use folderstorm_mcp_bridge::protocol::assistant::{EXECUTE_PLAN, PREVIEW_MOVE, PREVIEW_RENAME};

fn ids(range: std::ops::RangeInclusive<u128>) -> Vec<Uuid> {
    range.map(Uuid::from_u128).collect()
}

fn id_list(ids: &[Uuid]) -> Value {
    json!(ids.iter().map(Uuid::to_string).collect::<Vec<_>>())
}

fn take(desk: &mut Desk, inv: &Inventory, tool: &str, args: Value, now: Instant) -> Receiver<Result<Value, ToolError>> {
    let (c, answer) = call(tool, args, false);
    desk.take(c, &ctx(inv), now);
    answer
}

fn preview(desk: &mut Desk, inv: &Inventory, tool: &str, args: Value) -> Result<Value, ToolError> {
    answered(&take(desk, inv, tool, args, Instant::now()))
}

fn plan_id(preview: &Value) -> String {
    preview["plan_id"].as_str().expect("a plan").to_owned()
}

/// The rows as the preview gives them, from the shared planner.
fn expected(rows: &[bulk_plan::Row]) -> Value {
    json!(
        rows.iter()
            .map(|r| match &r.skip {
                None => json!({"id": r.id, "before": r.before, "after": r.after}),
                Some(skip) => json!({"id": r.id, "before": r.before, "skip": skip}),
            })
            .collect::<Vec<_>>()
    )
}

/// Run an allowed change as the app would, the demo server answering.
fn run_on(desk: &mut Desk, inv: &mut Inventory, run: Pending, now: Instant, outcome: Option<Outcome>) {
    let planned = plan(&run.tool, &run.arguments, &ctx(inv)).expect("still the plan");
    let Work::Inventory(change) = planned.work.clone() else {
        panic!("one inventory change")
    };
    desk.begin(run, &planned, false, now);
    let request = Uuid::new_v4();
    let outcome = match outcome {
        Some(outcome) => outcome,
        None => Outcome::Done(actions::demo_mutate(inv, AGENT, change).expect("applied").created),
    };
    desk.observe((request, false, outcome));
    desk.sent(Some(request), None);
}

#[test]
fn the_preview_rows_are_the_native_reviews() {
    let inv = demo_inventory();
    let mut desk = desk_with(allow_all());
    let chosen = ids(8100..=8112);
    let renamed = preview(
        &mut desk,
        &inv,
        PREVIEW_RENAME,
        json!({"ids": id_list(&chosen), "pattern": "Sorted {n} - {name}"}),
    )
    .expect("previewed");
    assert_eq!(
        renamed["rows"],
        expected(&bulk_plan::plan_rename(&inv, &chosen, "Sorted {n} - {name}"))
    );
    assert!(plan_id(&renamed).starts_with("pl-"));
    assert_eq!(renamed["execution"], "allow");
    assert_eq!(renamed["expires_in"], 300);
    let counts = &renamed["counts"];
    assert_eq!(counts["total"], 13);
    assert_eq!(
        counts["eligible"].as_u64().unwrap_or_default() + counts["skipped"].as_u64().unwrap_or_default(),
        13
    );
    let cx = ctx(&inv);
    let moved = preview(
        &mut desk,
        &inv,
        PREVIEW_MOVE,
        json!({"ids": id_list(&chosen), "folder_id": SUBFOLDER.to_string()}),
    )
    .expect("previewed");
    assert_eq!(
        moved["rows"],
        expected(&bulk_plan::plan_move(&inv, &chosen, SUBFOLDER, &cx.protected, &cx.worn))
    );
    // explicit names: the window's cleaning and rules
    let named = preview(
        &mut desk,
        &inv,
        PREVIEW_RENAME,
        json!({"items": [{"id": TEXTURE.to_string(), "name": "  Brick  "}, {"id": LINK.to_string(), "name": "x"}]}),
    )
    .expect("previewed");
    assert_eq!(named["rows"][0]["after"], "Brick");
    assert!(named["rows"][1]["skip"].is_string(), "a link keeps its name: {named}");
    // a preview changes nothing and runs nothing
    assert!(desk.tick(&IDLE, Instant::now()).is_empty());
    assert_eq!(desk.ledger().with(|s| s.all().count()), 0, "a read has no receipt");
}

#[test]
fn a_preview_needs_read_and_refuses_what_a_plan_must_not_take() {
    let inv = demo_inventory();
    let mut desk = desk_with(allow_all());
    let code = |desk: &mut Desk, tool: &str, args: Value| preview(desk, &inv, tool, args).expect_err("refused").code;
    let many: Vec<Uuid> = (0..51).map(|_| TEXTURE).collect();
    assert_eq!(
        code(
            &mut desk,
            PREVIEW_MOVE,
            json!({"ids": id_list(&many), "folder_id": SUBFOLDER.to_string()})
        ),
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        code(
            &mut desk,
            PREVIEW_MOVE,
            json!({"ids": id_list(&[TEXTURE, TEXTURE]), "folder_id": SUBFOLDER.to_string()})
        ),
        ErrorCode::InvalidArguments,
        "duplicate ids"
    );
    let trash = actions::system(&inv, 14).expect("trash");
    assert_eq!(
        code(
            &mut desk,
            PREVIEW_MOVE,
            json!({"ids": id_list(&[TEXTURE]), "folder_id": trash.to_string()})
        ),
        ErrorCode::InvalidArguments,
        "the Trash is inventory_trash's"
    );
    assert_eq!(
        code(
            &mut desk,
            PREVIEW_MOVE,
            json!({"ids": id_list(&[TEXTURE]), "folder_id": MARKETPLACE.to_string()})
        ),
        ErrorCode::NotAllowed
    );
    for bad in [
        json!({"ids": id_list(&[TEXTURE])}),
        json!({"pattern": "x"}),
        json!({"ids": id_list(&[TEXTURE, OBJECT]), "pattern": "same"}),
        json!({"items": [{"id": TEXTURE.to_string()}]}),
        json!({"ids": id_list(&[TEXTURE]), "pattern": "x", "other": 1}),
    ] {
        assert_eq!(code(&mut desk, PREVIEW_RENAME, bad.clone()), ErrorCode::InvalidArguments, "{bad}");
    }
    // a Marketplace entry is left out, with its reason
    let listed = preview(
        &mut desk,
        &inv,
        PREVIEW_RENAME,
        json!({"items": [{"id": MARKETPLACE.to_string(), "name": "Shop"}]}),
    )
    .expect("previewed");
    assert!(listed["rows"][0]["skip"].is_string(), "{listed}");
    // Read on Never: no preview
    let mut no_read = allow_all();
    no_read.set(Class::Read, Level::Never);
    desk.set_permissions(no_read);
    assert_eq!(
        code(
            &mut desk,
            PREVIEW_MOVE,
            json!({"ids": id_list(&[TEXTURE]), "folder_id": SUBFOLDER.to_string()})
        ),
        ErrorCode::PermissionDenied
    );
    // RLVa's @showinv
    desk.set_permissions(allow_all());
    let mut cx = ctx(&inv);
    cx.inventory_locked = true;
    let (c, answer) = call(
        PREVIEW_MOVE,
        json!({"ids": id_list(&[TEXTURE]), "folder_id": SUBFOLDER.to_string()}),
        false,
    );
    desk.take(c, &cx, Instant::now());
    assert_eq!(answered(&answer).expect_err("locked").code, ErrorCode::PermissionDenied);
}

#[test]
fn execute_needs_the_plans_own_class() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut levels = allow_all();
    levels.set(Class::Edit, Level::Never);
    let mut desk = desk_with(levels);
    let rename = preview(
        &mut desk,
        &inv,
        PREVIEW_RENAME,
        json!({"ids": id_list(&[TEXTURE]), "pattern": "Brick"}),
    )
    .expect("previewed");
    assert_eq!(rename["execution"], "never", "shown, even when its execute is refused");
    let answer = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": plan_id(&rename)}), now);
    assert_eq!(answered(&answer).expect_err("Edit is Never").code, ErrorCode::PermissionDenied);
    let moved = preview(
        &mut desk,
        &inv,
        PREVIEW_MOVE,
        json!({"ids": id_list(&[TEXTURE]), "folder_id": SUBFOLDER.to_string()}),
    )
    .expect("previewed");
    assert_eq!(moved["execution"], "allow");
    take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": plan_id(&moved)}), now);
    assert_eq!(desk.tick(&IDLE, now).len(), 1, "Move is Allow: it runs");
    // unknown arguments
    let answer = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": "pl-x", "other": 1}), now);
    assert_eq!(answered(&answer).expect_err("invalid").code, ErrorCode::InvalidArguments);
}

#[test]
fn one_dialog_for_the_whole_plan() {
    let inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let chosen = ids(8100..=8112);
    let moved = preview(
        &mut desk,
        &inv,
        PREVIEW_MOVE,
        json!({"ids": id_list(&chosen), "folder_id": SUBFOLDER.to_string()}),
    )
    .expect("previewed");
    assert_eq!(moved["execution"], "ask");
    let eligible = moved["counts"]["eligible"].as_u64().expect("counted") as usize;
    let skipped = moved["counts"]["skipped"].as_u64().expect("counted") as usize;
    assert!(eligible > 5, "{moved}");
    take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": plan_id(&moved)}), now);
    let q = desk.question(now).expect("one question");
    assert_eq!(q.waiting, 0, "one dialog for the whole plan");
    assert!(q.summary.contains(&eligible.to_string()), "{}", q.summary);
    assert!(q.summary.contains(&inv.folders[&SUBFOLDER].info.name), "{}", q.summary);
    assert_eq!(q.detail.len(), 5 + 1 + usize::from(skipped > 0), "{:?}", q.detail);
    assert!(q.detail[5].contains(&(eligible - 5).to_string()), "{:?}", q.detail);
    desk.decide(q.number, true);
    assert_eq!(desk.tick(&IDLE, now).len(), 1, "one change for the whole plan");
    // the receipt holds the rows, names with Read
    let status = take(&mut desk, &inv, "assistant_operation_status", json!({}), now);
    let receipt = answered(&status).expect("listed")["receipts"][0].clone();
    assert_eq!(receipt["plan_id"], moved["plan_id"]);
    assert_eq!(receipt["rows"].as_array().map(Vec::len), Some(13));
    assert_eq!(receipt["decision"], "approved");
}

#[test]
fn a_plan_whose_rows_changed_sends_nothing() {
    let mut inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    let args = json!({"ids": id_list(&[TEXTURE, OBJECT]), "pattern": "{name} (old)"});
    let first = preview(&mut desk, &inv, PREVIEW_RENAME, args.clone()).expect("previewed");
    // renamed elsewhere in between
    let rename = planned(&inv, "inventory_rename", json!({"id": TEXTURE.to_string(), "name": "Brick"}));
    apply(&mut inv, rename);
    let answer = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": plan_id(&first)}), now);
    let e = answered(&answer).expect_err("stale");
    assert_eq!(e.code, ErrorCode::Stale);
    assert!(desk.tick(&IDLE, now).is_empty(), "nothing sent");
    // the plan is gone: executing it again answers the same operation
    let again = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": plan_id(&first)}), now);
    let again = answered(&again).expect("the first operation");
    assert_eq!(again["operation_id"], json!(e.operation_id));
    assert_eq!(again["state"], "stale");
    // changed between the dialog and the send: the app's replan refuses it
    let second = preview(&mut desk, &inv, PREVIEW_RENAME, args).expect("previewed again");
    take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": plan_id(&second)}), now);
    let run = desk.tick(&IDLE, now).into_iter().next().expect("allowed");
    let moved = planned(
        &inv,
        "inventory_move",
        json!({"ids": [OBJECT.to_string()], "folder_id": SUBFOLDER.to_string()}),
    );
    apply(&mut inv, moved);
    let rename = planned(&inv, "inventory_rename", json!({"id": OBJECT.to_string(), "name": "Crate"}));
    apply(&mut inv, rename);
    let e = plan(&run.tool, &run.arguments, &ctx(&inv)).expect_err("stale before the send");
    assert_eq!(e.code, ErrorCode::Stale);
    let operation = run.operation_id.clone().expect("an operation");
    run.answer(Err(e));
    let status = take(
        &mut desk,
        &inv,
        "assistant_operation_status",
        json!({"operation_id": operation}),
        now,
    );
    let receipt = answered(&status).expect("found")["receipt"].clone();
    assert_eq!(receipt["state"], "stale");
    assert!(
        receipt["rows"].as_array().expect("rows").iter().all(|r| r["outcome"] != "done"),
        "{receipt}"
    );
}

#[test]
fn plans_expire_run_once_and_end_with_the_login() {
    let mut inv = demo_inventory();
    let now = Instant::now();
    let mut desk = desk_with(allow_all());
    desk.set_login(1, Some("vs-11111111111111111111111111111111".into()));
    let args = json!({"ids": id_list(&[TEXTURE]), "folder_id": SUBFOLDER.to_string()});
    // five minutes
    let old = preview(&mut desk, &inv, PREVIEW_MOVE, args.clone()).expect("previewed");
    let answer = take(
        &mut desk,
        &inv,
        EXECUTE_PLAN,
        json!({"plan_id": plan_id(&old)}),
        now + Duration::from_secs(301),
    );
    assert_eq!(answered(&answer).expect_err("expired").code, ErrorCode::NotFound);
    // once
    let fresh = preview(&mut desk, &inv, PREVIEW_MOVE, args.clone()).expect("previewed");
    let first = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": plan_id(&fresh)}), now);
    let run = desk.tick(&IDLE, now).into_iter().next().expect("runs");
    let operation = run.operation_id.clone().expect("an operation");
    run_on(&mut desk, &mut inv, run, now, None);
    let done = answered(&first).expect("done");
    assert_eq!(
        (done["state"].as_str(), done["operation_id"].as_str()),
        (Some("done"), Some(operation.as_str()))
    );
    assert_eq!(done["rows"][0], json!({"id": TEXTURE, "outcome": "done"}));
    assert_eq!(inv.items[&TEXTURE].parent, SUBFOLDER);
    let again = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": plan_id(&fresh)}), now);
    let again = answered(&again).expect("the same operation");
    assert_eq!(
        (again["operation_id"].as_str(), again["duplicate"].as_bool()),
        (Some(operation.as_str()), Some(true))
    );
    assert!(desk.tick(&IDLE, now).is_empty(), "never sent twice");
    // 8 open plans: the oldest unused one goes
    let mut kept = Vec::new();
    for _ in 0..9 {
        kept.push(plan_id(&preview(&mut desk, &inv, PREVIEW_MOVE, args.clone()).expect("previewed")));
    }
    let answer = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": kept[0]}), now);
    assert_eq!(answered(&answer).expect_err("pushed out").code, ErrorCode::NotFound);
    // another login: every plan ends
    desk.set_login(2, Some("vs-22222222222222222222222222222222".into()));
    let answer = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": kept[8]}), now);
    assert_eq!(answered(&answer).expect_err("ended").code, ErrorCode::NotFound);
}

#[test]
fn the_servers_answer_reaches_the_rows() {
    let now = Instant::now();
    for (outcome, state, row) in [
        (Some(Outcome::Failed("refused".into())), "failed", "unknown"),
        (Some(Outcome::Queued), "unconfirmed", "queued"),
        (None, "done", "done"),
    ] {
        let mut inv = demo_inventory();
        let mut desk = desk_with(allow_all());
        let p = preview(
            &mut desk,
            &inv,
            PREVIEW_RENAME,
            json!({"ids": id_list(&[TEXTURE, LINK]), "pattern": "{name} 2"}),
        )
        .expect("previewed");
        let answer = take(&mut desk, &inv, EXECUTE_PLAN, json!({"plan_id": plan_id(&p)}), now);
        let run = desk.tick(&IDLE, now).into_iter().next().expect("runs");
        let operation = run.operation_id.clone().expect("an operation");
        run_on(&mut desk, &mut inv, run, now, outcome);
        let _ = answered(&answer);
        let status = take(
            &mut desk,
            &inv,
            "assistant_operation_status",
            json!({"operation_id": operation}),
            now,
        );
        let receipt = answered(&status).expect("found")["receipt"].clone();
        assert_eq!(receipt["state"], state, "{receipt}");
        assert_eq!(receipt["rows"][0]["outcome"], row, "{receipt}");
        assert_eq!(receipt["rows"][1]["outcome"], "skipped", "the link keeps its name");
    }
}

/// The sidecar → a real socket → the bridge (Ask before changes) → the
/// desk: a move plan previewed, executed after the dialog, applied to the
/// demo inventory.
#[test]
fn an_assistant_previews_and_executes_a_plan_end_to_end() {
    use folderstorm_mcp::connect::Viewer;
    use folderstorm_mcp::jsonrpc::{Backend, Server};
    use folderstorm_mcp_bridge::Bridge;
    use folderstorm_mcp_bridge::server::Config;

    struct Live(Viewer);
    impl Backend for Live {
        fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
            self.0.call(tool, arguments)
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
    bridge.set_login(1, true);
    let folder = dir.path().to_path_buf();
    let assistant = std::thread::spawn(move || {
        let mut server = Server::new(Live(Viewer::new(Some(folder), None)));
        let mut send = |line: &str| -> Value { serde_json::from_str(&server.handle_line(line).expect("answer")).expect("json") };
        let preview = send(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{PREVIEW_MOVE}","arguments":{{"ids":["{TEXTURE}","{OBJECT}","{COF}"],"folder_id":"{SUBFOLDER}"}}}}}}"#
        ));
        let plan = preview["result"]["structuredContent"]["plan_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let executed = send(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{EXECUTE_PLAN}","arguments":{{"plan_id":"{plan}","client_request_id":"sort-1"}}}}}}"#
        ));
        (preview, executed)
    });
    let mut inv = demo_inventory();
    let mut desk = desk_with(Preset::AskBeforeChanges.permissions());
    let mut questions = 0;
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
        if let Some(q) = desk.question(now) {
            questions += 1;
            desk.decide(q.number, true);
        }
        for run in desk.tick(&IDLE, now) {
            run_on(&mut desk, &mut inv, run, now, None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let (preview, executed) = assistant.join().expect("assistant");
    let rows = &preview["result"]["structuredContent"]["rows"];
    assert_eq!(rows.as_array().map(Vec::len), Some(3), "{preview}");
    assert!(rows[2]["skip"].is_string(), "the Current Outfit stays");
    let executed = &executed["result"]["structuredContent"];
    assert_eq!(executed["state"], "done", "{executed}");
    assert_eq!(questions, 1, "one dialog");
    assert_eq!(inv.items[&TEXTURE].parent, SUBFOLDER);
    assert_eq!(inv.items[&OBJECT].parent, SUBFOLDER);
    bridge.stop();
}
