//! The icons a shortcut can wear instead of its target's own
//! (folderstorm/plans/margin-desktop.md, "Arranging and editing"): a short
//! list chosen for what people keep in folders (clothes, the body, the
//! home, places, tools), all of them Tabler icons the viewer already
//! embeds, so that the list adds no file and no hook to `ui/icons.rs`.
//!
//! A shortcut keeps the icon's Tabler name. One the list does not have (a
//! hand-edited file, a list that changed) is still drawn when the viewer
//! has it, and gives its place to an outline when it does not.

/// The icons of *Icon ›*, in the order of its grid.
pub const CHOICES: &[&str] = &[
    // folders, favourites, the home
    "folder-filled",
    "folder-open-filled",
    "star-filled",
    "heart-filled",
    "home-filled",
    "bookmark-filled",
    "flag-filled",
    "key-filled",
    // clothes and the body
    "shirt-filled",
    "hanger-2-filled",
    "jacket",
    "shoe",
    "scissors",
    "man-filled",
    "woman-filled",
    "mood-smile-filled",
    // making things
    "brush",
    "palette-filled",
    "wand",
    "cube",
    "package",
    "sparkles-filled",
    "crown-filled",
    "gift-filled",
    // places
    "building-store",
    "armchair",
    "building-castle",
    "tree",
    "leaf-filled",
    "paw-filled",
    "map-pin-filled",
    "compass-filled",
    // the rest
    "plane-tilt-filled",
    "shopping-cart-filled",
    "camera-filled",
    "photo-filled",
    "music",
    "book-filled",
    "device-gamepad-filled",
    "bolt-filled",
];

/// How many icons a line of the grid holds.
pub const PER_ROW: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icons_offered_are_embedded_and_listed_once() {
        assert_eq!(CHOICES.len(), 40);
        for (n, icon) in CHOICES.iter().enumerate() {
            assert!(crate::ui::icons::embedded(icon), "{icon} is not an embedded Tabler icon");
            assert!(!CHOICES[..n].contains(icon), "{icon} is listed twice");
            // a name the record keeps as it is
            assert_eq!(super::super::store::clean_name(icon), *icon);
        }
        assert_eq!(CHOICES.len() % PER_ROW, 0, "the grid's last line is full");
    }
}
