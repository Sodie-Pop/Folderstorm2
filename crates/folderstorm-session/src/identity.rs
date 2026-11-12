//! Checkpoint 7 of the session host, character colours and aliases
//! (session-host plan, section 13): what tells the characters apart in the
//! host's windows. Both are the user's own marks, kept in the host's
//! settings file and nowhere else: the host tells neither to a viewer, and
//! nothing of them reaches a grid.
//!
//! The rules, all pure:
//! - a colour is one of a small palette ([`Colour`]), or none. What is
//!   drawn is that colour **fitted to the theme** ([`Colour::fitted`]):
//!   moved towards white or black until it stands out enough from every
//!   background it is drawn on, whatever the skin, light or dark;
//! - an alias is a short name of the user's choice ([`alias`]): at most
//!   [`MAX_ALIAS`] characters, on one line, with nothing invisible in it.
//!   It replaces the character's name where the host shows one; the real
//!   name stays one hover away, and is always said in full where a line
//!   is about to be sent.

use serde::{Deserialize, Serialize};

/// A character's colour in the host's windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Colour {
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Blue,
    Violet,
    Pink,
    /// No colour of its own. Also what a name this build does not know
    /// reads as: a file written by a later build still reads.
    #[default]
    #[serde(other)]
    None,
}

/// The least contrast of a text with its background (WCAG 2, AA).
pub const TEXT_CONTRAST: f64 = 4.5;
/// The least contrast of a mark that is not a text (WCAG 2, 1.4.11).
pub const MARK_CONTRAST: f64 = 3.0;

/// How finely a colour is moved towards white or black.
const STEPS: u32 = 24;

impl Colour {
    /// The colours a character can be given, in the order they are
    /// offered.
    pub const PALETTE: [Colour; 8] = [
        Colour::Red,
        Colour::Orange,
        Colour::Yellow,
        Colour::Green,
        Colour::Teal,
        Colour::Blue,
        Colour::Violet,
        Colour::Pink,
    ];

    /// The colour as the palette defines it, before it is fitted to a
    /// theme.
    pub fn base(self) -> Option<[u8; 3]> {
        Some(match self {
            Colour::None => return None,
            Colour::Red => [0xEF, 0x44, 0x44],
            Colour::Orange => [0xF9, 0x73, 0x16],
            Colour::Yellow => [0xEA, 0xB3, 0x08],
            Colour::Green => [0x22, 0xC5, 0x5E],
            Colour::Teal => [0x14, 0xB8, 0xA6],
            Colour::Blue => [0x3B, 0x82, 0xF6],
            Colour::Violet => [0xA8, 0x55, 0xF7],
            Colour::Pink => [0xEC, 0x48, 0x99],
        })
    }

    /// The colour to draw on `backgrounds` so that it has at least the
    /// contrast `least` with each of them: the palette's own when that is
    /// enough, else moved towards white (dark backgrounds) or black (light
    /// ones) just as far as needed. None for [`Colour::None`].
    ///
    /// White or black, where the move ends, has a contrast of 4.58 or more
    /// with any single background: [`TEXT_CONTRAST`] is always met on one
    /// background, and on several of the same theme.
    pub fn fitted(self, backgrounds: &[[u8; 3]], least: f64) -> Option<[u8; 3]> {
        let base = self.base()?;
        let enough = |colour: [u8; 3]| backgrounds.iter().all(|background| contrast(colour, *background) >= least);
        if enough(base) {
            return Some(base);
        }
        // towards the end that the backgrounds are the furthest from
        let darkest = backgrounds.iter().map(|b| luminance(*b)).fold(f64::INFINITY, f64::min);
        let lightest = backgrounds.iter().map(|b| luminance(*b)).fold(0.0, f64::max);
        let to_white = 1.05 / (lightest + 0.05) >= (darkest + 0.05) / 0.05;
        let end: [u8; 3] = if to_white { [255; 3] } else { [0; 3] };
        let mixed = |step: u32| {
            let mut colour = base;
            for (c, e) in colour.iter_mut().zip(end) {
                let (from, to) = (i64::from(*c), i64::from(e));
                // (between two bytes: it is one)
                *c = (from + (to - from) * i64::from(step) / i64::from(STEPS)) as u8;
            }
            colour
        };
        Some((1..STEPS).map(mixed).find(|colour| enough(*colour)).unwrap_or(end))
    }
}

/// The relative luminance of an sRGB colour (WCAG 2), 0 to 1.
pub fn luminance(rgb: [u8; 3]) -> f64 {
    let linear = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

/// The contrast of two colours (WCAG 2), 1 to 21.
pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The longest alias, in characters.
pub const MAX_ALIAS: usize = 24;

/// Why a text is no alias.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AliasError {
    TooLong,
    /// A character an alias cannot have: a control character (a line
    /// break, a tab) or one that is not seen and changes how the rest is
    /// read (a direction override, a zero-width character).
    Character(char),
}

/// A character that is not seen, or reorders what is seen: an alias made
/// of them, or with one, could pass for another name.
fn unseen(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{00AD}' | '\u{061C}' | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// The alias a text is: the text without the spaces around it; None when
/// nothing is left (no alias: the character's own name is shown).
pub fn alias(text: &str) -> Result<Option<String>, AliasError> {
    let text = text.trim();
    if let Some(bad) = text.chars().find(|c| unseen(*c)) {
        return Err(AliasError::Character(bad));
    }
    if text.chars().count() > MAX_ALIAS {
        return Err(AliasError::TooLong);
    }
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Backgrounds of dark and light themes, and the worst there is for
    /// a contrast (the grey as far from white as from black).
    const DARK: [[u8; 3]; 3] = [[0x14, 0x12, 0x22], [0x1E, 0x1B, 0x33], [0x2A, 0x26, 0x45]];
    const LIGHT: [[u8; 3]; 3] = [[0xFF, 0xFF, 0xFF], [0xF3, 0xF4, 0xF6], [0xE5, 0xE7, 0xEB]];
    const WORST: [u8; 3] = [0x75, 0x75, 0x75];

    #[test]
    fn contrast_is_the_one_of_the_guidelines() {
        assert!((contrast([0; 3], [255; 3]) - 21.0).abs() < 1e-9);
        assert!((contrast([255; 3], [255; 3]) - 1.0).abs() < 1e-9);
        // (#767676 on white: the well-known 4.54)
        assert!((contrast([0x76; 3], [255; 3]) - 4.54).abs() < 0.01);
        assert_eq!(contrast([10, 200, 30], [0; 3]), contrast([0; 3], [10, 200, 30]));
    }

    #[test]
    fn every_colour_is_readable_on_dark_and_on_light_themes() {
        for colour in Colour::PALETTE {
            for backgrounds in [&DARK[..], &LIGHT[..], &[WORST][..], &[[0; 3]][..]] {
                for least in [MARK_CONTRAST, TEXT_CONTRAST] {
                    let fitted = colour.fitted(backgrounds, least).expect("a colour of the palette");
                    for background in backgrounds {
                        assert!(
                            contrast(fitted, *background) >= least,
                            "{colour:?} as {fitted:?} on {background:?}: {:.2} < {least}",
                            contrast(fitted, *background)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_colour_that_already_stands_out_is_left_as_it_is() {
        for colour in Colour::PALETTE {
            let base = colour.base().expect("a colour of the palette");
            if contrast(base, DARK[0]) >= TEXT_CONTRAST {
                assert_eq!(colour.fitted(&DARK[..1], TEXT_CONTRAST), Some(base));
            }
        }
        // the palette is made for marks on dark themes: none needs a move
        // there
        for colour in Colour::PALETTE {
            assert_eq!(colour.fitted(&DARK, MARK_CONTRAST), colour.base(), "{colour:?}");
        }
    }

    #[test]
    fn the_fitted_colours_stay_apart_from_one_another() {
        for backgrounds in [&DARK[..], &LIGHT[..]] {
            let fitted: Vec<[u8; 3]> = Colour::PALETTE
                .iter()
                .filter_map(|c| c.fitted(backgrounds, TEXT_CONTRAST))
                .collect();
            assert_eq!(fitted.len(), Colour::PALETTE.len());
            for (i, a) in fitted.iter().enumerate() {
                for b in &fitted[i + 1..] {
                    let apart: i32 = a.iter().zip(b).map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs()).sum();
                    assert!(apart >= 40, "{a:?} and {b:?} are too close");
                }
            }
        }
    }

    #[test]
    fn no_colour_has_no_colour() {
        assert_eq!(Colour::None.base(), None);
        assert_eq!(Colour::None.fitted(&DARK, TEXT_CONTRAST), None);
        assert_eq!(Colour::default(), Colour::None);
        assert!(!Colour::PALETTE.contains(&Colour::None));
    }

    #[test]
    fn a_colour_is_saved_by_its_name_and_an_unknown_name_reads_as_none() {
        assert_eq!(serde_json::to_string(&Colour::Teal).expect("json"), "\"teal\"");
        for colour in Colour::PALETTE.into_iter().chain([Colour::None]) {
            let text = serde_json::to_string(&colour).expect("json");
            assert_eq!(serde_json::from_str::<Colour>(&text).expect("json"), colour);
        }
        assert_eq!(serde_json::from_str::<Colour>("\"ultraviolet\"").expect("json"), Colour::None);
    }

    #[test]
    fn an_alias_is_short_on_one_line_and_has_nothing_unseen() {
        assert_eq!(alias("  Main  "), Ok(Some("Main".to_owned())));
        assert_eq!(alias("Shop keeper 2"), Ok(Some("Shop keeper 2".to_owned())));
        assert_eq!(alias("店番"), Ok(Some("店番".to_owned())));
        assert_eq!(alias(""), Ok(None));
        assert_eq!(alias("   "), Ok(None));
        let longest = "é".repeat(MAX_ALIAS);
        assert_eq!(alias(&longest), Ok(Some(longest.clone())), "characters, not bytes");
        assert_eq!(alias(&format!("{longest}x")), Err(AliasError::TooLong));
        for bad in ['\n', '\t', '\r', '\u{0}', '\u{7}', '\u{1b}', '\u{7f}', '\u{85}'] {
            assert_eq!(alias(&format!("a{bad}b")), Err(AliasError::Character(bad)), "{bad:?}");
        }
        for bad in ['\u{200B}', '\u{200E}', '\u{202E}', '\u{2066}', '\u{FEFF}', '\u{00AD}', '\u{2028}'] {
            assert_eq!(alias(&format!("a{bad}b")), Err(AliasError::Character(bad)), "{bad:?}");
        }
        // (the spaces around go first: a line break at the end is no error)
        assert_eq!(alias("Main\n"), Ok(Some("Main".to_owned())));
    }
}
