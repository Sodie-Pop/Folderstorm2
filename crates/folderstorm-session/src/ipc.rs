//! What the session host and a viewer it manages say to each other
//! (session-host plan, section 8), and how it is framed. Nothing here opens
//! a pipe: the framing reads and writes any byte stream, so every rule is
//! tested on any system.
//!
//! Two one-way channels per launched viewer, both local to the PC:
//! - **host → viewer**: the viewer's standard input, a pipe that only the
//!   host and that child hold (anonymous on Linux and macOS; on Windows,
//!   Rust makes it a randomly named pipe whose two ends are connected as
//!   it is made, first instance only). Its first frame is the
//!   [`Bootstrap`], then [`ToViewer`] commands. Its end (the host closed or
//!   crashed) is how a viewer learns that nobody manages it any more.
//! - **viewer → host**: a private endpoint the host made for that one
//!   launch (a named pipe of the user's logon on Windows, a socket only the
//!   user can open elsewhere), named in the bootstrap. Its first frame is
//!   [`Hello`] with the bootstrap's one-time token, then [`ToHost`] reports.
//!
//! Neither channel ever carries a password or a login token: each
//! character logs in on its own login screen.
//!
//! Since protocol 2 (checkpoint 3, plan section 9) the channels carry the
//! shared chat: a viewer tells the lines its own window shows
//! ([`ChatBatch`]) and what waits for an answer there ([`AttentionList`]);
//! the host asks a viewer to say a line the user typed for that character
//! ([`ToViewer::SendChat`]) and hears whether the viewer took it
//! ([`ToHost::SendResult`]). A line to say travels on the standard input
//! of the one viewer it is for, with that viewer's login generation: no
//! message names another character, so none can be sent as another.
//!
//! Since protocol 3 (checkpoint 5, plan section 12) a background viewer
//! may be asked for its monitor's pictures ([`Role::Monitor`]) and sends
//! them ([`ToHost::Frame`]).
//!
//! Since protocol 4 (checkpoint 8, plan section 15) the host hands each
//! viewer the bar to draw at the top of its window ([`ToViewer::Bar`]),
//! hears the clicks on it ([`ToHost::Bar`]), and may ask a background
//! character's window off the taskbar ([`Assignment::off_taskbar`]).
//!
//! A frame is a little-endian `u32` length, then that many bytes of JSON,
//! [`MAX_FRAME`] at most. Text that comes from the other side is cleaned
//! and bounded before anything shows it ([`Status::cleaned`]).
//!
//! **A monitor's picture** is the one thing that is not JSON: a 640 × 360
//! picture is 691 200 bytes of RGB, ten times [`MAX_FRAME`]. It travels on
//! the viewer's own endpoint, right after the [`ToHost::Frame`] message
//! that announces it, as exactly the bytes that message's
//! [`FrameHeader`] says ([`write_picture`], [`read_picture`]). The header
//! is checked first: a size no monitor shows, or a byte count that is not
//! that size's, and nothing more is read of that viewer
//! ([`FrameError::Picture`]); the most that is ever allocated for one is
//! [`crate::monitor::MAX_FRAME_BYTES`]. No encoder or decoder is involved,
//! and no other pipe, socket or shared memory than the endpoint the
//! viewer already reports on, with its checks (this logon only, the
//! launch's process and one-time token).

use crate::bar;
use crate::behind::Behind3d;
use crate::monitor::{self, FrameHeader};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};
use std::time::Duration;

/// Both sides refuse another number.
pub const PROTOCOL: u32 = 4;

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
    /// A background character whose monitor is watched in the host's
    /// window: as [`Role::Background`] in everything (3D off, sound,
    /// voice, standby), but it takes a small picture of its own view now
    /// and then, as the order says.
    Monitor(monitor::Order),
}

impl Role {
    /// Not the active character: behind, whether watched or not.
    pub fn background(self) -> bool {
        self != Self::Active
    }

    /// The pictures asked of this viewer, if any, never faster than a
    /// monitor's fastest rate.
    pub fn monitor(self) -> Option<monitor::Order> {
        match self {
            Self::Monitor(order) => Some(order.cleaned()),
            Self::Active | Self::Background => None,
        }
    }
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
    /// Checkpoint 8, Windows: this window stays off the taskbar and out
    /// of Alt-Tab while it is behind the active character's (the viewer
    /// decides how, `bar::taskbar`, and puts everything back by itself).
    #[serde(default)]
    pub off_taskbar: bool,
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
    /// Say `text` in `conversation`, as the chat bar or the conversation's
    /// field would. `id` grows with every line the host asks this viewer
    /// to say: one that is not above the last is never said again.
    /// `generation`: the login the user typed it for; another login
    /// refuses it.
    SendChat {
        id: u64,
        generation: u32,
        conversation: Conversation,
        text: String,
    },
    /// The user read that conversation in the host's window.
    MarkRead {
        generation: u32,
        conversation: Conversation,
    },
    /// Checkpoint 8: the bar to draw at the top of the window, whole.
    Bar(Box<bar::State>),
}

impl ToViewer {
    /// The command with its text bounded and cleaned: what a viewer acts
    /// on of what came down its pipe.
    pub fn cleaned(self) -> Self {
        match self {
            Self::SendChat {
                id,
                generation,
                conversation,
                text,
            } => Self::SendChat {
                id,
                generation,
                conversation,
                text: clean_text(&text, MAX_SEND),
            },
            Self::Bar(state) => Self::Bar(Box::new(state.cleaned())),
            other => other,
        }
    }
}

/// A conversation of one character. It names nothing by itself: the viewer
/// that receives it looks the session up in its own world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Conversation {
    /// The nearby chat.
    #[default]
    Nearby,
    /// A session of the viewer's conversations, by the two halves of its
    /// id: a resident's, a group's or a conference's.
    Session(u64, u64),
}

impl Conversation {
    pub fn session(id: u128) -> Self {
        Self::Session((id >> 64) as u64, id as u64)
    }

    /// The session's id; None for the nearby chat.
    pub fn id(self) -> Option<u128> {
        match self {
            Self::Nearby => None,
            Self::Session(high, low) => Some((u128::from(high) << 64) | u128::from(low)),
        }
    }
}

/// What a conversation is, as its viewer says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionKind {
    #[default]
    Nearby,
    Resident,
    Group,
    Conference,
}

/// Who a line is from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineKind {
    /// Someone else.
    #[default]
    Said,
    /// The character itself.
    Own,
    /// The viewer or the grid.
    System,
    /// A scripted object.
    Object,
    /// An offer shown as a card in the conversation: answered in the
    /// viewer's own window, never in the host.
    Card,
}

/// One line of a conversation, as the viewer's own window shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatLine {
    /// The viewer's number of the line in its conversation: grows by one
    /// per line, whatever is exported of them.
    pub number: u64,
    /// Seconds since the Unix epoch.
    pub time: u64,
    pub kind: LineKind,
    /// The line is an emote: `text` starts with the sender's name.
    pub emote: bool,
    pub from: String,
    pub text: String,
}

/// The longest texts of the chat, in characters.
pub const MAX_CHAT_TEXT: usize = 2048;
pub const MAX_SEND: usize = 1024;
pub const MAX_TITLE: usize = 80;
/// The most lines in a batch, and attention items in a list.
pub const MAX_BATCH_LINES: usize = 32;
pub const MAX_ATTENTION: usize = 16;
/// What a batch's lines may weigh before it is sent, well under
/// [`MAX_FRAME`] whatever their text.
const BATCH_BUDGET: usize = 40 * 1024;

/// New lines of one conversation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatBatch {
    /// The login these lines belong to ([`Status::generation`]).
    pub generation: u32,
    pub conversation: Conversation,
    pub kind: SessionKind,
    /// The conversation's name, as the viewer shows it.
    pub title: String,
    /// What the host holds of this conversation is replaced by `lines`:
    /// the viewer's numbers changed, or what it shows of earlier lines did
    /// (a restriction hides a name or a place now).
    pub restart: bool,
    /// Lines the viewer dropped before they could be told, just before
    /// `lines`: the host marks a gap.
    pub missed: u32,
    /// The viewer's own unread count of the conversation.
    pub unread: u32,
    pub lines: Vec<ChatLine>,
}

impl ChatBatch {
    /// Adds a line if the batch still fits a frame with it; false: the
    /// batch is full, the line goes in the next one. The first line always
    /// fits.
    pub fn push(&mut self, line: ChatLine) -> bool {
        // (JSON writes a character in at most twice its bytes once the
        // control characters are gone)
        let weight = |l: &ChatLine| 2 * (l.from.len() + l.text.len()) + 96;
        let held: usize = self.lines.iter().map(weight).sum();
        if !self.lines.is_empty() && (self.lines.len() >= MAX_BATCH_LINES || held + weight(&line) > BATCH_BUDGET) {
            return false;
        }
        self.lines.push(line);
        true
    }

    /// The batch with every text bounded and cleaned: what the host keeps
    /// and shows of what a viewer sent.
    pub fn cleaned(mut self) -> Self {
        self.title = clean(&self.title, MAX_TITLE);
        self.lines.truncate(MAX_BATCH_LINES);
        for line in &mut self.lines {
            line.from = clean(&line.from, MAX_NAME);
            line.text = clean_text(&line.text, MAX_CHAT_TEXT);
        }
        self
    }
}

/// What waits for the user's answer in a viewer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttentionKind {
    Teleport,
    Friendship,
    Inventory,
    Group,
    /// A script's dialog or text box.
    Script,
    /// A script asks for permissions.
    Permissions,
    Link,
    Call,
    #[default]
    Other,
}

/// One question waiting in a viewer. The host only shows it: the answer
/// is given in the viewer's own window.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionItem {
    /// The viewer's number of the question.
    pub id: u64,
    pub kind: AttentionKind,
    pub title: String,
    pub text: String,
}

/// The longest text of an attention item, in characters.
pub const MAX_ATTENTION_TEXT: usize = 240;

/// Everything that waits for an answer in a viewer, whole: the latest
/// list replaces the others.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionList {
    pub generation: u32,
    pub items: Vec<AttentionItem>,
    /// How many more wait than `items` tells.
    pub more: u32,
}

impl AttentionList {
    pub fn cleaned(mut self) -> Self {
        let over = self.items.len().saturating_sub(MAX_ATTENTION);
        self.items.truncate(MAX_ATTENTION);
        self.more = self.more.saturating_add(u32::try_from(over).unwrap_or(u32::MAX));
        for item in &mut self.items {
            item.title = clean(&item.title, MAX_TITLE);
            item.text = clean(&item.text, MAX_ATTENTION_TEXT);
        }
        self
    }
}

/// Why a viewer did not say a line the host asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendRefusal {
    /// The character is not in the world.
    NotInWorld,
    /// It logged out or in since the line was typed.
    Stale,
    /// The conversation is closed, or not one of this character's.
    Closed,
    /// A restriction (RLVa) forbids it.
    Restricted,
    /// Nothing to say once cleaned.
    Empty,
    /// The line's number was already seen: never said twice.
    Repeated,
}

/// What became of a line the host asked a viewer to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendOutcome {
    /// Handed to the viewer's own sending, with its rules: what was said
    /// comes back as a line of the conversation.
    Accepted,
    Refused(SendRefusal),
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
    /// Its monitor: off, live, slowed by a busy card, or why it has no
    /// picture.
    #[serde(default)]
    pub monitor: monitor::State,
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

/// A chat text bounded and without control characters, its line breaks
/// kept.
pub fn clean_text(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
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
    /// New lines of a conversation.
    Chat(Box<ChatBatch>),
    /// What waits for an answer in the viewer, whole.
    Attention(Box<AttentionList>),
    /// What became of [`ToViewer::SendChat`] number `id`.
    SendResult {
        id: u64,
        outcome: SendOutcome,
    },
    /// A monitor's picture follows: exactly the header's bytes, raw, right
    /// after this message ([`write_picture`], [`read_picture`]).
    Frame(FrameHeader),
    /// Checkpoint 8: a click on the bar this viewer draws.
    Bar(bar::Request),
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
    /// A picture's header that is not one a monitor shows: nothing of what
    /// follows is read.
    Picture(FrameHeader),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "closed"),
            Self::Io(e) => write!(f, "{e}"),
            Self::TooLong(n) => write!(f, "a frame of {n} bytes is over the limit"),
            Self::Invalid(e) => write!(f, "not a message ({e})"),
            Self::Picture(h) => write!(f, "not a monitor's picture ({} x {}, {} bytes)", h.width, h.height, h.bytes),
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

/// Writes a monitor's picture: the [`ToHost::Frame`] message, then `rgb`,
/// in a single write. Refused before anything is written when `rgb` is not
/// what the header says or the header is not a monitor's.
pub fn write_picture(to: &mut impl Write, header: FrameHeader, rgb: &[u8]) -> io::Result<()> {
    if header.payload() != Some(rgb.len()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a monitor's picture"));
    }
    let body = serde_json::to_vec(&ToHost::Frame(header)).map_err(io::Error::other)?;
    let mut frame = Vec::with_capacity(4 + body.len() + rgb.len());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    frame.extend_from_slice(rgb);
    to.write_all(&frame)?;
    to.flush()
}

/// Reads the picture a [`ToHost::Frame`] message announced. The header is
/// checked before anything is allocated or read; after any error the
/// stream is out of step and the caller stops reading it.
pub fn read_picture(from: &mut impl Read, header: &FrameHeader) -> Result<Vec<u8>, FrameError> {
    let bytes = header.payload().ok_or(FrameError::Picture(*header))?;
    let mut rgb = vec![0u8; bytes];
    from.read_exact(&mut rgb).map_err(FrameError::Io)?;
    Ok(rgb)
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
            off_taskbar: false,
        }
    }

    #[test]
    fn the_bar_goes_down_whole_and_cleaned_and_its_clicks_come_up() {
        let state = bar::State {
            docked: true,
            characters: vec![bar::Character {
                slot: 1,
                name: "Shop\u{7} keeper".to_owned(),
                real: Some("Jane Doe".to_owned()),
                colour: crate::identity::Colour::Teal,
                presence: bar::Presence::Inactive,
                quick: crate::host::Quick::Switch,
                monitor: true,
            }],
            inbox: 2,
            quick: vec![bar::QuickAction::Character(1), bar::QuickAction::Attention],
            ..bar::State::default()
        };
        let mut wire = Vec::new();
        write_frame(&mut wire, &ToViewer::Bar(Box::new(state.clone()))).expect("written");
        assert!(wire.len() < MAX_FRAME / 16, "{} bytes", wire.len());
        let read: ToViewer = read_frame(&mut wire.as_slice()).expect("read");
        assert_eq!(read, ToViewer::Bar(Box::new(state)));
        // what a viewer acts on is cleaned
        let ToViewer::Bar(cleaned) = read.cleaned() else {
            panic!("not the bar");
        };
        assert_eq!(cleaned.characters[0].name, "Shop keeper");
        assert_eq!(cleaned.characters[0].colour, crate::identity::Colour::Teal);
        // the fullest bar there can be fits a frame many times over
        let full = bar::State {
            characters: (0..MAX_SLOTS as u8)
                .map(|slot| bar::Character {
                    slot,
                    name: "ç".repeat(MAX_NAME),
                    real: Some("ç".repeat(MAX_NAME)),
                    ..bar::Character::default()
                })
                .collect(),
            quick: vec![bar::QuickAction::MuteBackground; bar::MAX_QUICK],
            ..bar::State::default()
        }
        .cleaned();
        let mut wire = Vec::new();
        write_frame(&mut wire, &ToViewer::Bar(Box::new(full))).expect("written");
        assert!(wire.len() < MAX_FRAME / 8, "{} bytes", wire.len());
        // a click
        let mut wire = Vec::new();
        for request in [
            bar::Request::Pick(2),
            bar::Request::Show(bar::Page::Quit),
            bar::Request::Dock(false),
        ] {
            write_frame(&mut wire, &ToHost::Bar(request)).expect("written");
        }
        let mut rest = wire.as_slice();
        assert_eq!(read_frame::<ToHost>(&mut rest).expect("read"), ToHost::Bar(bar::Request::Pick(2)));
        assert_eq!(
            read_frame::<ToHost>(&mut rest).expect("read"),
            ToHost::Bar(bar::Request::Show(bar::Page::Quit))
        );
        // an assignment of before protocol 4 has no word on the taskbar
        let old = r#"{"seq":3,"role":"Background","behind":"Off","standby":"Warm","sound_muted":true,"voice_allowed":false}"#;
        assert!(!serde_json::from_str::<Assignment>(old).expect("reads").off_taskbar);
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

    #[test]
    fn the_chat_messages_come_back_as_they_were_sent() {
        let conversation = Conversation::session(u128::MAX - 5);
        assert_eq!(conversation.id(), Some(u128::MAX - 5));
        assert_eq!(Conversation::session(7).id(), Some(7));
        assert_eq!(Conversation::Nearby.id(), None);
        let batch = ChatBatch {
            generation: 3,
            conversation,
            kind: SessionKind::Group,
            title: "Builders".into(),
            restart: true,
            missed: 2,
            unread: 1,
            lines: vec![ChatLine {
                number: 1 << 40,
                time: 1_800_000_000,
                kind: LineKind::Said,
                emote: false,
                from: "Jane Doe".into(),
                text: "hello \"there\"\nsecond line \u{1F600}".into(),
            }],
        };
        let attention = AttentionList {
            generation: 3,
            items: vec![AttentionItem {
                id: 9,
                kind: AttentionKind::Teleport,
                title: "Jane Doe".into(),
                text: "offers a teleport".into(),
            }],
            more: 0,
        };
        for message in [
            ToHost::Chat(Box::new(batch)),
            ToHost::Attention(Box::new(attention)),
            ToHost::SendResult {
                id: 4,
                outcome: SendOutcome::Accepted,
            },
            ToHost::SendResult {
                id: 5,
                outcome: SendOutcome::Refused(SendRefusal::Stale),
            },
        ] {
            let mut wire = Vec::new();
            write_frame(&mut wire, &message).expect("written");
            assert_eq!(read_frame::<ToHost>(&mut wire.as_slice()).expect("read"), message);
        }
        for message in [
            ToViewer::SendChat {
                id: 1,
                generation: 3,
                conversation,
                text: "hi".into(),
            },
            ToViewer::MarkRead {
                generation: 3,
                conversation: Conversation::Nearby,
            },
        ] {
            let mut wire = Vec::new();
            write_frame(&mut wire, &message).expect("written");
            assert_eq!(read_frame::<ToViewer>(&mut wire.as_slice()).expect("read"), message);
        }
    }

    #[test]
    fn chat_from_a_viewer_is_bounded_and_cleaned_before_it_is_kept() {
        let line = |text: &str| ChatLine {
            from: format!("Jane\u{7}{}", "x".repeat(200)),
            text: text.to_owned(),
            ..Default::default()
        };
        let batch = ChatBatch {
            title: format!("Group\r\n{}", "t".repeat(300)),
            lines: std::iter::repeat_n(line(&format!("a\u{1b}[31mb\nc\0{}", "z".repeat(5000))), 100).collect(),
            ..Default::default()
        }
        .cleaned();
        assert_eq!(batch.title.chars().count(), MAX_TITLE);
        assert_eq!(batch.lines.len(), MAX_BATCH_LINES);
        let first = &batch.lines[0];
        assert_eq!(first.from.chars().count(), MAX_NAME);
        assert_eq!(first.text.chars().count(), MAX_CHAT_TEXT);
        assert!(first.text.starts_with("a[31mb\nc"), "line breaks stay, control characters go");
        let list = AttentionList {
            items: std::iter::repeat_n(
                AttentionItem {
                    title: "T\n".repeat(100),
                    text: "x".repeat(1000),
                    ..Default::default()
                },
                20,
            )
            .collect(),
            more: 1,
            ..Default::default()
        }
        .cleaned();
        assert_eq!((list.items.len(), list.more), (MAX_ATTENTION, 5));
        assert_eq!(list.items[0].text.len(), MAX_ATTENTION_TEXT);
        assert!(!list.items[0].title.contains('\n'));
        // what the host asks a viewer to say is bounded there too
        let send = ToViewer::SendChat {
            id: 1,
            generation: 1,
            conversation: Conversation::Nearby,
            text: format!("  hi\u{0}\u{8} {}", "y".repeat(3000)),
        }
        .cleaned();
        let ToViewer::SendChat { text, .. } = send else {
            panic!("another command");
        };
        assert!(text.starts_with("hi y"));
        assert!((MAX_SEND - 2..=MAX_SEND).contains(&text.chars().count()));
        assert_eq!(clean_text(" \u{1}\n ", MAX_SEND), "");
    }

    #[test]
    fn a_batch_of_the_longest_lines_still_fits_a_frame() {
        // the worst text: four bytes a character, and quotes that JSON doubles
        let worst = |c: char| ChatLine {
            number: u64::MAX,
            time: u64::MAX,
            kind: LineKind::Object,
            emote: true,
            from: c.to_string().repeat(MAX_NAME),
            text: c.to_string().repeat(MAX_CHAT_TEXT),
        };
        for c in ['\u{1F600}', '"', '\\', '\n', 'a'] {
            let mut batch = ChatBatch {
                title: c.to_string().repeat(MAX_TITLE),
                ..Default::default()
            };
            let mut pushed = 0;
            while batch.push(worst(c)) {
                pushed += 1;
                assert!(pushed <= MAX_BATCH_LINES);
            }
            assert!(pushed >= 1, "the first line always fits");
            let mut wire = Vec::new();
            write_frame(&mut wire, &ToHost::Chat(Box::new(batch.clone()))).expect("a full batch is under the frame limit");
            assert_eq!(
                read_frame::<ToHost>(&mut wire.as_slice()).expect("read"),
                ToHost::Chat(Box::new(batch))
            );
        }
        // short lines: the count bounds the batch
        let mut batch = ChatBatch::default();
        let pushed = (0..100).filter(|_| batch.push(ChatLine::default())).count();
        assert_eq!(pushed, MAX_BATCH_LINES);
    }

    #[test]
    fn a_message_of_another_protocol_is_not_one() {
        // a chat line is not a command, a command is not a report
        let mut wire = Vec::new();
        write_frame(&mut wire, &ToHost::Chat(Box::default())).expect("written");
        assert!(matches!(read_frame::<ToViewer>(&mut wire.as_slice()), Err(FrameError::Invalid(_))));
        let mut wire = Vec::new();
        write_frame(
            &mut wire,
            &serde_json::json!({"SendChat": {"id": 1, "generation": 1, "conversation": "Nearby", "text": "x", "as": "someone else"}}),
        )
        .expect("written");
        // (an unknown field is ignored; a missing or misshapen one refuses the frame)
        assert!(read_frame::<ToViewer>(&mut wire.as_slice()).is_ok());
        let mut wire = Vec::new();
        write_frame(&mut wire, &serde_json::json!({"SendChat": {"id": 1, "text": "x"}})).expect("written");
        assert!(matches!(read_frame::<ToViewer>(&mut wire.as_slice()), Err(FrameError::Invalid(_))));
        let mut wire = Vec::new();
        write_frame(&mut wire, &serde_json::json!({"SendChatAs": {"slot": 2, "text": "x"}})).expect("written");
        assert!(matches!(read_frame::<ToViewer>(&mut wire.as_slice()), Err(FrameError::Invalid(_))));
    }

    #[test]
    fn a_monitor_s_picture_follows_its_header_and_the_reports_go_on_after_it() {
        let header = FrameHeader::new(2, 41, 320, 180).expect("a header");
        let rgb: Vec<u8> = (0..320 * 180 * 3).map(|i| (i % 251) as u8).collect();
        let mut wire = Vec::new();
        write_frame(&mut wire, &ToHost::Heartbeat { loop_count: 1 }).expect("written");
        write_picture(&mut wire, header, &rgb).expect("written");
        write_frame(&mut wire, &ToHost::Heartbeat { loop_count: 2 }).expect("written");
        let mut read = wire.as_slice();
        assert_eq!(read_frame::<ToHost>(&mut read).expect("read"), ToHost::Heartbeat { loop_count: 1 });
        let ToHost::Frame(told) = read_frame::<ToHost>(&mut read).expect("read") else {
            panic!("not a picture's header");
        };
        assert_eq!(told, header);
        assert_eq!(read_picture(&mut read, &told).expect("the picture"), rgb);
        // the stream is in step after the picture
        assert_eq!(read_frame::<ToHost>(&mut read).expect("read"), ToHost::Heartbeat { loop_count: 2 });
        // the largest picture passes, cut anywhere by the pipe
        let header = FrameHeader::new(0, 1, 640, 360).expect("a header");
        let rgb = vec![9u8; monitor::MAX_FRAME_BYTES];
        let mut wire = Vec::new();
        write_picture(&mut wire, header, &rgb).expect("written");
        let mut trickle = Trickle(&wire);
        assert_eq!(read_frame::<ToHost>(&mut trickle).expect("read"), ToHost::Frame(header));
        assert_eq!(
            read_picture(&mut trickle, &header).expect("the picture").len(),
            monitor::MAX_FRAME_BYTES
        );
    }

    #[test]
    fn a_picture_that_is_not_a_monitor_s_is_neither_written_nor_read() {
        let header = FrameHeader::new(0, 1, 320, 180).expect("a header");
        let rgb = vec![0u8; 320 * 180 * 3];
        // written whole or not at all
        let mut wire = Vec::new();
        assert!(write_picture(&mut wire, header, &rgb[1..]).is_err());
        assert!(write_picture(&mut wire, FrameHeader { width: 4000, ..header }, &rgb).is_err());
        assert!(wire.is_empty());
        // a header that lies is refused before a byte of what follows is
        // read or any room is made for it
        struct Never;
        impl Read for Never {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("nothing is read after a refused header");
            }
        }
        for lying in [
            FrameHeader { bytes: u32::MAX, ..header },
            FrameHeader {
                bytes: header.bytes + 1,
                ..header
            },
            FrameHeader { bytes: 0, ..header },
            FrameHeader {
                width: u16::MAX,
                height: u16::MAX,
                bytes: u32::MAX,
                ..header
            },
            FrameHeader {
                width: 0,
                height: 0,
                bytes: 0,
                ..header
            },
        ] {
            assert!(matches!(read_picture(&mut Never, &lying), Err(FrameError::Picture(_))), "{lying:?}");
        }
        // a picture cut short is an error, not a short picture
        let mut wire = Vec::new();
        write_picture(&mut wire, header, &rgb).expect("written");
        wire.truncate(wire.len() - 10);
        let mut read = wire.as_slice();
        let ToHost::Frame(told) = read_frame::<ToHost>(&mut read).expect("read") else {
            panic!("not a picture's header");
        };
        assert!(matches!(read_picture(&mut read, &told), Err(FrameError::Io(_))));
    }

    #[test]
    fn a_monitor_s_order_and_state_travel_with_the_assignment_and_the_status() {
        let order = monitor::Order {
            size: monitor::Size::Medium,
            interval_ms: 768,
        };
        let watched = Assignment {
            role: Role::Monitor(order),
            ..assignment(9)
        };
        let mut wire = Vec::new();
        write_frame(&mut wire, &ToViewer::Assign(watched)).expect("written");
        assert_eq!(
            read_frame::<ToViewer>(&mut wire.as_slice()).expect("read"),
            ToViewer::Assign(watched)
        );
        assert_eq!(watched.role.monitor(), Some(order));
        assert!(watched.role.background());
        assert_eq!(Role::Background.monitor(), None);
        assert_eq!(Role::Active.monitor(), None);
        assert!(!Role::Active.background());
        let status = Status {
            monitor: monitor::State::Slowed,
            ..Default::default()
        };
        let mut wire = Vec::new();
        write_frame(&mut wire, &ToHost::Status(Box::new(status.clone()))).expect("written");
        assert_eq!(
            read_frame::<ToHost>(&mut wire.as_slice()).expect("read"),
            ToHost::Status(Box::new(status))
        );
    }
}
