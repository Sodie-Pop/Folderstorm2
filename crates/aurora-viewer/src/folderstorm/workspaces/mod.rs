//! Folderstorm: layouts (FEATURES.md row 1.3; folderstorm/plans/workspaces.md).
//!
//! Folderstorm (C++) saved the placement of its bars around the viewport as
//! named profiles: apply, rename, delete, Reset (doc/help.md "Wide-screen
//! layout"), and kept window places in its workspaces. Here a **layout**
//! says where things go, in three optional parts: the viewer window (its
//! place and size, full screen or borderless over monitors, optionally
//! locked), the world viewport with the bars' place, and the position and
//! size of each window of an allowlist, with whether it was open: applying
//! a layout opens the windows it was saved with open (never an arbitrary
//! one: the allowlist, through the app's own way of opening each), and
//! closes none. A **workspace** says what is open ([`workspace`]): the
//! windows of the allowlist it has open, and optionally the layout applied
//! with it; applying it opens those windows and closes the others
//! (Conversations aside).
//!
//! - [`store`]: what `settings.json` keeps, its limits, the list.
//! - [`windows`]: the allowlist. [`workspace`]: the workspaces and their
//!   list.
//! - [`fit`]: a saved place in an interface of another size or scale.
//! - [`monitors`]: monitor setups, and a saved viewer window on the
//!   monitors of now.
//! - [`window`]: the viewer window as the app sees and drives it, and the
//!   steps that put it in place.
//! - [`viewer`]: the lock, the monitors' watch.
//! - [`capture`]: the arrangement of now.
//! - [`apply`]: applying, *Previous arrangement*, the "modified" mark.
//! - [`menu`]: World › Layouts. [`notice`]: the card when the monitors
//!   change.
//! - [`pick`]: which layout a quick switch means. [`preview`]: the picture
//!   of a layout before it is applied. [`switcher`]: the window to change
//!   layout from the keyboard. [`toolbar`]: the two toolbar commands.
//!   [`strip`]: the favorites on the row under the top bar.
//!   [`words`]: the layouts in words.
//!
//! A layout never holds what a window contains: no message, conversation,
//! Inventory folder or item, filter or selection has a field to go in.

pub mod apply;
pub mod capture;
pub mod fit;
pub mod menu;
pub mod monitors;
pub mod notice;
pub mod pick;
pub mod preview;
pub mod store;
pub mod strip;
pub mod switcher;
pub mod toolbar;
pub mod viewer;
pub mod window;
pub mod windows;
pub mod words;
pub mod workspace;

#[cfg(test)]
mod tests;

pub use apply::take_bare;
pub use store::Store;

/// The layouts' part of the interface's pass: the card shown when the
/// monitors changed, and the switcher. `hidden`: the interface is put away
/// or the camera is in mouselook; the card could not be clicked, so it is
/// not drawn and waits (the switcher is hidden with the other windows).
pub fn interface(ctx: &egui::Context, p: &crate::theme::Palette, hidden: bool) {
    apply::set_notice_hidden(ctx, hidden);
    strip::set_hidden(ctx, hidden);
    if !hidden {
        notice::show(ctx, p);
    }
    switcher::show(ctx, p);
}
