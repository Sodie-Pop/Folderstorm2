//! Folderstorm: Preferences › Graphics › Frames per second tab, the group
//! "Other Folderstorm2 windows" (`crate::folderstorm_characters`,
//! session-host plan section 6.1): what this window draws while another
//! Folderstorm2 window is in front, and, in a named profile, whether it
//! shares the main profile's cache.

use super::super::cx::{Cx, field};
use ::folderstorm_session::behind::{self, Behind3d};

pub(super) fn group(cx: &mut Cx) {
    use aurora_i18n::tr;
    let product = folderstorm_brand::PRODUCT;
    cx.group(&tr!("folderstorm-characters", product = product), |cx| {
        cx.unfolded().combo(
            &tr!("folderstorm-behind-3d", product = product),
            &tr!("folderstorm-behind-3d-help", product = product),
            &field!(folderstorm_characters.behind_3d),
            &[
                (Behind3d::Off, tr!("folderstorm-behind-3d-off")),
                (Behind3d::Limited, tr!("folderstorm-behind-3d-limited")),
                (Behind3d::Normal, tr!("folderstorm-behind-3d-normal")),
            ],
        );
        let off = cx.s.folderstorm_characters.behind_3d == Behind3d::Normal;
        cx.subs(off, |cx| {
            cx.slider(
                tr!("folderstorm-behind-fps"),
                tr!("folderstorm-behind-fps-help"),
                &field!(folderstorm_characters.behind_fps),
                behind::FPS_MIN..=behind::FPS_MAX,
                tr!("status-fps"),
            );
        });
        // a named profile only: the main profile's cache is its own
        if crate::folderstorm_characters::profile::current().is_some() {
            cx.toggle(
                tr!("folderstorm-share-main-cache"),
                tr!("folderstorm-share-main-cache-help"),
                &field!(folderstorm_characters.share_main_cache),
            );
        }
    });
}
