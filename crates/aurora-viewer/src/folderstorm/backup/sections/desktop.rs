//! The margins' desktop (`settings.folderstorm_desktop`, row 1.10): its
//! switches and the shortcuts of the account logged in. Each account has
//! its own desktop: the other accounts' shortcuts are neither written nor
//! touched, and the file's shortcuts are its account's (`How::source`).

use super::super::section::{Env, Exported, How, Mode, Outcome, Section, Target};
use crate::folderstorm::desktop::store::{self, Error, Target as Opens};
use aurora_i18n::tr;
use serde_json::Value;

pub struct Desktop;

impl Section for Desktop {
    fn id(&self) -> &'static str {
        "desktop"
    }

    fn label(&self) -> String {
        tr!("fs-backup-section-desktop").to_owned()
    }

    fn export(&self, env: &Env) -> Option<Exported> {
        // this account's shortcuts only, written as no account's (the
        // backup's manifest names its account)
        let mut mine = env.settings.folderstorm_desktop.clone();
        mine.account = None;
        mine.others.clear();
        mine.kept_accounts.clear();
        Some(Exported::value(serde_json::to_value(&mine).ok()?))
    }

    fn import(&self, target: &mut Target, value: Value, how: &How) -> Result<Outcome, String> {
        if !value.is_object() {
            return Err(tr!("fs-backup-unreadable").to_owned());
        }
        let mut incoming = store::Desktop::from_value(value);
        let mut out = Outcome::default();
        let before = incoming.shortcuts.len();
        incoming
            .shortcuts
            .retain(|s| matches!(s.target, Opens::Command { .. }) || !how.drops_untagged());
        let dropped = before - incoming.shortcuts.len();
        if dropped > 0 {
            out.note(tr!("fs-backup-note-desktop-inventory", count = dropped));
        }
        let desktop = &mut target.settings.folderstorm_desktop;
        match how.mode {
            Mode::Replace => {
                // the account logged in and the other accounts' desktops stay
                incoming.account = desktop.account;
                incoming.others = std::mem::take(&mut desktop.others);
                incoming.kept_accounts = std::mem::take(&mut desktop.kept_accounts);
                *desktop = incoming;
            }
            Mode::Merge => {
                let (mut full, mut there) = (0, 0);
                for shortcut in incoming.shortcuts {
                    let cell = shortcut.cell();
                    match desktop.add(shortcut.target, shortcut.slot, cell) {
                        Ok(id) => {
                            if let Some(added) = desktop.shortcuts.iter_mut().find(|s| s.id == id) {
                                added.name = shortcut.name;
                                added.icon = shortcut.icon;
                            }
                        }
                        Err(Error::Existing(_)) => there += 1,
                        Err(_) => full += 1,
                    }
                }
                if there > 0 {
                    out.note(tr!("fs-backup-note-desktop-there", count = there));
                }
                if full > 0 {
                    out.note(tr!("fs-backup-note-limit", count = full));
                }
            }
        }
        Ok(out)
    }

    fn inventory_refs(&self, value: &Value) -> usize {
        super::entries(value.get("shortcuts"))
            .iter()
            .filter(|s| {
                s.get("target")
                    .is_some_and(|t| ["folder", "item", "outfit"].iter().any(|key| t.get(*key).is_some()))
            })
            .count()
    }
}
