//! A managed viewer brings its window to the front when the session host
//! asks (`Show`, after the host's `AllowSetForegroundWindow`), and a
//! profile launched again brings its window forward (session-host plan,
//! checkpoints 2 and 4; follow-ups to checkpoint 7).
//!
//! winit's `focus_window` on Windows always sends an Alt press and release
//! before `SetForegroundWindow` (the Alt press lifts the foreground lock).
//! That release is a problem when the user still holds Alt: a hotkey such
//! as Ctrl+Alt+2 switched once, then Windows thought Alt was up although
//! the finger was still on it, so Ctrl+Alt+3 arrived as Ctrl+3 (the Build
//! tools' Edit key in the viewer that came forward) and every key had to
//! be released before the next switch.
//!
//! Here, on Windows:
//! 1. the window is asked to the front plainly: the host received the
//!    hotkey or the click, may set the foreground, and gave that right to
//!    this process, so this usually works with no key sent at all;
//! 2. only if it did not, the Alt press is sent, ending as the keyboard
//!    is: a held Alt is released and pressed again ([`alt_taps`]), so the
//!    user's keys stay as they are and no key stays held in either viewer
//!    (a viewer that loses the keyboard drops its held keys, `app.rs`;
//!    the one that takes it ignores winit's made-up presses and gets the
//!    real releases).
//!
//! Elsewhere winit's `focus_window` is used as before.

use winit::window::Window;

/// Brings the window to the front, with the keyboard.
pub fn bring_forward(window: &Window) {
    #[cfg(windows)]
    if imp::bring_forward(window) {
        return;
    }
    window.focus_window();
}

/// The Alt key events (true: pressed) sent to lift the foreground lock,
/// given whether the user holds Alt: a press, and the key left as the
/// user holds it.
pub fn alt_taps(alt_held: bool) -> [bool; 2] {
    if alt_held {
        // released, then pressed again: a press, and Alt still down
        [false, true]
    } else {
        // winit's own: pressed and released
        [true, false]
    }
}

#[cfg(windows)]
mod imp {
    use super::alt_taps;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC,
        MapVirtualKeyW, SendInput, VK_LMENU, VK_MENU, VK_RMENU,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic, IsWindowVisible, SetForegroundWindow};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// False when winit's way is to be used (no window handle, or a
    /// window hidden or minimised, which winit leaves as it is).
    pub fn bring_forward(window: &winit::window::Window) -> bool {
        let Ok(handle) = window.window_handle() else {
            return false;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return false;
        };
        let hwnd = handle.hwnd.get() as HWND;
        // SAFETY: window manager calls on the window this thread created,
        // and keyboard input built from plain values; a refused request
        // returns zero and changes nothing.
        unsafe {
            if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
                return false;
            }
            if GetForegroundWindow() == hwnd {
                return true;
            }
            // the right given by the host: no key sent
            if SetForegroundWindow(hwnd) != 0 && GetForegroundWindow() == hwnd {
                return true;
            }
            // (the high bit: the key is down now)
            let held = |vk: u16| GetAsyncKeyState(i32::from(vk)) < 0;
            let (vk, extended) = if held(VK_RMENU) {
                (VK_RMENU, KEYEVENTF_EXTENDEDKEY)
            } else if held(VK_LMENU) {
                (VK_LMENU, 0)
            } else {
                // as winit sends it
                (VK_LMENU, KEYEVENTF_EXTENDEDKEY)
            };
            let scan = MapVirtualKeyW(u32::from(VK_MENU), MAPVK_VK_TO_VSC) as u16;
            let inputs = alt_taps(held(VK_MENU)).map(|down| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: scan,
                        dwFlags: extended | if down { 0 } else { KEYEVENTF_KEYUP },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
            SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
            SetForegroundWindow(hwnd);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::alt_taps;

    /// The keys sent always hold a press of Alt (what lifts the lock), and
    /// leave Alt as the user holds it: held, it is still down; not held,
    /// it is up. A held Alt released by the window coming forward made
    /// the next hotkey with Alt a different key.
    #[test]
    fn the_alt_sent_ends_as_the_user_holds_it() {
        for held in [false, true] {
            let mut down = held;
            let mut pressed = false;
            for event in alt_taps(held) {
                pressed |= event && !down;
                down = event;
            }
            assert!(pressed, "a press of Alt (held: {held})");
            assert_eq!(down, held, "Alt is left as the user holds it");
        }
    }
}
