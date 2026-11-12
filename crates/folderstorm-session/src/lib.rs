//! The pure rules of Folderstorm2's multi-character session
//! (`folderstorm/plans/session-host.md`): no window, no file, no thread, so
//! that every rule is unit-tested on any system.
//!
//! Checkpoint 1 (no host yet):
//! - [`profile`]: `--profile <name>`, its name check and the folders, link
//!   hand-over scope, log name and window title derived from it (plan
//!   section 5);
//! - [`behind`]: what a window does while another Folderstorm2 window is in
//!   front, Off / Limited / Normal (section 6.1);
//! - [`gpu`]: a GPU stall or device loss on a window that is not in front
//!   never shows a box over the active character, never lowers its saved
//!   graphics and never closes it (rules R4 and R5 of section 6.3).

//!
//! Checkpoint 2 (the session host):
//! - [`ipc`]: what the host and a viewer it manages say to each other, and
//!   its framing (section 8);
//! - [`managed`]: what a managed viewer makes of the host's assignments:
//!   role over focus, sound and voice (sections 6.3 and 10);
//! - [`host`]: the host's desk: slots, the active character, voice given
//!   up before it is given, liveness without false deaths, restart and
//!   "Quit all" (sections 10 and 11).
//!
//! Checkpoint 3 (shared chat, section 9):
//! - [`export`]: which lines of a viewer's conversations are new, by
//!   observation of the viewer's own line numbers, and a line to say said
//!   at most once;
//! - [`chat`]: the host's chat: conversations per character and "All",
//!   drafts bound to their character, login and conversation, bounded
//!   history with gaps, unread counts, uncertain sends, attention items.

//!
//! Checkpoint 4 (comfort, section 13):
//! - [`hotkeys`]: the host's global hotkeys: which keys, for what, and
//!   what is wrong with a choice;
//! - [`copy`]: "New character profile…": what a new profile takes from
//!   the one it is made from, and what it never takes;
//! - [`host`]: quick actions, next and previous, who holds the voice.
//!
//! Checkpoint 5 (monitors, section 12):
//! - [`monitor`]: sizes, rates and the budget all monitors share, when a
//!   viewer takes a picture and how it lowers its rate on a busy card,
//!   the picture's size and its header's checks, "latest only";
//! - [`host`]: which characters are watched, and what each is asked.
//!
//! Checkpoint 6 (pop-out panels, section 7):
//! - [`panels`]: which of the host's panels have a window of their own
//!   and where, a saved place brought back onto a connected screen,
//!   "closing a panel docks it", and the monitors seen window by window.
//!
//! Checkpoint 7 (polish, section 13):
//! - [`inbox`]: one list across all characters of what waits for an
//!   answer in their own windows; nothing is answered from the host;
//! - [`identity`]: a character's colour, fitted to the theme's contrast,
//!   and its alias; both only ever in the host's settings file;
//! - [`host`]: the condensed layout's choice, with the other settings.
//!
//! Checkpoint 8 (the docked bar, section 15):
//! - [`bar`]: the row at the top of every managed viewer: its settings,
//!   what the host tells a viewer to draw and what a click asks of the
//!   host, when the host's own window hides, and what a background
//!   window is on the taskbar;
//! - [`host`]: "Mute background sound", "Voice follows active", what the
//!   bar shows of the desk and what its clicks do.
//!
//! Checkpoint 9 (monitors in their own windows):
//! - [`monitor_windows`]: what is remembered of each monitor's window, a
//!   monitor that follows the character that is not in front, and the
//!   cycling monitor's turns, which do not pass while the user types;
//! - [`chips`]: a character's conversations as chips: their order, names
//!   that shrink to fit, two rows at most and "+N" for the rest;
//! - [`chat`]: a conversation closed or pinned in the host, and what
//!   several windows show of the chat;
//! - [`host`]: the monitors' settings, the swap on a switch, what the
//!   cycling monitor asks.

pub mod bar;
pub mod behind;
pub mod chat;
pub mod chips;
pub mod copy;
pub mod export;
pub mod gpu;
pub mod host;
pub mod hotkeys;
pub mod identity;
pub mod inbox;
pub mod ipc;
pub mod managed;
pub mod monitor;
pub mod monitor_windows;
pub mod panels;
pub mod profile;
