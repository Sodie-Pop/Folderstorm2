//! Folderstorm: who takes a press of the mouse in the margins that a custom
//! world viewport leaves (`crate::folderstorm::viewport`).
//!
//! The HUD attachments are drawn over the whole window, so one can be moved
//! out of the world into a margin. There, three things of ours are drawn
//! too, from the bottom up: the margins' background
//! (`crate::folderstorm::backgrounds`), under the HUD attachments unless
//! *HUDs over the background* is off, then the HUD attachments, then the
//! desktop's shortcuts (`crate::folderstorm::desktop`), each desktop being an
//! area over its whole room. A press goes to what is seen at its place:
//! a shortcut first, then a HUD the background does not hide, then the
//! room of the desktop. A desktop that took every press of its room left a
//! HUD moved into the margin there for good: it could be moved in, never
//! out again. The windows, the bars and the desktop's taskbar (an island of
//! the bars) above all of these are the interface's, decided before this.

/// What is drawn under the pointer, of what this decides between.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Under {
    /// A desktop shortcut (drawn over the HUD attachments).
    pub shortcut: bool,
    /// The surface of a HUD attachment.
    pub hud: bool,
    /// The margins' background, painted over the HUD attachments here
    /// (*HUDs over the background* off, the pointer in a margin): a HUD
    /// there is not seen.
    pub background_over_huds: bool,
    /// The room of a desktop (the margin around its shortcuts).
    pub room: bool,
}

/// Who takes a press at the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taker {
    /// The desktop: one of its shortcuts or its room.
    Desktop,
    /// The HUD attachment under the pointer: the world's press, as in the
    /// world viewport (a click, a touch, the HUD move gesture, the build
    /// tools).
    Hud,
    /// The background over the HUD attachments, with no desktop: nothing
    /// takes the press, and nothing hidden under the picture is clicked.
    Background,
    /// Nothing of the margins: the world's press, as before.
    World,
}

impl Taker {
    /// The press is the interface's, never the world's
    /// (`App::ui_takes_pointer`).
    pub fn is_interface(self) -> bool {
        matches!(self, Taker::Desktop | Taker::Background)
    }
}

/// Who takes a press over what is drawn there, `under`: what is seen on
/// top wins.
pub fn taker(under: Under) -> Taker {
    if under.shortcut {
        Taker::Desktop
    } else if under.hud && !under.background_over_huds {
        Taker::Hud
    } else if under.room {
        Taker::Desktop
    } else if under.hud {
        // (hidden under the picture: not to be grabbed blind)
        Taker::Background
    } else {
        Taker::World
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn under(shortcut: bool, hud: bool, background_over_huds: bool, room: bool) -> Under {
        Under {
            shortcut,
            hud,
            background_over_huds,
            room,
        }
    }

    #[test]
    fn a_hud_in_a_desktop_room_takes_the_press_and_the_room_keeps_the_rest() {
        // the report: moved into the margin, the HUD could not be moved back
        assert_eq!(taker(under(false, true, false, true)), Taker::Hud);
        assert_eq!(taker(under(false, false, false, true)), Taker::Desktop);
        assert!(Taker::Desktop.is_interface() && !Taker::Hud.is_interface());
    }

    #[test]
    fn a_shortcut_drawn_over_a_hud_keeps_its_press() {
        assert_eq!(taker(under(true, true, false, true)), Taker::Desktop);
        assert_eq!(taker(under(true, false, false, true)), Taker::Desktop);
        assert_eq!(taker(under(true, true, true, true)), Taker::Desktop);
    }

    #[test]
    fn a_hud_the_background_hides_takes_nothing() {
        assert_eq!(taker(under(false, true, true, true)), Taker::Desktop, "the room under the picture");
        assert_eq!(taker(under(false, true, true, false)), Taker::Background, "no desktop");
        assert!(Taker::Background.is_interface());
    }

    #[test]
    fn without_a_hud_or_a_desktop_the_press_is_the_worlds() {
        assert_eq!(taker(Under::default()), Taker::World);
        assert_eq!(taker(under(false, false, true, false)), Taker::World, "a bare margin, as before");
        assert_eq!(
            taker(under(false, true, false, false)),
            Taker::Hud,
            "a HUD in the world or a bare margin"
        );
        assert!(!Taker::World.is_interface());
    }
}
