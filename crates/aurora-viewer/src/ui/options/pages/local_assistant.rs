//! Folderstorm: Preferences › Privacy › Local assistant (FEATURES.md 4.1,
//! 4.4 and 4.5; Folderstorm (C++)'s panel_preferences_local_assistant.xml and
//! doc/plans/assistant-*.md). Access, "this session only", the permission
//! presets, the bridge's state and recent activity, and the Setup rows.
//! The token is never shown nor copied.

use super::super::cx::{Cx, Nothing, field};
use super::super::{segmented, toggle};
use crate::mcp_bridge::{self, setup};
use crate::ui::widgets;
use folderstorm_mcp_bridge::server::Outcome;
use folderstorm_mcp_bridge::{Class, Level, LocalAssistant, Preset};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// "Custom" chosen while the levels still match a preset: the individual
/// rows stay open (choosing Custom changes no level).
static CUSTOM_OPEN: AtomicBool = AtomicBool::new(false);
/// The assistant picked in Setup.
static CLIENT: AtomicU8 = AtomicU8::new(0);
/// "Copy configuration" was used since the page opened.
static COPIED: AtomicBool = AtomicBool::new(false);

/// Recent activity entries shown.
const SHOWN_ACTIVITY: usize = 5;

fn client() -> setup::Client {
    match CLIENT.load(Ordering::Relaxed) {
        1 => setup::Client::Cursor,
        2 => setup::Client::Codex,
        _ => setup::Client::ClaudeCode,
    }
}

fn set_client(client: setup::Client) {
    let n = match client {
        setup::Client::ClaudeCode => 0,
        setup::Client::Cursor => 1,
        setup::Client::Codex => 2,
    };
    CLIENT.store(n, Ordering::Relaxed);
}

/// Store new settings where they are in force: the session's copy, or the
/// saved settings (true: the app saves them).
fn store(s: &mut crate::settings::Settings, session: Option<LocalAssistant>, new: LocalAssistant) -> bool {
    if session.is_some() {
        mcp_bridge::set_session(Some(new));
        false
    } else {
        let changed = s.local_assistant != new;
        s.local_assistant = new;
        changed
    }
}

pub(in crate::ui::options) fn group(cx: &mut Cx) {
    use aurora_i18n::tr;
    cx.group(tr!("fs-mcp-group"), |cx| {
        cx.note(tr!("fs-mcp-intro"));
        let p = cx.p;
        let session = mcp_bridge::session();
        let current = session.unwrap_or(cx.s.local_assistant);

        // access, saved or for this session only
        if session.is_none() {
            cx.keywords("MCP assistant --mcp-api")
                .toggle(tr!("fs-mcp-allow"), tr!("fs-mcp-allow-hint"), &field!(local_assistant.enabled));
        } else {
            cx.keywords("MCP assistant --mcp-api")
                .row(tr!("fs-mcp-allow"), tr!("fs-mcp-allow-hint"), &Nothing, |ui, s| {
                    let mut new = current;
                    if toggle(ui, &p, &mut new.enabled) {
                        store(s, session, new);
                    }
                    false
                });
        }
        cx.row(tr!("fs-mcp-session-only"), tr!("fs-mcp-session-only-hint"), &Nothing, |ui, s| {
            let mut only = session.is_some();
            if toggle(ui, &p, &mut only) {
                mcp_bridge::set_session(only.then_some(s.local_assistant));
            }
            false
        });

        // the presets; Custom opens the individual levels
        let derived = current.permissions.preset();
        let shown = if CUSTOM_OPEN.load(Ordering::Relaxed) {
            Preset::Custom
        } else {
            derived
        };
        let choices = [
            (Preset::ReadOnly, tr!("fs-mcp-preset-read-only"), false),
            (Preset::AskBeforeChanges, tr!("fs-mcp-preset-ask"), false),
            (Preset::Custom, tr!("fs-mcp-preset-custom"), false),
        ];
        let saved_permissions = field!(local_assistant.permissions);
        let track: &dyn super::super::cx::Track = if session.is_none() { &saved_permissions } else { &Nothing };
        cx.wide()
            .row(tr!("fs-mcp-permissions"), tr!("fs-mcp-permissions-hint"), track, |ui, s| {
                let mut preset = shown;
                if !segmented(ui, &p, &mut preset, &choices) || preset == shown {
                    return false;
                }
                CUSTOM_OPEN.store(preset == Preset::Custom, Ordering::Relaxed);
                if preset == Preset::Custom {
                    return false;
                }
                store(
                    s,
                    session,
                    LocalAssistant {
                        permissions: preset.permissions(),
                        ..current
                    },
                )
            });
        cx.conditional(shown == Preset::Custom, |cx| {
            let rows = [
                (
                    Class::Read,
                    tr!("fs-mcp-read"),
                    tr!("fs-mcp-read-hint"),
                    field!(local_assistant.permissions.read),
                ),
                (
                    Class::Edit,
                    tr!("fs-mcp-edit"),
                    tr!("fs-mcp-edit-hint"),
                    field!(local_assistant.permissions.edit),
                ),
                (
                    Class::Move,
                    tr!("fs-mcp-move"),
                    tr!("fs-mcp-move-hint"),
                    field!(local_assistant.permissions.move_),
                ),
                (
                    Class::Wear,
                    tr!("fs-mcp-wear"),
                    tr!("fs-mcp-wear-hint"),
                    field!(local_assistant.permissions.wear),
                ),
                (
                    Class::Trash,
                    tr!("fs-mcp-trash"),
                    tr!("fs-mcp-trash-hint"),
                    field!(local_assistant.permissions.trash),
                ),
                (
                    Class::Camera,
                    tr!("fs-mcp-camera"),
                    tr!("fs-mcp-camera-hint"),
                    field!(local_assistant.permissions.camera),
                ),
            ];
            for (class, label, hint, saved) in rows {
                let mut levels = vec![(Level::Allow, tr!("fs-mcp-level-allow"), false)];
                if class.can_ask() {
                    levels.push((Level::Ask, tr!("fs-mcp-level-ask"), false));
                }
                levels.push((Level::Never, tr!("fs-mcp-level-never"), false));
                let track: &dyn super::super::cx::Track = if session.is_none() { &saved } else { &Nothing };
                cx.row(label, hint, track, |ui, s| {
                    let mut level = current.permissions.level(class);
                    if !segmented(ui, &p, &mut level, &levels) {
                        return false;
                    }
                    let mut new = current;
                    new.permissions.set(class, level);
                    store(s, session, new)
                });
            }
        });
        cx.note(tr!("fs-mcp-changes-note"));

        // what the bridge does now
        let status = mcp_bridge::status();
        cx.row(tr!("fs-mcp-status"), tr!("fs-mcp-status-hint"), &Nothing, |ui, _| {
            let (text, color) = match (status.port, &status.failure) {
                (Some(port), _) => (tr!("fs-mcp-status-ready", port = port.to_string()), p.success),
                (None, Some(reason)) if current.enabled => (tr!("fs-mcp-status-failed", reason = reason.clone()), p.danger),
                _ => (tr!("fs-mcp-status-off").to_owned(), p.muted),
            };
            ui.label(egui::RichText::new(text).size(12.5).color(color));
            false
        });
        // the label above, one entry per line across the whole row (in the
        // control column each entry wrapped over several lines)
        cx.below()
            .row(tr!("fs-mcp-activity"), tr!("fs-mcp-activity-hint"), &Nothing, |ui, _| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    if status.activity.is_empty() {
                        ui.label(egui::RichText::new(tr!("fs-mcp-activity-none")).size(12.0).color(p.muted));
                    }
                    let now = std::time::SystemTime::now();
                    for entry in status.activity.iter().rev().take(SHOWN_ACTIVITY) {
                        let outcome = match entry.outcome {
                            Outcome::Done => tr!("fs-mcp-outcome-done"),
                            Outcome::Refused => tr!("fs-mcp-outcome-refused"),
                            Outcome::Failed(_) => tr!("fs-mcp-outcome-failed"),
                        };
                        let seconds = now.duration_since(entry.at).map_or(0, |d| d.as_secs());
                        let ago = if seconds < 60 {
                            tr!("fs-mcp-ago-seconds", count = seconds)
                        } else {
                            tr!("fs-mcp-ago-minutes", count = seconds / 60)
                        };
                        let line = tr!(
                            "fs-mcp-activity-entry",
                            tool = entry.tool.clone(),
                            outcome = outcome.to_owned(),
                            ago = ago
                        );
                        ui.add(egui::Label::new(egui::RichText::new(line).size(12.0).color(p.ink)).truncate());
                    }
                });
                false
            });

        // Setup: the entry for an assistant's MCP settings, and a check
        let clients = [
            (setup::Client::ClaudeCode, "Claude Code", false),
            (setup::Client::Cursor, "Cursor", false),
            (setup::Client::Codex, "Codex", false),
        ];
        cx.keywords("Claude Code Cursor Codex MCP")
            .row(tr!("fs-mcp-setup-client"), tr!("fs-mcp-setup-client-hint"), &Nothing, |ui, _| {
                let mut chosen = client();
                if segmented(ui, &p, &mut chosen, &clients) {
                    set_client(chosen);
                    COPIED.store(false, Ordering::Relaxed);
                }
                false
            });
        let sidecar = setup::sidecar_path();
        let present = sidecar.as_ref().is_some_and(|path| path.is_file());
        cx.wide()
            .row(tr!("fs-mcp-setup-copy"), tr!("fs-mcp-setup-copy-hint"), &Nothing, |ui, _| {
                ui.horizontal_wrapped(|ui| {
                    let copy = widgets::action_button_if(
                        ui,
                        &p,
                        "copy",
                        tr!("fs-mcp-setup-copy"),
                        widgets::ActionKind::Flat,
                        egui::Vec2::new(0.0, 24.0),
                        sidecar.is_some(),
                    );
                    if copy.clicked()
                        && let Some(path) = &sidecar
                    {
                        ui.ctx()
                            .copy_text(setup::configuration(client(), path, &mcp_bridge::discovery_dir()));
                        COPIED.store(true, Ordering::Relaxed);
                    }
                    let note = if !present {
                        Some((tr!("fs-mcp-setup-missing"), p.warn))
                    } else if COPIED.load(Ordering::Relaxed) {
                        Some((tr!("fs-mcp-setup-copied"), p.muted))
                    } else {
                        None
                    };
                    if let Some((text, color)) = note {
                        ui.label(egui::RichText::new(text).size(12.0).color(color));
                    }
                });
                false
            });
        let running = status.port.is_some();
        cx.wide().row(tr!("fs-mcp-check"), tr!("fs-mcp-check-hint"), &Nothing, |ui, _| {
            ui.horizontal_wrapped(|ui| {
                let check = setup::check();
                let busy = check == setup::Check::Running;
                let clicked = widgets::action_button_if(
                    ui,
                    &p,
                    "refresh",
                    tr!("fs-mcp-check"),
                    widgets::ActionKind::Flat,
                    egui::Vec2::new(0.0, 24.0),
                    running && !busy,
                )
                .clicked();
                if clicked {
                    setup::start_check(mcp_bridge::discovery_dir());
                }
                let (text, color) = match &check {
                    _ if !running => (tr!("fs-mcp-check-off").to_owned(), p.muted),
                    setup::Check::NotRun => (String::new(), p.muted),
                    setup::Check::Running => (tr!("fs-mcp-check-running").to_owned(), p.muted),
                    setup::Check::Passed { logged_in: true } => (tr!("fs-mcp-check-passed").to_owned(), p.success),
                    setup::Check::Passed { logged_in: false } => (tr!("fs-mcp-check-passed-logged-out").to_owned(), p.success),
                    setup::Check::Failed(setup::Failure::Missing) => (tr!("fs-mcp-check-missing").to_owned(), p.danger),
                    setup::Check::Failed(setup::Failure::Launch) => (tr!("fs-mcp-check-launch").to_owned(), p.danger),
                    setup::Check::Failed(setup::Failure::Timeout) => (tr!("fs-mcp-check-timeout").to_owned(), p.danger),
                    setup::Check::Failed(setup::Failure::Stage(stage)) => (tr!("fs-mcp-check-failed", stage = stage.clone()), p.danger),
                };
                if !text.is_empty() {
                    ui.label(egui::RichText::new(text).size(12.0).color(color));
                }
                if busy {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                }
            });
            false
        });
        if !running {
            setup::forget_check();
        }
    });
}
