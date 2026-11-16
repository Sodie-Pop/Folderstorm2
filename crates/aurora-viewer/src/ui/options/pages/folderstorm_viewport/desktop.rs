//! Folderstorm: the margins' desktop in Preferences
//! (`crate::folderstorm::desktop`, FEATURES.md row 1.10): a group of
//! Layout › Bars and windows, after *Background around the world*.

use super::super::super::cx::{Cx, field};
use super::super::folderstorm_search::words;
use crate::folderstorm::desktop::store::Size;
use crate::folderstorm::desktop::surface::size_label;
use aurora_i18n::tr;

/// *Desktop in the margins*: the shortcuts' switch and their size.
pub(in crate::ui::options) fn group(cx: &mut Cx) {
    cx.group(tr!("fs-desktop-group"), |cx| {
        if !cx.s.folderstorm_viewport.enabled {
            cx.note(tr!("fs-desktop-intro"));
        }
        words(cx, &["fs-search-desktop"]).toggle(
            tr!("fs-desktop-shortcuts"),
            tr!("fs-desktop-shortcuts-help"),
            &field!(folderstorm_desktop.shortcuts_on),
        );
        let off = !cx.s.folderstorm_desktop.shortcuts_on;
        cx.greyed(off);
        let sizes = Size::ALL.map(|size| (size, size_label(size), false));
        words(cx, &["fs-search-desktop"]).choice(
            tr!("fs-desktop-size"),
            tr!("fs-desktop-size-help"),
            &field!(folderstorm_desktop.size),
            &sizes,
        );
        cx.greyed(false);
    });
}
