//! Folderstorm: Preferences › Chat › Image uploads
//! (`crate::folderstorm::chat_upload`, FEATURES.md 3.3). Folderstorm (C++)
//! had them under Chat › Media › Uploads.
//!
//! The token is never part of the settings: "Save token" writes it to the
//! system's credential store for the host shown, at once, and the field is
//! emptied. The built-in folder is never shown, only whether the build has
//! one; a build never carries a token (`chat_upload::token`).

use super::super::cx::{Cx, Nothing, field};
use crate::folderstorm::chat_upload::{self, TokenStatus, settings, token};
use crate::ui::widgets;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};

/// What is typed in the token field, until it is saved. Never saved
/// elsewhere, and wiped once the field is no longer shown (`forget`).
static TYPED: Mutex<String> = Mutex::new(String::new());
/// The last frame the token field was drawn in.
static DRAWN: AtomicU64 = AtomicU64::new(0);
/// `forget` runs at the start of each frame of this context.
static WATCHING: Once = Once::new();

/// At the start of a frame: a token typed but not saved is wiped when its
/// field was not drawn in the previous frame (Preferences closed, another
/// page or another host shown).
fn forget(ui: &mut egui::Ui) {
    let ctx = ui.ctx();
    let mut typed = TYPED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !typed.is_empty() && !shown_last_frame(DRAWN.load(Ordering::Relaxed), ctx.cumulative_frame_nr()) {
        wipe(&mut typed);
    }
}

/// Whether a field drawn in frame `drawn` was still shown just before
/// frame `now`.
pub(in crate::ui::options) fn shown_last_frame(drawn: u64, now: u64) -> bool {
    drawn.saturating_add(1) >= now
}

/// Empties `text` after writing over its bytes, so the token does not stay
/// in the freed memory.
pub(in crate::ui::options) fn wipe(text: &mut String) {
    let len = text.len();
    text.clear();
    // within the capacity: the same bytes are written over
    text.extend(std::iter::repeat_n('\0', len));
    text.clear();
}

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
                &field!(folderstorm_chat_upload.folders),
                |ui, s| {
                    let upload = &mut s.folderstorm_chat_upload;
                    // the folder belongs to the host shown: another host has its own
                    let Some(origin) = upload.origin() else {
                        ui.label(egui::RichText::new(tr!("fs-chat-upload-host-invalid")).color(p.muted));
                        return false;
                    };
                    let mut folder = upload.folder_text(&origin).to_owned();
                    // a folder that takes uploads is as good as a key: masked
                    let changed = ui
                        .add(
                            egui::TextEdit::singleline(&mut folder)
                                .password(true)
                                .desired_width(super::super::COMBO_W)
                                .hint_text(tr!("fs-chat-upload-folder-placeholder")),
                        )
                        .changed();
                    if !folder.trim().is_empty() && !settings::valid_folder(folder.trim()) {
                        ui.label(egui::RichText::new(tr!("fs-chat-upload-folder-invalid")).color(p.danger));
                    }
                    if changed {
                        upload.set_folder(&origin, folder);
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
    WATCHING.call_once(|| ui.ctx().on_begin_pass("fs-chat-upload-token", std::sync::Arc::new(forget)));
    DRAWN.store(ui.ctx().cumulative_frame_nr(), Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    #[test]
    fn a_typed_token_is_wiped_once_its_field_is_gone() {
        // drawn in the frame before: kept; one frame or more missed: wiped
        assert!(super::shown_last_frame(9, 10));
        assert!(super::shown_last_frame(10, 10));
        assert!(!super::shown_last_frame(8, 10));
        assert!(super::shown_last_frame(u64::MAX, 3));
        let mut typed = "secret-token".to_owned();
        let capacity = typed.capacity();
        super::wipe(&mut typed);
        assert!(typed.is_empty());
        assert_eq!(typed.capacity(), capacity, "the same buffer, written over");
        // the hook wipes after a frame without the field
        let ctx = egui::Context::default();
        *super::TYPED.lock().expect("lock") = "unsaved".into();
        super::DRAWN.store(0, std::sync::atomic::Ordering::Relaxed);
        for _ in 0..3 {
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        }
        ctx.run_ui(egui::RawInput::default(), super::forget).textures_delta.clear();
        assert!(super::TYPED.lock().expect("lock").is_empty());
    }
}
