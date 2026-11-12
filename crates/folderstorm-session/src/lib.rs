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

pub mod behind;
pub mod gpu;
pub mod profile;
