//! The layouts in words, shared by Preferences › Layout › Layouts and the
//! switcher: why a change was refused, what an apply did, what a layout
//! holds.

use super::apply::Report;
use super::store::{Layout, MAX_LAYOUTS, MAX_NAME, NameError, StoreError, WindowFrame};
use super::windows;
use aurora_i18n::tr;

/// The text of a refusal.
pub fn refusal(error: StoreError) -> String {
    match error {
        StoreError::Name(NameError::Empty) => tr!("fs-layouts-error-empty").to_owned(),
        StoreError::Name(NameError::TooLong) => tr!("fs-layouts-error-long", count = MAX_NAME),
        StoreError::Name(NameError::Control) => tr!("fs-layouts-error-control").to_owned(),
        StoreError::Name(NameError::Taken) => tr!("fs-layouts-error-taken").to_owned(),
        StoreError::Full => tr!("fs-layouts-error-full", count = MAX_LAYOUTS),
        StoreError::Invalid | StoreError::Missing => tr!("fs-layouts-error-invalid").to_owned(),
    }
}

/// What the last apply did, in a sentence or two.
pub fn report_text(report: &Report, waiting: usize) -> String {
    let mut text = match &report.name {
        Some(name) if report.windows > 0 => tr!("fs-layouts-applied-windows", name = name.as_str(), count = report.windows),
        Some(name) => tr!("fs-layouts-applied", name = name.as_str()),
        None => tr!("fs-layouts-previous-done").to_owned(),
    };
    let mut add = |more: String| {
        text.push(' ');
        text.push_str(&more);
    };
    if report.fitted > 0 {
        add(tr!("fs-layouts-applied-fitted", count = report.fitted));
    }
    if report.unknown > 0 {
        add(tr!("fs-layouts-applied-unknown", count = report.unknown));
    }
    if report.opened > 0 {
        add(tr!("fs-layouts-applied-opened", count = report.opened));
    }
    if report.blocked > 0 {
        add(tr!("fs-layouts-applied-blocked", count = report.blocked));
    }
    if waiting > 0 {
        add(tr!("fs-layouts-waiting", count = waiting));
    }
    if report.missing > 0 {
        add(tr!("fs-layouts-applied-missing", count = report.missing));
    }
    if report.left {
        add(tr!("fs-layouts-applied-left").to_owned());
    }
    if report.unlocked {
        add(tr!("fs-layouts-applied-unlocked").to_owned());
    }
    if report.no_window {
        add(tr!("fs-layouts-applied-no-window").to_owned());
    }
    text
}

/// A layout's viewer window, in a few words.
pub fn window_words(frame: &WindowFrame) -> String {
    let mut words = match frame {
        WindowFrame::Normal { maximized: true, .. } => tr!("fs-layouts-holds-window-maximized").to_owned(),
        WindowFrame::Normal { width, height, .. } => tr!("fs-layouts-holds-window", width = *width, height = *height),
        WindowFrame::Fullscreen { monitor } => tr!("fs-layouts-holds-window-fullscreen", n = usize::from(*monitor) + 1),
        WindowFrame::Borderless { monitors, .. } => tr!("fs-layouts-holds-window-borderless", count = monitors.len()),
    };
    if frame.locked() {
        words.push_str(", ");
        words.push_str(tr!("fs-layouts-holds-locked"));
    }
    words
}

/// What a layout holds, in a line.
pub fn holds(layout: &Layout) -> String {
    let mut parts = Vec::new();
    if let Some(frame) = &layout.window {
        parts.push(window_words(frame));
    }
    if layout.world.is_some() {
        parts.push(tr!("fs-layouts-part-world").to_owned());
    }
    if let Some(saved) = &layout.windows {
        let known = saved.windows.keys().filter(|id| windows::find(id).is_some()).count();
        parts.push(tr!("fs-layouts-holds-windows", count = known));
    }
    if !layout.monitors.is_empty() {
        parts.push(tr!("fs-layouts-holds-monitors", count = layout.monitors.len()));
    }
    parts.join(" · ")
}
