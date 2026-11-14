//! The layouts in words, shared by Preferences › Layout › Layouts and the
//! switcher: why a change was refused, what an apply did, what a layout
//! holds.

use super::apply::Report;
use super::store::{Layout, MAX_LAYOUTS, MAX_NAME, NameError, StoreError, WindowFrame};
use super::windows;
use super::workspace::{MAX_WORKSPACES, Workspace};
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

/// The text of a refusal about a workspace.
pub fn workspace_refusal(error: StoreError) -> String {
    match error {
        StoreError::Name(NameError::Empty) => tr!("fs-workspaces-error-empty").to_owned(),
        StoreError::Name(NameError::Taken) => tr!("fs-workspaces-error-taken").to_owned(),
        StoreError::Full => tr!("fs-workspaces-error-full", count = MAX_WORKSPACES),
        StoreError::Invalid | StoreError::Missing => tr!("fs-workspaces-error-invalid").to_owned(),
        other => refusal(other),
    }
}

/// What a workspace does, in a line or two: the layout that goes with it,
/// and the windows it opens by name.
pub fn workspace_holds(workspace: &Workspace) -> String {
    let layout = match &workspace.layout {
        Some(name) => tr!("fs-workspaces-holds-layout", name = name.as_str()),
        None => tr!("fs-workspaces-holds-no-layout").to_owned(),
    };
    let names: Vec<&str> = workspace.opens().map(windows::Window::label).collect();
    let opens = if names.is_empty() {
        tr!("fs-workspaces-holds-none").to_owned()
    } else {
        tr!("fs-workspaces-holds-opens", names = names.join(", "))
    };
    match &workspace.folders {
        Some(folders) => format!(
            "{layout} {opens} {}",
            tr!("fs-workspaces-holds-folders", count = folders.windows.len())
        ),
        None => format!("{layout} {opens}"),
    }
}

/// What the last apply did, in a sentence or two.
pub fn report_text(report: &Report, waiting: usize) -> String {
    let layout = match &report.name {
        Some(name) if report.windows > 0 => Some(tr!("fs-layouts-applied-windows", name = name.as_str(), count = report.windows)),
        Some(name) => Some(tr!("fs-layouts-applied", name = name.as_str())),
        None => None,
    };
    let mut text = match (&report.workspace, layout) {
        (Some(workspace), Some(layout)) => format!("{} {layout}", tr!("fs-workspaces-applied", name = workspace.as_str())),
        (Some(workspace), None) => tr!("fs-workspaces-applied", name = workspace.as_str()),
        (None, Some(layout)) => layout,
        (None, None) => tr!("fs-layouts-previous-done").to_owned(),
    };
    let mut add = |more: String| {
        text.push(' ');
        text.push_str(&more);
    };
    if report.no_layout {
        add(tr!("fs-workspaces-applied-no-layout").to_owned());
    }
    if report.fitted > 0 {
        add(tr!("fs-layouts-applied-fitted", count = report.fitted));
    }
    if report.unknown > 0 {
        add(tr!("fs-layouts-applied-unknown", count = report.unknown));
    }
    // (a workspace's windows, or the ones its layout was saved with open)
    let listed = report.workspace.is_some() || (report.name.is_none() && report.closed > 0);
    if report.opened > 0 && listed {
        add(tr!("fs-workspaces-applied-opened", count = report.opened));
    } else if report.opened > 0 {
        add(tr!("fs-layouts-applied-opened", count = report.opened));
    }
    if report.closed > 0 {
        add(tr!("fs-workspaces-applied-closed", count = report.closed));
    }
    if report.blocked > 0 && listed {
        add(tr!("fs-workspaces-applied-blocked", count = report.blocked));
    } else if report.blocked > 0 {
        add(tr!("fs-layouts-applied-blocked", count = report.blocked));
    }
    if report.folders_missing > 0 {
        add(tr!("fs-workspaces-folders-missing", count = report.folders_missing));
    }
    if report.folders_blocked > 0 {
        add(tr!("fs-workspaces-folders-blocked").to_owned());
    }
    if report.folders_other_account {
        add(tr!("fs-workspaces-folders-other-account").to_owned());
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
