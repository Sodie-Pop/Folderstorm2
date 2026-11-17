//! Folderstorm: Preferences › Language and updates › *Folderstorm setup
//! backup* (`crate::folderstorm::backup`, FEATURES.md row 0.6), after
//! Aurora's own Backup group: Export…, Import…, the backups folder, the
//! daily automatic backup and the list of automatic backups with Restore,
//! and the import dialog.
//!
//! The work itself (dialogs, disk, the import) is `backup::jobs`'s; this
//! page draws its state and asks for it.

use super::super::cx::{Cx, Nothing, field};
use super::folderstorm_search::words;
use crate::folderstorm::backup::{self, Mode, apply::Plan, jobs};
use crate::settings::Settings;
use crate::theme::Palette;
use crate::ui::widgets::{ActionKind, action_button, action_button_if, checkbox, primary_button_sized, radio};
use aurora_i18n::tr;
use egui::{RichText, Vec2};

const BUTTON: Vec2 = Vec2::new(0.0, 26.0);

/// The group, with its rows.
pub(in crate::ui::options) fn group(cx: &mut Cx) {
    let p = cx.p;
    cx.group(tr!("fs-backup-group"), |cx| {
        cx.note(tr!("fs-backup-intro"));
        words(cx, &["fs-search-backup"]).row_with(tr!("fs-backup-export"), tr!("fs-backup-export-help"), &Nothing, |ui, s, _, _| {
            let ctx = ui.ctx().clone();
            let (busy, status) = jobs::with(&ctx, |st| (st.busy(), st.export_status.clone())).unwrap_or_default();
            ui.vertical(|ui| {
                if action_button_if(ui, &p, "file-download-filled", tr!("fs-backup-export-button"), ActionKind::Flat, BUTTON, !busy).clicked() {
                    jobs::export(&ctx, s);
                }
                status_line(ui, &p, &status);
            });
            false
        });
        words(cx, &["fs-search-backup"]).row_with(tr!("fs-backup-import"), tr!("fs-backup-import-help"), &Nothing, |ui, s, _, _| {
            let ctx = ui.ctx().clone();
            let (busy, status) = jobs::with(&ctx, |st| (st.busy(), st.import_status.clone())).unwrap_or_default();
            ui.vertical(|ui| {
                if action_button_if(ui, &p, "file-upload-filled", tr!("fs-backup-import-button"), ActionKind::Flat, BUTTON, !busy).clicked() {
                    jobs::import(&ctx);
                }
                status_line(ui, &p, &status);
            });
            dialog(&ctx, &p, s);
            false
        });
        words(cx, &["fs-search-backup"]).row_with(tr!("fs-backup-folder"), tr!("fs-backup-folder-help"), &Nothing, |ui, _, _, _| {
            if action_button(ui, &p, "folder-open-filled", tr!("fs-backup-folder-button"), ActionKind::Flat, BUTTON).clicked() {
                jobs::open_folder();
            }
            false
        });
        words(cx, &["fs-search-backup"]).toggle(tr!("fs-backup-daily"), tr!("fs-backup-daily-help"), &field!(folderstorm_backup.daily));
        let off = !cx.s.folderstorm_backup.daily;
        cx.subs(off, |cx| {
            cx.slider(
                tr!("fs-backup-keep"),
                tr!("fs-backup-keep-help"),
                &field!(folderstorm_backup.keep),
                backup::KEEP,
                "",
            );
        });
        words(cx, &["fs-search-backup"]).row_with(tr!("fs-backup-list"), tr!("fs-backup-list-help"), &Nothing, |ui, _, _, _| {
            let ctx = ui.ctx().clone();
            let busy = jobs::with(&ctx, |st| st.busy()).unwrap_or(false);
            let listed = jobs::list(&ctx);
            ui.vertical(|ui| {
                if listed.is_empty() {
                    status_line(ui, &p, tr!("fs-backup-list-empty"));
                }
                for entry in listed {
                    ui.horizontal(|ui| {
                        let who = entry.account.clone().unwrap_or_else(|| tr!("fs-backup-no-account-name").to_owned());
                        ui.label(
                            RichText::new(tr!(
                                "fs-backup-list-entry",
                                date = entry.created.as_str(),
                                kind = entry.kind.label(),
                                account = who.as_str(),
                                count = entry.sections
                            ))
                            .size(12.0)
                            .color(p.ink),
                        );
                        if action_button_if(ui, &p, "restore", tr!("fs-backup-restore-button"), ActionKind::Flat, Vec2::new(0.0, 22.0), !busy).clicked() {
                            jobs::restore(&ctx, entry.path.clone());
                        }
                    });
                }
            });
            false
        });
    });
}

fn status_line(ui: &mut egui::Ui, p: &Palette, text: &str) {
    if !text.is_empty() {
        ui.add(egui::Label::new(RichText::new(text).size(11.5).color(p.muted)).wrap());
    }
}

/// The import dialog, while a backup read waits for its confirmation.
fn dialog(ctx: &egui::Context, p: &Palette, s: &Settings) {
    let shared = jobs::state(ctx);
    let mut confirmed = false;
    let mut closed = false;
    {
        let Ok(mut st) = shared.lock() else { return };
        let Some(plan) = st.plan.as_mut() else { return };
        let modal = crate::ui::widgets::Modal::new(egui::Id::unique("folderstorm_backup_import")).show(ctx, |ui| {
            ui.set_width(460.0);
            let (ok, cancel) = body(ui, p, plan);
            confirmed |= ok;
            closed |= cancel;
        });
        closed |= modal.should_close();
        if closed && !confirmed {
            st.plan = None;
        }
    }
    if confirmed {
        jobs::confirm(ctx, s);
    }
}

/// The dialog's content; (Import, Cancel) clicked.
fn body(ui: &mut egui::Ui, p: &Palette, plan: &mut Plan) -> (bool, bool) {
    let title = if plan.restore { tr!("fs-backup-restore-title") } else { tr!("fs-backup-import-title") };
    ui.label(RichText::new(title).size(15.0).strong().color(p.ink));
    ui.add_space(6.0);
    let backup = plan.backup.clone();
    let manifest = &backup.manifest;
    let who = manifest
        .account
        .as_ref()
        .map_or_else(|| tr!("fs-backup-no-account-name").to_owned(), |a| a.name.clone());
    let from = tr!(
        "fs-backup-from",
        account = who.as_str(),
        date = backup.created(),
        version = manifest.viewer.as_str()
    );
    ui.label(RichText::new(from).size(12.5).color(p.muted));
    let warn = |ui: &mut egui::Ui, text: &str| {
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(p.warn)).wrap());
    };
    if backup.newer() {
        warn(ui, tr!("fs-backup-newer"));
    }
    let unknown: Vec<String> = backup.unknown().iter().map(|e| e.label.clone()).collect();
    if !unknown.is_empty() {
        warn(ui, &tr!("fs-backup-unknown", sections = unknown.join(", ")));
    }
    if plan.other_account() {
        let target = plan.target.as_ref().map(|a| a.name.clone()).unwrap_or_default();
        warn(ui, &tr!("fs-backup-other-account", account = who.as_str(), current = target.as_str()));
    } else if plan.target.is_none() && manifest.account.is_some() {
        warn(ui, tr!("fs-backup-not-logged-in"));
    }
    let refs = plan.inventory_refs();
    if refs > 0 && (plan.other_account() || plan.target.is_none()) {
        checkbox(ui, p, &mut plan.skip_inventory, tr!("fs-backup-skip-inventory", count = refs));
    }
    ui.add_space(8.0);
    if plan.choices.is_empty() {
        ui.label(RichText::new(tr!("fs-backup-nothing")).size(12.5).color(p.muted));
    }
    for choice in &mut plan.choices {
        ui.horizontal(|ui| {
            checkbox(ui, p, &mut choice.on, choice.label.as_str());
            if choice.merges && choice.on {
                ui.add_space(8.0);
                if radio(ui, p, choice.mode == Mode::Merge, tr!("fs-backup-merge")).clicked() {
                    choice.mode = Mode::Merge;
                }
                if radio(ui, p, choice.mode == Mode::Replace, tr!("fs-backup-replace")).clicked() {
                    choice.mode = Mode::Replace;
                }
            }
        });
    }
    ui.add_space(6.0);
    ui.add(egui::Label::new(RichText::new(tr!("fs-backup-merge-help")).size(11.5).color(p.muted)).wrap());
    ui.add(egui::Label::new(RichText::new(tr!("fs-backup-safety-note")).size(11.5).color(p.muted)).wrap());
    ui.add_space(10.0);
    let (mut ok, mut cancel) = (false, false);
    ui.horizontal(|ui| {
        ui.add_enabled_ui(plan.any(), |ui| {
            let label = if plan.restore { tr!("fs-backup-restore-confirm") } else { tr!("fs-backup-import-confirm") };
            ok = primary_button_sized(ui, p, label, Vec2::new(110.0, 26.0)).clicked();
        });
        cancel = action_button(ui, p, "x", tr!("common-cancel"), ActionKind::Flat, BUTTON).clicked();
    });
    (ok, cancel)
}
