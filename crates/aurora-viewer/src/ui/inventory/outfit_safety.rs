//! Folderstorm: outfit safety in the inventory's menus.
//!
//! Port of Folderstorm's inventory outfit safety (doc/inventory-outfit-safety.md,
//! Folderstorm contributors, originally LGPL 2.1):
//! - an item's **Wear** entry is greyed until "Enable Wear in inventory menus"
//!   is turned on (off by default): a misclick no longer replaces what is worn.
//!   Add, Attach To, double-click and the Outfits window keep their behaviour;
//! - a folder's **Replace Current Outfit** and **Add to Current Outfit** always
//!   ask first. Cancel is the default and Escape cancels; there is no "don't
//!   ask again". Accepting checks the folder again, as the menu did, before
//!   anything is worn.
//!
//! Folderstorm's menus run through Aurora's catalog (`context_catalog.rs`):
//! [`apply`] adjusts its entries once it is built.

use super::context_catalog::{Catalog, Command};
use super::menu_layout::Kind;
use super::*;
use crate::ui::widgets::{Floater, flat_button};
use crate::world::appearance::{Action as Wear, WearMode};

/// A folder waiting for the wearer's answer.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ask {
    folder: Uuid,
    mode: WearMode,
}

impl Ask {
    pub(super) fn new(folder: Uuid, mode: WearMode) -> Self {
        Self { folder, mode }
    }
}

/// Wear greyed unless allowed; the folder outfit entries ask first.
pub(super) fn apply(c: &mut Catalog, prefs: &InventoryPreferences) {
    for entry in &mut c.entries {
        let Kind::Action(index) = entry.kind else { continue };
        match entry.id.as_str() {
            "wear" if !prefs.context_wear => entry.enabled = false,
            "replace-outfit" | "append-outfit" => {
                if let Some(Some(command)) = c.commands.get_mut(index)
                    && let Command::Invoke(InvAction::Appearance(Wear::WearContents { folder, mode })) = command
                {
                    *command = Command::ConfirmOutfit(Ask::new(*folder, *mode));
                }
            }
            _ => {}
        }
    }
}

/// The folder can still be worn the way its menu entry allowed: it exists,
/// is not in the Library, has something to wear, no outfit change is under
/// way, and RLVa does not lock adding from it.
fn still_allowed(inv: &Inventory, facts: &Facts, ask: &Ask) -> bool {
    inv.folders.contains_key(&ask.folder)
        && !rules::library(inv, ask.folder)
        && !facts.appearance_busy
        && facts.rlva.folder_allows(ask.folder, true)
}

pub(super) fn show(ctx: &egui::Context, p: &Palette, inv: &Inventory, st: &mut InventoryUi, facts: &Facts, actions: &mut Vec<InvAction>) {
    let Some(ask) = st.outfit_ask.take() else { return };
    let mut open = true;
    let mut accept = false;
    let replace = matches!(ask.mode, WearMode::ReplaceOutfit);
    let window_id = format!("inventory_outfit_ask_{}", st.ui_id);
    Floater::new(
        &window_id,
        aurora_i18n::tr!(if replace {
            "inventory-outfit-ask-replace-title"
        } else {
            "inventory-outfit-ask-add-title"
        }),
        ctx.content_rect().center() - egui::vec2(210.0, 70.0),
        Vec2::new(420.0, 140.0),
    )
    .fixed()
    .show(ctx, p, &mut open, |ui| {
        let name = folder_name(inv, ask.folder);
        ui.label(if replace {
            aurora_i18n::tr!("inventory-outfit-ask-replace", folder = name.as_str())
        } else {
            aurora_i18n::tr!("inventory-outfit-ask-add", folder = name.as_str())
        });
        let allowed = still_allowed(inv, facts, &ask);
        if !allowed {
            ui.label(RichText::new(aurora_i18n::tr!("inventory-outfit-ask-unavailable")).color(p.warn));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            // Cancel first and focused: Enter or Escape never wear by accident
            let cancel = flat_button(ui, p, aurora_i18n::tr!("common-cancel"));
            if !ui.memory(|m| m.focused().is_some()) {
                cancel.request_focus();
            }
            if cancel.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                open = false;
            }
            ui.add_enabled_ui(allowed, |ui| {
                let label = if replace {
                    aurora_i18n::tr!("ui-inventory-context-replace-current-outfit")
                } else {
                    aurora_i18n::tr!("ui-inventory-context-add-to-current-outfit")
                };
                if flat_button(ui, p, label).clicked() {
                    accept = true;
                }
            });
        });
    });
    if accept && still_allowed(inv, facts, &ask) {
        actions.push(InvAction::Appearance(Wear::WearContents {
            folder: ask.folder,
            mode: ask.mode,
        }));
        return;
    }
    if open && !accept {
        st.outfit_ask = Some(ask);
    }
}

/// `AURORA_DEMO_INVENTORY=outfit-ask|aurora-outfit-ask`: the confirmation of
/// Replace Current Outfit open on the demo's "Right-click tests" folder, over
/// the Legacy or Aurora inventory.
pub fn demo(st: &mut InventoryUi) {
    st.outfit_ask = Some(Ask::new(Uuid::from_u128(8000), WearMode::ReplaceOutfit));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::inventory::menu_layout::Entry;

    fn catalog(entries: Vec<(&str, Command)>) -> Catalog {
        let mut c = Catalog::default();
        for (index, (id, command)) in entries.into_iter().enumerate() {
            c.commands.push(Some(command));
            c.entries.push(Entry {
                id: id.into(),
                label: String::new(),
                icon: String::new(),
                enabled: true,
                checked: None,
                kind: Kind::Action(index),
                zone: String::new(),
                slot: None,
            });
        }
        c
    }

    fn wear_folder(mode: WearMode) -> Command {
        Command::Invoke(InvAction::Appearance(Wear::WearContents {
            folder: Uuid::from_u128(8000),
            mode,
        }))
    }

    #[test]
    fn wear_is_greyed_until_allowed() {
        let wear = || {
            catalog(vec![(
                "wear",
                Command::Invoke(InvAction::Appearance(Wear::WearItem {
                    item: Uuid::from_u128(8100),
                    replace: true,
                    point: 0,
                })),
            )])
        };
        let mut prefs = InventoryPreferences::default();
        assert!(!prefs.context_wear, "off by default");
        let mut c = wear();
        apply(&mut c, &prefs);
        assert!(!c.entries[0].enabled);
        prefs.context_wear = true;
        let mut c = wear();
        apply(&mut c, &prefs);
        assert!(c.entries[0].enabled);
    }

    #[test]
    fn replace_and_add_ask_first_but_wear_objects_does_not() {
        let mut c = catalog(vec![
            ("replace-outfit", wear_folder(WearMode::ReplaceOutfit)),
            ("append-outfit", wear_folder(WearMode::Append)),
            ("wear-contents", wear_folder(WearMode::ReplaceItems)),
        ]);
        apply(&mut c, &InventoryPreferences::default());
        assert!(matches!(
            c.commands[0],
            Some(Command::ConfirmOutfit(Ask { folder, mode: WearMode::ReplaceOutfit })) if folder == Uuid::from_u128(8000)
        ));
        assert!(matches!(
            c.commands[1],
            Some(Command::ConfirmOutfit(Ask { folder, mode: WearMode::Append })) if folder == Uuid::from_u128(8000)
        ));
        assert!(matches!(c.commands[2], Some(Command::Invoke(_))));
        assert!(c.entries.iter().all(|e| e.enabled), "asking keeps them available");
    }

    #[test]
    fn accepting_checks_the_folder_again() {
        let mut inv = Inventory {
            root: Uuid::from_u128(1),
            ..Default::default()
        };
        crate::world::inventory::demo::seed(&mut inv, Uuid::from_u128(2));
        let ask = Ask::new(Uuid::from_u128(8000), WearMode::ReplaceOutfit);
        let mut facts = Facts::default();
        assert!(still_allowed(&inv, &facts, &ask));
        facts.appearance_busy = true;
        assert!(!still_allowed(&inv, &facts, &ask), "an outfit change under way");
        facts.appearance_busy = false;
        assert!(!still_allowed(&inv, &facts, &Ask::new(Uuid::from_u128(424242), WearMode::Append)), "gone");
    }
}
