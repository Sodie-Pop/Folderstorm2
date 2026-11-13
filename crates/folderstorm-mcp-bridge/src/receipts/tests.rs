use super::*;

const S1: &str = "vs-11111111111111111111111111111111";
const S2: &str = "vs-22222222222222222222222222222222";

fn op(n: usize) -> String {
    format!("op-{n:032x}")
}

fn request<'a>(n: usize, id: Option<&str>, session: &str, arguments: &'a Value) -> Request<'a> {
    Request {
        operation_id: op(n),
        client_request_id: id.map(str::to_owned),
        tool: "inventory_rename",
        client: Some("Claude\u{7} Code".into()),
        viewer_session: Some(session.to_owned()),
        arguments,
    }
}

fn done(store: &mut Store, n: usize, now: Instant) {
    store.finish(&op(n), &Ok(json!({})), now);
}

#[test]
fn ids_have_their_form() {
    let id = new_operation_id().expect("random");
    assert!(is_operation_id(&id), "{id}");
    assert_ne!(new_operation_id().expect("random"), id);
    assert!(!is_operation_id("op-12"));
    assert!(!is_operation_id(&format!("vs-{}", &id[3..])));
    let mut args = json!({"id": "x", "client_request_id": "retry-1"});
    assert_eq!(take_client_request_id(&mut args).expect("valid").as_deref(), Some("retry-1"));
    assert_eq!(args, json!({"id": "x"}), "taken out: the tool reads its arguments as before");
    let mut none = json!({"id": "x"});
    assert_eq!(take_client_request_id(&mut none).expect("none"), None);
    for bad in [
        json!(""),
        json!(" padded "),
        json!("é"),
        json!("a\nb"),
        json!(7),
        json!("x".repeat(65)),
    ] {
        let mut args = json!({ "client_request_id": bad.clone() });
        let e = take_client_request_id(&mut args).expect_err("refused");
        assert_eq!(e.code, ErrorCode::InvalidArguments, "{bad}");
    }
    assert!(valid_client_request_id(&"x".repeat(64)));
}

#[test]
fn text_is_cleaned_and_cut_on_a_character_boundary() {
    assert_eq!(clean("a\u{0}b\nc\u{1b}d", 10), "abcd");
    let long = "é".repeat(400);
    let cut = clean(&long, SUMMARY_MAX);
    assert_eq!(cut.chars().count(), SUMMARY_MAX);
    assert!(cut.chars().all(|c| c == 'é'));
    let row = Row::new("id", &"n".repeat(500), "after\u{7}", None);
    assert_eq!(row.before.chars().count(), NAME_MAX);
    assert_eq!(row.after, "after");
    assert_eq!(row.outcome, RowOutcome::Pending);
    assert_eq!(Row::new("id", "a", "", Some("why")).outcome, RowOutcome::Skipped);
    let mut store = Store::default();
    let args = json!({});
    store.admit(request(1, None, S1, &args), Instant::now());
    let receipt = store.find(Some(S1), &op(1)).expect("kept");
    assert_eq!(receipt.client.as_deref(), Some("Claude Code"), "the client name cleaned");
}

#[test]
fn a_client_request_id_runs_once() {
    let now = Instant::now();
    let mut store = Store::default();
    let args = json!({"id": "a", "name": "B"});
    let reordered: Value = serde_json::from_str(r#"{"name": "B", "id": "a"}"#).expect("json");
    assert_eq!(store.admit(request(1, Some("r1"), S1, &args), now), Admission::New);
    // the same id and arguments, even while the first is live: its receipt
    match store.admit(request(2, Some("r1"), S1, &reordered), now) {
        Admission::Duplicate(r) => assert_eq!((r.operation_id.as_str(), r.state), (op(1).as_str(), State::Waiting)),
        other => panic!("{other:?}"),
    }
    done(&mut store, 1, now);
    match store.admit(request(3, Some("r1"), S1, &args), now) {
        Admission::Duplicate(r) => assert_eq!(r.state, State::Done),
        other => panic!("{other:?}"),
    }
    // other arguments: conflict
    let other = json!({"id": "a", "name": "C"});
    assert_eq!(store.admit(request(4, Some("r1"), S1, &other), now), Admission::Conflict);
    // the same id on another login is another request
    assert_eq!(store.admit(request(5, Some("r1"), S2, &args), now), Admission::New);
    assert_eq!(store.len(), 2, "duplicates and conflicts make no receipt");
}

#[test]
fn eviction_takes_finished_receipts_oldest_first_and_retires_their_ids() {
    let now = Instant::now();
    let mut store = Store::default();
    let args = json!({});
    for n in 0..RECEIPTS_MAX {
        let id = format!("r{n}");
        assert_eq!(store.admit(request(n, Some(&id), S1, &args), now), Admission::New);
    }
    // every receipt live: a new change is busy, and nothing is evicted
    assert_eq!(store.admit(request(500, None, S1, &args), now), Admission::Busy);
    assert_eq!(store.len(), RECEIPTS_MAX);
    // the 3rd and the 7th finish: the 3rd (oldest finished) goes first
    done(&mut store, 7, now);
    done(&mut store, 3, now);
    assert_eq!(store.admit(request(501, None, S1, &args), now), Admission::New);
    assert!(store.find(Some(S1), &op(3)).is_none(), "the oldest finished left");
    assert!(store.find(Some(S1), &op(7)).is_some());
    assert!(store.find(Some(S1), &op(0)).is_some(), "live receipts stay, however old");
    // an evicted id is never taken for a new request
    assert_eq!(store.admit(request(502, Some("r3"), S1, &args), now), Admission::Conflict);
    assert_eq!(store.admit(request(503, None, S1, &args), now), Admission::New, "the 7th goes now");
    assert!(store.find(Some(S1), &op(7)).is_none());
}

#[test]
fn finished_receipts_leave_after_two_hours_and_a_login_change_clears_all() {
    let now = Instant::now();
    let mut store = Store::default();
    let args = json!({});
    store.admit(request(1, Some("old"), S1, &args), now);
    store.admit(request(2, None, S1, &args), now);
    done(&mut store, 1, now);
    store.prune(now + RETENTION - Duration::from_secs(1));
    assert_eq!(store.len(), 2);
    store.prune(now + RETENTION);
    assert_eq!(store.len(), 1, "the live one stays");
    assert_eq!(
        store.admit(request(3, Some("old"), S1, &args), now + RETENTION),
        Admission::Conflict
    );
    store.clear();
    assert_eq!(store.len(), 0);
    assert_eq!(store.admit(request(4, Some("old"), S1, &args), now), Admission::New, "a new login");
}

#[test]
fn another_login_does_not_find_a_receipt() {
    let now = Instant::now();
    let mut store = Store::default();
    let args = json!({});
    store.admit(request(1, Some("mine"), S1, &args), now);
    assert!(store.find(Some(S1), &op(1)).is_some());
    assert!(store.find(Some(S2), &op(1)).is_none());
    assert!(store.find(None, &op(1)).is_none());
    assert!(store.find_request(Some(S2), "mine").is_none());
    for n in 2..30 {
        store.admit(request(n, None, if n % 2 == 0 { S1 } else { S2 }, &args), now);
    }
    let recent = store.recent(Some(S1), STATUS_RECENT);
    assert_eq!(recent.len(), 15);
    assert_eq!(recent[0].operation_id, op(28), "newest first");
    assert!(recent.iter().all(|r| r.viewer_session.as_deref() == Some(S1)));
}

#[test]
fn rows_are_bounded_across_receipts() {
    let now = Instant::now();
    let mut store = Store::default();
    let args = json!({});
    let rows = |n: usize| (0..n).map(|i| Row::new(format!("{i}"), "a", "b", None)).collect::<Vec<_>>();
    for n in 0..40 {
        store.admit(request(n, None, S1, &args), now);
        assert!(store.set_rows(&op(n), rows(50)), "{n}");
        done(&mut store, n, now);
    }
    // 40 x 50 = 2,000: the next plan pushes the oldest out
    store.admit(request(40, None, S1, &args), now);
    assert!(store.set_rows(&op(40), rows(50)));
    assert!(store.find(Some(S1), &op(0)).is_none());
    let total: usize = store.all().map(|r| r.rows.len()).sum();
    assert!(total <= ROWS_MAX, "{total}");
    // live rows are never pushed out
    let mut live = Store::default();
    for n in 0..40 {
        live.admit(request(n, None, S1, &args), now);
        assert!(live.set_rows(&op(n), rows(50)));
    }
    live.admit(request(40, None, S1, &args), now);
    assert!(!live.set_rows(&op(40), rows(1)), "no room");
}

#[test]
fn an_answer_ends_its_receipt() {
    let now = Instant::now();
    let mut store = Store::default();
    let args = json!({});
    let finish = |store: &mut Store, n: usize, sent: bool, result: Result<Value, ToolError>| -> Receipt {
        store.admit(request(n, None, S1, &args), now);
        if sent {
            store.update(&op(n), Receipt::mark_sent);
        }
        store.finish(&op(n), &result, now);
        store.find(Some(S1), &op(n)).expect("kept").clone()
    };
    let r = finish(
        &mut store,
        1,
        true,
        Ok(json!({"folder_id": "00000000-0000-0000-0000-000000000005", "name": "New"})),
    );
    assert_eq!(
        (r.state, r.server, r.decision),
        (State::Done, Some(Server::Confirmed), Some(Decision::Allowed))
    );
    assert_eq!(r.result, vec!["00000000-0000-0000-0000-000000000005"]);
    let r = finish(&mut store, 2, false, Ok(json!({"id": "x"})));
    assert_eq!((r.state, r.server), (State::Done, None), "nothing needed sending");
    let r = finish(&mut store, 3, true, Ok(json!({"queued": true, "done": false})));
    assert_eq!(
        (r.state, r.server),
        (State::Unconfirmed, Some(Server::Queued)),
        "queued is never done"
    );
    let r = finish(
        &mut store,
        4,
        true,
        Err(ToolError::new(ErrorCode::Failed, "the server refused the change: nope")),
    );
    assert_eq!(
        (r.state, r.server, r.error),
        (State::Failed, Some(Server::Refused), Some(ErrorCode::Failed))
    );
    assert!(r.server_reason.as_deref().is_some_and(|s| s.contains("nope")));
    let r = finish(&mut store, 5, true, Err(ToolError::new(ErrorCode::Timeout, "may still land")));
    assert_eq!((r.state, r.server), (State::Unconfirmed, Some(Server::NoAnswer)));
    let r = finish(&mut store, 6, false, Err(ToolError::new(ErrorCode::NotAllowed, "rules")));
    assert_eq!((r.state, r.decision), (State::Refused, Some(Decision::RefusedByViewer)));
    let r = finish(&mut store, 7, false, Err(ToolError::new(ErrorCode::PermissionDenied, "levels")));
    assert_eq!((r.state, r.decision), (State::Refused, Some(Decision::RefusedPermissionsChanged)));
    let r = finish(&mut store, 8, false, Err(ToolError::new(ErrorCode::SessionChanged, "login")));
    assert_eq!(r.decision, Some(Decision::RefusedSessionChanged));
    // the user's own refusal is kept
    store.admit(request(9, None, S1, &args), now);
    store.update(&op(9), |r| r.decide(Decision::RefusedByUser));
    store.finish(&op(9), &Err(ToolError::new(ErrorCode::PermissionDenied, "user")), now);
    assert_eq!(store.find(Some(S1), &op(9)).expect("kept").decision, Some(Decision::RefusedByUser));
    // an end is final
    assert_eq!(
        store.finish(&op(1), &Err(ToolError::new(ErrorCode::Failed, "late")), now),
        Some(State::Done)
    );
}

#[test]
fn a_late_answer_settles_an_unconfirmed_receipt() {
    let now = Instant::now();
    let mut store = Store::default();
    let args = json!({});
    for n in 1..=3 {
        store.admit(request(n, None, S1, &args), now);
        store.update(&op(n), |r| {
            r.mark_sent();
            r.rows = vec![Row::new("a", "x", "y", None), Row::new("b", "x", "", Some("skipped"))];
        });
        store.finish(&op(n), &Err(ToolError::new(ErrorCode::Timeout, "no answer")), now);
    }
    assert_eq!(store.settle(&op(1), Late::Confirmed, now), Some(State::Done));
    let r = store.find(Some(S1), &op(1)).expect("kept");
    assert_eq!((r.server, r.error), (Some(Server::Confirmed), None));
    assert_eq!(r.rows[0].outcome, RowOutcome::Done);
    assert_eq!(r.rows[1].outcome, RowOutcome::Skipped);
    assert_eq!(store.settle(&op(2), Late::Refused("gone".into()), now), Some(State::Failed));
    assert_eq!(store.find(Some(S1), &op(2)).expect("kept").rows[0].outcome, RowOutcome::Unknown);
    assert_eq!(store.settle(&op(3), Late::Queued, now), Some(State::Unconfirmed));
    // a settled receipt does not move again
    assert_eq!(store.settle(&op(1), Late::Refused("x".into()), now), Some(State::Done));
}

#[test]
fn names_reach_the_assistant_only_with_read() {
    let now = Instant::now();
    let mut store = Store::default();
    let args = json!({});
    store.admit(request(1, Some("r"), S1, &args), now);
    store.update(&op(1), |r| {
        r.set_summary("Rename “Secret” to “Public”");
        r.rows = vec![
            Row::new("a", "Secret", "Public", None),
            Row::new("b", "Other", "", Some("no Modify")),
        ];
        r.mark_sent();
    });
    store.finish(&op(1), &Err(ToolError::new(ErrorCode::Failed, "“Secret” is gone")), now);
    let receipt = store.find(Some(S1), &op(1)).expect("kept");
    let with = receipt.to_json(true).to_string();
    assert!(
        with.contains("Secret") && with.contains("Public") && with.contains("no Modify"),
        "{with}"
    );
    let without = receipt.to_json(false);
    let text = without.to_string();
    for name in ["Secret", "Public", "Other", "no Modify", "Rename"] {
        assert!(!text.contains(name), "{name} in {text}");
    }
    assert_eq!(without["rows"][0], json!({"id": "a", "outcome": "unknown"}));
    assert_eq!(without["row_counts"], json!({"total": 2, "skipped": 1, "unknown": 1}));
    assert_eq!(without["state"], "failed");
    assert_eq!(without["client_request_id"], "r");
    assert!(without["times_ms"]["asked"].as_u64().is_some_and(|ms| ms > 0));
}

#[test]
fn the_result_keeps_ids_never_other_values() {
    let answer = json!({
        "folder_id": "00000000-0000-0000-0000-000000000001",
        "moved": ["00000000-0000-0000-0000-000000000002", "not an id"],
        "restored": [{"id": "00000000-0000-0000-0000-000000000003", "folder_id": "00000000-0000-0000-0000-000000000004", "name": "00000000-0000-0000-0000-000000000009"}],
        "asset_id": "00000000-0000-0000-0000-000000000099",
        "texture_id": "00000000-0000-0000-0000-000000000098",
        "name": "00000000-0000-0000-0000-000000000097"
    });
    let ids = result_ids(&answer);
    assert_eq!(
        ids,
        vec![
            "00000000-0000-0000-0000-000000000001",
            "00000000-0000-0000-0000-000000000002",
            "00000000-0000-0000-0000-000000000003",
            "00000000-0000-0000-0000-000000000004",
        ]
    );
}

#[test]
fn a_tracked_answer_carries_its_operation() {
    let ledger = Ledger::default();
    let args = json!({});
    ledger.with(|s| s.admit(request(1, None, S1, &args), Instant::now()));
    ledger.with(|s| s.admit(request(2, None, S1, &args), Instant::now()));
    let one = Tracked {
        ledger: ledger.clone(),
        operation_id: op(1),
    };
    let answer = one.decorate(Ok(json!({"id": "x"}))).expect("ok");
    assert_eq!(answer["operation_id"], op(1));
    assert_eq!(answer["state"], "done");
    let two = Tracked {
        ledger: ledger.clone(),
        operation_id: op(2),
    };
    let e = two.decorate(Err(ToolError::new(ErrorCode::NotAllowed, "no"))).expect_err("refused");
    assert_eq!(e.operation_id.as_deref(), Some(op(2).as_str()));
    assert!(e.to_string().contains(&op(2)), "{e}");
    assert_eq!(ledger.with(|s| s.find(Some(S1), &op(2)).map(|r| r.state)), Some(State::Refused));
}
