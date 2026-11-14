use super::*;
use crate::host::{Config, Effect};
use crate::ipc::{Screen, Status, ToViewer};

/// A desk of three characters: `Home` (with access), `Alt1` (with access)
/// and `Alt2` (without).
fn desk() -> Desk {
    let mut d = Desk::new(Config::default());
    for name in ["Home", "Alt1", "Alt2"] {
        d.add(name).expect("added");
    }
    d.set_controller_access(0, true);
    d.set_controller_access(1, true);
    d.take_effects();
    d
}

fn on() -> Settings {
    Settings {
        enabled: true,
        metadata: false,
        levels: HostLevels::all(Level::Allow),
    }
}

/// The character of `slot` started and on that screen.
fn running(d: &mut Desk, slot: usize, screen: Screen, now: Instant) {
    d.launch(slot, slot == 0);
    let launch = d.slot(slot).expect("slot").launch();
    d.connected(slot, launch, now);
    let status = Status {
        screen,
        name: format!("Avatar {}", slot + 1),
        ..Status::default()
    };
    d.status(slot, launch, status, now);
    d.heartbeat(slot, launch, 1, now);
}

fn request(tool: &str, args: Value) -> Request {
    parse(tool, &args).expect("parsed")
}

#[test]
fn the_settings_are_off_by_default_and_read_from_older_files() {
    let s = Settings::default();
    assert!(!s.enabled && !s.metadata);
    assert!(HostClass::ALL.iter().all(|c| s.levels.level(*c) == Level::Ask));
    // a checkpoint 10 file: no controller, no access
    let config: Config = serde_json::from_value(json!({"slots": [{"slot": 0, "profile": "Home"}]})).expect("read");
    assert_eq!(config.controller, Settings::default());
    assert!(!config.slots[0].controller);
    // saved and read back
    let mut d = desk();
    let mut s = on();
    s.metadata = true;
    s.levels.set(HostClass::Logout, Level::Never);
    d.set_controller(s);
    assert!(d.take_effects().contains(&Effect::Save));
    let text = serde_json::to_string(&d.config()).expect("json");
    let back = Desk::new(serde_json::from_str(&text).expect("read"));
    assert_eq!(back.controller(), s);
    assert!(back.slot(0).expect("home").controller && !back.slot(2).expect("alt2").controller);
    // nothing written for a character without access
    assert!(
        !serde_json::to_string(&Config::demo(1))
            .expect("json")
            .contains("\"controller\":true")
    );
}

#[test]
fn arguments_are_read_strictly() {
    assert_eq!(request("host_characters", json!({})).action, Action::List);
    assert_eq!(request("host_characters", Value::Null).action, Action::List);
    let r = request("host_background_mode", json!({"profile": "Alt1", "mode": "economy", "launch": 2}));
    assert_eq!(
        r,
        Request {
            action: Action::Background(Standby::Economy),
            profile: Some("Alt1".into()),
            launch: Some(2)
        }
    );
    assert_eq!(
        request("host_monitor", json!({"cycling": true, "open": true})).action,
        Action::Cycling(true)
    );
    assert_eq!(
        request("host_monitor", json!({"profile": "Alt1", "open": false})).action,
        Action::Monitor(false)
    );
    for (tool, args) in [
        ("host_activate", json!({})),
        ("host_activate", json!({"profile": ""})),
        ("host_activate", json!({"profile": 3})),
        ("host_activate", json!({"profile": "Alt1", "slot": 2})),
        ("host_activate", json!({"profile": "Alt1", "launch": 0})),
        ("host_activate", json!({"profile": "Alt1", "launch": "x"})),
        ("host_background_mode", json!({"profile": "Alt1", "mode": "hot"})),
        ("host_monitor", json!({"profile": "Alt1"})),
        ("host_monitor", json!({"profile": "Alt1", "cycling": true, "open": true})),
        ("host_monitor", json!({"cycling": true})),
        ("host_characters", json!({"profile": "Alt1"})),
        ("host_logout", json!([1])),
        ("host_reboot", json!({})),
    ] {
        assert!(matches!(parse(tool, &args), Err(Refusal::Invalid(_))), "{tool} {args}");
    }
}

#[test]
fn off_never_and_characters_without_access_are_refused() {
    let now = Instant::now();
    let mut d = desk();
    for slot in 0..3 {
        running(&mut d, slot, Screen::World, now);
    }
    let activate = |p: &str| request("host_activate", json!({"profile": p}));
    // off: even the list
    assert_eq!(
        admit(&Settings::default(), &d, &request("host_characters", json!({}))),
        Err(Refusal::Off)
    );
    let mut s = on();
    assert_eq!(admit(&s, &d, &request("host_characters", json!({}))), Ok(Admitted::List));
    assert_eq!(admit(&s, &d, &activate("alt1")), Ok(Admitted::Act { slot: Some(1), ask: false }));
    // no access, or no such profile: the same answer
    assert_eq!(admit(&s, &d, &activate("Alt2")), Err(Refusal::NotFound));
    assert_eq!(admit(&s, &d, &activate("Nobody")), Err(Refusal::NotFound));
    assert_eq!(admit(&s, &d, &activate("../x")), Err(Refusal::NotFound));
    // Ask and Never
    s.levels.set(HostClass::Activate, Level::Ask);
    assert_eq!(admit(&s, &d, &activate("Alt1")), Ok(Admitted::Act { slot: Some(1), ask: true }));
    s.levels.set(HostClass::Activate, Level::Never);
    assert_eq!(admit(&s, &d, &activate("Alt1")), Err(Refusal::Never));
    // a launch that is not the character's now
    let r = request("host_logout", json!({"profile": "Alt1", "launch": 1}));
    assert!(admit(&s, &d, &r).is_ok());
    let r = request("host_logout", json!({"profile": "Alt1", "launch": 2}));
    assert_eq!(admit(&s, &d, &r), Err(Refusal::Restarted));
    // the cycling monitor names nobody
    let r = request("host_monitor", json!({"cycling": true, "open": true}));
    assert_eq!(admit(&s, &d, &r), Ok(Admitted::Act { slot: None, ask: false }));
}

#[test]
fn each_action_is_offered_only_when_the_card_would_offer_it() {
    let now = Instant::now();
    let mut d = desk();
    let check = |d: &Desk, slot, action| eligible(d, slot, action);
    // stopped
    assert_eq!(check(&d, 1, Action::Activate), Err(Refusal::NotReady(Why::NoViewer)));
    assert_eq!(check(&d, 1, Action::OpenLogin), Ok(()));
    assert_eq!(check(&d, 1, Action::Logout), Err(Refusal::NotReady(Why::NoViewer)));
    assert_eq!(check(&d, 1, Action::Restart), Err(Refusal::NotReady(Why::NoViewer)));
    assert_eq!(check(&d, 1, Action::StopManaging), Err(Refusal::NotReady(Why::NoViewer)));
    assert_eq!(check(&d, 1, Action::Background(Standby::Economy)), Ok(()));
    // starting
    d.launch(1, false);
    assert_eq!(check(&d, 1, Action::OpenLogin), Err(Refusal::NotReady(Why::Starting)));
    assert_eq!(check(&d, 1, Action::Activate), Err(Refusal::NotReady(Why::Starting)));
    assert_eq!(check(&d, 1, Action::Restart), Err(Refusal::NotReady(Why::Starting)));
    assert_eq!(check(&d, 1, Action::Logout), Ok(()));
    // at its login screen
    running(&mut d, 0, Screen::World, now);
    let launch = d.slot(1).expect("alt1").launch();
    d.connected(1, launch, now);
    d.status(
        1,
        launch,
        Status {
            screen: Screen::Login,
            ..Status::default()
        },
        now,
    );
    assert_eq!(check(&d, 1, Action::OpenLogin), Ok(()));
    assert_eq!(check(&d, 1, Action::Activate), Err(Refusal::NotReady(Why::NotInWorld)));
    assert_eq!(check(&d, 1, Action::Monitor(true)), Err(Refusal::NotReady(Why::NotInWorld)));
    assert_eq!(check(&d, 1, Action::Restart), Ok(()));
    // in the world, behind the active one
    d.status(
        1,
        launch,
        Status {
            screen: Screen::World,
            ..Status::default()
        },
        now,
    );
    assert_eq!(check(&d, 1, Action::OpenLogin), Err(Refusal::NotReady(Why::LoggedIn)));
    assert_eq!(check(&d, 1, Action::Activate), Ok(()));
    assert_eq!(check(&d, 1, Action::Monitor(true)), Ok(()));
    // the active one has no monitor
    assert_eq!(d.active(), Some(0));
    assert_eq!(check(&d, 0, Action::Monitor(true)), Err(Refusal::NotReady(Why::Active)));
    assert_eq!(check(&d, 0, Action::Monitor(false)), Ok(()));
    // closing
    d.close(1);
    for action in [Action::Activate, Action::Logout, Action::StopManaging, Action::Restart] {
        assert_eq!(check(&d, 1, action), Err(Refusal::NotReady(Why::Closing)), "{action:?}");
    }
    // ended: started again by a restart or a login
    d.exited(1, launch, true, now);
    assert_eq!(check(&d, 1, Action::Restart), Ok(()));
    assert_eq!(check(&d, 1, Action::OpenLogin), Ok(()));
    // "Quit all" under way
    d.quit_all(now);
    assert_eq!(check(&d, 1, Action::Restart), Err(Refusal::NotReady(Why::Quitting)));
    assert_eq!(check(&d, 1, Action::OpenLogin), Err(Refusal::NotReady(Why::NoViewer)));
}

#[test]
fn the_actions_are_the_host_s_own_clicks() {
    let now = Instant::now();
    let mut d = desk();
    running(&mut d, 0, Screen::World, now);
    running(&mut d, 1, Screen::World, now);
    d.take_effects();
    // activate
    apply(&mut d, Some(1), Action::Activate, now);
    assert_eq!(d.active(), Some(1));
    // background mode, monitors, the cycling monitor
    apply(&mut d, Some(0), Action::Background(Standby::Economy), now);
    assert_eq!(d.slot(0).expect("home").standby, Standby::Economy);
    apply(&mut d, Some(0), Action::Monitor(true), now);
    assert!(d.slot(0).expect("home").monitor.is_some());
    apply(&mut d, Some(0), Action::Monitor(false), now);
    assert!(d.slot(0).expect("home").monitor.is_none());
    apply(&mut d, None, Action::Cycling(true), now);
    assert!(d.monitors().cycling.open);
    // logout: the card's Close
    d.take_effects();
    apply(&mut d, Some(0), Action::Logout, now);
    assert!(d.slot(0).expect("home").closing);
    assert!(d.take_effects().iter().any(|e| matches!(
        e,
        Effect::Send {
            slot: 0,
            message: ToViewer::Quit,
            ..
        }
    )));
    // restart: Close, then started again at its end, not activated
    apply(&mut d, Some(1), Action::Restart, now);
    assert!(d.slot(1).expect("alt1").closing);
    d.take_effects();
    d.exited(1, 1, true, now);
    assert!(d.take_effects().contains(&Effect::Spawn { slot: 1, launch: 2 }));
    assert_eq!(d.slot(1).expect("alt1").phase, Phase::Starting);
    // an ended character's restart starts it at once; nothing for one
    // that ended by a plain close
    d.exited(0, 1, true, now);
    assert!(
        !d.take_effects().iter().any(|e| matches!(e, Effect::Spawn { slot: 0, .. })),
        "closed, not restarted"
    );
    apply(&mut d, Some(0), Action::Restart, now);
    assert!(d.take_effects().contains(&Effect::Spawn { slot: 0, launch: 2 }));
    // stop managing: let go, no longer active
    let launch = d.slot(1).expect("alt1").launch();
    d.connected(1, launch, now);
    d.switch(1, now);
    d.take_effects();
    apply(&mut d, Some(1), Action::StopManaging, now);
    assert_eq!(d.slot(1).expect("alt1").phase, Phase::Unmanaged);
    assert_eq!(d.active(), None);
    assert!(d.take_effects().contains(&Effect::LetGo { slot: 1, launch }));
    assert_eq!(eligible(&d, 1, Action::Logout), Err(Refusal::NotReady(Why::Unmanaged)));
    // open a login: started, at its login screen
    d.exited(1, launch, true, now);
    d.take_effects();
    apply(&mut d, Some(1), Action::OpenLogin, now);
    assert!(d.take_effects().iter().any(|e| matches!(e, Effect::Spawn { slot: 1, .. })));
    // a restart during "Quit all" starts nothing again
    let mut q = desk();
    running(&mut q, 0, Screen::World, now);
    q.restart(0);
    q.quit_all(now);
    q.take_effects();
    q.exited(0, 1, true, now);
    assert!(!q.take_effects().iter().any(|e| matches!(e, Effect::Spawn { .. })));
}

#[test]
fn the_list_shows_names_and_monitors_only_with_metadata() {
    let now = Instant::now();
    let mut d = desk();
    running(&mut d, 0, Screen::World, now);
    running(&mut d, 1, Screen::World, now);
    running(&mut d, 2, Screen::World, now);
    d.set_alias(1, "Shop").expect("alias");
    d.set_colour(1, crate::identity::Colour::Teal);
    d.open_monitor(1);
    let mut s = on();
    let plain = list(&s, &d);
    let rows = plain["characters"].as_array().expect("rows");
    assert_eq!(rows.len(), 2, "Alt2 has no access");
    assert_eq!(rows[0]["profile"], "Home");
    assert_eq!(rows[0]["slot"], 1);
    assert_eq!(rows[0]["state"], "in_world");
    assert_eq!(rows[0]["active"], true);
    assert_eq!(rows[1]["background_mode"], "warm");
    assert_eq!(rows[1]["launch"], 1);
    let text = plain.to_string();
    for hidden in ["Avatar", "Shop", "teal", "Teal", "monitor", "cycling", "Alt2"] {
        assert!(!text.contains(hidden), "{hidden} in {text}");
    }
    s.metadata = true;
    let full = list(&s, &d);
    let alt = &full["characters"][1];
    assert_eq!(alt["character"], "Avatar 2");
    assert_eq!(alt["alias"], "Shop");
    assert!(alt["colour"].is_string());
    assert_eq!(alt["monitor"]["open"], true);
    assert_eq!(full["cycling_monitor"]["open"], false);
    assert!(!full.to_string().contains("Alt2") && !full.to_string().contains("Avatar 3"));
    // the states
    let mut e = desk();
    assert_eq!(list(&s, &e)["characters"][0]["state"], "stopped");
    e.launch(0, true);
    assert_eq!(list(&s, &e)["characters"][0]["state"], "starting");
    e.exited(0, 1, false, now);
    assert_eq!(list(&s, &e)["characters"][0]["state"], "crashed");
}

#[test]
fn questions_are_decided_again_while_they_wait() {
    let start = Instant::now();
    let mut d = desk();
    running(&mut d, 0, Screen::World, start);
    running(&mut d, 1, Screen::World, start);
    let mut s = on();
    s.levels = HostLevels::all(Level::Ask);
    let mut q: Questions<u32> = Questions::default();
    let logout = request("host_logout", json!({"profile": "Alt1"}));
    let ask = |q: &mut Questions<u32>, d: &Desk, r: &Request, tag| q.ask(d, Some(1), r.clone(), Some("Claude".into()), tag, start);
    let first = ask(&mut q, &d, &logout, 1);
    let second = ask(&mut q, &d, &request("host_activate", json!({"profile": "Alt1"})), 2);
    assert_eq!(q.front().map(|f| f.number), Some(first), "oldest first");
    assert!(q.review(&s, &d, start).is_empty());
    // Ask → Allow: still asks
    s.levels = HostLevels::all(Level::Allow);
    assert!(q.review(&s, &d, start).is_empty());
    assert_eq!(q.len(), 2);
    // a class on Never refuses its question only
    s.levels.set(HostClass::Logout, Level::Never);
    let refused = q.review(&s, &d, start);
    assert_eq!(
        refused.iter().map(|(q, why)| (q.payload, *why)).collect::<Vec<_>>(),
        [(1, Refusal::Never)]
    );
    assert_eq!(q.front().map(|f| f.number), Some(second));
    assert!(q.take(first).is_none(), "a click after the refusal does nothing");
    // access removed
    d.set_controller_access(1, false);
    let refused = q.review(&s, &d, start);
    assert_eq!(refused.iter().map(|(_, why)| *why).collect::<Vec<_>>(), [Refusal::NotFound]);
    d.set_controller_access(1, true);
    // the character started again since the question
    let tag = ask(&mut q, &d, &logout, 3);
    s.levels = HostLevels::all(Level::Ask);
    d.end(1);
    d.exited(1, 1, false, start);
    d.launch(1, false);
    let refused = q.review(&s, &d, start);
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].0.number, tag);
    // (a starting viewer can log out: it is the other launch that refuses)
    assert_eq!(refused[0].1, Refusal::Restarted);
    // no longer possible: it logged out meanwhile
    let mut d = desk();
    running(&mut d, 0, Screen::World, start);
    running(&mut d, 1, Screen::World, start);
    ask(&mut q, &d, &request("host_activate", json!({"profile": "Alt1"})), 4);
    d.status(
        1,
        1,
        Status {
            screen: Screen::Login,
            ..Status::default()
        },
        start,
    );
    assert_eq!(q.review(&s, &d, start)[0].1, Refusal::NotReady(Why::NotInWorld));
    // 60 s
    ask(&mut q, &d, &logout, 5);
    let late = start + ASK_TIMEOUT;
    assert_eq!(q.front().map(|f| f.seconds_left(start + Duration::from_secs(15))), Some(45));
    assert!(q.review(&s, &d, start + Duration::from_secs(59)).is_empty());
    assert_eq!(q.review(&s, &d, late)[0].1, Refusal::TimedOut);
    // the controller off: everything
    ask(&mut q, &d, &logout, 6);
    ask(&mut q, &d, &request("host_characters", json!({})), 7);
    s.enabled = false;
    let refused = q.review(&s, &d, start);
    assert_eq!(refused.len(), 2);
    assert!(refused.iter().all(|(_, why)| *why == Refusal::Off));
    assert!(q.is_empty());
    // an answer takes the question out
    let n = ask(&mut q, &d, &logout, 8);
    assert_eq!(q.take(n).map(|q| q.payload), Some(8));
    assert!(q.drain().is_empty());
}

#[test]
fn an_assistant_s_question_is_listed_whatever_the_kinds_chosen() {
    use crate::ipc::AttentionKind;
    use crate::settings::Attention;
    let none = Attention {
        notices: false,
        teleports: false,
        permissions: false,
        inventory: false,
        newest_first: false,
    };
    assert!(none.lists(true, AttentionKind::Assistant));
    assert!(!none.lists(true, AttentionKind::Teleport));
    assert!(!none.lists(false, AttentionKind::Assistant), "a character left out stays out");
}
