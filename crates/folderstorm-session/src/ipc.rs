//! What the session host and a viewer it manages say to each other
//! (session-host plan, section 8), and how it is framed. Nothing here opens
//! a pipe: the framing reads and writes any byte stream, so every rule is
//! tested on any system.
//!
//! Two one-way channels per launched viewer, both local to the PC:
//! - **host → viewer**: the viewer's standard input, an anonymous pipe that
//!   only the host and that child hold. Its first frame is the
//!   [`Bootstrap`], then [`ToViewer`] commands. Its end (the host closed or
//!   crashed) is how a viewer learns that nobody manages it any more.
//! - **viewer → host**: a private endpoint the host made for that one
//!   launch (a named pipe of the user's logon on Windows, a socket only the
//!   user can open elsewhere), named in the bootstrap. Its first frame is
//!   [`Hello`] with the bootstrap's one-time token, then [`ToHost`] reports.
//!
//! Neither channel ever carries a password, a login token or a chat line
//! in this checkpoint: each character logs in on its own login screen.
//!
//! A frame is a little-endian `u32` length, then that many bytes of JSON,
//! [`MAX_FRAME`] at most. Text that comes from the other side is cleaned
//! and bounded before anything shows it ([`Status::cleaned`]).

use crate::behind::Behind3d;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};
use std::time::Duration;

/// Both sides refuse another number.
pub const PROTOCOL: u32 = 1;

/// The largest frame, either way.
pub const MAX_FRAME: usize = 64 * 1024;

/// How many characters a host runs (decision 3 of the plan).
pub const MAX_SLOTS: usize = 5;

/// How often a viewer's link thread says it is alive.
pub const HEARTBEAT: Duration = Duration::from_secs(1);

/// The length of a token: 64 hexadecimal digits (two random UUIDs, 244
/// random bits).
pub const TOKEN_LEN: usize = 64;

/// The longest endpoint name a bootstrap may carry.
pub const MAX_ENDPOINT: usize = 256;

/// The first frame on a managed viewer's standard input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bootstrap {
    pub protocol: u32,
    /// The host's slot of this viewer, 0 to [`MAX_SLOTS`] - 1.
    pub slot: u8,
    /// The host's process: on Windows the viewer checks that the endpoint
    /// is served by it.
    pub host_pid: u32,
    /// Where the viewer reports: a pipe name or a socket path.
    pub endpoint: String,
    /// One-time, for this launch: the viewer gives it back in [`Hello`].
    pub token: String,
}

impl Bootstrap {
    /// Whether a viewer may act on it.
    pub fn valid(&self) -> bool {
        self.protocol == PROTOCOL
            && usize::from(self.slot) < MAX_SLOTS
            && self.host_pid != 0
            && !self.endpoint.is_empty()
            && self.endpoint.len() <= MAX_ENDPOINT
            && !self.endpoint.contains('\0')
            && token_shaped(&self.token)
    }
}

/// Whether `token` has the shape of one (it says nothing of its value).
pub fn token_shaped(token: &str) -> bool {
    token.len() == TOKEN_LEN && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether the token a viewer sent is the one its launch was given. Every
/// byte is compared, whatever the first difference: the time taken says
/// nothing of how much matched.
pub fn token_matches(expected: &str, given: &str) -> bool {
    if !token_shaped(expected) || given.len() != expected.len() {
        return false;
    }
    expected.bytes().zip(given.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// The active character, or one of the others.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// In front for the host, whatever has the keyboard (the host's own
    /// window may): full 3D, sound, voice when allowed.
    Active,
    #[default]
    Background,
}

/// How a background character waits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Standby {
    /// Its window stays behind the active one: the quickest switch.
    #[default]
    Warm,
    /// Its window is minimised: Aurora then gives its textures back.
    Economy,
}

/// What the host wants of a viewer, whole: the latest one replaces the
/// others, so a lost or late message cannot leave a viewer half set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    /// Grows with every change the host sends to this viewer; an older or
    /// repeated one is dropped, and the viewer's status says which it holds.
    pub seq: u64,
    pub role: Role,
    /// The 3D of a background character (Off by default).
    pub behind: Behind3d,
    pub standby: Standby,
    /// Every sound of this viewer is cut for now; nothing saved changes.
    pub sound_muted: bool,
    /// Voice may connect and transmit: the active character only, and only
    /// once every other character has given it up.
    pub voice_allowed: bool,
}

/// Where the characters' windows go: the outer rectangle of a window in
/// physical pixels of the desktop, or maximised on that screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stage {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximised: bool,
}

impl Stage {
    /// A rectangle a window can take: a size a viewer can show and a place
    /// on a desktop, not numbers to overflow with.
    pub fn valid(&self) -> bool {
        const FAR: i32 = 1 << 16;
        (200..=16_384).contains(&self.width)
            && (150..=16_384).contains(&self.height)
            && (-FAR..=FAR).contains(&self.x)
            && (-FAR..=FAR).contains(&self.y)
    }
}

/// Host → viewer, after the bootstrap.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToViewer {
    Assign(Assignment),
    /// Come to the front (restored if minimised), on the stage when there
    /// is one.
    Show {
        stage: Option<Stage>,
    },
    /// Minimise the window (Economy standby).
    Minimise,
    /// Quit as if the user had chosen Quit: log out, save, exit. Unsaved
    /// work in an editor still asks its question in the viewer.
    Quit,
}

/// The first frame a viewer sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    /// Version and build of the viewer: the host refuses another build
    /// than its own.
    pub build: String,
    pub token: String,
    pub slot: u8,
    pub pid: u32,
}

/// Why the host refuses a viewer's first frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Protocol,
    Build,
    Slot,
    Token,
}

impl Hello {
    /// The host's check of a viewer's first frame, for the launch it made:
    /// `slot`, `token` and its own `build`.
    pub fn check(&self, slot: u8, token: &str, build: &str) -> Result<(), Refusal> {
        if self.protocol != PROTOCOL {
            Err(Refusal::Protocol)
        } else if self.slot != slot {
            Err(Refusal::Slot)
        } else if !token_matches(token, &self.token) {
            Err(Refusal::Token)
        } else if self.build != build {
            Err(Refusal::Build)
        } else {
            Ok(())
        }
    }
}

/// The screen a viewer shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Screen {
    /// Not there yet: the window is being made.
    #[default]
    Starting,
    Login,
    /// Logging in, or arriving.
    Loading,
    World,
}

/// The graphics card as a viewer sees it (rules R4 and R5: none of these
/// ever closes a background viewer).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gpu {
    #[default]
    Ok,
    /// The 3D is paused because the card is overloaded (Aurora's busy pause).
    Busy,
    /// The card holds a frame.
    Stalled,
    /// The device is lost; behind, it waits for the window to come forward.
    Lost,
}

/// A viewer's window, as it reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    /// Its outer rectangle; None where the system does not tell (Wayland).
    pub rect: Option<Stage>,
    pub minimised: bool,
    pub fullscreen: bool,
}

/// A viewer's state, sent when it changes and once a second.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// Changes at every login and logout of this viewer.
    pub generation: u32,
    /// The [`Assignment::seq`] the viewer holds (0: none yet).
    pub assign_seq: u64,
    pub screen: Screen,
    /// The character's name once logged in.
    pub name: String,
    /// Its account's id and grid once logged in: two slots on one account
    /// are told to the user.
    pub agent: String,
    pub grid: String,
    pub region: String,
    /// Unread IMs, group chats and conferences.
    pub unread: u32,
    /// The window has the keyboard.
    pub focused: bool,
    pub window: WindowState,
    pub gpu: Gpu,
    /// The 3D is paused (a background character with Off).
    pub paused_3d: bool,
    pub sound_muted: bool,
    pub voice_allowed: bool,
}

/// The longest texts a status may carry, in characters.
pub const MAX_NAME: usize = 64;
pub const MAX_REGION: usize = 80;
pub const MAX_ID: usize = 64;

impl Status {
    /// The status with every text bounded and without control characters:
    /// what the host keeps and shows of what a viewer sent.
    pub fn cleaned(mut self) -> Self {
        self.name = clean(&self.name, MAX_NAME);
        self.agent = clean(&self.agent, MAX_ID);
        self.grid = clean(&self.grid, MAX_ID);
        self.region = clean(&self.region, MAX_REGION);
        self.window.rect = self.window.rect.filter(Stage::valid);
        self
    }
}

fn clean(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Viewer → host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToHost {
    Hello(Hello),
    Status(Box<Status>),
    /// From the link's own thread, every [`HEARTBEAT`], with the number of
    /// turns the main loop has made: a loop that stopped turning is slow,
    /// a thread that stopped writing is not answering (rule R6).
    Heartbeat {
        loop_count: u64,
    },
}

/// Why a frame was not read.
#[derive(Debug)]
pub enum FrameError {
    /// The other side closed, between two frames.
    Closed,
    Io(io::Error),
    /// A length over [`MAX_FRAME`]: nothing of it is read.
    TooLong(usize),
    /// Not the JSON of the expected message.
    Invalid(serde_json::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "closed"),
            Self::Io(e) => write!(f, "{e}"),
            Self::TooLong(n) => write!(f, "a frame of {n} bytes is over the limit"),
            Self::Invalid(e) => write!(f, "not a message ({e})"),
        }
    }
}

/// Writes one frame, in a single write so that a frame is whole or absent.
pub fn write_frame<T: Serialize>(to: &mut impl Write, message: &T) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    if body.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame over the limit"));
    }
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    to.write_all(&frame)?;
    to.flush()
}

/// Reads one frame. After any error but [`FrameError::Closed`] the stream
/// is out of step: the caller stops reading it.
pub fn read_frame<T: DeserializeOwned>(from: &mut impl Read) -> Result<T, FrameError> {
    let mut length = [0u8; 4];
    let mut got = 0;
    while got < length.len() {
        match from.read(&mut length[got..]) {
            Ok(0) if got == 0 => return Err(FrameError::Closed),
            Ok(0) => return Err(FrameError::Io(io::ErrorKind::UnexpectedEof.into())),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(FrameError::Io(e)),
        }
    }
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(FrameError::TooLong(length));
    }
    let mut body = vec![0u8; length];
    from.read_exact(&mut body).map_err(FrameError::Io)?;
    serde_json::from_slice(&body).map_err(FrameError::Invalid)
}

/// The name of a launch's endpoint, without its folder or pipe prefix:
/// `<prefix><session>-<host pid>-<slot>-<random>` (`random`: hexadecimal
/// digits only, so the name holds nothing to escape).
pub fn endpoint_name(session: u32, host_pid: u32, slot: u8, random: &str) -> Option<String> {
    (!random.is_empty() && random.len() <= TOKEN_LEN && random.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("{}{session}-{host_pid}-{slot}-{random}", folderstorm_brand::SESSION_PIPE_PREFIX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(fill: char) -> String {
        std::iter::repeat_n(fill, TOKEN_LEN).collect()
    }

    fn assignment(seq: u64) -> Assignment {
        Assignment {
            seq,
            role: Role::Background,
            behind: Behind3d::Off,
            standby: Standby::Economy,
            sound_muted: true,
            voice_allowed: false,
        }
    }

    #[test]
    fn frames_come_back_as_they_were_sent_one_after_the_other() {
        let mut wire = Vec::new();
        let sent = [
            ToViewer::Assign(assignment(7)),
            ToViewer::Show {
                stage: Some(Stage {
                    x: -1920,
                    y: 0,
                    width: 1920,
                    height: 1080,
                    maximised: false,
                }),
            },
            ToViewer::Minimise,
            ToViewer::Quit,
        ];
        for message in &sent {
            write_frame(&mut wire, message).expect("written");
        }
        let mut read = wire.as_slice();
        for message in &sent {
            assert_eq!(&read_frame::<ToViewer>(&mut read).expect("read"), message);
        }
        assert!(matches!(read_frame::<ToViewer>(&mut read), Err(FrameError::Closed)));

        let status = Status {
            generation: 2,
            assign_seq: 7,
            screen: Screen::World,
            name: "Sam Resident".into(),
            region: "Sandbox".into(),
            unread: 3,
            ..Default::default()
        };
        let mut wire = Vec::new();
        for message in [
            ToHost::Status(Box::new(status)),
            ToHost::Heartbeat { loop_count: 99 },
            ToHost::Heartbeat { loop_count: u64::MAX },
        ] {
            write_frame(&mut wire, &message).expect("written");
            assert_eq!(read_frame::<ToHost>(&mut wire.as_slice()).expect("read"), message);
            wire.clear();
        }
    }

    /// A reader that gives one byte at a time: frames never depend on how
    /// the pipe cuts them.
    struct Trickle<'a>(&'a [u8]);
    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let Some((first, rest)) = self.0.split_first() else {
                return Ok(0);
            };
            buf[0] = *first;
            self.0 = rest;
            Ok(1)
        }
    }

    #[test]
    fn a_frame_cut_anywhere_by_the_pipe_is_read_whole() {
        let mut wire = Vec::new();
        write_frame(&mut wire, &ToHost::Heartbeat { loop_count: 5 }).expect("written");
        let read: ToHost = read_frame(&mut Trickle(&wire)).expect("read");
        assert_eq!(read, ToHost::Heartbeat { loop_count: 5 });
    }

    #[test]
    fn bad_frames_are_refused_without_reading_them() {
        // a length over the limit: refused before any allocation of it
        let mut wire = (u32::MAX).to_le_bytes().to_vec();
        wire.extend_from_slice(b"junk");
        assert!(matches!(
            read_frame::<ToHost>(&mut wire.as_slice()),
            Err(FrameError::TooLong(n)) if n == u32::MAX as usize
        ));
        let over = (MAX_FRAME as u32 + 1).to_le_bytes();
        assert!(matches!(read_frame::<ToHost>(&mut over.as_slice()), Err(FrameError::TooLong(_))));
        // text typed at a terminal is not a frame
        assert!(matches!(
            read_frame::<Bootstrap>(&mut b"hello there\n".as_slice()),
            Err(FrameError::TooLong(_))
        ));
        // cut in the length or in the body
        assert!(matches!(read_frame::<ToHost>(&mut [1u8, 0].as_slice()), Err(FrameError::Io(_))));
        let mut wire = Vec::new();
        write_frame(&mut wire, &ToHost::Heartbeat { loop_count: 1 }).expect("written");
        wire.pop();
        assert!(matches!(read_frame::<ToHost>(&mut wire.as_slice()), Err(FrameError::Io(_))));
        // JSON, but not a message; and a message of the other direction
        let mut wire = Vec::new();
        write_frame(&mut wire, &serde_json::json!({"Launch": "calc.exe"})).expect("written");
        assert!(matches!(read_frame::<ToViewer>(&mut wire.as_slice()), Err(FrameError::Invalid(_))));
        let mut wire = Vec::new();
        write_frame(&mut wire, &ToHost::Heartbeat { loop_count: 1 }).expect("written");
        assert!(matches!(read_frame::<ToViewer>(&mut wire.as_slice()), Err(FrameError::Invalid(_))));
        // a frame over the limit is not written either
        let long = "x".repeat(MAX_FRAME);
        assert!(write_frame(&mut Vec::new(), &long).is_err());
    }

    #[test]
    fn a_bootstrap_is_checked_before_anything_is_opened() {
        let good = Bootstrap {
            protocol: PROTOCOL,
            slot: 4,
            host_pid: 1234,
            endpoint: r"\\.\pipe\folderstorm2-session-1-1234-4-abcd".into(),
            token: token('a'),
        };
        assert!(good.valid());
        let bad = |change: fn(&mut Bootstrap)| {
            let mut b = good.clone();
            change(&mut b);
            !b.valid()
        };
        assert!(bad(|b| b.protocol += 1));
        assert!(bad(|b| b.slot = MAX_SLOTS as u8));
        assert!(bad(|b| b.host_pid = 0));
        assert!(bad(|b| b.endpoint.clear()));
        assert!(bad(|b| b.endpoint = "x".repeat(MAX_ENDPOINT + 1)));
        assert!(bad(|b| b.endpoint.push('\0')));
        assert!(bad(|b| b.token.clear()));
        assert!(bad(|b| b.token = "z".repeat(TOKEN_LEN)));
        assert!(bad(|b| b.token.push('a')));
    }

    #[test]
    fn only_the_launch_s_own_token_slot_build_and_protocol_are_accepted() {
        let hello = Hello {
            protocol: PROTOCOL,
            build: "1.0 (Release)".into(),
            token: token('a'),
            slot: 2,
            pid: 77,
        };
        assert_eq!(hello.check(2, &token('a'), "1.0 (Release)"), Ok(()));
        assert_eq!(hello.check(2, &token('b'), "1.0 (Release)"), Err(Refusal::Token));
        assert_eq!(hello.check(3, &token('a'), "1.0 (Release)"), Err(Refusal::Slot));
        assert_eq!(hello.check(2, &token('a'), "1.1 (Release)"), Err(Refusal::Build));
        let mut other = hello.clone();
        other.protocol += 1;
        assert_eq!(other.check(2, &token('a'), "1.0 (Release)"), Err(Refusal::Protocol));
        // a wrong token is told before a wrong build: nothing about the
        // host is learnt without the token
        assert_eq!(hello.check(2, &token('b'), "other"), Err(Refusal::Token));
    }

    #[test]
    fn tokens_match_whole_or_not_at_all() {
        let a = token('a');
        assert!(token_matches(&a, &a));
        assert!(token_matches(&a, &a.to_ascii_lowercase()));
        let mut near = a.clone();
        near.replace_range(TOKEN_LEN - 1.., "b");
        assert!(!token_matches(&a, &near));
        assert!(!token_matches(&a, &a[..TOKEN_LEN - 1]));
        assert!(!token_matches(&a, ""));
        // an empty or misshapen expected token matches nothing, itself included
        assert!(!token_matches("", ""));
        assert!(!token_matches("short", "short"));
    }

    #[test]
    fn what_a_viewer_reports_is_bounded_and_cleaned_before_it_is_shown() {
        let status = Status {
            name: format!("  Sam\u{7}\n{}", "x".repeat(200)),
            region: "Sand\tbox\u{0}".into(),
            agent: "a".repeat(500),
            grid: "agni\r\n".into(),
            window: WindowState {
                rect: Some(Stage {
                    x: i32::MAX,
                    y: 0,
                    width: 800,
                    height: 600,
                    maximised: false,
                }),
                ..Default::default()
            },
            ..Default::default()
        }
        .cleaned();
        assert!(status.name.starts_with("Sam"));
        assert_eq!(status.name.chars().count(), MAX_NAME - 2);
        assert_eq!(status.region, "Sandbox");
        assert_eq!(status.agent.len(), MAX_ID);
        assert_eq!(status.grid, "agni");
        assert_eq!(status.window.rect, None, "a rectangle off any desktop is dropped");
    }

    #[test]
    fn a_stage_is_a_window_a_viewer_can_take() {
        let stage = Stage {
            x: -2560,
            y: 120,
            width: 2560,
            height: 1440,
            maximised: true,
        };
        assert!(stage.valid());
        assert!(!Stage { width: 0, ..stage }.valid());
        assert!(!Stage { height: 10, ..stage }.valid());
        assert!(!Stage { width: u32::MAX, ..stage }.valid());
        assert!(!Stage { x: i32::MIN, ..stage }.valid());
        assert!(!Stage { y: 1 << 20, ..stage }.valid());
    }

    #[test]
    fn endpoint_names_hold_the_session_host_slot_and_random_part_only() {
        assert_eq!(
            endpoint_name(1, 4321, 2, "0af3").as_deref(),
            Some("folderstorm2-session-1-4321-2-0af3")
        );
        assert_eq!(endpoint_name(1, 1, 0, ""), None);
        assert_eq!(endpoint_name(1, 1, 0, r"..\x"), None);
        assert_eq!(endpoint_name(1, 1, 0, "a/b"), None);
    }
}
