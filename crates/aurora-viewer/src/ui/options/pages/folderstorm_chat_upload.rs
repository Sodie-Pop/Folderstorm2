//! Folderstorm: Preferences › Chat › Image uploads
//! (`crate::folderstorm::chat_upload`, FEATURES.md 3.3). Folderstorm (C++)
//! had them under Chat › Media › Uploads.
//!
//! The token is never part of the settings: "Save token" writes it to the
//! system's credential store for the host shown, at once, and the field is
//! emptied. The built-in token and folder are never shown, only whether the
//! build has them.

use super::super::cx::{Cx, Nothing, field};
use crate::folderstorm::chat_upload::{self, TokenStatus, settings, token};
use crate::ui::widgets;
use std::sync::Mutex;

/// What is typed in the token field, until it is saved. Never saved
/// elsewhere.
static TYPED: Mutex<String> = Mutex::new(String::new());

pub(in crate::ui::options) fn group(cx: &mut Cx) {
    use aurora_i18n::tr;
    let p = cx.p;
    cx.group(tr!("fs-chat-upload-group"), |cx| {
        cx.note(tr!("fs-chat-upload-intro"));
        cx.keywords("image upload Zipline picture paste").toggle(
            tr!("fs-chat-upload-enable"),
            tr!("fs-chat-upload-enable-help"),
            &field!(folderstorm_chat_upload.enabled),
        );
        let off = !cx.s.folderstorm_chat_upload.enabled;
        cx.subs(off, |cx| {
            cx.wide().row(
                tr!("fs-chat-upload-host"),
                tr!("fs-chat-upload-host-help"),
                &field!(folderstorm_chat_upload.host),
                |ui, s| {
                    let host = &mut s.folderstorm_chat_upload.host;
                    let changed = ui
                        .add(egui::TextEdit::singleline(host).desired_width(super::super::COMBO_W))
                        .changed();
                    if settings::origin(host).is_none() {
                        ui.label(egui::RichText::new(tr!("fs-chat-upload-host-invalid")).color(p.danger));
                    }
                    changed
                },
            );
            cx.wide().row(
                tr!("fs-chat-upload-folder"),
                tr!("fs-chat-upload-folder-help"),
                &field!(folderstorm_chat_upload.folder),
                |ui, s| {
                    let folder = &mut s.folderstorm_chat_upload.folder;
                    // a folder that takes uploads is as good as a key: masked
                    let changed = ui
                        .add(
                            egui::TextEdit::singleline(folder)
                                .password(true)
                                .desired_width(super::super::COMBO_W)
                                .hint_text(tr!("fs-chat-upload-folder-placeholder")),
                        )
                        .changed();
                    if !folder.trim().is_empty() && !settings::valid_folder(folder.trim()) {
                        ui.label(egui::RichText::new(tr!("fs-chat-upload-folder-invalid")).color(p.danger));
                    }
                    changed
                },
            );
            cx.row(
                tr!("fs-chat-upload-builtin-folder"),
                tr!("fs-chat-upload-builtin-folder-help"),
                &Nothing,
                |ui, _| {
                    present(ui, &p, settings::built_in_folder().is_some());
                    false
                },
            );
            cx.wide()
                .row(tr!("fs-chat-upload-token"), tr!("fs-chat-upload-token-help"), &Nothing, |ui, s| {
                    token_row(ui, &p, &s.folderstorm_chat_upload);
                    false
                });
            cx.row(
                tr!("fs-chat-upload-builtin-token"),
                tr!("fs-chat-upload-builtin-token-help"),
                &Nothing,
                |ui, _| {
                    present(ui, &p, token::built_in().is_some());
                    false
                },
            );
            cx.slider(
                tr!("fs-chat-upload-max"),
                tr!("fs-chat-upload-max-help"),
                &field!(folderstorm_chat_upload.max_mib),
                1..=settings::MAX_MIB_LIMIT,
                aurora_i18n::tr!("memory-mb-suffix"),
            );
            cx.toggle(
                tr!("fs-chat-upload-confirm"),
                tr!("fs-chat-upload-confirm-help"),
                &field!(folderstorm_chat_upload.confirm),
            );
        });
        cx.note(tr!("fs-chat-upload-public"));
    });
}

fn present(ui: &mut egui::Ui, p: &crate::theme::Palette, yes: bool) {
    use aurora_i18n::tr;
    let (text, color) = if yes {
        (tr!("fs-chat-upload-present"), p.success)
    } else {
        (tr!("fs-chat-upload-absent"), p.muted)
    };
    ui.label(egui::RichText::new(text).color(color));
}

/// Whether a token is saved for the host shown, the masked field, Save
/// token and Remove token.
fn token_row(ui: &mut egui::Ui, p: &crate::theme::Palette, s: &settings::UploadSettings) {
    use aurora_i18n::tr;
    let Some(origin) = s.origin() else {
        ui.label(egui::RichText::new(tr!("fs-chat-upload-host-invalid")).color(p.muted));
        return;
    };
    let status = chat_upload::token_status(ui.ctx(), &origin);
    ui.vertical(|ui| {
        let (text, color) = match status {
            TokenStatus::Checking => (tr!("fs-chat-upload-token-checking"), p.muted),
            TokenStatus::Saved => (tr!("fs-chat-upload-token-saved"), p.success),
            TokenStatus::Absent => (tr!("fs-chat-upload-token-absent"), p.muted),
            TokenStatus::Failed => (tr!("fs-chat-upload-token-failed"), p.danger),
        };
        ui.label(egui::RichText::new(text).color(color));
        ui.horizontal(|ui| {
            let mut typed = TYPED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            ui.add(
                egui::TextEdit::singleline(&mut *typed)
                    .password(true)
                    .desired_width(super::super::COMBO_W)
                    .hint_text(tr!("fs-chat-upload-token-placeholder")),
            );
            let size = egui::Vec2::new(0.0, 24.0);
            let can_save = token::usable(&typed) && status != TokenStatus::Checking;
            if widgets::action_button_if(
                ui,
                p,
                "device-floppy-filled",
                tr!("fs-chat-upload-token-save"),
                widgets::ActionKind::Flat,
                size,
                can_save,
            )
            .clicked()
            {
                chat_upload::set_token(ui.ctx(), &origin, Some(std::mem::take(&mut *typed)));
            }
            if widgets::action_button_if(
                ui,
                p,
                "trash-filled",
                tr!("fs-chat-upload-token-remove"),
                widgets::ActionKind::Danger,
                size,
                status == TokenStatus::Saved,
            )
            .clicked()
            {
                chat_upload::set_token(ui.ctx(), &origin, None);
            }
        });
    });
}
