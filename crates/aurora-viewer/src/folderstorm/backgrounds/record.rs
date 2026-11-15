//! What a background is, as `settings.json` keeps it: the default in the
//! layouts' store, and a layout's own (folderstorm/plans/workspaces.md,
//! "1.8 in detail", section 1).

use serde::{Deserialize, Serialize};

/// The least and the most of [`Background::brightness`], in percent.
pub const BRIGHTNESS: std::ops::RangeInclusive<u8> = 10..=100;

/// The extensions a copied picture may have: the formats the viewer
/// decodes (`image`'s features in Cargo.toml).
pub const EXTENSIONS: [&str; 6] = ["png", "jpg", "bmp", "tga", "gif", "webp"];

/// What the margins show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Nothing is painted: the margins stay black, as without this row.
    #[default]
    None,
    /// The colour alone.
    Colour,
    /// The picture, over the colour.
    Picture,
}

/// How the picture fills each area (a monitor's part of the window, or
/// the window).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Covers the area, its proportions kept: what overflows is cut.
    #[default]
    Fill,
    /// Shown whole, its proportions kept: the colour around it.
    Fit,
    /// Covers the area exactly, its proportions lost.
    Stretch,
    /// At its own size in pixels, in the middle.
    Centre,
    /// At its own size in pixels, repeated from the top-left corner.
    Tile,
}

/// A picture copied into the viewer's own folder (`super::import`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picture {
    /// The copy's name in that folder: 16 hex digits and an extension of
    /// [`EXTENSIONS`], never a path ([`managed_name`]).
    pub file: String,
    /// The original file's name, without its folder: shown in Preferences.
    #[serde(default)]
    pub name: String,
}

/// A background: the default, or a layout's own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Background {
    pub kind: Kind,
    /// The colour, opaque: alone, or behind and around the picture.
    pub colour: [u8; 4],
    /// The picture chosen; kept while another kind is chosen, so that
    /// choosing "A picture" again brings it back.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient_picture")]
    pub picture: Option<Picture>,
    pub fit: Fit,
    /// One picture over the whole window instead of one per monitor.
    pub span: bool,
    /// The picture's brightness in percent ([`BRIGHTNESS`]); the colour is
    /// not dimmed.
    pub brightness: u8,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            kind: Kind::None,
            colour: [0, 0, 0, 255],
            picture: None,
            fit: Fit::Fill,
            span: false,
            brightness: 100,
        }
    }
}

/// Whether `file` is a name [`super::import`] gives a copy: 16 lower-case
/// hex digits, a dot, and one of [`EXTENSIONS`]. Nothing else is ever
/// opened, so a hand-edited `settings.json` cannot point the viewer at
/// another file.
pub fn managed_name(file: &str) -> bool {
    let Some((stem, extension)) = file.split_once('.') else {
        return false;
    };
    stem.len() == 16 && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) && EXTENSIONS.contains(&extension)
}

/// The most characters of [`Picture::name`] kept.
const MAX_NAME: usize = 120;

/// A file's name as Preferences shows it: no control character, not too
/// long.
pub fn tidy_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect()
}

/// A picture as read: one with a name that is not ours is dropped.
fn lenient_picture<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Picture>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value::<Picture>(value)
        .ok()
        .filter(|picture| managed_name(&picture.file))
        .map(|picture| Picture {
            name: tidy_name(&picture.name),
            ..picture
        }))
}

impl Background {
    /// Reads one from the file: anything that is not a background reads as
    /// the default, and the brightness is kept in [`BRIGHTNESS`].
    pub fn read(raw: serde_json::Value) -> Self {
        if raw.is_null() {
            return Self::default();
        }
        match serde_json::from_value::<Background>(raw) {
            Ok(background) => background.sanitized(),
            Err(_) => {
                log::warn!("settings.json: a background cannot be read, the default is used");
                Self::default()
            }
        }
    }

    /// The same with its values in range: the brightness, an opaque colour.
    pub fn sanitized(mut self) -> Self {
        self.brightness = self.brightness.clamp(*BRIGHTNESS.start(), *BRIGHTNESS.end());
        self.colour[3] = 255;
        self
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether anything is painted at all.
    pub fn paints(&self) -> bool {
        self.kind != Kind::None
    }

    /// The picture to draw, when the picture is chosen.
    pub fn shown_picture(&self) -> Option<&Picture> {
        self.picture.as_ref().filter(|_| self.kind == Kind::Picture)
    }
}

/// A layout's own background as read: one that cannot be read is dropped
/// (the layout keeps the default) rather than making the layout unusable.
pub fn lenient_own<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Background>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    if value.is_null() {
        return Ok(None);
    }
    Ok(serde_json::from_value::<Background>(value).ok().map(Background::sanitized))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn picture() -> Background {
        Background {
            kind: Kind::Picture,
            colour: [16, 20, 32, 255],
            picture: Some(Picture {
                file: "0123456789abcdef.jpg".into(),
                name: "beach.jpg".into(),
            }),
            fit: Fit::Tile,
            span: true,
            brightness: 60,
        }
    }

    #[test]
    fn a_background_survives_the_file_and_the_default_reads_from_nothing() {
        let saved = picture();
        let text = serde_json::to_value(&saved).expect("written");
        assert_eq!(
            text,
            json!({
                "kind": "picture",
                "colour": [16, 20, 32, 255],
                "picture": { "file": "0123456789abcdef.jpg", "name": "beach.jpg" },
                "fit": "tile",
                "span": true,
                "brightness": 60
            })
        );
        assert_eq!(Background::read(text), saved);
        assert_eq!(Background::read(serde_json::Value::Null), Background::default());
        assert_eq!(Background::read(json!({})), Background::default());
        assert_eq!(Background::read(json!("a word")), Background::default());
        assert_eq!(Background::read(json!({ "kind": "plaid" })), Background::default());
        // a colour alone has no picture written
        let colour = Background {
            kind: Kind::Colour,
            ..Default::default()
        };
        assert!(serde_json::to_value(&colour).expect("written").get("picture").is_none());
    }

    #[test]
    fn only_a_name_of_our_own_is_kept_as_a_picture() {
        for file in ["0123456789abcdef.png", "ffffffffffffffff.webp", "0000000000000000.tga"] {
            assert!(managed_name(file), "{file}");
        }
        for file in [
            "../0123456789abcdef.png",
            "C:\\Users\\x\\beach.jpg",
            "/home/x/beach.jpg",
            "0123456789ABCDEF.png",
            "0123456789abcdef.exe",
            "0123456789abcdef.png.png",
            "0123456789abcde.png",
            "0123456789abcdef",
            "",
        ] {
            assert!(!managed_name(file), "{file}");
        }
        let read = Background::read(json!({
            "kind": "picture",
            "picture": { "file": "../../secret.png", "name": "secret.png" }
        }));
        assert_eq!((read.kind, read.picture), (Kind::Picture, None));
        // control characters out of the name shown
        let read = Background::read(json!({
            "picture": { "file": "0123456789abcdef.png", "name": "be\u{7}ach\n.png" }
        }));
        assert_eq!(read.picture.expect("kept").name, "beach.png");
    }

    #[test]
    fn the_brightness_and_the_colour_are_kept_in_range() {
        let read = Background::read(json!({ "brightness": 0, "colour": [1, 2, 3, 0] }));
        assert_eq!((read.brightness, read.colour), (10, [1, 2, 3, 255]));
        assert_eq!(Background::read(json!({ "brightness": 250 })).brightness, 100);
        // out of a byte: the whole entry cannot be read
        assert_eq!(Background::read(json!({ "brightness": 300 })), Background::default());
    }

    #[test]
    fn what_is_painted() {
        let mut b = picture();
        assert!(b.paints() && b.shown_picture().is_some());
        b.kind = Kind::Colour;
        assert!(b.paints() && b.shown_picture().is_none() && b.picture.is_some());
        b.kind = Kind::None;
        assert!(!b.paints());
        assert!(Background::default().is_default() && !picture().is_default());
    }
}
