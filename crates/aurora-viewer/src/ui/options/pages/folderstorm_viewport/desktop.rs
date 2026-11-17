//! Folderstorm: the margins' desktop in Preferences
//! (`crate::folderstorm::desktop`, FEATURES.md row 1.10): a group of
//! Workspaces › Margins, after *Background around the world*.

use super::super::super::cx::{Cx, field};
use super::super::folderstorm_search::words;
use crate::folderstorm::desktop::store::Size;
use crate::folderstorm::desktop::surface::size_label;
use aurora_i18n::tr;

/// *Desktop in the margins*: the shortcuts' switch, their size behind its
/// cog; greyed while the viewport is `off`.
pub(in crate::ui::options) fn group(cx: &mut Cx, off: bool) {
    cx.group(tr!("fs-desktop-group"), |cx| {
        cx.greyed(off);
        words(cx, &["fs-search-desktop"]).toggle(
            tr!("fs-desktop-shortcuts"),
            tr!("fs-desktop-shortcuts-help"),
            &field!(folderstorm_desktop.shortcuts_on),
        );
        let shortcuts_off = !cx.s.folderstorm_desktop.shortcuts_on;
        cx.subs(shortcuts_off, |cx| {
            let sizes = Size::ALL.map(|size| (size, size_label(size), false));
            words(cx, &["fs-search-desktop"]).choice(
                tr!("fs-desktop-size"),
                tr!("fs-desktop-size-help"),
                &field!(folderstorm_desktop.size),
                &sizes,
            );
        });
    });
}
