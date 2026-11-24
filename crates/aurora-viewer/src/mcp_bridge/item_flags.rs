//! The `flags` of an item in `inventory_item` and `inventory_get_many`.
//!
//! An inventory item's flags (`II_FLAGS_*`, indra/llinventory/
//! llinventorydefines.h) mean different things by asset type: the low byte
//! is a subtype (the wearable type, a landmark visited, a settings type) and
//! the higher bits are an object's coalesced and rez-override flags. The
//! object gets every flag its type has, `false` included, so that a missing
//! key never stands for "no". The raw value is given too, as `flags_raw`.
//! None of these flags carries an asset id, so the creators' permissions
//! rule (`inventory_item`'s `asset_id`) does not apply to them.

use aurora_net::inventory::InvItem;
use serde_json::{Value, json};

/// AT_OBJECT, AT_LANDMARK and AT_SETTINGS.
const AT_OBJECT: i32 = 6;
const AT_LANDMARK: i32 = 3;
const AT_SETTINGS: i32 = 56;

/// II_FLAGS_SUBTYPE_MASK.
const SUBTYPE: u32 = 0xff;
/// II_FLAGS_LANDMARK_VISITED.
const LANDMARK_VISITED: u32 = 0x1;
/// II_FLAGS_OBJECT_SLAM_PERM: the next owner's permissions apply on rez.
const SLAM_PERM: u32 = 0x100;
/// II_FLAGS_OBJECT_SLAM_SALE: the item's sale terms apply on rez.
const SLAM_SALE: u32 = 0x1000;
/// II_FLAGS_OBJECT_PERM_OVERWRITE_BASE, _OWNER, _GROUP, _EVERYONE and
/// _NEXTOWNER: the item's masks replace the object's on rez.
const OVERWRITE: [(&str, u32); 5] = [
    ("base", 0x01_0000),
    ("owner", 0x02_0000),
    ("group", 0x04_0000),
    ("everyone", 0x08_0000),
    ("next_owner", 0x10_0000),
];
/// II_FLAGS_OBJECT_HAS_MULTIPLE_ITEMS: several objects saved as one.
const COALESCED: u32 = 0x20_0000;

/// The decoded flags of `item` (the wearable type is `wearable`, beside).
pub(super) fn of(item: &InvItem) -> Value {
    let f = item.flags;
    match item.asset_type {
        AT_OBJECT => json!({
            "coalesced": f & COALESCED != 0,
            "slam_perm": f & SLAM_PERM != 0,
            "slam_sale": f & SLAM_SALE != 0,
            "perm_overwrite": OVERWRITE
                .iter()
                .map(|(name, bit)| (name.to_string(), json!(f & bit != 0)))
                .collect::<serde_json::Map<_, _>>(),
        }),
        AT_LANDMARK => json!({"visited": f & LANDMARK_VISITED != 0}),
        // LLSettingsType: ST_SKY, ST_WATER, ST_DAYCYCLE
        AT_SETTINGS => json!({"settings_type": match f & SUBTYPE {
            0 => "sky",
            1 => "water",
            2 => "day_cycle",
            _ => "unknown",
        }}),
        _ => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(asset_type: i32, flags: u32) -> InvItem {
        InvItem {
            asset_type,
            flags,
            ..Default::default()
        }
    }

    #[test]
    fn an_object_gives_every_flag_of_its_type() {
        let plain = of(&item(AT_OBJECT, 0));
        assert_eq!(
            plain,
            json!({
                "coalesced": false,
                "slam_perm": false,
                "slam_sale": false,
                "perm_overwrite": {"base": false, "owner": false, "group": false, "everyone": false, "next_owner": false},
            })
        );
        let coalesced = of(&item(AT_OBJECT, COALESCED));
        assert_eq!(coalesced["coalesced"], true);
        assert_eq!(coalesced["slam_perm"], false);
        let rez = of(&item(AT_OBJECT, SLAM_PERM | SLAM_SALE | 0x04_0000 | 0x08_0000));
        assert_eq!(rez["slam_perm"], true);
        assert_eq!(rez["slam_sale"], true);
        assert_eq!(
            rez["perm_overwrite"],
            json!({"base": false, "owner": false, "group": true, "everyone": true, "next_owner": false})
        );
        assert_eq!(rez["coalesced"], false);
    }

    #[test]
    fn other_types_read_their_own_subtype_only() {
        assert_eq!(of(&item(AT_LANDMARK, LANDMARK_VISITED)), json!({"visited": true}));
        assert_eq!(of(&item(AT_LANDMARK, 0)), json!({"visited": false}));
        assert_eq!(of(&item(AT_SETTINGS, 2)), json!({"settings_type": "day_cycle"}));
        assert_eq!(of(&item(AT_SETTINGS, 9)), json!({"settings_type": "unknown"}));
        // the coalesced bit means nothing on a texture
        assert_eq!(of(&item(0, COALESCED)), json!({}));
    }
}
