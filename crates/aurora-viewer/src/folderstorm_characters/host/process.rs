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
//! A **monitor's picture** (checkpoint 5) is not an event: the reader
//! thread reads it whole (after its header passed `ipc`'s checks), makes
//! the image the window will show, and leaves it in a place for one
//! (`monitor::Latest`). The window takes the latest when it looks; one it
//! had no time to show is replaced, never queued.
//!
//! The host never waits on a viewer: both threads may block for as long
//! as a viewer is slow, the window's thread only ever tries to queue.

use super::super::{managed, transport};
use ::folderstorm_session::ipc::{self, AttentionList, Bootstrap, ChatBatch, Refusal, SendOutcome, Status, ToHost, ToViewer};
use ::folderstorm_session::monitor::{FrameHeader, Latest};
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
    /// New lines of one of its conversations.
    Chat(Box<ChatBatch>),
    /// What waits for an answer in its window.
    Attention(Box<AttentionList>),
    /// What became of a line the host asked it to say.
    SendResult(u64, SendOutcome),
    /// Checkpoint 8: a click on the bar it draws.
    Bar(::folderstorm_session::bar::Request),
    /// The viewer never reached its endpoint: only other clients came, or
    /// the endpoint failed. Nothing more comes from this launch.
    Unreachable,
}

/// Wakes the host's window for an event that should not wait for its
/// next look (checkpoint 8: a click on a viewer's bar). Set once by the
/// window's loop; without one (the tests) events wait for the next look.
static WAKER: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> = std::sync::OnceLock::new();

pub fn set_waker(wake: impl Fn() + Send + Sync + 'static) {
    let _ = WAKER.set(Box::new(wake));
}

fn wake() {
    if let Some(wake) = WAKER.get() {
        wake();
    }
}

/// A monitor's picture as the window shows it.
pub struct Shot {
    pub header: FrameHeader,
    pub image: egui::ColorImage,
}

/// Where a viewer's reader thread leaves its monitor's latest picture.
pub type Shots = Arc<parking_lot::Mutex<Latest<Shot>>>;

/// A started viewer.
pub struct Viewer {
    pub launch: u32,
    child: Child,
    /// None once the host let the viewer go (`let_go`).
    commands: Option<Sender<ToViewer>>,
    endpoint: String,
    /// The reader thread stops waiting for the viewer.
    cancelled: Arc<AtomicBool>,
    /// The viewer reached the endpoint.
    arrived: Arc<AtomicBool>,
    /// Its monitor's latest picture, not looked at yet.
    shots: Shots,
}

impl Viewer {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Queues a command; false when the viewer has not read the earlier
    /// ones (the desk sends an assignment again, a click can be repeated).
    pub fn send(&self, message: ToViewer) -> bool {
        self.commands.as_ref().is_some_and(|c| c.try_send(message).is_ok())
    }

    /// Its monitor's latest picture since the last call.
    pub fn shot(&self) -> Option<Shot> {
        self.shots.lock().take()
    }

    /// The host lets the viewer go (refused, or it never reached the
    /// host): its command pipe closes with the writer thread, so the
    /// viewer learns it is not managed and goes on by itself. The process
    /// is still watched for its end, and never ended here.
    pub fn let_go(&mut self) {
        self.commands = None;
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

/// The arguments of a character's viewer: its profile, `--managed`, its
/// slot (where its startup splash sits, `splash.rs`), and the host's own
/// language and title when it was given them.
fn arguments(profile: &ProfileName, slot: usize, language: Option<&str>, title: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--profile".to_owned(),
        profile.shown().to_owned(),
        "--managed".to_owned(),
        super::super::splash::slot_argument(slot),
    ];
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
        .args(arguments(profile, slot, cli.language.as_deref(), cli.title.as_deref()))
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
        commands: Some(commands),
        endpoint,
        cancelled: cancelled.clone(),
        arrived: arrived.clone(),
        shots: Shots::default(),
    };
    let shots = viewer.shots.clone();
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
                        let _ = events.send(Event {
                            slot,
                            launch,
                            kind: Kind::Unreachable,
                        });
                    }
                    return;
                }
            };
            arrived.store(true, Ordering::Release);
            read_reports(from_viewer, slot, launch, &token, &managed::build_id(), &events, &shots);
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
fn read_reports(mut from_viewer: impl Read, slot: usize, launch: u32, token: &str, build: &str, events: &Sender<Event>, shots: &Shots) {
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
            Ok(ToHost::Chat(batch)) => Kind::Chat(batch),
            Ok(ToHost::Attention(list)) => Kind::Attention(list),
            Ok(ToHost::SendResult { id, outcome }) => Kind::SendResult(id, outcome),
            Ok(ToHost::Bar(request)) => Kind::Bar(request),
            Ok(ToHost::Frame(header)) => {
                // its size is checked before a byte of it is read
                match ipc::read_picture(&mut from_viewer, &header) {
                    Ok(rgb) => {
                        let size = [usize::from(header.width), usize::from(header.height)];
                        let image = egui::ColorImage::from_rgb(size, &rgb);
                        shots.lock().put(Shot { header, image });
                        continue;
                    }
                    Err(e) => {
                        log::warn!(
                            "session host: character {} sent a monitor picture that is not one ({e}); its reports are no longer read",
                            slot + 1
                        );
                        return;
                    }
                }
            }
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
        let urgent = matches!(kind, Kind::Bar(_));
        if !tell(kind) {
            return;
        }
        if urgent {
            wake();
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
        told_and_shown(wire).0
    }

    /// What the reader thread tells of `wire`, and the monitor's picture
    /// it leaves.
    fn told_and_shown(wire: &[u8]) -> (Vec<Kind>, Option<Shot>, u64) {
        let (tx, rx) = crossbeam_channel::unbounded();
        let shots = Shots::default();
        read_reports(wire, 1, 7, &token(), "build", &tx, &shots);
        drop(tx);
        let told = told_from(&rx);
        let mut shots = shots.lock();
        (told, shots.take(), shots.replaced())
    }

    fn told_from(rx: &Receiver<Event>) -> Vec<Kind> {
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
    fn a_monitor_s_pictures_are_read_between_the_reports_and_only_the_latest_is_left() {
        let picture = |seq, width: u32, height: u32, fill: u8| {
            let header = FrameHeader::new(1, seq, width, height).expect("a header");
            (header, vec![fill; (width * height * 3) as usize])
        };
        let mut wire = Vec::new();
        write_frame(&mut wire, &hello(token())).expect("written");
        write_frame(&mut wire, &ToHost::Heartbeat { loop_count: 1 }).expect("written");
        for (header, rgb) in [picture(1, 320, 180, 10), picture(2, 320, 180, 20), picture(3, 240, 180, 30)] {
            ipc::write_picture(&mut wire, header, &rgb).expect("written");
        }
        write_frame(&mut wire, &ToHost::Heartbeat { loop_count: 2 }).expect("written");
        let (told, shot, replaced) = told_and_shown(&wire);
        // the reports around the pictures read as before: in step
        assert!(matches!(told.as_slice(), [Kind::Connected, Kind::Heartbeat(1), Kind::Heartbeat(2)]));
        // nobody looked in between: the two older pictures were replaced
        let shot = shot.expect("the latest picture");
        assert_eq!((shot.header.seq, shot.image.size), (3, [240, 180]));
        assert!(shot.image.pixels.iter().all(|c| *c == egui::Color32::from_rgb(30, 30, 30)));
        assert_eq!(replaced, 2);
    }

    #[test]
    fn a_picture_that_is_not_one_ends_the_reading_of_that_viewer() {
        let header = FrameHeader::new(1, 1, 320, 180).expect("a header");
        // a header that asks for far more than a monitor's picture: nothing
        // of what follows is read, and nothing after it is taken
        for lying in [
            FrameHeader { bytes: u32::MAX, ..header },
            FrameHeader {
                width: 4000,
                height: 4000,
                bytes: 48_000_000,
                ..header
            },
            FrameHeader { bytes: 0, ..header },
        ] {
            let mut wire = Vec::new();
            write_frame(&mut wire, &hello(token())).expect("written");
            write_frame(&mut wire, &ToHost::Frame(lying)).expect("written");
            write_frame(&mut wire, &ToHost::Heartbeat { loop_count: 9 }).expect("written");
            let (told, shot, _) = told_and_shown(&wire);
            assert!(matches!(told.as_slice(), [Kind::Connected]), "{lying:?}");
            assert!(shot.is_none());
        }
        // a picture cut short by the viewer's end
        let mut wire = Vec::new();
        write_frame(&mut wire, &hello(token())).expect("written");
        ipc::write_picture(&mut wire, header, &vec![0; 320 * 180 * 3]).expect("written");
        wire.truncate(wire.len() - 1);
        let (told, shot, _) = told_and_shown(&wire);
        assert!(matches!(told.as_slice(), [Kind::Connected]));
        assert!(shot.is_none());
        // no picture before the hello
        let mut wire = Vec::new();
        ipc::write_picture(&mut wire, header, &vec![0; 320 * 180 * 3]).expect("written");
        let (told, shot, _) = told_and_shown(&wire);
        assert!(matches!(told.as_slice(), [Kind::Refused(Refusal::Protocol)]));
        assert!(shot.is_none());
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

    #[cfg(unix)]
    #[test]
    fn letting_a_viewer_go_closes_its_command_pipe() {
        let boot = Bootstrap {
            protocol: ipc::PROTOCOL,
            slot: 1,
            host_pid: 9,
            endpoint: "e".into(),
            token: token(),
        };
        let (tx, rx) = crossbeam_channel::bounded(8);
        let writer = std::thread::spawn(move || {
            let mut wire = Vec::new();
            write_commands(&mut wire, &boot, &rx);
            wire
        });
        let mut viewer = Viewer {
            launch: 1,
            child: Command::new("true").spawn().expect("a process"),
            commands: Some(tx),
            endpoint: String::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
            arrived: Arc::new(AtomicBool::new(true)),
            shots: Shots::default(),
        };
        assert!(viewer.send(ToViewer::Minimise));
        viewer.let_go();
        assert!(!viewer.send(ToViewer::Quit), "nothing more is sent");
        // the writer thread ends: the viewer's input ends after what was
        // queued, which it takes as "nobody manages me"
        let wire = writer.join().expect("writer ended");
        let mut read = wire.as_slice();
        ipc::read_frame::<Bootstrap>(&mut read).expect("bootstrap");
        assert_eq!(ipc::read_frame::<ToViewer>(&mut read).expect("command"), ToViewer::Minimise);
        assert!(matches!(ipc::read_frame::<ToViewer>(&mut read), Err(ipc::FrameError::Closed)));
        // the process is still watched
        while viewer.exited().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn a_character_s_viewer_gets_its_profile_and_never_a_login() {
        let name = ProfileName::parse("Alt1").expect("name");
        assert_eq!(
            arguments(&name, 0, None, None),
            ["--profile", "Alt1", "--managed", "--folderstorm-slot=0"]
        );
        let args = arguments(&name, 2, Some("fr"), Some("Test host"));
        assert_eq!(
            args,
            [
                "--profile",
                "Alt1",
                "--managed",
                "--folderstorm-slot=2",
                "--language",
                "fr",
                "--title",
                "Test host"
            ]
        );
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
                read_reports(from_viewer, 0, 1, &token, &managed::build_id(), &events_tx, &Shots::default());
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
                Kind::Chat(_) | Kind::Attention(_) | Kind::SendResult(..) | Kind::Bar(_) => {}
                Kind::Unreachable => desk.unreachable(slot, launch),
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
                read_reports(from_viewer, 1, 1, &expected, &managed::build_id(), &events_tx, &Shots::default());
            });
            let mut stdin = Vec::new();
            ipc::write_frame(&mut stdin, &boot).expect("written");
            managed::play_viewer(&stdin, Status::default());
        });
        drop(events_tx);
        let kinds: Vec<Kind> = events.iter().map(|e| e.kind).collect();
        assert!(matches!(kinds.as_slice(), [Kind::Refused(Refusal::Token)]), "{kinds:?}");
    }

    /// A monitor whole, over the real endpoint: the host asks a background
    /// viewer for pictures, the viewer's link sends what its main thread
    /// leaves, the largest picture arrives intact between two reports, and
    /// pictures the host had no time to look at are replaced.
    #[cfg(unix)]
    #[test]
    fn a_monitor_s_pictures_cross_the_real_endpoint_latest_only() {
        use ::folderstorm_session::ipc::Role;
        use ::folderstorm_session::monitor::{Order, Size};
        use std::os::unix::net::UnixStream;
        use std::time::{Duration, Instant};

        const SLOT: usize = 3;
        const LAUNCH: u32 = 1;
        let listener = transport::Listener::bind(SLOT as u8).expect("bound");
        let boot = Bootstrap {
            protocol: ipc::PROTOCOL,
            slot: SLOT as u8,
            host_pid: std::process::id(),
            endpoint: listener.endpoint().to_owned(),
            token: transport::random_hex(ipc::TOKEN_LEN),
        };
        let (events_tx, events) = crossbeam_channel::unbounded();
        let token = boot.token.clone();
        let shots = Shots::default();
        let reader_shots = shots.clone();
        let reader = std::thread::spawn(move || {
            let cancelled = AtomicBool::new(false);
            let from_viewer = listener.accept(std::process::id(), &cancelled).expect("accepted");
            read_reports(from_viewer, SLOT, LAUNCH, &token, &managed::build_id(), &events_tx, &reader_shots);
        });
        let (mut to_viewer, viewer_input) = UnixStream::pair().expect("pipe");
        let (link, viewer) = managed::start_viewer(viewer_input);
        ipc::write_frame(&mut to_viewer, &boot).expect("bootstrap");
        let deadline = Instant::now() + Duration::from_secs(20);
        let viewer_event = || loop {
            if let Some(event) = link.poll().next() {
                return event;
            }
            assert!(Instant::now() < deadline, "the viewer heard nothing");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(viewer_event(), managed::Event::Linked { slot: SLOT as u8 });

        // the host's order reaches the viewer as its role
        let order = Order {
            size: Size::Large,
            interval_ms: 1000,
        };
        let assignment = ::folderstorm_session::ipc::Assignment {
            seq: 1,
            role: Role::Monitor(order),
            behind: ::folderstorm_session::behind::Behind3d::Off,
            standby: ::folderstorm_session::ipc::Standby::Warm,
            sound_muted: true,
            voice_allowed: false,
            off_taskbar: false,
        };
        ipc::write_frame(&mut to_viewer, &ToViewer::Assign(assignment)).expect("written");
        let managed::Event::Command(ToViewer::Assign(taken)) = viewer_event() else {
            panic!("not an assignment");
        };
        assert_eq!(taken.role.monitor(), Some(order));

        // the largest picture, a pattern that would show any byte out of place
        let rgba: Vec<u8> = (0..640 * 360)
            .flat_map(|i: u32| [i as u8, (i >> 8) as u8, (i >> 16) as u8, 255])
            .collect();
        let picture = |seq, rgba: Vec<u8>| managed::Picture {
            generation: 1,
            seq,
            width: 640,
            height: 360,
            rgba,
        };
        link.picture(picture(1, rgba.clone()));
        let wait_for = |seq: u64| loop {
            if let Some(shot) = shots.lock().take().filter(|shot| shot.header.seq == seq) {
                return shot;
            }
            assert!(Instant::now() < deadline, "picture {seq} never came");
            std::thread::sleep(Duration::from_millis(5));
        };
        let shot = wait_for(1);
        assert_eq!(shot.image.size, [640, 360]);
        let expected: Vec<egui::Color32> = (0..640 * 360u32)
            .map(|i| egui::Color32::from_rgb(i as u8, (i >> 8) as u8, (i >> 16) as u8))
            .collect();
        assert!(shot.image.pixels == expected, "the picture arrived as it was taken");

        // a burst nobody looks at: the last one is what is left, and the
        // reports that follow are still read
        for seq in 2..=6 {
            link.picture(picture(seq, vec![seq as u8; 640 * 360 * 4]));
            std::thread::sleep(Duration::from_millis(30));
        }
        let last = wait_for(6);
        assert!(last.image.pixels.iter().all(|c| *c == egui::Color32::from_rgb(6, 6, 6)));
        assert!(shots.lock().take().is_none(), "nothing older waits behind it");
        assert!(link.report(Status {
            unread: 7,
            ..Default::default()
        }));
        loop {
            let event = events.recv_timeout(Duration::from_secs(20)).expect("a report");
            if matches!(&event.kind, Kind::Status(status) if status.unread == 7) {
                break;
            }
        }
        // the host goes: the viewer is on its own, the reader ends
        drop(to_viewer);
        assert_eq!(viewer_event(), managed::Event::Gone);
        viewer.join().expect("the viewer's link ended");
        reader.join().expect("the reader ended");
    }

    /// The shared chat whole, over the real endpoint and a real pipe for
    /// the viewer's input: a line heard by the character reaches the
    /// host's chat, a line typed in the host is said once by that viewer
    /// and answered, and a line the viewer never answers because the link
    /// broke is uncertain and never handed again.
    #[cfg(unix)]
    #[test]
    fn a_conversation_goes_both_ways_and_a_broken_link_leaves_an_uncertain_line() {
        use ::folderstorm_session::chat::{self, Chat, Entry, Note};
        use ::folderstorm_session::export::Sends;
        use ::folderstorm_session::ipc::{ChatBatch, ChatLine, Conversation, LineKind, Screen, SendOutcome, SessionKind};
        use std::os::unix::net::UnixStream;
        use std::time::{Duration, Instant};

        const SLOT: usize = 2;
        const LAUNCH: u32 = 1;
        let listener = transport::Listener::bind(SLOT as u8).expect("bound");
        let boot = Bootstrap {
            protocol: ipc::PROTOCOL,
            slot: SLOT as u8,
            host_pid: std::process::id(),
            endpoint: listener.endpoint().to_owned(),
            token: transport::random_hex(ipc::TOKEN_LEN),
        };
        let (events_tx, events) = crossbeam_channel::unbounded();
        let token = boot.token.clone();
        let reader = std::thread::spawn(move || {
            let cancelled = AtomicBool::new(false);
            let from_viewer = listener.accept(std::process::id(), &cancelled).expect("accepted");
            read_reports(
                from_viewer,
                SLOT,
                LAUNCH,
                &token,
                &managed::build_id(),
                &events_tx,
                &Shots::default(),
            );
        });
        // the viewer's standard input: a pipe the host writes
        let (mut to_viewer, viewer_input) = UnixStream::pair().expect("pipe");
        let (link, viewer) = managed::start_viewer(viewer_input);
        ipc::write_frame(&mut to_viewer, &boot).expect("bootstrap");

        let deadline = Instant::now() + Duration::from_secs(20);
        // the viewer's main thread, as the test plays it
        let viewer_event = || loop {
            if let Some(event) = link.poll().next() {
                return event;
            }
            assert!(Instant::now() < deadline, "the viewer heard nothing");
            std::thread::sleep(Duration::from_millis(5));
        };
        // the host's window thread: reports into the chat, until `done`
        let mut host_chat = Chat::default();
        let pump = |chat: &mut Chat, done: &dyn Fn(&Chat) -> bool| {
            while !done(chat) {
                let Event { slot, launch, kind } = events.recv_timeout(Duration::from_secs(20)).expect("a report");
                match kind {
                    Kind::Status(status) => chat.status(slot, launch, status.generation, status.screen == Screen::World),
                    Kind::Chat(batch) => chat.batch(slot, launch, *batch),
                    Kind::Attention(list) => chat.attention_list(slot, launch, *list),
                    Kind::SendResult(id, outcome) => chat.result(slot, launch, id, outcome),
                    Kind::Connected | Kind::Heartbeat(_) | Kind::Bar(_) => {}
                    Kind::Refused(refusal) => panic!("refused: {refusal:?}"),
                    Kind::Unreachable => panic!("the viewer never reached its endpoint"),
                }
            }
        };
        let said = |number, kind, from: &str, text: &str| ChatBatch {
            generation: 1,
            conversation: Conversation::Nearby,
            kind: SessionKind::Nearby,
            restart: number == 1,
            lines: vec![ChatLine {
                number,
                time: 0,
                kind,
                emote: false,
                from: from.into(),
                text: text.into(),
            }],
            ..Default::default()
        };
        let texts = |chat: &Chat| -> Vec<String> {
            chat.thread(SLOT, Conversation::Nearby)
                .map(|t| {
                    t.entries()
                        .map(|e| match e {
                            Entry::Line(l) => format!("{}: {}", l.from, l.text),
                            Entry::Note(Note::Uncertain { text }) => format!("uncertain: {text}"),
                            other => format!("{other:?}"),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        // 1. the character hears a neighbour: the host's chat shows it
        assert_eq!(viewer_event(), managed::Event::Linked { slot: SLOT as u8 });
        assert!(link.report(Status {
            generation: 1,
            screen: Screen::World,
            ..Default::default()
        }));
        assert!(link.send(ToHost::Chat(Box::new(said(1, LineKind::Said, "Jane Doe", "hello")))));
        pump(&mut host_chat, &|chat| chat.thread(SLOT, Conversation::Nearby).is_some());
        assert_eq!(texts(&host_chat), ["Jane Doe: hello"]);

        // 2. a line typed in the host: handed to that viewer's input, once
        let now = Instant::now();
        assert!(host_chat.set_draft(SLOT, Conversation::Nearby, "hi Jane"));
        let first = host_chat.submit(SLOT, Conversation::Nearby, now).expect("sent");
        let mut handed = Vec::new();
        let mut hand_over = |chat: &mut Chat, to_viewer: &mut UnixStream| {
            for effect in chat.take_effects() {
                if let chat::Effect::Send { slot, launch, message } = effect {
                    assert_eq!((slot, launch), (SLOT, LAUNCH));
                    ipc::write_frame(to_viewer, &message).expect("written");
                    handed.push(message);
                }
            }
        };
        hand_over(&mut host_chat, &mut to_viewer);
        let mut sends = Sends::default();
        let managed::Event::Command(ToViewer::SendChat {
            id,
            generation,
            conversation,
            text,
        }) = viewer_event()
        else {
            panic!("not a line to say");
        };
        assert_eq!(
            (id, generation, conversation, text.as_str()),
            (first, 1, Conversation::Nearby, "hi Jane")
        );
        assert!(sends.fresh(id));
        // the viewer says it, answers, and its own line comes back as any line
        assert!(link.send(ToHost::SendResult {
            id,
            outcome: SendOutcome::Accepted
        }));
        assert!(link.send(ToHost::Chat(Box::new(said(2, LineKind::Own, "Sam", "hi Jane")))));
        pump(&mut host_chat, &|chat| {
            chat.thread(SLOT, Conversation::Nearby).is_some_and(|t| t.entries().len() == 2)
        });
        assert_eq!(texts(&host_chat), ["Jane Doe: hello", "Sam: hi Jane"]);
        assert_eq!(host_chat.sending(SLOT, Conversation::Nearby), 0);

        // 3. a second line is handed over, and the link breaks before the
        // viewer answers (its process died, or the host's pipe did)
        assert!(host_chat.set_draft(SLOT, Conversation::Nearby, "are you there?"));
        let second = host_chat.submit(SLOT, Conversation::Nearby, now).expect("sent");
        hand_over(&mut host_chat, &mut to_viewer);
        assert!(matches!(
            viewer_event(),
            managed::Event::Command(ToViewer::SendChat { id, .. }) if id == second && sends.fresh(id)
        ));
        drop(to_viewer);
        assert_eq!(viewer_event(), managed::Event::Gone);
        viewer.join().expect("the viewer's link thread ends");
        // (the host learns it from the process watch)
        host_chat.ended(SLOT, LAUNCH);
        host_chat.tick(now + chat::SEND_WAIT * 2);
        assert_eq!(
            texts(&host_chat),
            ["Jane Doe: hello", "Sam: hi Jane", "uncertain: are you there?"],
            "told as uncertain"
        );
        assert_eq!(host_chat.draft(SLOT, Conversation::Nearby), "", "not offered for a second send");
        assert!(host_chat.take_effects().is_empty(), "never handed again");
        // each line went down the pipe exactly once, and a frame repeated
        // by anything would not be said twice
        assert_eq!(handed.len(), 2);
        assert!(!sends.fresh(first) && !sends.fresh(second));
        reader.join().expect("the host's reader ends with the viewer");
    }
}
