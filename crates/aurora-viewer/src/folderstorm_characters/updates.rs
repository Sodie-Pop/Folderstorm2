//! Updates with the session host (release plan, finding F6 and PR 3;
//! session-host plan, section 5 "Updater").
//!
//! The host and its characters all run from the same program folder. If
//! one character installed an update, the updater would move the files of
//! the host and of the other characters while they run, and restart that
//! character out of the host. So:
//!
//! - a viewer the host started (`--managed`) still checks for new versions
//!   and shows them, but never downloads or installs one: its update window
//!   has no "Install" button, and says why instead ([`held`], hooks in
//!   `updater.rs` and `ui/update.rs`);
//! - it tells the host the version it found (`Status::update`), and the
//!   host says "An update is ready. Close all characters to install it.",
//!   on its characters' page and in its settings' "Updates and support"
//!   ([`ready`], drawn by `host/ui`);
//! - a viewer not under the host is unchanged.
//!
//! "Quit all and update" was dropped by the human (session-host plan,
//! section 18): the host only says what to do.

use crate::theme::Palette;
use ::folderstorm_session::host::Desk;
use aurora_i18n::tr;
use egui::RichText;

#[cfg(test)]
thread_local! {
    /// A test's answer for [`held`], instead of the command line.
    static HELD: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// This viewer was started by the session host: it leaves the install of
/// an update to the user, once every character is closed. For the whole
/// life of the process, even after the host is gone: the other characters
/// may still run from the same folder.
pub fn held() -> bool {
    #[cfg(test)]
    if let Some(held) = HELD.with(std::cell::Cell::get) {
        return held;
    }
    crate::cli::get().managed
}

#[cfg(test)]
pub(crate) fn hold_for_test(held: Option<bool>) {
    HELD.with(|cell| cell.set(held));
}

/// What a managed viewer's update window says in place of its "Install"
/// button. Nothing in a viewer not under the host.
pub fn managed_note(ui: &mut egui::Ui, p: &Palette) {
    if held() {
        ui.add_space(4.0);
        ui.label(RichText::new(tr!("folderstorm-update-managed")).size(13.0).color(p.amber));
    }
}

/// The version a character's updater found, for its status to the host.
pub fn found(updater: &crate::updater::Updater) -> String {
    updater.release.as_ref().map(|r| r.version.clone()).unwrap_or_default()
}

/// The newest version a character told the host it found, if any.
pub fn ready(desk: &Desk) -> Option<String> {
    desk.slots()
        .filter_map(|(_, slot)| slot.status.as_ref())
        .map(|status| status.update.as_str())
        .filter(|version| !version.is_empty())
        .max_by(|a, b| {
            let (va, vb) = (aurora_updater::feed::version(a), aurora_updater::feed::version(b));
            va.cmp(&vb).then_with(|| a.cmp(b))
        })
        .map(str::to_owned)
}

/// The host's notice, on its characters' page.
pub fn notice(ui: &mut egui::Ui, p: &Palette, desk: &Desk) {
    if ready(desk).is_some() {
        ui.label(RichText::new(tr!("folderstorm-host-update-ready")).color(p.amber));
    }
}

/// The host's settings, "Updates and support": the version found, the
/// notice and how to install it.
pub fn host_section(ui: &mut egui::Ui, p: &Palette, desk: &Desk) {
    let Some(version) = ready(desk) else {
        return;
    };
    ui.label(
        RichText::new(tr!(
            "folderstorm-settings-update-version",
            version = version,
            current = aurora_version::version().to_owned()
        ))
        .strong()
        .color(p.ink),
    );
    ui.label(RichText::new(tr!("folderstorm-host-update-ready")).color(p.amber));
    ui.label(RichText::new(tr!("folderstorm-settings-update-how")).color(p.muted));
    ui.add_space(8.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::{State, Updater};
    use aurora_updater::feed::{Channel, Release};

    fn release(version: &str) -> Release {
        Release {
            tag: format!("v{version}"),
            version: version.into(),
            notes: "notes".into(),
            url: String::new(),
            archive: None,
            sha256: None,
            required: false,
            minimum: None,
            grace: std::time::Duration::ZERO,
            maintenance: Default::default(),
        }
    }

    fn offering(rt: &tokio::runtime::Runtime) -> Updater {
        let mut updater = Updater::new(rt.handle(), Channel::Stable);
        updater.enabled = true;
        updater.release = Some(release("0.4.0"));
        updater.state = State::Offer;
        updater.open = true;
        updater
    }

    /// The update window as the viewer draws it, its texts.
    fn window_texts(updater: &mut Updater, rt: &tokio::runtime::Runtime) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::theme::Theme::default().apply(&ctx, 1.0);
        let p = crate::theme::Theme::default().palette();
        let icons = crate::ui::icons::Icons::default();
        let mut settings = crate::settings::Settings::default();
        let mut texts = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(1280.0, 900.0))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                crate::ui::update::show_real(ui.ctx(), &p, &icons, updater, &mut settings, rt.handle());
            });
            output.textures_delta.clear();
            texts = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
        }
        texts
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime")
    }

    #[test]
    fn a_managed_viewer_shows_the_update_without_its_install_action() {
        let rt = runtime();
        hold_for_test(Some(true));
        let mut updater = offering(&rt);
        let shown = window_texts(&mut updater, &rt);
        assert!(shown.iter().any(|t| t == "0.4.0"), "the update is shown: {shown:?}");
        assert!(!shown.iter().any(|t| t == tr!("update-install")), "{shown:?}");
        assert!(shown.iter().any(|t| t == tr!("folderstorm-update-managed")), "{shown:?}");
        // the notice's "Update" (or Automatic mode) asks for the download:
        // nothing is downloaded, the window says why
        updater.open = false;
        updater.download(rt.handle());
        assert!(matches!(updater.state, State::Offer) && updater.open);
        // and nothing prepared is ever installed
        updater.restart();
        assert!(matches!(updater.state, State::Offer));
        assert_eq!(found(&updater), "0.4.0", "told to the host");
        hold_for_test(None);
    }

    #[test]
    fn a_viewer_not_under_the_host_is_unchanged() {
        let rt = runtime();
        hold_for_test(Some(false));
        let mut updater = offering(&rt);
        let shown = window_texts(&mut updater, &rt);
        assert!(shown.iter().any(|t| t == tr!("update-install")), "{shown:?}");
        assert!(!shown.iter().any(|t| t == tr!("folderstorm-update-managed")), "{shown:?}");
        // "Install" goes on as before (here: no package to install, said)
        updater.download(rt.handle());
        assert!(!matches!(updater.state, State::Offer));
        hold_for_test(None);
    }

    #[test]
    fn the_host_takes_the_newest_version_its_characters_found() {
        use ::folderstorm_session::ipc::{Screen, Status};
        let now = std::time::Instant::now();
        let mut desk = Desk::new(::folderstorm_session::host::Config::demo(3));
        assert_eq!(ready(&desk), None);
        for (slot, update) in ["0.9.0", "0.10.0", ""].into_iter().enumerate() {
            desk.launch(slot, slot == 0);
            desk.connected(slot, 1, now);
            let status = Status {
                screen: Screen::Login,
                update: update.into(),
                ..Default::default()
            };
            desk.status(slot, 1, status, now);
        }
        assert_eq!(ready(&desk).as_deref(), Some("0.10.0"));
    }
}
