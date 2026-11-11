//! A viewer the session host started: its process, and the two threads of
//! its link (session-host plan, section 8).
//!
//! - The viewer is this same program, `--profile <name> --managed`. The
//!   host never gives it a login, a password or a link: each character
//!   logs in on its own login screen.
//! - Its standard input is a pipe only the host and that child hold: the
//!   bootstrap, then the commands (the **writer** thread). When the host
//!   ends, however it ends, the pipe ends and the viewer goes on by itself.
//! - The **reader** thread waits for the viewer on the private endpoint of
//!   this launch (`transport.rs`), takes nothing from it before its hello
//!   passes (process, slot, one-time token, protocol, build), then hands
//!   its reports to the host's window.
//!
//! The host never waits on a viewer: both threads may block for as long
//! as a viewer is slow, the window's thread only ever tries to queue.

use super::super::{managed, transport};
use ::folderstorm_session::ipc::{self, Bootstrap, Refusal, Status, ToHost, ToViewer};
use ::folderstorm_session::profile::ProfileName;
use crossbeam_channel::{Receiver, Sender};
use std::io::{self, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// What a viewer's reader thread tells the host's window.
#[derive(Debug)]
pub struct Event {
    pub slot: usize,
    pub launch: u32,
    pub kind: Kind,
}

#[derive(Debug)]
pub enum Kind {
    /// The viewer's hello passed: it is managed.
    Connected,
    /// Its hello did not pass: nothing else of it is read.
    Refused(Refusal),
    Status(Box<Status>),
    Heartbeat(u64),
}

/// A started viewer.
pub struct Viewer {
    pub launch: u32,
    child: Child,
    commands: Sender<ToViewer>,
    endpoint: String,
    /// The reader thread stops waiting for the viewer.
    cancelled: Arc<AtomicBool>,
    /// The viewer reached the endpoint.
    arrived: Arc<AtomicBool>,
}

impl Viewer {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Queues a command; false when the viewer has not read the earlier
    /// ones (the desk sends an assignment again, a click can be repeated).
    pub fn send(&self, message: ToViewer) -> bool {
        self.commands.try_send(message).is_ok()
    }

    /// Whether the process ended, and cleanly.
    pub fn exited(&mut self) -> Option<bool> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.success()),
            Ok(None) => None,
            Err(e) => {
                log::warn!("session host: a viewer's process cannot be watched ({e})");
                Some(false)
            }
        }
    }

    /// Ends the process: the user's confirmed choice only.
    pub fn kill(&mut self) {
        if let Err(e) = self.child.kill() {
            log::warn!("session host: the viewer's process was not ended ({e})");
        }
    }
}

impl Drop for Viewer {
    /// The host lets go of the viewer: the process is **not** ended. Its
    /// command pipe closes with the writer thread (the viewer then goes on
    /// by itself), and a reader thread still waiting for it stops.
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if !self.arrived.load(Ordering::Acquire) {
            transport::unblock(&self.endpoint);
        }
    }
}

/// The arguments of a character's viewer: its profile, `--managed`, and
/// the host's own language and title when it was given them.
fn arguments(profile: &ProfileName, language: Option<&str>, title: Option<&str>) -> Vec<String> {
    let mut args = vec!["--profile".to_owned(), profile.shown().to_owned(), "--managed".to_owned()];
    if let Some(language) = language {
        args.extend(["--language".to_owned(), language.to_owned()]);
    }
    if let Some(title) = title {
        args.extend(["--title".to_owned(), title.to_owned()]);
    }
    args
}

/// Starts the viewer of `profile` as launch `launch` of `slot`.
pub fn spawn(slot: usize, launch: u32, profile: &ProfileName, events: &Sender<Event>) -> io::Result<Viewer> {
    let listener = transport::Listener::bind(slot as u8)?;
    let endpoint = listener.endpoint().to_owned();
    let boot = Bootstrap {
        protocol: ipc::PROTOCOL,
        slot: slot as u8,
        host_pid: std::process::id(),
        endpoint: endpoint.clone(),
        token: transport::random_hex(ipc::TOKEN_LEN),
    };
    let cli = crate::cli::get();
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(arguments(profile, cli.language.as_deref(), cli.title.as_deref()))
        // the demo's "start a host" is the host's, not its viewers'
        .env_remove(super::DEMO_VARIABLE)
        .stdin(Stdio::piped());
    let mut child = command.spawn()?;
    let Some(stdin) = child.stdin.take() else {
        let _ = child.kill();
        return Err(io::Error::other("the viewer has no command pipe"));
    };
    // (bounded: a viewer that stops reading fills its pipe, then this)
    let (commands, queued) = crossbeam_channel::bounded(64);
    let cancelled = Arc::new(AtomicBool::new(false));
    let arrived = Arc::new(AtomicBool::new(false));
    let viewer = Viewer {
        launch,
        child,
        commands,
        endpoint,
        cancelled: cancelled.clone(),
        arrived: arrived.clone(),
    };
    let pid = viewer.pid();
    let token = boot.token.clone();
    std::thread::Builder::new()
        .name(format!("folderstorm-host-out-{slot}"))
        .spawn(move || write_commands(stdin, &boot, &queued))?;
    let events = events.clone();
    std::thread::Builder::new()
        .name(format!("folderstorm-host-in-{slot}"))
        .spawn(move || {
            let from_viewer = match listener.accept(pid, &cancelled) {
                Ok(from_viewer) => from_viewer,
                Err(e) => {
                    if !cancelled.load(Ordering::Acquire) {
                        log::warn!("session host: character {} did not reach its endpoint ({e})", slot + 1);
                    }
                    return;
                }
            };
            arrived.store(true, Ordering::Release);
            read_reports(from_viewer, slot, launch, &token, &managed::build_id(), &events);
        })?;
    log::info!(
        "session host: character {} started (profile {}, process {pid})",
        slot + 1,
        profile.key()
    );
    Ok(viewer)
}

/// The writer thread: the bootstrap, then each queued command, until the
/// host drops the viewer or the viewer's end of the pipe closes.
fn write_commands(mut to_viewer: impl Write, boot: &Bootstrap, queued: &Receiver<ToViewer>) {
    if let Err(e) = ipc::write_frame(&mut to_viewer, boot) {
        log::warn!("session host: the bootstrap was not written ({e})");
        return;
    }
    for message in queued {
        if let Err(e) = ipc::write_frame(&mut to_viewer, &message) {
            log::info!("session host: a viewer's command pipe closed ({e})");
            return;
        }
    }
}

/// The reader thread, once the viewer reached the endpoint: its hello,
/// then its reports until it ends or says something that is not one.
fn read_reports(mut from_viewer: impl Read, slot: usize, launch: u32, token: &str, build: &str, events: &Sender<Event>) {
    let tell = |kind| events.send(Event { slot, launch, kind }).is_ok();
    let hello = match ipc::read_frame::<ToHost>(&mut from_viewer) {
        Ok(ToHost::Hello(hello)) => hello.check(slot as u8, token, build),
        Ok(_) | Err(ipc::FrameError::Invalid(_) | ipc::FrameError::TooLong(_)) => Err(Refusal::Protocol),
        Err(e) => {
            log::info!("session host: character {} ended before its first report ({e})", slot + 1);
            return;
        }
    };
    if let Err(refusal) = hello {
        log::warn!("session host: character {} refused ({refusal:?})", slot + 1);
        tell(Kind::Refused(refusal));
        return;
    }
    if !tell(Kind::Connected) {
        return;
    }
    loop {
        let kind = match ipc::read_frame::<ToHost>(&mut from_viewer) {
            Ok(ToHost::Status(status)) => Kind::Status(status),
            Ok(ToHost::Heartbeat { loop_count }) => Kind::Heartbeat(loop_count),
            // a second hello is not a report
            Ok(ToHost::Hello(_)) => {
                log::warn!(
                    "session host: character {} repeated its hello; its reports are no longer read",
                    slot + 1
                );
                return;
            }
            Err(e) => {
                // closed: the process watch says how it ended; anything
                // else: out of step, never guessed at
                log::info!("session host: character {} reports ended ({e})", slot + 1);
                return;
            }
        };
        if !tell(kind) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::folderstorm_session::ipc::{Hello, write_frame};

    fn token() -> String {
        "0f".repeat(ipc::TOKEN_LEN / 2)
    }

    fn hello(token: String) -> ToHost {
        ToHost::Hello(Hello {
            protocol: ipc::PROTOCOL,
            build: "build".into(),
            token,
            slot: 1,
            pid: 5,
        })
    }

    /// What the reader thread tells of `frames`.
    fn told(frames: &[ToHost]) -> Vec<Kind> {
        let mut wire = Vec::new();
        for frame in frames {
            write_frame(&mut wire, frame).expect("written");
        }
        told_of(&wire)
    }

    fn told_of(wire: &[u8]) -> Vec<Kind> {
        let (tx, rx) = crossbeam_channel::unbounded();
        read_reports(wire, 1, 7, &token(), "build", &tx);
        drop(tx);
        rx.iter()
            .map(|e| {
                assert_eq!((e.slot, e.launch), (1, 7));
                e.kind
            })
            .collect()
    }

    #[test]
    fn a_viewer_is_managed_only_after_its_hello_passes() {
        let status = Status {
            unread: 2,
            ..Default::default()
        };
        let events = told(&[
            hello(token()),
            ToHost::Status(Box::new(status)),
            ToHost::Heartbeat { loop_count: 3 },
        ]);
        assert!(matches!(
            events.as_slice(),
            [Kind::Connected, Kind::Status(s), Kind::Heartbeat(3)] if s.unread == 2
        ));
    }

    #[test]
    fn nothing_is_taken_from_a_client_without_the_token() {
        let wrong = "1e".repeat(ipc::TOKEN_LEN / 2);
        let events = told(&[hello(wrong), ToHost::Heartbeat { loop_count: 3 }]);
        assert!(matches!(events.as_slice(), [Kind::Refused(Refusal::Token)]));
        // a report before any hello, or something that is no message
        let events = told(&[ToHost::Heartbeat { loop_count: 3 }, hello(token())]);
        assert!(matches!(events.as_slice(), [Kind::Refused(Refusal::Protocol)]));
        assert!(matches!(
            told_of(b"GET / HTTP/1.1\r\n\r\n").as_slice(),
            [Kind::Refused(Refusal::Protocol)]
        ));
        // a client that says nothing is not even refused
        assert!(told_of(b"").is_empty());
    }

    #[test]
    fn reports_stop_at_the_first_thing_that_is_not_one() {
        let mut wire = Vec::new();
        write_frame(&mut wire, &hello(token())).expect("written");
        write_frame(&mut wire, &ToHost::Heartbeat { loop_count: 1 }).expect("written");
        write_frame(&mut wire, &serde_json::json!({"Status": {"name": 5}})).expect("written");
        write_frame(&mut wire, &ToHost::Heartbeat { loop_count: 2 }).expect("written");
        assert!(matches!(told_of(&wire).as_slice(), [Kind::Connected, Kind::Heartbeat(1)]));
        // a second hello does not start a new link
        let events = told(&[hello(token()), hello(token()), ToHost::Heartbeat { loop_count: 2 }]);
        assert!(matches!(events.as_slice(), [Kind::Connected]));
    }

    #[test]
    fn the_writer_sends_the_bootstrap_first_then_the_commands_in_order() {
        let boot = Bootstrap {
            protocol: ipc::PROTOCOL,
            slot: 1,
            host_pid: 9,
            endpoint: "e".into(),
            token: token(),
        };
        let (tx, rx) = crossbeam_channel::bounded(8);
        tx.send(ToViewer::Minimise).expect("queued");
        tx.send(ToViewer::Quit).expect("queued");
        drop(tx);
        let mut wire = Vec::new();
        write_commands(&mut wire, &boot, &rx);
        let mut read = wire.as_slice();
        assert_eq!(ipc::read_frame::<Bootstrap>(&mut read).expect("bootstrap"), boot);
        assert_eq!(ipc::read_frame::<ToViewer>(&mut read).expect("command"), ToViewer::Minimise);
        assert_eq!(ipc::read_frame::<ToViewer>(&mut read).expect("command"), ToViewer::Quit);
        assert!(read.is_empty());
    }

    #[test]
    fn a_character_s_viewer_gets_its_profile_and_never_a_login() {
        let name = ProfileName::parse("Alt1").expect("name");
        assert_eq!(arguments(&name, None, None), ["--profile", "Alt1", "--managed"]);
        let args = arguments(&name, Some("fr"), Some("Test host"));
        assert_eq!(args, ["--profile", "Alt1", "--managed", "--language", "fr", "--title", "Test host"]);
        assert!(!args.iter().any(|a| a == "--login" || a == "--url" || a == "--session-host"));
    }

    /// Host and viewer sides whole, over the real local endpoint: the
    /// host's bootstrap and commands on the viewer's input, the viewer's
    /// hello and report on the socket, into the desk.
    #[cfg(unix)]
    #[test]
    fn host_and_viewer_meet_over_the_real_endpoint() {
        use ::folderstorm_session::host::{Config, Desk, Effect, Phase};
        use ::folderstorm_session::ipc::Screen;
        use std::time::Instant;

        let now = Instant::now();
        let mut desk = Desk::new(Config::demo(1));
        desk.launch(0, true);
        assert_eq!(desk.take_effects(), vec![Effect::Spawn { slot: 0, launch: 1 }]);

        // the host's side of a launch, as `spawn` makes it
        let listener = transport::Listener::bind(0).expect("bound");
        let boot = Bootstrap {
            protocol: ipc::PROTOCOL,
            slot: 0,
            host_pid: std::process::id(),
            endpoint: listener.endpoint().to_owned(),
            token: transport::random_hex(ipc::TOKEN_LEN),
        };
        let (events_tx, events) = crossbeam_channel::unbounded();
        let cancelled = AtomicBool::new(false);
        let token = boot.token.clone();
        let viewer_events = std::thread::scope(|scope| {
            scope.spawn(|| {
                // (the "viewer" is this very process)
                let from_viewer = listener.accept(std::process::id(), &cancelled).expect("accepted");
                read_reports(from_viewer, 0, 1, &token, &managed::build_id(), &events_tx);
            });
            // what the writer thread puts on the viewer's standard input
            let (commands, queued) = crossbeam_channel::bounded(8);
            commands.send(ToViewer::Minimise).expect("queued");
            commands.send(ToViewer::Quit).expect("queued");
            drop(commands);
            let mut stdin = Vec::new();
            write_commands(&mut stdin, &boot, &queued);
            let report = Status {
                screen: Screen::World,
                name: "Demo Resident".into(),
                region: "Demo".into(),
                unread: 2,
                ..Default::default()
            };
            managed::play_viewer(&stdin, report)
        });
        assert_eq!(
            viewer_events,
            vec![
                managed::Event::Linked { slot: 0 },
                managed::Event::Command(ToViewer::Minimise),
                managed::Event::Command(ToViewer::Quit),
                managed::Event::Gone,
            ]
        );

        // what the host's window thread then tells the desk
        drop(events_tx);
        let mut kinds = Vec::new();
        for Event { slot, launch, kind } in events.iter() {
            match &kind {
                Kind::Connected => desk.connected(slot, launch, now),
                Kind::Status(status) => desk.status(slot, launch, (**status).clone(), now),
                Kind::Heartbeat(count) => desk.heartbeat(slot, launch, *count, now),
                Kind::Refused(refusal) => desk.refused(slot, launch, *refusal),
            }
            kinds.push(kind);
        }
        assert!(matches!(kinds.first(), Some(Kind::Connected)), "{kinds:?}");
        assert!(kinds.iter().any(|k| matches!(k, Kind::Status(_))), "{kinds:?}");
        let slot = desk.slot(0).expect("slot");
        assert_eq!(slot.phase, Phase::Running);
        assert_eq!(desk.active(), Some(0));
        let status = slot.status.as_ref().expect("status");
        assert_eq!(
            (status.name.as_str(), status.region.as_str(), status.unread),
            ("Demo Resident", "Demo", 2)
        );
    }

    /// The same meeting with a token that is not the launch's: the viewer
    /// side sends its hello, the host takes nothing of it.
    #[cfg(unix)]
    #[test]
    fn a_viewer_with_another_token_is_refused_over_the_real_endpoint() {
        let listener = transport::Listener::bind(1).expect("bound");
        let boot = Bootstrap {
            protocol: ipc::PROTOCOL,
            slot: 1,
            host_pid: std::process::id(),
            endpoint: listener.endpoint().to_owned(),
            token: transport::random_hex(ipc::TOKEN_LEN),
        };
        let (events_tx, events) = crossbeam_channel::unbounded();
        let cancelled = AtomicBool::new(false);
        let expected = transport::random_hex(ipc::TOKEN_LEN);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let from_viewer = listener.accept(std::process::id(), &cancelled).expect("accepted");
                read_reports(from_viewer, 1, 1, &expected, &managed::build_id(), &events_tx);
            });
            let mut stdin = Vec::new();
            ipc::write_frame(&mut stdin, &boot).expect("written");
            managed::play_viewer(&stdin, Status::default());
        });
        drop(events_tx);
        let kinds: Vec<Kind> = events.iter().map(|e| e.kind).collect();
        assert!(matches!(kinds.as_slice(), [Kind::Refused(Refusal::Token)]), "{kinds:?}");
    }
}
