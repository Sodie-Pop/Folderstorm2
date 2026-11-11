//! Folderstorm: the Outfit Gallery's thumbnail size.
//!
//! Port of Folderstorm's gallery thumbnail slider (doc/help.md "Outfit
//! Gallery", Folderstorm contributors, originally LGPL 2.1): the photo width
//! goes from 80 to 256 pixels (256 is the size the gallery fetches), and as
//! many columns as fit follow from it. Aurora's own layout (two columns, or
//! three from a 540-pixel wide list) stays the default: the size is 0 until
//! the slider is turned on.

use crate::theme::Palette;
use crate::ui::menu;

/// The narrowest and widest photo, in points.
pub(crate) const MIN: f32 = 80.0;
pub(crate) const MAX: f32 = 256.0;
/// Between two photos (the gallery row's item spacing).
const GAP: f32 = 10.0;

/// The columns and the photo width for this list. `size` 0 is Aurora's
/// automatic layout; `list_width` decides its columns (the list's width
/// before its scroll bar), `available` what one row can use.
pub(super) fn layout(list_width: f32, available: f32, size: f32) -> (usize, f32) {
    if size <= 0.0 {
        let columns = if list_width >= 540.0 { 3 } else { 2 };
        let width = ((available - GAP * (columns - 1) as f32) / columns as f32).max(90.0);
        return (columns, width);
    }
    let size = size.clamp(MIN, MAX);
    let columns = (((available + GAP) / (size + GAP)).floor() as usize).max(1);
    (columns, size.min(available.max(MIN)))
}

/// The options menu's rows: a switch for a chosen size, and its slider.
pub(super) fn menu_rows(ui: &mut egui::Ui, p: &Palette, size: &mut f32) {
    let mut custom = *size > 0.0;
    if menu::toggle(ui, p, "photo-filled", aurora_i18n::tr!("ui-appearance-gallery-size"), &mut custom) {
        *size = if custom { 160.0 } else { 0.0 };
    }
    if custom {
        ui.horizontal(|ui| {
            ui.add_space(28.0);
            ui.add(
                egui::Slider::new(size, MIN..=MAX)
                    .step_by(8.0)
                    .suffix(aurora_i18n::tr!("ui-appearance-gallery-size-unit")),
            )
            .on_hover_text(aurora_i18n::tr!("ui-appearance-gallery-size-help"));
        });
    }
}

/// A saved size: 0, or within the slider's range.
pub(crate) fn sanitized(size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        size.clamp(MIN, MAX)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_keeps_aurora_layout() {
        assert_eq!(layout(400.0, 380.0, 0.0), (2, 185.0));
        assert_eq!(layout(600.0, 580.0, 0.0).0, 3);
    }

    #[test]
    fn a_chosen_size_fits_as_many_columns_as_it_can() {
        assert_eq!(layout(600.0, 580.0, 80.0), (6, 80.0));
        assert_eq!(layout(600.0, 580.0, 256.0), (2, 256.0));
        // never wider than the list, never under one column
        assert_eq!(layout(200.0, 180.0, 256.0), (1, 180.0));
        // out-of-range values are brought back
        assert_eq!(layout(600.0, 580.0, 20.0).1, MIN);
        assert_eq!(layout(600.0, 580.0, 900.0).1, MAX);
    }

    #[test]
    fn saved_sizes_are_sanitized() {
        assert_eq!(sanitized(0.0), 0.0);
        assert_eq!(sanitized(-4.0), 0.0);
        assert_eq!(sanitized(f32::NAN), 0.0);
        assert_eq!(sanitized(50.0), MIN);
        assert_eq!(sanitized(120.0), 120.0);
        assert_eq!(sanitized(1000.0), MAX);
    }
}
