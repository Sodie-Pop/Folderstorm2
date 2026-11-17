//! The margins' desktop (`settings.folderstorm_desktop`, row 1.10): its
//! switches and its shortcuts. A folder shortcut keeps the account it was
//! made by: another account sees it greyed, as here.

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
        Some(Exported::value(serde_json::to_value(&env.settings.folderstorm_desktop).ok()?))
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
            .retain(|s| !matches!(s.target, Opens::Folder { account, .. } if how.drops(account)));
        let dropped = before - incoming.shortcuts.len();
        if dropped > 0 {
            out.note(tr!("fs-backup-note-desktop-inventory", count = dropped));
        }
        let desktop = &mut target.settings.folderstorm_desktop;
        match how.mode {
            Mode::Replace => *desktop = incoming,
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
            .filter(|s| s.get("target").and_then(|t| t.get("folder")).is_some())
            .count()
    }
}
