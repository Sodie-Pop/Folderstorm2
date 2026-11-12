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
//! - A **monitor's picture** (checkpoint 5) waits in a place for one
//!   (`monitor::Latest`): the main thread puts the pixels the renderer
//!   read back and goes on; the out thread packs them and writes them
//!   after the reports. A picture not written by the time the next one is
//!   taken is replaced, never queued: the host only ever gets the latest.
//!
//! Nothing read here is trusted before it is checked (`ipc`), and nothing
//! of it is a credential: the host never logs a character in.

use super::transport;
use ::folderstorm_session::ipc::{self, Bootstrap, Hello, Status, ToHost, ToViewer};
use ::folderstorm_session::monitor::{self, FrameHeader, Latest};
use crossbeam_channel::{Receiver, Sender, TrySendError};
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

/// A monitor's picture as the renderer read it back: `width` × `height`
/// pixels of four bytes, rows from the top.
#[derive(Debug)]
pub struct Picture {
    /// The login it was taken in, and its number.
    pub generation: u32,
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The main thread's side of the link.
pub struct Link {
    events: Receiver<Event>,
    reports: Sender<ToHost>,
    /// Wakes the out thread for a picture.
    wake: Sender<()>,
    shared: &'static Shared,
}

/// The out thread's side: the reports queued, and the wake for a picture.
struct Outbox {
    reports: Receiver<ToHost>,
    wake: Receiver<()>,
}

/// What the threads share.
#[derive(Default)]
struct Shared {
    /// Turns of the main loop.
    turns: AtomicU64,
    /// The link is over: the out thread stops.
    gone: AtomicBool,
    /// The monitor's latest picture, not written yet.
    picture: parking_lot::Mutex<Latest<Picture>>,
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
    fn new() -> (Self, Sender<Event>, Outbox) {
        // bounded both ways: a main loop that stops reading holds the in
        // thread, then the host's writer, never the memory
        let (to_main, events) = crossbeam_channel::bounded(64);
        let (reports, from_main) = crossbeam_channel::bounded(4);
        // (one wake is enough for any number of pictures: the place holds one)
        let (wake, woken) = crossbeam_channel::bounded(1);
        let shared: &'static Shared = Box::leak(Box::default());
        let link = Self {
            events,
            reports,
            wake,
            shared,
        };
        let outbox = Outbox {
            reports: from_main,
            wake: woken,
        };
        (link, to_main, outbox)
    }

    /// Leaves a monitor's picture for the out thread and returns at once;
    /// true when it took the place of one that was not written yet (the
    /// host reads slowly: only the latest is sent). Nothing is kept once
    /// the link is over.
    pub fn picture(&self, picture: Picture) -> bool {
        if self.shared.gone.load(Ordering::Acquire) {
            return false;
        }
        let replaced = self.shared.picture.lock().put(picture);
        let _ = self.wake.try_send(());
        replaced
    }

    /// The monitor closed: a picture not written yet is not sent.
    pub fn drop_picture(&self) {
        self.shared.picture.lock().clear();
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
    from_main: Outbox,
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

/// The out thread: reports as they come, the monitor's latest picture,
/// and the heartbeat.
fn write_reports(mut out: impl std::io::Write, from_main: &Outbox, shared: &Shared) {
    let mut beat = Instant::now();
    // (a wake that can come no more is not waited for: the reports queued
    // are still written)
    let never = crossbeam_channel::never::<()>();
    let mut wake = &from_main.wake;
    loop {
        let wait = ipc::HEARTBEAT.saturating_sub(beat.elapsed());
        let sent = crossbeam_channel::select! {
            recv(from_main.reports) -> report => match report {
                Ok(report) => Some(ipc::write_frame(&mut out, &report)),
                Err(_) => break,
            },
            recv(wake) -> woken => {
                if woken.is_err() {
                    // (looked at again at once: a report may be queued)
                    wake = &never;
                    None
                } else {
                    Some(Ok(()))
                }
            },
            default(wait) => Some(Ok(())),
        };
        let Some(sent) = sent else {
            continue;
        };
        if shared.gone.load(Ordering::Acquire) {
            break;
        }
        let sent = sent.and_then(|()| write_picture(&mut out, shared)).and_then(|()| {
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
    // nothing of a monitor outlives the link
    shared.picture.lock().clear();
}

/// Writes the monitor's picture that waits, if one does: packed to RGB
/// here, off the main thread. One that is not a monitor's picture (a size
/// no monitor shows) is dropped, never written.
fn write_picture(out: &mut impl std::io::Write, shared: &Shared) -> std::io::Result<()> {
    let Some(picture) = shared.picture.lock().take() else {
        return Ok(());
    };
    // (the size first: nothing is packed of a picture no monitor shows)
    let header = FrameHeader::new(picture.generation, picture.seq, picture.width, picture.height);
    let rgb = header.and_then(|_| monitor::rgb_from_rgba(&picture.rgba, picture.width, picture.height));
    match (header, rgb) {
        (Some(header), Some(rgb)) => ipc::write_picture(out, header, &rgb),
        _ => {
            log::warn!(
                "session host: a monitor's picture of {} x {} ({} bytes) is not one, dropped",
                picture.width,
                picture.height,
                picture.rgba.len()
            );
            Ok(())
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
            off_taskbar: false,
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

    /// What the out thread wrote, read back as the host would.
    fn read_back(bytes: &[u8]) -> Vec<(ToHost, Option<Vec<u8>>)> {
        let mut rest = bytes;
        let mut seen = Vec::new();
        while let Ok(report) = ipc::read_frame::<ToHost>(&mut rest) {
            let picture = match &report {
                ToHost::Frame(header) => Some(ipc::read_picture(&mut rest, header).expect("the picture")),
                _ => None,
            };
            seen.push((report, picture));
        }
        seen
    }

    fn picture(seq: u64, width: u32, height: u32, fill: u8) -> Picture {
        Picture {
            generation: 1,
            seq,
            width,
            height,
            rgba: vec![fill; (width * height * 4) as usize],
        }
    }

    #[test]
    fn only_the_latest_picture_is_written_and_the_reports_stay_in_step() {
        let (link, _to_main, from_main) = Link::new();
        let wire = Wire::default();
        let shared = link.shared;
        // three pictures before the out thread runs: the host reads slowly
        assert!(!link.picture(picture(1, 320, 180, 10)));
        assert!(link.picture(picture(2, 320, 180, 20)), "the first was never written: replaced");
        assert!(link.picture(picture(3, 480, 270, 30)));
        assert!(link.report(Status {
            unread: 1,
            ..Default::default()
        }));
        let out = wire.clone();
        let writer = std::thread::spawn(move || write_reports(out, &from_main, shared));
        let deadline = Instant::now() + ipc::HEARTBEAT * 5;
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            seen = read_back(&wire.0.lock().expect("wire").clone());
            if seen.iter().any(|(report, _)| matches!(report, ToHost::Heartbeat { .. })) {
                break;
            }
        }
        let pictures: Vec<_> = seen
            .iter()
            .filter_map(|(report, rgb)| rgb.as_ref().map(|rgb| (report, rgb)))
            .collect();
        assert_eq!(pictures.len(), 1, "one picture, the latest");
        let (ToHost::Frame(header), rgb) = pictures[0] else {
            panic!("not a picture");
        };
        assert_eq!((header.seq, header.width, header.height), (3, 480, 270));
        assert_eq!(rgb.len(), 480 * 270 * 3);
        assert!(rgb.iter().all(|byte| *byte == 30));
        // the status and the heartbeat around it read as before
        assert!(seen.iter().any(|(report, _)| matches!(report, ToHost::Status(s) if s.unread == 1)));
        assert!(seen.iter().any(|(report, _)| matches!(report, ToHost::Heartbeat { .. })));
        // a picture that is no monitor's is dropped, and the next one passes
        assert!(!link.picture(picture(4, 4000, 2000, 0)));
        let before = wire.0.lock().expect("wire").len();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!link.picture(Picture {
            rgba: vec![0; 7],
            ..picture(5, 320, 180, 0)
        }));
        std::thread::sleep(Duration::from_millis(100));
        let after = read_back(&wire.0.lock().expect("wire")[before..]);
        assert!(after.iter().all(|(_, rgb)| rgb.is_none()), "{after:?}");
        assert!(!link.picture(picture(6, 320, 180, 60)));
        let deadline = Instant::now() + ipc::HEARTBEAT * 3;
        let mut last = None;
        while last.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            last = read_back(&wire.0.lock().expect("wire").clone())
                .into_iter()
                .filter_map(|(report, _)| match report {
                    ToHost::Frame(header) => Some(header.seq),
                    _ => None,
                })
                .next_back();
        }
        assert_eq!(last, Some(6));
        // a closed monitor's picture is not sent; nothing is kept once the
        // link is over
        link.picture(picture(7, 320, 180, 0));
        link.drop_picture();
        shared.gone.store(true, Ordering::Release);
        writer.join().expect("the out thread ends with the link");
        assert!(!link.picture(picture(8, 320, 180, 0)));
        assert!(shared.picture.lock().take().is_none());
    }
}
