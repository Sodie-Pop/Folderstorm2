//! `AURORA_DEMO_BACKUP=<mode>` (docs/TESTING.md): a small setup made once
//! at the start of a demo run (a layout, a desktop shortcut, a custom
//! Inventory tab, a gesture board), two automatic backups written into the
//! demo's own backups folder, and, for the modes that say so, the import
//! dialog opened on a backup made here:
//!
//! - `1`: the setup and the automatic backups only;
//! - `import`: a backup of this account with one more layout and tab;
//! - `other`: the same, made by another account, with a folder shortcut
//!   and a tab's folder of that account (the warning and its box);
//! - `newer`: a backup of a newer format with a section this version does
//!   not know.
//!
//! Open Preferences on the page with `AURORA_DEMO_OPTIONS=system`.

use super::file::{Backup, Kind};
use super::jobs::{State, capture};
use super::{Account, Folders, apply::Plan, folder};
use crate::settings::Settings;
use serde_json::json;
use uuid::Uuid;

/// The frame the dialog opens at: the demo's login has happened.
const OPEN_AT: u64 = 90;

/// The other account of `other`.
pub const OTHER: Uuid = Uuid::from_u128(0x0de0_0a17);

fn mode() -> Option<String> {
    std::env::var_os("AURORA_DEMO")?;
    std::env::var("AURORA_DEMO_BACKUP").ok()
}

/// Once a frame from `super::jobs::frame`; true when the setup was made.
pub(super) fn frame(ctx: &egui::Context, st: &mut State, settings: &mut Settings) -> bool {
    let Some(mode) = mode() else { return false };
    if !matches!(mode.as_str(), "1" | "import" | "other" | "newer") {
        if !st.demo_done {
            log::warn!("AURORA_DEMO_BACKUP={mode}: not a mode of the demo");
            st.demo_done = true;
        }
        return false;
    }
    if st.demo_done || ctx.cumulative_frame_nr_for(egui::ViewportId::ROOT) < OPEN_AT {
        return false;
    }
    st.demo_done = true;
    let account = st.account.clone();
    seed(settings, account.as_ref().map(|a| a.id));
    let folders = Folders::of_this_viewer();
    for (kind, stamp) in [(Kind::Daily, "2026-11-16_090000"), (Kind::BeforeImport, "2026-11-16_213012")] {
        let mut backup = capture(settings, account.clone(), &folders, kind);
        backup.manifest.created = stamp.to_owned();
        let written = backup
            .to_zip()
            .and_then(|bytes| folder::write(&folders.config, kind, stamp, account.as_ref().map(|a| a.id), &bytes));
        if let Err(e) = written {
            log::warn!("demo backup: an automatic backup was not written ({e})");
        }
    }
    if mode != "1" {
        let backup = sample(settings, account.clone(), &folders, &mode);
        st.plan = Some(Plan::new(backup, account, false));
    }
    log::info!("demo backup: setup made ({mode})");
    true
}

/// The demo's setup.
fn seed(s: &mut Settings, account: Option<Uuid>) {
    use crate::folderstorm::desktop::store::Target;
    use crate::folderstorm::workspaces::store::Layout;
    if s.folderstorm_workspaces.find("Desk").is_none() {
        let _ = s.folderstorm_workspaces.add(Layout::new("Desk", None, None, Vec::new()));
    }
    let _ = s.folderstorm_desktop.add(Target::command("people"), 0, (0, 0));
    let tabs = &mut s.inventory.custom_tabs;
    if !crate::ui::inventory::custom_tabs::name_taken(tabs, "Outfits to try", None) {
        crate::ui::inventory::custom_tabs::add(tabs, "Outfits to try", Default::default(), String::new());
    }
    if let Some(account) = account {
        let boards = s.folderstorm_gesture_boards.accounts.entry(account).or_default();
        if boards.boards.is_empty() {
            let _ = boards.new_board("Greetings");
        }
    }
}

/// The backup the dialog opens on.
fn sample(settings: &Settings, account: Option<Account>, folders: &Folders, mode: &str) -> Backup {
    let mut backup = capture(settings, account, folders, Kind::Manual);
    let mut add = |id: &str, list: &str, entry: serde_json::Value| {
        if let Some(serde_json::Value::Array(items)) = backup.values.get_mut(id).and_then(|v| v.get_mut(list)) {
            items.push(entry);
        }
    };
    add("layouts", "layouts", json!({ "schema": 1, "name": "Couch" }));
    if let Some(serde_json::Value::Array(tabs)) = backup.values.get_mut("inventory_tabs") {
        let mut tab = json!({ "id": 900, "name": "Hair" });
        if mode == "other" {
            let mut folders = serde_json::Map::new();
            folders.insert(OTHER.to_string(), json!([Uuid::from_u128(0xf01d).to_string()]));
            tab["folders"] = serde_json::Value::Object(folders);
        }
        tabs.push(tab);
    }
    match mode {
        "other" => {
            backup.manifest.account = Some(Account {
                id: OTHER,
                name: "Demo Alt".into(),
            });
            if let Some(serde_json::Value::Array(shortcuts)) = backup.values.get_mut("desktop").and_then(|v| v.get_mut("shortcuts")) {
                shortcuts.push(json!({
                    "id": 900,
                    "target": { "folder": Uuid::from_u128(0xf01d).to_string(), "account": OTHER.to_string() },
                    "slot": 0, "col": 1, "row": 0
                }));
            }
        }
        "newer" => {
            backup.manifest.version = 99;
            backup.manifest.viewer = "99.0.0".into();
            backup.manifest.sections.push(super::file::Entry {
                id: "desk_icons".into(),
                label: "Desktop icon styles".into(),
                files: Vec::new(),
            });
            backup.values.insert("desk_icons".into(), json!({ "style": "glass" }));
        }
        _ => {}
    }
    backup
}
