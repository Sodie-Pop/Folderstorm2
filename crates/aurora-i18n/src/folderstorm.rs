//! Folderstorm: the product name in the bundled catalogs.
//!
//! Aurora's catalogs name the viewer "Aurora Viewer" or "Aurora". Rather than
//! editing about 200 lines in each of the en, fr and ja catalogs (and
//! conflicting with every upstream change to them), the catalog sources are
//! rewritten once, when they load:
//!
//! - "Aurora Viewer" (and the Japanese "Auroraビューア") becomes the
//!   product name in every message, except the keys of [`KEEP`];
//! - a bare "Aurora" becomes the product name only in the messages of
//!   [`BARE`], where it names the viewer. Elsewhere "Aurora" is also the name
//!   of the interface style (Aurora / Legacy), of grid-side items
//!   (`#Aurora` folders, the Aurora bridge, Aurora Library) and of the demo
//!   places, which keep it.
//!
//! French elision follows: "d'Aurora Viewer" becomes "de Folderstorm2",
//! "qu'Aurora" becomes "que Folderstorm2".
//!
//! New upstream messages are covered by the first rule on their own; a new
//! message that names the viewer with a bare "Aurora" needs its key (or
//! prefix) added to [`BARE`].

use folderstorm_brand::PRODUCT;

/// Messages left exactly as upstream wrote them.
const KEEP: &[&str] = &[
    // The badge of Aurora's own team members: still true of them.
    "nameplate-aurora-staff",
    // Links to Aurora's website and wiki, until the fork has its own.
    "ui-options-privacy-policy-hint",
    "ui-bars-wiki",
    // A sample notecard that links to aurora-viewer.org.
    "notecard-markdown-demo",
];

/// Messages where a bare "Aurora" names the viewer (key prefixes).
const BARE: &[&str] = &[
    "about-",
    "app-gpu-",
    "app-graphics-",
    "bug-",
    "cli-help",
    "cmdline-mod-aurora-does-not-have",
    "common-not-available-in-aurora-yet",
    "crash-",
    "dev-toolbox-nothing-sent",
    "exit-",
    "login-driver-old-version",
    "login-photo-quality-off",
    "login-safe-graphics-active",
    "media-plugins-",
    "monitor-",
    "photo-quality-off-after-crash-help",
    "ui-bars-aurora-menu",
    "ui-contacts-coming-soon-in-aurora",
    "ui-contacts-multi-person-conference-coming-soon-in-aurora",
    "ui-context-aurora-recommends-",
    "ui-context-this-link-leads-",
    "ui-login-grid-link-text",
    "ui-options-about-confirmations",
    "ui-options-backup-",
    "ui-options-camera-vr-at-start-help",
    "ui-options-chat-logs-import-same",
    "ui-options-flash-on-",
    "ui-options-gpu-recovery-tip",
    "ui-options-pause-when-card-busy-tip",
    "ui-options-remove-aurora",
    "ui-options-slurl-links-open",
    "ui-options-system-notifications-hint",
    "ui-options-unread-ims-in-window-title-hint",
    "uninstall-",
    "update-",
    "vr-restart-",
];

/// What follows a bare "Aurora" that names a grid-side item, not the viewer.
const ITEM_NAMES: &[&str] = &[" bridge", " Bridge", " Library", " AO", " LiveMesh", " plug-in", " Team"];

#[derive(Clone, Copy, PartialEq)]
enum Rule {
    Keep,
    FullName,
    Bare,
}

fn rule_for(key: &str) -> Rule {
    if KEEP.iter().any(|kept| key.starts_with(kept)) {
        Rule::Keep
    } else if BARE.iter().any(|prefix| key.starts_with(prefix)) {
        Rule::Bare
    } else {
        Rule::FullName
    }
}

/// The message id a line starts, if it starts one (`key = …` at column 0).
fn entry_key(line: &str) -> Option<&str> {
    let first = line.chars().next()?;
    if !first.is_ascii_lowercase() {
        return None;
    }
    let (key, _) = line.split_once(" =")?;
    key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-').then_some(key)
}

/// Each line of a catalog with the rule of the message it belongs to.
fn lines_with_rules(source: &str) -> impl Iterator<Item = (&str, Rule)> {
    let mut rule = Rule::Keep;
    source.split_inclusive('\n').map(move |line| {
        if let Some(key) = entry_key(line) {
            rule = rule_for(key);
        } else if line.starts_with('#') || line.trim().is_empty() {
            // a comment or a blank line ends the message
            rule = Rule::Keep;
        }
        (line, if line.starts_with('#') { Rule::Keep } else { rule })
    })
}

/// Rewrites a bundled catalog's source; see the module documentation.
pub(crate) fn rebrand(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 50);
    for (line, rule) in lines_with_rules(source) {
        if rule == Rule::Keep {
            out.push_str(line);
            continue;
        }
        let mut text = replace(line, "Aurora Viewer", false);
        text = replace(&text, "Auroraビューア", false);
        if rule == Rule::Bare {
            text = replace(&text, "Aurora", true);
        }
        out.push_str(&text);
    }
    out
}

/// Replaces each whole-word `needle` by the product name.
fn replace(text: &str, needle: &str, bare: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(needle) {
        let (before, from) = rest.split_at(at);
        let after = &from[needle.len()..];
        let prev = before.chars().next_back().or_else(|| out.chars().next_back());
        let word_start = !prev.is_some_and(|c| c.is_ascii_alphanumeric() || c == '#' || c == '_' || c == '-');
        let word_end = !after.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let item = bare && ITEM_NAMES.iter().any(|name| after.starts_with(name));
        if !(word_start && word_end) || item {
            out.push_str(before);
            out.push_str(needle);
        } else {
            out.push_str(&elide(before));
            out.push_str(PRODUCT);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// French: "d'" and "qu'" before the old name become "de " and "que "
/// before the new one, which starts with a consonant.
fn elide(before: &str) -> String {
    for (elided, full) in [
        ("d'", "de "),
        ("d’", "de "),
        ("D'", "De "),
        ("D’", "De "),
        ("qu'", "que "),
        ("qu’", "que "),
        ("Qu'", "Que "),
        ("Qu’", "Que "),
    ] {
        if let Some(stem) = before.strip_suffix(elided)
            && !stem.chars().next_back().is_some_and(char::is_alphanumeric)
        {
            return format!("{stem}{full}");
        }
    }
    before.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(line: &str) -> String {
        rebrand(line)
    }

    #[test]
    fn full_name_everywhere_but_kept_keys() {
        assert_eq!(one("exit-title = Close Aurora Viewer?\n"), "exit-title = Close Folderstorm2?\n");
        assert_eq!(one("ui-bars-about = About Aurora Viewer\n"), "ui-bars-about = About Folderstorm2\n");
        assert_eq!(
            one("cli-aurora-viewer = { \"Aurora Viewer \" }{ $version }\n"),
            "cli-aurora-viewer = { \"Folderstorm2 \" }{ $version }\n"
        );
        assert_eq!(
            one("cli-aurora-viewer = { \"Auroraビューア\" }{ $version }\n"),
            "cli-aurora-viewer = { \"Folderstorm2\" }{ $version }\n"
        );
        let kept = "nameplate-aurora-staff-tooltip = A member of the Aurora Viewer team\n";
        assert_eq!(one(kept), kept);
    }

    #[test]
    fn bare_name_only_where_it_names_the_viewer() {
        assert_eq!(
            one("update-notice-title = Aurora { $version } is available\n"),
            "update-notice-title = Folderstorm2 { $version } is available\n"
        );
        // the interface style and grid-side items keep their names
        let style = "ui-options-aurora-workspace = Aurora\n";
        assert_eq!(one(style), style);
        let library = "texture-library-empty = Place resources in #Aurora/Aurora Library or any of its subfolders.\n";
        assert_eq!(one(library), library);
        assert_eq!(
            one("bug-heading = Help improve Aurora with the Aurora bridge\n"),
            "bug-heading = Help improve Folderstorm2 with the Aurora bridge\n"
        );
    }

    #[test]
    fn multi_line_messages_and_comments() {
        let source = "app-graphics-start-failed-body =\n    Aurora Viewer could not open a graphics card.\n# Aurora Viewer comment\nother = Aurora\n";
        assert_eq!(
            rebrand(source),
            "app-graphics-start-failed-body =\n    Folderstorm2 could not open a graphics card.\n# Aurora Viewer comment\nother = Aurora\n"
        );
    }

    #[test]
    fn french_elision() {
        assert_eq!(
            one("exit-title = Quitter d'Aurora Viewer ? Qu’Aurora attende.\n"),
            "exit-title = Quitter de Folderstorm2 ? Que Folderstorm2 attende.\n"
        );
    }

    #[test]
    fn identifiers_and_urls_are_not_words() {
        let line = "update-x = aurora-viewer.org #Aurora AuroraViewer Aurora_x\n";
        assert_eq!(one(line), line);
    }

    /// Every bundled catalog still parses after the rewrite, keeps the same
    /// messages, and no longer says "Aurora Viewer" outside the kept keys.
    #[test]
    fn bundled_catalogs_still_parse() {
        for &(code, source) in crate::SOURCES {
            let rewritten = rebrand(source);
            let resource =
                fluent_bundle::FluentResource::try_new(rewritten.clone()).unwrap_or_else(|(_, errors)| panic!("{code}: {errors:?}"));
            let original = fluent_bundle::FluentResource::try_new(source.to_owned()).expect("bundled catalog parses");
            assert_eq!(resource.entries().count(), original.entries().count(), "{code}");
            for (line, rule) in lines_with_rules(&rewritten) {
                if rule != Rule::Keep {
                    assert!(
                        !line.contains("Aurora Viewer") && !line.contains("Auroraビューア"),
                        "{code}: {line}"
                    );
                }
            }
        }
    }
}
