//! A managed viewer's window on the taskbar and in the Alt-Tab list
//! (session-host plan, checkpoint 8, "one taskbar button"). What the
//! window should be is the `folderstorm-session` crate's pure rule
//! (`bar::taskbar`); this file applies it to the viewer's **own** window,
//! in its own process, and remembers what it applied.
//!
//! On Windows:
//! - the **taskbar button** goes and comes back through winit's
//!   `set_skip_taskbar` (`ITaskbarList::DeleteTab` / `AddTab`; winit puts
//!   the choice back by itself when Explorer restarts);
//! - the **Alt-Tab list** follows the window's extended style: a tool
//!   window (`WS_EX_TOOLWINDOW`) is not listed. The bit alone is set: the
//!   window is never hidden, cloaked or given an owner (rule R2), and its
//!   frame is not asked to change. When the bit is taken away the frame is
//!   recalculated once, so that a caption Windows redrew in the meantime
//!   as a tool window's is the usual one again.
//! - winit rewrites a window's styles from its own flags when one of them
//!   changes (maximised, decorations…), which drops the bit: what is wanted
//!   is looked at again every [`RECHECK`] and put back.
//!
//! Elsewhere nothing is done: no system this viewer runs on has the same
//! notion, and the rule there is "nothing changes".
//!
//! Everything is put back by the same path: `bar::taskbar` answers "as
//! any window" as soon as the host is gone, the character is the active
//! one or the window has the keyboard, and [`Tracker::due`] then applies
//! it. A window that closes takes its state with it.

use ::folderstorm_session::bar::Taskbar;
use std::time::{Duration, Instant};
use winit::window::{Window, WindowId};

/// How often a window that is not as usual is looked at again.
pub const RECHECK: Duration = Duration::from_millis(500);

/// What to do to the window now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    /// Give the taskbar button back (true) or take it away; None: as it is.
    pub tab: Option<bool>,
    /// The window is in the Alt-Tab list (the tool-window bit is off).
    /// Idempotent: looked at again even when nothing was asked anew.
    pub switcher: bool,
}

/// What this viewer last made of its window.
#[derive(Debug, Default)]
pub struct Tracker {
    /// The window it was applied to (a GPU recovery makes a new one, which
    /// is as any window until told otherwise).
    window: Option<WindowId>,
    applied: Taskbar,
    checked: Option<Instant>,
}

impl Tracker {
    /// What is to be done so that `window` is `wanted`, if anything. A
    /// window that is as usual and wanted so is never touched: a viewer no
    /// host manages makes no call at all.
    pub fn due(&mut self, window: WindowId, wanted: Taskbar, now: Instant) -> Option<Change> {
        if self.window != Some(window) {
            // a new window: as the system made it
            self.window = Some(window);
            self.applied = Taskbar::NORMAL;
            self.checked = None;
        }
        if wanted != self.applied {
            let tab = (wanted.tab != self.applied.tab).then_some(wanted.tab);
            self.applied = wanted;
            self.checked = Some(now);
            return Some(Change {
                tab,
                switcher: wanted.switcher,
            });
        }
        // out of Alt-Tab: its style may have been rewritten since
        let again = !wanted.switcher && self.checked.is_none_or(|at| now.saturating_duration_since(at) >= RECHECK);
        again.then(|| {
            self.checked = Some(now);
            Change {
                tab: None,
                switcher: false,
            }
        })
    }

    /// What the window was last made.
    #[cfg(test)]
    pub fn applied(&self) -> Taskbar {
        self.applied
    }
}

/// Keeps `window` as `wanted`: applies what is due, if anything.
pub fn keep(window: &Window, tracker: &mut Tracker, wanted: Taskbar, now: Instant) {
    if let Some(change) = tracker.due(window.id(), wanted, now) {
        if change.tab.is_some() {
            log::info!(
                "session host: this window is {} the taskbar and {} Alt-Tab",
                if wanted.tab { "on" } else { "off" },
                if wanted.switcher { "in" } else { "out of" }
            );
        }
        apply(window, change);
    }
}

#[cfg(not(windows))]
fn apply(_window: &Window, _change: Change) {}

#[cfg(windows)]
fn apply(window: &Window, change: Change) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
        WS_EX_TOOLWINDOW,
    };
    use winit::platform::windows::WindowExtWindows;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    if let Some(tab) = change.tab {
        window.set_skip_taskbar(!tab);
    }
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as HWND;
    let tool = WS_EX_TOOLWINDOW as isize;
    // SAFETY: window manager calls on the window this thread created, with
    // plain values; a refused call returns zero and changes nothing.
    unsafe {
        let style = ex_style(hwnd, GWL_EXSTYLE);
        let wanted = if change.switcher { style & !tool } else { style | tool };
        if wanted == style {
            return;
        }
        set_ex_style(hwnd, GWL_EXSTYLE, wanted);
        if change.switcher {
            // back among the usual windows: the frame as its style says
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

/// `GetWindowLongPtrW` where the system has it (64 bits).
#[cfg(all(windows, target_pointer_width = "64"))]
unsafe fn ex_style(hwnd: windows_sys::Win32::Foundation::HWND, index: i32) -> isize {
    // SAFETY: the caller's (a window of this thread).
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, index) }
}

#[cfg(all(windows, target_pointer_width = "64"))]
unsafe fn set_ex_style(hwnd: windows_sys::Win32::Foundation::HWND, index: i32, style: isize) {
    // SAFETY: the caller's.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd, index, style);
    }
}

#[cfg(all(windows, target_pointer_width = "32"))]
unsafe fn ex_style(hwnd: windows_sys::Win32::Foundation::HWND, index: i32) -> isize {
    // SAFETY: the caller's.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongW(hwnd, index) as isize }
}

#[cfg(all(windows, target_pointer_width = "32"))]
unsafe fn set_ex_style(hwnd: windows_sys::Win32::Foundation::HWND, index: i32, style: isize) {
    // SAFETY: the caller's.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongW(hwnd, index, style as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFF: Taskbar = Taskbar {
        tab: false,
        switcher: false,
    };
    const BUTTON_ONLY: Taskbar = Taskbar {
        tab: false,
        switcher: true,
    };

    fn window(n: u64) -> WindowId {
        WindowId::from(n)
    }

    #[test]
    fn a_window_that_is_as_usual_and_wanted_so_is_never_touched() {
        let mut tracker = Tracker::default();
        let now = Instant::now();
        for step in 0..20 {
            assert_eq!(tracker.due(window(1), Taskbar::NORMAL, now + RECHECK * step), None);
        }
        assert_eq!(tracker.applied(), Taskbar::NORMAL);
    }

    #[test]
    fn a_change_is_applied_at_once_and_only_what_changed() {
        let mut tracker = Tracker::default();
        let now = Instant::now();
        // behind the active character: no button, out of Alt-Tab
        assert_eq!(
            tracker.due(window(1), OFF, now),
            Some(Change {
                tab: Some(false),
                switcher: false
            })
        );
        assert_eq!(tracker.due(window(1), OFF, now), None, "nothing new, looked at a moment ago");
        // about to be minimised (Economy): the style goes back, the button stays away
        assert_eq!(tracker.due(window(1), BUTTON_ONLY, now), Some(Change { tab: None, switcher: true }));
        // in front, or the host gone: everything back
        assert_eq!(
            tracker.due(window(1), Taskbar::NORMAL, now),
            Some(Change {
                tab: Some(true),
                switcher: true
            })
        );
        assert_eq!(tracker.due(window(1), Taskbar::NORMAL, now + RECHECK * 3), None);
    }

    #[test]
    fn a_window_out_of_alt_tab_is_looked_at_again_now_and_then() {
        let mut tracker = Tracker::default();
        let now = Instant::now();
        assert!(tracker.due(window(1), OFF, now).is_some());
        assert_eq!(tracker.due(window(1), OFF, now + RECHECK / 2), None);
        // (winit may have rewritten the window's styles: the bit is set again)
        assert_eq!(
            tracker.due(window(1), OFF, now + RECHECK),
            Some(Change {
                tab: None,
                switcher: false
            })
        );
        assert_eq!(tracker.due(window(1), OFF, now + RECHECK + RECHECK / 2), None);
        // one that only lost its button has nothing to look at again
        assert!(tracker.due(window(1), BUTTON_ONLY, now + RECHECK * 2).is_some());
        assert_eq!(tracker.due(window(1), BUTTON_ONLY, now + RECHECK * 9), None);
    }

    #[test]
    fn a_new_window_is_as_any_window_until_it_is_told_otherwise() {
        let mut tracker = Tracker::default();
        let now = Instant::now();
        assert!(tracker.due(window(1), OFF, now).is_some());
        // a GPU recovery made another window: what was applied to the old
        // one says nothing of it
        assert_eq!(
            tracker.due(window(2), OFF, now),
            Some(Change {
                tab: Some(false),
                switcher: false
            })
        );
        // and a new window wanted as usual is left alone
        assert_eq!(tracker.due(window(3), Taskbar::NORMAL, now), None);
        assert_eq!(tracker.applied(), Taskbar::NORMAL);
    }
}
