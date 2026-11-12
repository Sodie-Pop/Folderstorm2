//! The session host's global hotkeys, registered with the system
//! (session-host plan, section 13, checkpoint 4). Which keys and what is
//! wrong with a choice are `folderstorm_session::hotkeys`'s rules; this
//! module asks Windows for the keys and hears them pressed.
//!
//! **Windows only.** `RegisterHotKey` without a window: the key's message
//! (`WM_HOTKEY`) is posted to the thread that registered it, the host's
//! main thread, whichever program has the keyboard; winit's message hook
//! (as `raw_mouse.rs` uses it) sees it before winit drops it. Elsewhere
//! nothing is registered and every hotkey reads "not available": X11 would
//! need a key grab on the root window and its own event connection, and a
//! Wayland program cannot take a key from the others at all.
//!
//! A key that the system refuses (another program holds it), and one that
//! would take a character from the keyboard (AltGr is Ctrl+Alt), are told
//! to the user beside the hotkey, never dropped silently.

use ::folderstorm_session::hotkeys::{ACTIONS, Action, Config, Outcome};
use crossbeam_channel::{Receiver, Sender};

/// The hotkeys the host holds, and what was pressed.
pub struct Registrar {
    pressed: Receiver<Action>,
    /// What the message hook sends on (kept so that the channel lives
    /// where there is no hook).
    _hook: Sender<Action>,
    /// The actions whose hotkey the system holds for this thread.
    held: Vec<Action>,
    outcomes: [Option<Outcome>; ACTIONS],
}

impl Default for Registrar {
    /// No hook: nothing is ever pressed (the tests, and the systems
    /// without global hotkeys).
    fn default() -> Self {
        let (hook, pressed) = crossbeam_channel::unbounded();
        Self {
            pressed,
            _hook: hook,
            held: Vec::new(),
            outcomes: [None; ACTIONS],
        }
    }
}

impl Registrar {
    /// The registrar of the event loop being built: on Windows its message
    /// hook hears the hotkeys. To be called, like every method here, on
    /// the thread that will run the loop.
    pub fn install(builder: &mut winit::event_loop::EventLoopBuilder<()>) -> Self {
        let registrar = Self::default();
        #[cfg(all(windows, not(test)))]
        imp::hook(builder, registrar._hook.clone());
        #[cfg(any(not(windows), test))]
        let _ = builder;
        registrar
    }

    /// Whether this system has global hotkeys.
    pub fn available() -> bool {
        cfg!(all(windows, not(test)))
    }

    /// Registers the hotkeys of `config` in place of those held. What the
    /// system answered for each is kept for [`Registrar::outcome`].
    pub fn apply(&mut self, config: &Config) {
        for action in self.held.drain(..) {
            imp::unregister(action);
        }
        self.outcomes = [None; ACTIONS];
        for (action, chord) in config.bindings() {
            let outcome = imp::register(action, chord);
            match outcome {
                Outcome::Registered => self.held.push(action),
                Outcome::Taken => log::warn!("session host: the hotkey {chord} was refused by the system (held by another program?)"),
                Outcome::TypesCharacter => {
                    log::warn!("session host: the hotkey {chord} types a character on this keyboard and was not taken");
                }
                Outcome::Unavailable => {}
            }
            self.outcomes[action.index()] = Some(outcome);
        }
        if !self.held.is_empty() {
            log::info!("session host: {} global hotkeys registered", self.held.len());
        }
    }

    /// What became of an action's hotkey at the last [`Registrar::apply`].
    pub fn outcome(&self, action: Action) -> Option<Outcome> {
        self.outcomes[action.index()]
    }

    /// The hotkeys pressed since the last call.
    pub fn pressed(&self) -> Vec<Action> {
        self.pressed.try_iter().collect()
    }
}

impl Drop for Registrar {
    fn drop(&mut self) {
        for action in self.held.drain(..) {
            imp::unregister(action);
        }
    }
}

// (a test never takes a key from the user's programs: the tests run with
// the other systems' stand-in)
#[cfg(all(windows, not(test)))]
mod imp {
    use super::{Action, Outcome, Sender};
    use ::folderstorm_session::hotkeys::{self, Chord};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MapVirtualKeyExW, RegisterHotKey, ToUnicodeEx,
        UnregisterHotKey, VK_CONTROL, VK_MENU, VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, WM_HOTKEY};

    // the numbers `hotkeys::Chord::modifiers` gives are the system's
    const _: () = assert!(hotkeys::MOD_ALT == MOD_ALT && hotkeys::MOD_CONTROL == MOD_CONTROL && hotkeys::MOD_SHIFT == MOD_SHIFT);

    /// Hears `WM_HOTKEY` on the loop's thread and sends its action on.
    pub fn hook(builder: &mut winit::event_loop::EventLoopBuilder<()>, pressed: Sender<Action>) {
        use winit::platform::windows::EventLoopBuilderExtWindows;

        builder.with_msg_hook(move |msg| {
            // SAFETY: winit gives the `MSG` it has just taken from the
            // queue, alive for the duration of the call.
            let msg = unsafe { &*msg.cast::<MSG>() };
            if msg.message != WM_HOTKEY || !msg.hwnd.is_null() {
                // winit handles the message as usual
                return false;
            }
            // (a number that is not ours is dropped: nothing else on this
            // thread registers a hotkey without a window)
            if let Some(action) = i64::try_from(msg.wParam).ok().and_then(Action::from_id) {
                let _ = pressed.send(action);
            }
            // a message for the thread, not for a window: nothing to dispatch
            true
        });
    }

    pub fn register(action: Action, chord: Chord) -> Outcome {
        if types_character(chord) {
            return Outcome::TypesCharacter;
        }
        // SAFETY: no pointer but the null window, which asks for the
        // message on this thread's queue; a refusal is the answer zero.
        let registered = unsafe { RegisterHotKey(std::ptr::null_mut(), action.id(), chord.modifiers() | MOD_NOREPEAT, chord.vk()) };
        if registered == 0 {
            log::info!("session host: RegisterHotKey refused {chord} ({})", std::io::Error::last_os_error());
            return Outcome::Taken;
        }
        Outcome::Registered
    }

    pub fn unregister(action: Action) {
        // SAFETY: no pointer but the null window, as in `register`; a
        // number that is not registered is only refused.
        unsafe {
            UnregisterHotKey(std::ptr::null_mut(), action.id());
        }
    }

    /// Whether Ctrl+Alt with that key types a character on the keyboard
    /// layout in use: on a keyboard with an AltGr key, AltGr is Ctrl+Alt,
    /// and a hotkey on it would take that character from every program
    /// (`#`, `{`, `[` and `@` on many European layouts).
    fn types_character(chord: Chord) -> bool {
        if !chord.may_be_altgr() {
            return false;
        }
        let mut keys = [0u8; 256];
        keys[usize::from(VK_CONTROL)] = 0x80;
        keys[usize::from(VK_MENU)] = 0x80;
        if chord.shift {
            keys[usize::from(VK_SHIFT)] = 0x80;
        }
        let mut typed = [0u16; 8];
        // bit 2: the keyboard's own state (a pending dead key) is left as
        // it is (Windows 10 version 1607 and later)
        const KEEP_STATE: u32 = 1 << 2;
        // SAFETY: `keys` is the 256-byte key state the call reads, `typed`
        // a buffer of the length given; the layout handle is this
        // thread's own, valid for the calls.
        let count = unsafe {
            let layout = GetKeyboardLayout(0);
            let scan = MapVirtualKeyExW(chord.vk(), MAPVK_VK_TO_VSC, layout);
            ToUnicodeEx(
                chord.vk(),
                scan,
                keys.as_ptr(),
                typed.as_mut_ptr(),
                typed.len() as i32,
                KEEP_STATE,
                layout,
            )
        };
        // a character, or a dead key (negative)
        count != 0
    }
}

#[cfg(any(not(windows), test))]
mod imp {
    use super::{Action, Outcome};
    use ::folderstorm_session::hotkeys::Chord;

    pub fn register(_action: Action, _chord: Chord) -> Outcome {
        Outcome::Unavailable
    }

    pub fn unregister(_action: Action) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::folderstorm_session::hotkeys::{Chord, Key, State, Wanted};

    #[test]
    fn without_a_hook_nothing_is_pressed_and_every_hotkey_has_an_answer() {
        let mut registrar = Registrar::default();
        assert!(registrar.pressed().is_empty());
        assert_eq!(registrar.outcome(Action::Next), None);
        let config = Config::default();
        assert!(!Registrar::available(), "a test never takes a key from the user's programs");
        registrar.apply(&config);
        let plan = config.plan();
        for action in Action::all() {
            let state = State::of(&plan[action.index()], config.enabled, registrar.outcome(action));
            assert_eq!(state, State::Unavailable, "{action:?}");
        }
        // disabled: nothing is asked of the system
        registrar.apply(&Config { enabled: false, ..config });
        assert!(Action::all().all(|action| registrar.outcome(action).is_none()));
        assert!(matches!(plan[0], Wanted::Chord(_)));
    }

    /// The physical key of a hotkey, as the viewer's bindings name it.
    fn code(key: Key) -> winit::keyboard::KeyCode {
        use winit::keyboard::KeyCode as K;
        const DIGITS: [K; 10] = [
            K::Digit0,
            K::Digit1,
            K::Digit2,
            K::Digit3,
            K::Digit4,
            K::Digit5,
            K::Digit6,
            K::Digit7,
            K::Digit8,
            K::Digit9,
        ];
        const FUNCTIONS: [K; 12] = [
            K::F1,
            K::F2,
            K::F3,
            K::F4,
            K::F5,
            K::F6,
            K::F7,
            K::F8,
            K::F9,
            K::F10,
            K::F11,
            K::F12,
        ];
        match key {
            Key::Digit(d) => DIGITS[usize::from(d)],
            Key::Function(n @ 1..=12) => FUNCTIONS[usize::from(n) - 1],
            Key::PageUp => K::PageUp,
            Key::PageDown => K::PageDown,
            other => panic!("no key code for {other} in this test"),
        }
    }

    /// The viewer's default binding a hotkey would take from it while the
    /// host holds the key: a press bound to exactly those keys, or a held
    /// action (walking, the camera) that those keys would start.
    fn taken_from_the_viewer(chord: Chord) -> Option<crate::keybinds::Action> {
        use crate::keybinds::{Input, KeyBindings, Mods, all_actions};
        let bindings = KeyBindings::default();
        let input = Input::key(code(chord.key));
        let mods = Mods {
            ctrl: chord.ctrl,
            shift: chord.shift,
            alt: chord.alt,
        };
        let down = std::collections::HashSet::from([input.clone()]);
        // (the HUD gesture is Ctrl itself, held with any key: not a key's)
        bindings.triggered(&input, mods).or_else(|| {
            all_actions()
                .filter(|action| *action != crate::keybinds::Action::HudDrag)
                .find(|action| bindings.held(*action, &down, mods))
        })
    }

    /// The default hotkeys are keys of none of the viewer's default
    /// bindings: holding them in every program takes nothing from a
    /// character's own window. (The Build tools' fixed keys, Ctrl+1 to
    /// Ctrl+5, B, D, L…, and the menus' Ctrl+Shift+digit consoles use no
    /// function key either.)
    #[test]
    fn the_default_hotkeys_take_no_key_of_the_viewer() {
        for (action, chord) in Config::default().bindings() {
            assert_eq!(taken_from_the_viewer(chord), None, "{action:?}: {chord}");
            assert!(matches!(chord.key, Key::Function(_)), "{chord}");
            assert!(chord.ctrl && chord.shift && !chord.alt, "{chord}");
        }
        // the check sees a clash: checkpoint 4's Ctrl+Alt+PageUp turned the
        // camera (Alt+PageUp), Ctrl+Shift+1 is the Performance window
        let first = |text: &str| Chord::parse(text).expect("a hotkey");
        assert!(taken_from_the_viewer(first("Ctrl+Alt+PageUp")).is_some());
        assert!(taken_from_the_viewer(first("Ctrl+Alt+PageDown")).is_some());
        assert!(taken_from_the_viewer(first("Ctrl+Shift+1")).is_some());
    }
}
