//! How a shortcut's icon looks, besides which icon it is: a colour of its
//! own, or a picture instead of the icon (folderstorm/plans/margin-desktop.md,
//! "Icon colour and picture"). Pure: nothing here touches egui.
//!
//! A shortcut keeps its style with the rest of its account's desktop
//! (`folderstorm_desktop.accounts`, field `style`, left out while it is
//! the default), so an older version's desktop reads as it was.
//!
//! - **The colour** replaces the theme's (or the folder's colour of the
//!   Inventory) on the stock icon. A picture is drawn in its own colours:
//!   the colour is kept for when the picture goes.
//! - **A picture from a file** is copied into the account's own folder
//!   ([`folder`]), reduced to [`ICON_SIDE`] pixels a side, so that it stays
//!   when the original moves or goes; only the copy's name is kept, never a
//!   path. The checks and the copy are the backgrounds' own
//!   (`backgrounds::import`), and so is the name rule: nothing but a copy
//!   of ours is ever opened, whatever `settings.json` says.
//! - **A texture of the Inventory** is kept by its item id, never its
//!   asset id, and only a texture whose asset UUID the Legacy Viewer would
//!   show the user is offered or drawn ([`texture_asset`]): the asset id is
//!   read when the icon is drawn, to fetch the texture, and is never shown
//!   nor written.

use crate::folderstorm::backgrounds::import::{self, ImportError};
use crate::folderstorm::backgrounds::record::{self, managed_name, tidy_name};
use crate::world::inventory::{Inventory, actions as rules};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The largest side of a copied picture, in pixels: a larger one is
/// reduced before it is written (the Large size draws icons of 48 points).
pub const ICON_SIDE: u32 = 256;

/// LLAssetType::AT_TEXTURE (indra/llcommon/llassettype.h): a texture, and
/// a snapshot, of the Inventory.
pub const AT_TEXTURE: i32 = 0;

/// The Trash's folder type (LLFolderType::FT_TRASH).
const FT_TRASH: i32 = 14;

/// At most this many textures are listed at once: a search narrows them.
pub const MAX_LISTED: usize = 200;

/// Colours offered in one click, readable on the dark and the light theme
/// (the Inventory's folder colours).
pub const PRESETS: [[u8; 3]; 12] = [
    [0xE5, 0x48, 0x4D], // red
    [0xF7, 0x6B, 0x15], // orange
    [0xFF, 0xB2, 0x24], // amber
    [0x8D, 0xB6, 0x00], // lime
    [0x30, 0xA4, 0x6C], // green
    [0x12, 0xA5, 0x94], // teal
    [0x05, 0xA2, 0xC2], // cyan
    [0x3E, 0x63, 0xDD], // blue
    [0x8E, 0x4E, 0xC6], // purple
    [0xD6, 0x40, 0x9F], // pink
    [0xAD, 0x7F, 0x58], // brown
    [0x8B, 0x8D, 0x98], // grey
];

/// What a shortcut draws instead of its icon.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Picture {
    /// A picture copied into the account's folder ([`folder`]): the copy's
    /// name there ([`managed_name`]) and the original file's name, without
    /// its folder (shown in the icon's window).
    File {
        file: String,
        #[serde(default)]
        name: String,
    },
    /// A texture of the account's Inventory: its item id, never its asset
    /// id ([`texture_asset`]).
    Texture { texture: Uuid },
    /// A picture of a kind this version does not know (a newer version's):
    /// written back as it was read, drawn as the stock icon.
    Kept(serde_json::Value),
}

/// A shortcut's colour and picture; the default is the stock icon in the
/// theme's colour.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    /// The colour of the stock icon (red, green, blue).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<[u8; 3]>,
    /// Drawn instead of the icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<Picture>,
}

impl Style {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The copy of a file this style refers to.
    pub fn file(&self) -> Option<&str> {
        match &self.picture {
            Some(Picture::File { file, .. }) => Some(file),
            _ => None,
        }
    }

    /// The same, with what cannot be used dropped: a file whose name is not
    /// one of our copies (a hand-edited path), a nil texture.
    pub fn normalized(mut self) -> Self {
        self.picture = match self.picture {
            Some(Picture::File { file, name }) if managed_name(&file) => Some(Picture::File {
                file,
                name: tidy_name(&name),
            }),
            Some(Picture::File { .. }) => None,
            Some(Picture::Texture { texture }) if texture.is_nil() => None,
            // (a newer version's kind: anything but a file of ours or a
            // texture, so never one this version opens)
            Some(Picture::Kept(value)) if value.get("file").is_some() || value.get("texture").is_some() => None,
            other => other,
        };
        self
    }
}

/// Reads a shortcut's `style` leniently: what cannot be read is the
/// default (or no picture, or no colour), and the shortcut still loads.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Style, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    let colour = value.get("colour").cloned().and_then(|v| serde_json::from_value::<[u8; 3]>(v).ok());
    let picture = value
        .get("picture")
        .filter(|v| !v.is_null())
        .cloned()
        .and_then(|v| serde_json::from_value::<Picture>(v).ok());
    Ok(Style { colour, picture }.normalized())
}

/// The folder an account's copied pictures live in:
/// `folderstorm_desktop_icons` in its own folder (LL_PATH_PER_SL_ACCOUNT,
/// `settings::account_dir`), so that each account keeps its own.
pub fn folder(account: Uuid) -> PathBuf {
    crate::settings::account_dir(&account).join("folderstorm_desktop_icons")
}

/// Copies the picture at `source` into `folder`, reduced to [`ICON_SIDE`]
/// pixels a side (as a PNG): checked first as a background picture is
/// (its size on disk, its format and size in pixels, that it decodes).
/// Runs on the file dialog's thread.
pub fn import(source: &Path, folder: &Path) -> Result<Picture, ImportError> {
    let length = std::fs::metadata(source).map_err(|_| ImportError::Disk)?.len();
    if length > import::MAX_BYTES {
        return Err(ImportError::TooLarge);
    }
    let bytes = std::fs::read(source).map_err(|_| ImportError::Disk)?;
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    store(&bytes, &name, folder)
}

/// [`import`] of bytes already read (`name`: the original file's name).
pub fn store(bytes: &[u8], name: &str, folder: &Path) -> Result<Picture, ImportError> {
    let (picture, _) = import::checked(bytes)?;
    let picture = if picture.width() > ICON_SIDE || picture.height() > ICON_SIDE {
        picture.resize(ICON_SIDE, ICON_SIDE, image::imageops::FilterType::Triangle)
    } else {
        picture
    };
    let mut png = std::io::Cursor::new(Vec::new());
    picture
        .to_rgba8()
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|_| ImportError::Unreadable)?;
    let copy = import::store(&png.into_inner(), name, folder)?;
    Ok(Picture::File {
        file: copy.file,
        name: copy.name,
    })
}

/// Deletes the copies in `folder` that no shortcut of its account refers
/// to (`kept`): only files of our own names (`backgrounds::import::prune`).
pub fn prune(folder: &Path, kept: &std::collections::BTreeSet<String>) {
    import::prune(folder, kept);
}

/// The copies that the shortcuts `styles` and the entries this version
/// keeps unread (`kept`, raw) refer to.
pub fn referenced<'a>(styles: impl Iterator<Item = &'a Style>, kept: &[serde_json::Value]) -> std::collections::BTreeSet<String> {
    let mut out: std::collections::BTreeSet<String> = styles.filter_map(|s| s.file().map(str::to_owned)).collect();
    for entry in kept {
        if let Some(file) = entry["style"]["picture"]["file"].as_str()
            && record::managed_name(file)
        {
            out.insert(file.to_owned());
        }
    }
    out
}

/// The asset of the Inventory texture `item`, when it may be a shortcut's
/// picture: a texture (or a snapshot) of the account's own Inventory, out
/// of the Trash and not a link, whose asset UUID the Legacy Viewer would
/// show the user: copy, modify and transfer for the agent
/// (LLViewerInventoryItem::getProtectedAssetUUID,
/// indra/newview/llviewerinventory.cpp, the rule of the UUID field of
/// LLFloaterTexturePicker; `actions::held` with `actions::FULL`). Stricter
/// than the Legacy Viewer: a god sees no more here, and a texture of the
/// Library is never offered. The asset id is only used to fetch the
/// texture; it is never shown.
pub fn texture_asset(inv: &Inventory, item: Uuid) -> Option<Uuid> {
    let it = inv.items.get(&item)?;
    if it.asset_type != AT_TEXTURE || it.asset_id.is_nil() || inv.root.is_nil() {
        return None;
    }
    if !rules::under(inv, item, inv.root) || rules::in_type(inv, item, FT_TRASH) {
        return None;
    }
    (rules::held(inv, it) & rules::FULL == rules::FULL).then_some(it.asset_id)
}

/// The textures offered as a shortcut's picture ([`texture_asset`]) whose
/// name holds `query` (any case), by name: their item ids and names, at
/// most [`MAX_LISTED`]; true when more matched.
pub fn offered(inv: &Inventory, query: &str) -> (Vec<(Uuid, String)>, bool) {
    let query = query.trim().to_lowercase();
    let mut found: Vec<(Uuid, String)> = inv
        .items
        .values()
        .filter(|it| it.asset_type == AT_TEXTURE)
        .filter(|it| query.is_empty() || it.name.to_lowercase().contains(&query))
        .filter(|it| texture_asset(inv, it.id).is_some())
        .map(|it| (it.id, it.name.clone()))
        .collect();
    found.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.cmp(&b.0)));
    let more = found.len() > MAX_LISTED;
    found.truncate(MAX_LISTED);
    (found, more)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_net::inventory::InvItem;
    use serde_json::json;

    #[test]
    fn a_style_is_read_leniently_and_what_is_not_ours_is_dropped() {
        let read = |value: serde_json::Value| -> Style {
            #[derive(Deserialize)]
            struct Holder {
                #[serde(deserialize_with = "lenient")]
                style: Style,
            }
            serde_json::from_value::<Holder>(json!({ "style": value })).expect("read").style
        };
        let style = read(json!({ "colour": [1, 2, 3], "picture": { "file": "0123456789abcdef.png", "name": "cat\n.png" } }));
        assert_eq!(style.colour, Some([1, 2, 3]));
        assert_eq!(
            style.picture,
            Some(Picture::File {
                file: "0123456789abcdef.png".into(),
                name: "cat.png".into()
            })
        );
        // a path is never kept: only a copy of ours is opened
        assert_eq!(read(json!({ "picture": { "file": "../../secret.png" } })).picture, None);
        assert_eq!(read(json!({ "picture": { "file": "C:\\x\\cat.png" } })).picture, None);
        assert_eq!(read(json!({ "picture": { "texture": Uuid::nil() } })).picture, None);
        assert_eq!(read(json!({ "picture": { "texture": "not an id" } })).picture, None);
        // a colour out of range, or not a colour: none; the picture stays
        let style = read(json!({ "colour": [300, 0, 0], "picture": { "texture": Uuid::from_u128(5) } }));
        assert_eq!(style.colour, None);
        assert_eq!(
            style.picture,
            Some(Picture::Texture {
                texture: Uuid::from_u128(5)
            })
        );
        assert_eq!(read(json!("damaged")), Style::default());
        assert_eq!(read(json!(null)), Style::default());
        // a newer version's kind of picture is kept and written back
        let newer = json!({ "emoji": "cat" });
        let style = read(json!({ "picture": newer.clone() }));
        assert_eq!(style.picture, Some(Picture::Kept(newer.clone())));
        assert_eq!(serde_json::to_value(&style).expect("written"), json!({ "picture": newer }));
        // the default is written as nothing
        assert_eq!(serde_json::to_value(Style::default()).expect("written"), json!({}));
    }

    #[test]
    fn a_picture_is_copied_reduced_into_the_folder() {
        let dir = crate::folderstorm::backgrounds::import::tests::scratch("desktop-icon");
        let source = dir.join("Big cat.png");
        std::fs::write(&source, crate::folderstorm::backgrounds::import::tests::png(600, 300)).expect("written");
        let into = dir.join("icons");
        let Picture::File { file, name } = import(&source, &into).expect("taken") else {
            panic!("a file picture");
        };
        assert!(managed_name(&file) && file.ends_with(".png"), "{file}");
        assert_eq!(name, "Big cat.png");
        // the original may go: the copy stays, reduced
        std::fs::remove_file(&source).expect("removed");
        let decoded = import::decode(&into, &file, ICON_SIDE).expect("decoded");
        assert_eq!(decoded.size, [ICON_SIDE, ICON_SIDE / 2], "its proportions kept");
        // a small one is kept at its size
        let Picture::File { file: small, .. } = store(&import::tests::png(20, 10), "small.png", &into).expect("taken") else {
            panic!("a file picture");
        };
        assert_eq!(import::decode(&into, &small, ICON_SIDE).expect("decoded").size, [20, 10]);
        // what is not a picture is refused, nothing written
        assert_eq!(store(b"not a picture", "x.txt", &into), Err(ImportError::Unreadable));
        assert_eq!(import(&dir.join("gone.png"), &into), Err(ImportError::Disk));
        assert_eq!(std::fs::read_dir(&into).expect("listed").count(), 2);
        // pruned: only the copies no shortcut refers to
        let kept = [json!({ "id": 9, "style": { "picture": { "file": small.clone() } } })];
        let referenced = referenced(std::iter::empty(), &kept);
        prune(&into, &referenced);
        assert!(into.join(&small).is_file());
        assert!(!into.join(&file).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An inventory with a root, a Trash, and textures of `owner` and of
    /// someone else.
    fn inventory(agent: Uuid) -> Inventory {
        let mut inv = Inventory {
            root: Uuid::from_u128(77),
            owner: agent,
            ..Default::default()
        };
        crate::world::inventory::demo::seed(&mut inv, agent);
        inv
    }

    fn texture(id: u128, parent: Uuid, owner: Uuid, owner_mask: u32, everyone_mask: u32) -> InvItem {
        InvItem {
            id: Uuid::from_u128(id),
            parent,
            name: format!("Texture {id}"),
            asset_type: AT_TEXTURE,
            inv_type: 0,
            asset_id: Uuid::from_u128(id + 0x1000),
            owner,
            owner_mask,
            everyone_mask,
            base_mask: rules::FULL,
            ..Default::default()
        }
    }

    #[test]
    fn only_textures_whose_uuid_the_legacy_viewer_shows_are_offered() {
        let agent = Uuid::from_u128(1);
        let mut inv = inventory(agent);
        let root = inv.root;
        let trash = rules::system(&inv, FT_TRASH).expect("a Trash");
        let full = rules::FULL | rules::MOVE;
        inv.add_items(vec![
            texture(9001, root, agent, full, 0),
            // each right missing: refused
            texture(9002, root, agent, rules::FULL & !rules::COPY, 0),
            texture(9003, root, agent, rules::FULL & !rules::MODIFY, 0),
            texture(9004, root, agent, rules::FULL & !rules::TRANSFER, 0),
            // in the Trash
            texture(9005, trash, agent, full, 0),
            // someone else's: the everyone mask counts
            texture(9006, root, Uuid::from_u128(2), full, 0),
            texture(9007, root, Uuid::from_u128(2), 0, rules::FULL),
            // no asset yet
            InvItem {
                asset_id: Uuid::nil(),
                ..texture(9008, root, agent, full, 0)
            },
            // a notecard, full permissions
            InvItem {
                asset_type: 7,
                inv_type: 7,
                ..texture(9009, root, agent, full, 0)
            },
            // a link to the first
            InvItem {
                asset_type: 24,
                asset_id: Uuid::from_u128(9001),
                ..texture(9010, root, agent, full, 0)
            },
        ]);
        let allowed = |n: u128| texture_asset(&inv, Uuid::from_u128(n));
        assert_eq!(allowed(9001), Some(Uuid::from_u128(9001 + 0x1000)));
        for n in [9002, 9003, 9004, 9005, 9006, 9008, 9009, 9010, 4242] {
            assert_eq!(allowed(n), None, "{n} is not offered");
        }
        assert_eq!(allowed(9007), Some(Uuid::from_u128(9007 + 0x1000)), "full for everyone");
        // the same rule as the shared one, item by item
        for n in [9001, 9002, 9003, 9004, 9006, 9007] {
            let it = &inv.items[&Uuid::from_u128(n)];
            assert_eq!(allowed(n).is_some(), rules::held(&inv, it) & rules::FULL == rules::FULL, "{n}");
        }
        // the list: those, by name, searched
        let (all, more) = offered(&inv, "");
        let ids: Vec<_> = all.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&Uuid::from_u128(9001)) && ids.contains(&Uuid::from_u128(9007)));
        assert!(!more);
        for (id, _) in &all {
            assert!(texture_asset(&inv, *id).is_some());
        }
        let (found, _) = offered(&inv, "  TEXTURE 9007 ");
        assert_eq!(found, vec![(Uuid::from_u128(9007), "Texture 9007".to_owned())]);
        assert!(offered(&inv, "texture 9002").0.is_empty());
    }
}
