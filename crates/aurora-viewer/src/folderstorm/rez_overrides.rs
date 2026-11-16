//! Rez overrides of a stored object: the inventory flags that make the
//! simulator apply an item's edited permissions and sale terms when the
//! object is rezzed, instead of those saved in its asset.
//!
//! Port of LLSidepanelItemInfo::updatePermissions / updateSaleInfo and
//! LLFloaterProperties (Firestorm, indra/newview/llsidepaneliteminfo.cpp,
//! llfloaterproperties.cpp, originally LGPL 2.1) with the flag values of
//! LLInventoryItemFlags (indra/llinventory/llinventorydefines.h). The Legacy
//! Viewer sets them on the item it sends, for the agent's inventory (AIS or
//! UpdateInventoryItem) and for an object's contents (UpdateTaskInventory)
//! alike, and keeps every flag the item already has.

use aurora_llsd::Llsd;

/// AT_OBJECT: only stored objects are rezzed with their item's terms.
const AT_OBJECT: i32 = 6;
/// II_FLAGS_OBJECT_SLAM_PERM: apply the item's next owner mask on rez.
pub const SLAM_PERM: u32 = 0x100;
/// II_FLAGS_OBJECT_SLAM_SALE: apply the item's sale terms on rez.
pub const SLAM_SALE: u32 = 0x1000;
/// II_FLAGS_OBJECT_PERM_OVERWRITE_GROUP.
pub const OVERWRITE_GROUP: u32 = 0x04_0000;
/// II_FLAGS_OBJECT_PERM_OVERWRITE_EVERYONE.
pub const OVERWRITE_EVERYONE: u32 = 0x08_0000;

/// What the Properties window edits on an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terms {
    pub next_owner: u32,
    pub group: u32,
    pub everyone: u32,
    pub sale_type: u8,
    pub sale_price: i32,
}

impl Terms {
    pub fn of_item(it: &aurora_net::inventory::InvItem) -> Self {
        Self {
            next_owner: it.next_owner_mask,
            group: it.group_mask,
            everyone: it.everyone_mask,
            sale_type: it.sale_type,
            sale_price: it.sale_price,
        }
    }

    pub fn of_edit(edit: &crate::ui::inventory::PropertyEdit) -> Self {
        Self {
            next_owner: edit.next,
            group: edit.group,
            everyone: edit.everyone,
            sale_type: edit.sale_type,
            sale_price: edit.price,
        }
    }

    pub fn of_task_item(it: &aurora_net::build::TaskItem) -> Self {
        Self {
            next_owner: it.next_owner_mask,
            group: it.group_mask,
            everyone: it.everyone_mask,
            sale_type: it.sale_type,
            sale_price: it.sale_price,
        }
    }
}

/// The item's flags once `before` becomes `after`: each changed term of an
/// object adds its override to the flags it has; nothing else changes.
pub fn flags(asset_type: i32, flags: u32, before: Terms, after: Terms) -> u32 {
    if asset_type != AT_OBJECT {
        return flags;
    }
    let mut flags = flags;
    if after.next_owner != before.next_owner {
        flags |= SLAM_PERM;
    }
    if after.everyone != before.everyone {
        flags |= OVERWRITE_EVERYONE;
    }
    if after.group != before.group {
        flags |= OVERWRITE_GROUP;
    }
    // LLSaleInfo::operator!=: the type or the price.
    if (after.sale_type, after.sale_price) != (before.sale_type, before.sale_price) {
        flags |= SLAM_SALE;
    }
    flags
}

/// Adds `flags` to the Properties window's AIS patch of an agent inventory
/// item when the edit gives the object new overrides. A patch without
/// `flags` leaves the item's flags as they are.
pub fn add_to_patch(it: &aurora_net::inventory::InvItem, edit: &crate::ui::inventory::PropertyEdit, body: &mut Llsd) {
    let new = flags(it.asset_type, it.flags, Terms::of_item(it), Terms::of_edit(edit));
    if new != it.flags
        && let Llsd::Map(fields) = body
    {
        fields.insert("flags".into(), Llsd::Integer(new as i32));
    }
}

/// Sets the overrides on an object's content item edited in the Properties
/// window, before it is sent with UpdateTaskInventory.
pub fn apply_to_task_item(before: &aurora_net::build::TaskItem, after: &mut aurora_net::build::TaskItem) {
    after.flags = flags(
        i32::from(after.asset_type),
        after.flags,
        Terms::of_task_item(before),
        Terms::of_task_item(after),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_llsd::llsd_map;
    use aurora_net::inventory::InvItem;

    const NOTECARD: i32 = 7;
    /// The attachment point an object keeps in the low byte of its flags.
    const ATTACH_POINT: u32 = 0x05;

    fn terms() -> Terms {
        Terms {
            next_owner: 0x0008_2000,
            group: 0,
            everyone: 0,
            sale_type: 0,
            sale_price: 10,
        }
    }

    #[test]
    fn a_new_next_owner_mask_on_an_object_slams_its_permissions_on_rez() {
        let after = Terms {
            next_owner: 0x0000_e000,
            ..terms()
        };
        assert_eq!(flags(AT_OBJECT, 0, terms(), after), SLAM_PERM);
    }

    #[test]
    fn each_changed_term_adds_only_its_own_override() {
        let cases = [
            (
                Terms {
                    group: 0x0000_e000,
                    ..terms()
                },
                OVERWRITE_GROUP,
            ),
            (
                Terms {
                    everyone: 0x0000_8000,
                    ..terms()
                },
                OVERWRITE_EVERYONE,
            ),
            (Terms { sale_type: 2, ..terms() }, SLAM_SALE),
            (Terms { sale_price: 25, ..terms() }, SLAM_SALE),
        ];
        for (after, flag) in cases {
            assert_eq!(flags(AT_OBJECT, 0, terms(), after), flag, "{after:?}");
        }
        let all = Terms {
            next_owner: 0,
            group: 1,
            everyone: 1,
            sale_type: 1,
            sale_price: 0,
        };
        assert_eq!(
            flags(AT_OBJECT, 0, terms(), all),
            SLAM_PERM | OVERWRITE_GROUP | OVERWRITE_EVERYONE | SLAM_SALE
        );
    }

    #[test]
    fn other_items_and_unchanged_terms_keep_their_flags() {
        let after = Terms { next_owner: 0, ..terms() };
        assert_eq!(flags(NOTECARD, 0x20, terms(), after), 0x20);
        assert_eq!(flags(AT_OBJECT, ATTACH_POINT, terms(), terms()), ATTACH_POINT);
    }

    #[test]
    fn the_flags_an_object_already_has_are_kept() {
        let existing = ATTACH_POINT | OVERWRITE_GROUP | 0x0020_0000;
        let after = Terms { next_owner: 0, ..terms() };
        assert_eq!(flags(AT_OBJECT, existing, terms(), after), existing | SLAM_PERM);
    }

    fn object() -> InvItem {
        InvItem {
            asset_type: AT_OBJECT,
            inv_type: 6,
            flags: ATTACH_POINT,
            next_owner_mask: 0x0008_2000,
            sale_price: 10,
            ..Default::default()
        }
    }

    fn body() -> Llsd {
        llsd_map! { "name" => "Chair" }
    }

    #[test]
    fn the_properties_patch_carries_flags_only_when_the_object_gets_an_override() {
        let it = object();
        let mut edit = crate::ui::inventory::PropertyEdit::from(&it);
        let mut unchanged = body();
        add_to_patch(&it, &edit, &mut unchanged);
        assert!(!unchanged.has("flags"), "a rename leaves the flags to the server");

        edit.next = 0x0000_e000;
        let mut patch = body();
        add_to_patch(&it, &edit, &mut patch);
        assert_eq!(patch["flags"].as_u32(), ATTACH_POINT | SLAM_PERM);
        assert_eq!(patch["name"].to_string_value(), "Chair");

        let notecard = InvItem {
            asset_type: NOTECARD,
            inv_type: 7,
            ..object()
        };
        let mut patch = body();
        add_to_patch(&notecard, &edit, &mut patch);
        assert!(!patch.has("flags"));
    }

    #[test]
    fn a_content_item_gets_the_overrides_of_its_changed_terms() {
        let before = aurora_net::build::TaskItem {
            asset_type: AT_OBJECT as i8,
            flags: ATTACH_POINT,
            next_owner_mask: 0x0008_2000,
            ..Default::default()
        };
        let mut after = aurora_net::build::TaskItem {
            next_owner_mask: 0x0000_e000,
            everyone_mask: 0x0000_8000,
            ..before.clone()
        };
        apply_to_task_item(&before, &mut after);
        assert_eq!(after.flags, ATTACH_POINT | SLAM_PERM | OVERWRITE_EVERYONE);

        let mut script = aurora_net::build::TaskItem {
            asset_type: 10,
            next_owner_mask: 0,
            ..before.clone()
        };
        apply_to_task_item(&before, &mut script);
        assert_eq!(script.flags, ATTACH_POINT);
    }
}
