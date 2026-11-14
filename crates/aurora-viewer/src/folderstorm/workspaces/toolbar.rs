//! The layouts' two toolbar commands (folderstorm/plans/workspaces.md
//! section 9.4; checkpoint 5 in detail, point 5), as the hooks of
//! `ui/bars.rs` see them. Folderstorm (C++): "add Workspaces to a toolbar"
//! (doc/help.md "Workspaces").
//!
//! - `workspaces` shows the switcher or puts it away; lit while it is open.
//! - `layout_toggle` applies the one of its two layouts that is not in
//!   use; its tooltip names it. Until the two are chosen it is dimmed, says
//!   where to choose them, and its click opens Preferences there.
//!
//! Both are entries of `ui/toolbar/catalogue.rs`, so the Toolbar buttons
//! window lists them. The bars are drawn without the settings in reach:
//! a click leaves a request (`super::menu`), and the toggle's name comes
//! from the view the frame call publishes.

use super::menu::{self, Request};
use super::switcher;
use aurora_i18n::tr;

/// The command that opens the switcher. Named for what it will hold once
/// workspaces are in it (checkpoint 6), so that a saved toolbar stays valid.
pub const SWITCHER: &str = "workspaces";
/// The command that switches between the two chosen layouts.
pub const TOGGLE: &str = "layout_toggle";

/// Whether `command` is one of ours.
pub fn own(command: &str) -> bool {
    matches!(command, SWITCHER | TOGGLE)
}

/// A click on the button of `command` (the arm of `bars::run_command`).
pub fn run(ctx: &egui::Context, command: &str) {
    match command {
        SWITCHER => menu::ask(ctx, Request::Switcher),
        TOGGLE => menu::ask(ctx, Request::Toggle),
        _ => {}
    }
}

/// How the button of `command` looks (the line of `bars::toolbar_row`):
/// whether it is lit, whether it has something to do now, its tooltip.
/// Another command's look is given back as it came.
pub fn look(ctx: &egui::Context, command: &str, active: bool, usable: bool, tip: String) -> (bool, bool, String) {
    match command {
        SWITCHER => (switcher::is_open(ctx), usable, tip),
        TOGGLE => match menu::view(ctx).toggle.as_deref() {
            Some(name) => (false, usable, tr!("fs-switcher-toggle-to", name = name)),
            None => (false, false, format!("{tip}\n{}", tr!("fs-switcher-toggle-unset"))),
        },
        _ => (active, usable, tip),
    }
}
