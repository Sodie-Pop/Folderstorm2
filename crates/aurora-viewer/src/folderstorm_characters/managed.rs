//! `--managed`: this viewer was started by the session host
//! (`aurora-viewer --session-host`, session-host plan section 8). Its link
//! to the host: two threads that do the blocking reads and writes, and the
//! queues the main thread polls without ever waiting.
//!
//! - The **in** thread reads the viewer's standard input, a pipe that
//!   only the host and this process hold (anonymous on Linux and macOS, a
//!   randomly named pipe connected at creation on Windows): first the
//!   bootstrap (where to report, and a one-time token), then the host's
//!   commands. The end of that pipe is the end of the host: the viewer goes
//!   on by itself, logged in and visible, as one started by hand.
//! - The **out** thread writes this viewer's reports to the host's private
//!   endpoint (`transport.rs`), and a heartbeat every second with the
//!   number of turns the main loop has made: the host can tell a slow
//!   viewer from one that does not answer, and never takes either for a
//!   dead one (rule R6).
//!
//! Nothing read here is trusted before it is checked (`ipc`), and nothing
//! of it is a credential: the host never logs a character in.

use super::transport;
use ::folderstorm_session::ipc::{self, Bootstrap, Hello, Status, ToHost, ToViewer};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError};
use std::io::Read;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

/// What the main thread learns from the link.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// The host's bootstrap was accepted and the first report sent.
    Linked {
        slot: u8,
    },
    Command(ToViewer),
    /// The host is gone, or was never there: unmanaged from now on.
    Gone,
}

/// The main thread's side of the link.
pub struct Link {
    events: Receiver<Event>,
    reports: Sender<ToHost>,
    shared: &'static Shared,
}

/// What the threads share.
#[derive(Default)]
struct Shared {
    /// Turns of the main loop.
    turns: AtomicU64,
    /// The link is over: the out thread stops.
    gone: AtomicBool,
}

static LINK: OnceLock<Link> = OnceLock::new();

/// The viewer's version and build: a host and a viewer of different builds
/// refuse each other.
pub fn build_id() -> String {
    format!(
        "{} {} {}",
        aurora_version::version(),
        aurora_version::commit(),
        env!("AURORA_BUILD_PROFILE")
    )
}

/// Starts the link when the host started this viewer (`--managed` with a
/// named profile). Once, before the window: the host sees early that its
/// viewer lives. Without `--managed` nothing is read from standard input.
pub fn start() {
    if !crate::cli::get().managed || super::profile::current().is_none() {
        return;
    }
    LINK.get_or_init(|| {
        let (link, to_main, from_main) = Link::new();
        let shared = link.shared;
        let spawned = std::thread::Builder::new()
            .name("folderstorm-session-in".into())
            .spawn(move || run(std::io::stdin().lock(), &to_main, from_main, shared, transport::connect));
        if let Err(e) = spawned {
            // (no log yet: the console)
            eprintln!("session host: no link thread ({e}), starting unmanaged");
            shared.gone.store(true, Ordering::Release);
        }
        link
    });
}

/// The link, in a viewer the host started.
pub fn link() -> Option<&'static Link> {
    LINK.get()
}

impl Link {
    fn new() -> (Self, Sender<Event>, Receiver<ToHost>) {
        // bounded both ways: a main loop that stops reading holds the in
        // thread, then the host's writer, never the memory
        let (to_main, events) = crossbeam_channel::bounded(64);
        let (reports, from_main) = crossbeam_channel::bounded(4);
        let shared: &'static Shared = Box::leak(Box::default());
        (Self { events, reports, shared }, to_main, from_main)
    }

    /// The main loop made a turn.
    pub fn turn(&self) {
        self.shared.turns.fetch_add(1, Ordering::Relaxed);
    }

    /// What came since the last call; never waits.
    pub fn poll(&self) -> impl Iterator<Item = Event> + '_ {
        self.events.try_iter()
    }

    /// Queues a report for the host; false when the queue is full (the
    /// host reads slowly: the next report replaces this one) or the link
    /// is over.
    pub fn report(&self, status: Status) -> bool {
        self.send(ToHost::Status(Box::new(status)))
    }

    /// Queues anything for the host (chat lines, the answer to a line it
    /// asked for); false when the queue is full or the link is over: the
    /// caller keeps it and tries again, nothing is dropped here.
    pub fn send(&self, message: ToHost) -> bool {
        match self.reports.try_send(message) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        }
    }
}

/// The in thread: the bootstrap, the endpoint, then the host's commands
/// until its pipe ends. `connect` opens the endpoint (the real transport,
/// or a test's).
fn run<W: std::io::Write + Send + 'static>(
    mut input: impl Read,
    to_main: &Sender<Event>,
    from_main: Receiver<ToHost>,
    shared: &'static Shared,
    connect: impl FnOnce(&str, u32) -> std::io::Result<W>,
) {
    let outcome = bootstrap(&mut input, connect);
    let (boot, out) = match outcome {
        Ok(linked) => linked,
        Err(why) => {
            log::warn!("session host: not managed ({why})");
            shared.gone.store(true, Ordering::Release);
            let _ = to_main.send(Event::Gone);
            return;
        }
    };
    let writer = std::thread::Builder::new()
        .name("folderstorm-session-out".into())
        .spawn(move || write_reports(out, &from_main, shared));
    if let Err(e) = writer {
        log::warn!("session host: no report thread ({e}); commands are still taken");
    }
    if to_main.send(Event::Linked { slot: boot.slot }).is_err() {
        return;
    }
    loop {
        match ipc::read_frame::<ToViewer>(&mut input) {
            Ok(command) => {
                // (its text bounded and cleaned before anything acts on it)
                if to_main.send(Event::Command(command.cleaned())).is_err() {
                    break;
                }
            }
            Err(why) => {
                // closed: the host quit or crashed; anything else: its
                // pipe is out of step. Either way nobody manages us now.
                log::info!("session host: link ended ({why}), the viewer goes on by itself");
                break;
            }
        }
    }
    shared.gone.store(true, Ordering::Release);
    let _ = to_main.send(Event::Gone);
}

/// Reads and checks the bootstrap, opens the endpoint and says hello.
fn bootstrap<W: std::io::Write>(
    input: &mut impl Read,
    connect: impl FnOnce(&str, u32) -> std::io::Result<W>,
) -> Result<(Bootstrap, W), String> {
    let boot: Bootstrap = ipc::read_frame(input).map_err(|e| format!("no bootstrap: {e}"))?;
    if !boot.valid() {
        return Err("the bootstrap is not valid".into());
    }
    let mut out = connect(&boot.endpoint, boot.host_pid).map_err(|e| format!("the host's endpoint did not open: {e}"))?;
    let hello = ToHost::Hello(Hello {
        protocol: ipc::PROTOCOL,
        build: build_id(),
        token: boot.token.clone(),
        slot: boot.slot,
        pid: std::process::id(),
    });
    ipc::write_frame(&mut out, &hello).map_err(|e| format!("the first report was not sent: {e}"))?;
    Ok((boot, out))
}

/// The out thread: reports as they come, and the heartbeat.
fn write_reports(mut out: impl std::io::Write, from_main: &Receiver<ToHost>, shared: &Shared) {
    let mut beat = Instant::now();
    loop {
        let wait = ipc::HEARTBEAT.saturating_sub(beat.elapsed());
        let sent = match from_main.recv_timeout(wait) {
            Ok(report) => ipc::write_frame(&mut out, &report),
            Err(RecvTimeoutError::Timeout) => Ok(()),
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if shared.gone.load(Ordering::Acquire) {
            break;
        }
        let sent = sent.and_then(|()| {
            if beat.elapsed() < ipc::HEARTBEAT {
                return Ok(());
            }
            beat = Instant::now();
            let loop_count = shared.turns.load(Ordering::Relaxed);
            ipc::write_frame(&mut out, &ToHost::Heartbeat { loop_count })
        });
        if let Err(e) = sent {
            log::info!("session host: reports stopped ({e})");
            break;
        }
    }
}

/// A whole viewer side on the real transport, for the host's tests: one
/// report queued, then the link's in thread on `input` to its end. Gives
/// what the main thread would have polled.
#[cfg(all(test, unix))]
pub(super) fn play_viewer(input: &[u8], report: Status) -> Vec<Event> {
    let (link, to_main, from_main) = Link::new();
    assert!(link.report(report));
    run(input, &to_main, from_main, link.shared, transport::connect);
    link.poll().collect()
}

/// A viewer's link on the real transport that stays up for as long as
/// `input` does, for the host's tests of a conversation both ways: the
/// test plays the viewer's main thread on the link it is given.
#[cfg(all(test, unix))]
pub(super) fn start_viewer(input: impl Read + Send + 'static) -> (&'static Link, std::thread::JoinHandle<()>) {
    let (link, to_main, from_main) = Link::new();
    let link: &'static Link = Box::leak(Box::new(link));
    let shared = link.shared;
    let thread = std::thread::spawn(move || run(input, &to_main, from_main, shared, transport::connect));
    (link, thread)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::folderstorm_session::behind::Behind3d;
    use ::folderstorm_session::ipc::{Assignment, Role, Standby, write_frame};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// What the "host" receives, shared with the test.
    #[derive(Clone, Default)]
    struct Wire(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Wire {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("wire").extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn boot() -> Bootstrap {
        Bootstrap {
            protocol: ipc::PROTOCOL,
            slot: 2,
            host_pid: 4242,
            endpoint: "the-endpoint".into(),
            token: "ab".repeat(ipc::TOKEN_LEN / 2),
        }
    }

    fn assignment() -> Assignment {
        Assignment {
            seq: 1,
            role: Role::Background,
            behind: Behind3d::Off,
            standby: Standby::Warm,
            sound_muted: true,
            voice_allowed: false,
        }
    }

    /// Runs the in thread's function on `input` to its end.
    fn played(input: &[u8]) -> (Vec<Event>, Wire, Option<(String, u32)>) {
        let (link, to_main, from_main) = Link::new();
        let wire = Wire::default();
        let mut asked = None;
        run(input, &to_main, from_main, link.shared, |endpoint: &str, pid| {
            asked = Some((endpoint.to_owned(), pid));
            Ok(wire.clone())
        });
        (link.poll().collect(), wire, asked)
    }

    #[test]
    fn the_bootstrap_links_then_commands_come_until_the_host_s_pipe_ends() {
        let mut input = Vec::new();
        write_frame(&mut input, &boot()).expect("written");
        write_frame(&mut input, &ToViewer::Assign(assignment())).expect("written");
        write_frame(&mut input, &ToViewer::Minimise).expect("written");
        let (events, wire, asked) = played(&input);
        assert_eq!(
            events,
            vec![
                Event::Linked { slot: 2 },
                Event::Command(ToViewer::Assign(assignment())),
                Event::Command(ToViewer::Minimise),
                Event::Gone,
            ]
        );
        assert_eq!(asked, Some(("the-endpoint".to_owned(), 4242)));
        // the first thing the host reads is the hello, with its token
        let bytes = wire.0.lock().expect("wire").clone();
        let hello: ToHost = ipc::read_frame(&mut bytes.as_slice()).expect("hello");
        let ToHost::Hello(hello) = hello else {
            panic!("not a hello: {hello:?}");
        };
        assert_eq!(hello.check(2, &boot().token, &build_id()), Ok(()));
        assert_eq!(hello.pid, std::process::id());
    }

    #[test]
    fn without_a_valid_bootstrap_nothing_is_opened_and_the_viewer_is_unmanaged() {
        let mut wrong_protocol = Vec::new();
        write_frame(
            &mut wrong_protocol,
            &Bootstrap {
                protocol: ipc::PROTOCOL + 1,
                ..boot()
            },
        )
        .expect("written");
        let mut not_a_bootstrap = Vec::new();
        write_frame(&mut not_a_bootstrap, &ToViewer::Quit).expect("written");
        for input in [&b""[..], b"typed at a terminal\n", &wrong_protocol, &not_a_bootstrap] {
            let (events, wire, asked) = played(input);
            assert_eq!(events, vec![Event::Gone]);
            assert_eq!(asked, None, "no endpoint is opened");
            assert!(wire.0.lock().expect("wire").is_empty());
        }
    }

    #[test]
    fn an_endpoint_that_does_not_open_leaves_the_viewer_unmanaged() {
        let mut input = Vec::new();
        write_frame(&mut input, &boot()).expect("written");
        write_frame(&mut input, &ToViewer::Quit).expect("written");
        let (link, to_main, from_main) = Link::new();
        run(input.as_slice(), &to_main, from_main, link.shared, |_: &str, _| {
            Err::<Wire, _>(std::io::ErrorKind::PermissionDenied.into())
        });
        // the Quit that followed is never acted on
        assert_eq!(link.poll().collect::<Vec<_>>(), vec![Event::Gone]);
    }

    #[test]
    fn a_command_that_is_not_one_ends_the_link_and_nothing_after_it_is_taken() {
        let mut input = Vec::new();
        write_frame(&mut input, &boot()).expect("written");
        write_frame(&mut input, &serde_json::json!({"Run": "something"})).expect("written");
        write_frame(&mut input, &ToViewer::Quit).expect("written");
        let (events, ..) = played(&input);
        assert_eq!(events, vec![Event::Linked { slot: 2 }, Event::Gone]);
    }

    #[test]
    fn reports_and_heartbeats_reach_the_host_and_stop_with_the_link() {
        let (link, _to_main, from_main) = Link::new();
        let wire = Wire::default();
        let shared = link.shared;
        let out = wire.clone();
        let writer = std::thread::spawn(move || write_reports(out, &from_main, shared));
        link.turn();
        link.turn();
        assert!(link.report(Status {
            unread: 4,
            ..Default::default()
        }));
        // a status at once, a heartbeat within its period
        let deadline = Instant::now() + ipc::HEARTBEAT * 5;
        let mut seen = Vec::new();
        while seen.len() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            let bytes = wire.0.lock().expect("wire").clone();
            let mut rest = bytes.as_slice();
            seen.clear();
            while let Ok(report) = ipc::read_frame::<ToHost>(&mut rest) {
                seen.push(report);
            }
        }
        assert!(matches!(&seen[0], ToHost::Status(s) if s.unread == 4), "{seen:?}");
        assert_eq!(seen[1], ToHost::Heartbeat { loop_count: 2 });
        shared.gone.store(true, Ordering::Release);
        writer.join().expect("the out thread ends with the link");
        // a full queue drops the report, it never waits
        let (link, _to_main, _from_main) = Link::new();
        let queued = (0..16).filter(|_| link.report(Status::default())).count();
        assert_eq!(queued, 4);
    }
}
