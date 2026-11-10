//! Folderstorm: "Restore previous look", the look worn before the last
//! outfit replacement of this login.
//!
//! Port of Folderstorm's previous look (LLAppearanceMgr::capturePreviousLook
//! and restorePreviousLook, llappearancemgr.cpp, Folderstorm contributors,
//! originally LGPL 2.1): just before a folder or outfit replaces the current
//! outfit, the Current Outfit folder's links are remembered (body parts,
//! clothing in its layer order, attachments, gestures and the saved-outfit
//! link). Restoring puts them back in one outfit change, once. It is refused,
//! changing nothing, when an item went away or into Trash (their names are
//! listed) or while RLVa locks something worn.
//!
//! The look is kept in the interface's memory for this login: leaving the
//! world (`forget`) drops it.

use crate::theme::Palette;
use crate::ui::menu;
use crate::world::appearance::Action;
use crate::world::inventory::Inventory;
use aurora_net::outfits::OutfitLink;
use uuid::Uuid;

#[derive(Clone, Debug, Default)]
struct Stored {
    agent: Uuid,
    links: Vec<OutfitLink>,
}

fn id() -> egui::Id {
    egui::Id::unique("folderstorm_previous_look")
}

fn stored(ctx: &egui::Context) -> Option<Stored> {
    ctx.data(|d| d.get_temp::<Option<Stored>>(id())).flatten()
}

fn store(ctx: &egui::Context, look: Option<Stored>) {
    ctx.data_mut(|d| d.insert_temp::<Option<Stored>>(id(), look));
}

/// Drops the remembered look (the world was left: a new login starts empty).
pub fn forget(ctx: &egui::Context) {
    if stored(ctx).is_some() {
        store(ctx, None);
    }
}

/// A look is remembered for this login.
pub fn available(ctx: &egui::Context) -> bool {
    stored(ctx).is_some_and(|s| !s.links.is_empty())
}

/// The actions that replace the whole outfit (LLAppearanceMgr's
/// wearInventoryCategory without append): what is worn before them is
/// remembered.
pub fn replaces(action: &Action) -> bool {
    matches!(
        action,
        Action::Wear(_, false)
            | Action::WearOutfits { append: false, .. }
            | Action::WearContents {
                mode: crate::world::appearance::WearMode::ReplaceOutfit,
                ..
            }
            | Action::Revert
    )
}

/// The links of a remembered look that can be worn again, or why not: an
/// item that went away or into Trash refuses the whole look, naming them.
pub fn restorable(inv: &Inventory, links: Vec<OutfitLink>) -> Result<Vec<OutfitLink>, String> {
    let trash = crate::world::inventory::actions::system(inv, 14);
    let gone: Vec<&str> = links
        .iter()
        .filter(|l| {
            let present = if l.folder {
                inv.folders.contains_key(&l.target)
            } else {
                inv.items.contains_key(&l.target)
            };
            !present
                || !crate::world::inventory::actions::under(inv, l.target, inv.root)
                || trash.is_some_and(|t| crate::world::inventory::actions::under(inv, l.target, t))
        })
        .map(|l| l.name.as_str())
        .collect();
    if !gone.is_empty() {
        return Err(aurora_i18n::tr!("appearance-previous-look-missing", items = gone.join(", ")));
    }
    if links.iter().all(|l| l.folder) {
        return Err(aurora_i18n::tr!("appearance-previous-look-none").into());
    }
    Ok(links)
}

/// Before an appearance action leaves: remembers the look an outfit
/// replacement is about to take off, and fills "Restore previous look" with
/// the remembered links (forgotten once used). `rlva_locked`: something
/// worn is locked, the look cannot be restored.
pub fn prepare(ctx: &egui::Context, inv: &Inventory, agent: Uuid, rlva_locked: bool, action: Action) -> Result<Action, String> {
    if let Action::RestoreLook(_) = action {
        if rlva_locked {
            return Err(aurora_i18n::tr!("appearance-previous-look-restricted").into());
        }
        let look = stored(ctx)
            .filter(|s| s.agent == agent)
            .ok_or_else(|| aurora_i18n::tr!("appearance-previous-look-none").to_owned())?;
        let links = restorable(inv, look.links)?;
        store(ctx, None);
        return Ok(Action::RestoreLook(links));
    }
    if replaces(&action) {
        capture(ctx, inv, agent);
    }
    Ok(action)
}

/// What the Current Outfit folder links to now, when it is complete.
fn capture(ctx: &egui::Context, inv: &Inventory, agent: Uuid) {
    let Some(cof) = crate::world::appearance::cof(inv) else { return };
    if !crate::world::appearance::loaded(inv, cof) {
        return;
    }
    let links = crate::world::appearance::usable_links(inv, cof);
    if links.iter().any(|l| !l.folder) {
        store(ctx, Some(Stored { agent, links }));
    }
}

/// RLVa holds something: a worn item that cannot come off, or an item of the
/// remembered look that cannot be worn (Folderstorm refused with any wear,
/// attachment or folder lock active; refusing these is as strict where it
/// matters).
pub fn rlva_locked(world: &crate::world::World, ctx: &egui::Context) -> bool {
    let locks = world.rlva_wear_locks();
    let inv = &world.inventory;
    let worn_fixed = crate::world::appearance::cof(inv).is_some_and(|cof| {
        crate::world::appearance::usable_links(inv, cof)
            .iter()
            .any(|l| !l.folder && !locks.can_remove(l.target))
    });
    let look_refused = stored(ctx).is_some_and(|s| {
        s.links
            .iter()
            .any(|l| !l.folder && inv.items.contains_key(&l.target) && !locks.wear(inv, l.target, 0).replace)
    });
    worn_fixed || look_refused
}

/// The Appearance window's menu entry; true when chosen.
pub fn menu_item(ui: &mut egui::Ui, p: &Palette, idle: bool) -> bool {
    let shown = available(ui.ctx());
    menu::item_if(ui, p, "arrow-back-up", aurora_i18n::tr!("appearance-previous-look"), shown && idle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(target: u128, name: &str) -> OutfitLink {
        OutfitLink {
            target: Uuid::from_u128(target),
            name: name.into(),
            desc: String::new(),
            folder: false,
            inv_type: 0,
        }
    }

    fn inventory() -> Inventory {
        let mut inv = Inventory::default();
        crate::world::appearance::seed_demo(&mut inv, Uuid::from_u128(2));
        inv
    }

    #[test]
    fn only_whole_outfit_replacements_remember_the_look() {
        use crate::world::appearance::WearMode;
        let folder = Uuid::from_u128(704);
        assert!(replaces(&Action::Wear(folder, false)));
        assert!(!replaces(&Action::Wear(folder, true)));
        assert!(replaces(&Action::WearContents {
            folder,
            mode: WearMode::ReplaceOutfit
        }));
        assert!(!replaces(&Action::WearContents {
            folder,
            mode: WearMode::Append
        }));
        assert!(!replaces(&Action::WearContents {
            folder,
            mode: WearMode::ReplaceItems
        }));
        assert!(!replaces(&Action::RestoreLook(Vec::new())));
    }

    #[test]
    fn a_look_with_a_missing_item_is_refused_naming_it() {
        let inv = inventory();
        let present = inv.items.keys().next().copied().expect("demo items");
        let mut kept = link(present.as_u128(), "Kept");
        kept.target = present;
        let err = restorable(&inv, vec![kept.clone(), link(999_999, "Lost boots")]).expect_err("refused");
        assert!(err.contains("Lost boots"), "{err}");
        // folder links alone wear nothing
        let mut base = link(1, "Base");
        base.folder = true;
        assert!(restorable(&inv, vec![base]).is_err());
    }

    #[test]
    fn a_replacement_remembers_and_a_restore_uses_it_once() {
        let ctx = egui::Context::default();
        let inv = inventory();
        let agent = Uuid::from_u128(2);
        assert!(!available(&ctx));
        let replace = Action::Wear(Uuid::from_u128(704), false);
        assert!(prepare(&ctx, &inv, agent, false, replace).is_ok());
        assert!(available(&ctx), "the demo's current outfit was remembered");
        // RLVa locks refuse it and keep it
        assert!(prepare(&ctx, &inv, agent, true, Action::RestoreLook(Vec::new())).is_err());
        assert!(available(&ctx));
        // another avatar cannot use it
        assert!(prepare(&ctx, &inv, Uuid::from_u128(3), false, Action::RestoreLook(Vec::new())).is_err());
        let restored = prepare(&ctx, &inv, agent, false, Action::RestoreLook(Vec::new())).expect("restored");
        assert!(matches!(restored, Action::RestoreLook(links) if !links.is_empty()));
        assert!(!available(&ctx), "used once");
        // leaving the world forgets it
        prepare(&ctx, &inv, agent, false, Action::Wear(Uuid::from_u128(704), false)).expect("remembered");
        forget(&ctx);
        assert!(!available(&ctx));
    }
}
