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

pub mod behind;
pub mod chat;
pub mod export;
pub mod gpu;
pub mod host;
pub mod ipc;
pub mod managed;
pub mod profile;
