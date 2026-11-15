//! Folderstorm: the words the Preferences search finds the Folderstorm rows
//! by, besides their labels and help (FEATURES.md row 1.9,
//! `folderstorm/plans/settings-search.md`). Folderstorm (C++) listed such
//! words for its panels ("MCP", "dual monitor", "workspace", "undo").
//!
//! Each row names catalogue entries (`fs-search-*`) holding space-separated
//! words, through Aurora's own [`Cx::keywords`]. The search reads them in the
//! interface's language and, in every language, in English as well: the
//! fork's names (MCP, workspace, layout) are often typed in English whatever
//! the language of the interface.

use super::super::cx::Cx;
use crate::keybinds::Action;

/// The language whose words every language's search reads too.
const ALWAYS: &str = "en";

/// The words of the entries `keys` for the next row; read only while
/// searching, like the other rows' costly keywords.
pub(super) fn words<'c, 'a>(cx: &'c mut Cx<'a>, keys: &[&'static str]) -> &'c mut Cx<'a> {
    if cx.searching() {
        let language = aurora_i18n::language();
        for key in keys {
            cx.keywords(&in_language(language, key));
        }
    }
    cx
}

/// The words of `key` in `language`, followed by the English ones when they
/// differ.
pub(super) fn in_language(language: &str, key: &'static str) -> String {
    let own = aurora_i18n::text_in(language, key);
    let english = aurora_i18n::text_in(ALWAYS, key);
    if own == english {
        own.to_owned()
    } else {
        format!("{own} {english}")
    }
}

/// A keyboard action of Preferences › Shortcuts: the Folderstorm ones (the
/// layouts' switcher and its toggle) are found as the layouts are.
pub(super) fn shortcut(cx: &mut Cx, action: Action) {
    if matches!(action, Action::Folderstorm(_)) {
        words(cx, &["fs-search-layouts", "fs-search-layouts-quick"]);
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::cx::{self, Counts};
    use super::super::super::{OptionsResult, OptionsUi, Page, count, run_ui};
    use super::*;
    use crate::settings::Settings;

    /// Every entry of this module, as the pages use them.
    const KEYS: &[&str] = &[
        "fs-search-mcp",
        "fs-search-mcp-activity",
        "fs-search-viewport",
        "fs-search-photo",
        "fs-search-layouts",
        "fs-search-layouts-list",
        "fs-search-layouts-save",
        "fs-search-layouts-previous",
        "fs-search-layouts-lock",
        "fs-search-layouts-monitors",
        "fs-search-layouts-reset",
        "fs-search-layouts-quick",
        "fs-search-characters",
        "fs-search-chat-upload",
        "fs-search-outfit-safety",
    ];

    /// What a user types, in each language, and the entry it must find.
    const FOUND: &[(&str, &str, &str)] = &[
        ("en", "MCP", "fs-search-mcp"),
        ("fr", "IA", "fs-search-mcp"),
        ("ja", "エージェント", "fs-search-mcp"),
        ("en", "dual monitor", "fs-search-viewport"),
        ("en", "multi-monitor", "fs-search-viewport"),
        ("en", "margins", "fs-search-viewport"),
        ("fr", "double écran", "fs-search-viewport"),
        ("fr", "multi ecran", "fs-search-viewport"),
        ("fr", "marges", "fs-search-viewport"),
        ("ja", "デュアル モニター", "fs-search-viewport"),
        ("ja", "マルチモニター", "fs-search-viewport"),
        ("ja", "余白", "fs-search-viewport"),
        ("en", "photo mode", "fs-search-photo"),
        ("fr", "mode photo", "fs-search-photo"),
        ("ja", "写真モード", "fs-search-photo"),
        ("en", "workspace", "fs-search-layouts"),
        ("fr", "espace de travail", "fs-search-layouts"),
        ("ja", "ワークスペース", "fs-search-layouts"),
        ("en", "favorites", "fs-search-layouts-list"),
        ("fr", "favoris", "fs-search-layouts-list"),
        ("ja", "お気に入り", "fs-search-layouts-list"),
        ("en", "undo", "fs-search-layouts-previous"),
        ("fr", "annuler", "fs-search-layouts-previous"),
        ("ja", "元に戻す", "fs-search-layouts-previous"),
        ("en", "quick switch", "fs-search-layouts-quick"),
        ("en", "switcher", "fs-search-layouts-quick"),
        ("fr", "bascule rapide", "fs-search-layouts-quick"),
        ("fr", "sélecteur", "fs-search-layouts-quick"),
        ("ja", "クイック 切り替え", "fs-search-layouts-quick"),
        ("ja", "スイッチャー", "fs-search-layouts-quick"),
        ("en", "multiple accounts", "fs-search-characters"),
        ("en", "alts", "fs-search-characters"),
        ("en", "session host", "fs-search-characters"),
        ("fr", "plusieurs comptes", "fs-search-characters"),
        ("fr", "hote de session", "fs-search-characters"),
        ("ja", "複数 アカウント", "fs-search-characters"),
        ("ja", "サブ垢", "fs-search-characters"),
        ("en", "image host", "fs-search-chat-upload"),
        ("en", "screenshot", "fs-search-chat-upload"),
        ("fr", "hébergeur", "fs-search-chat-upload"),
        ("fr", "téléverser", "fs-search-chat-upload"),
        ("ja", "画像 アップロード", "fs-search-chat-upload"),
        ("en", "outfit safety", "fs-search-outfit-safety"),
        ("fr", "tenue sécurité", "fs-search-outfit-safety"),
        ("ja", "誤クリック", "fs-search-outfit-safety"),
    ];

    #[test]
    fn every_entry_is_in_every_language() {
        for (language, _) in aurora_i18n::available_languages() {
            for key in KEYS {
                let text = aurora_i18n::text_in(language, key);
                assert_ne!(text, *key, "{key} missing in {language}");
                assert!(!text.trim().is_empty(), "{key} empty in {language}");
            }
        }
    }

    #[test]
    fn the_words_are_found_in_their_language() {
        for (language, query, key) in FOUND {
            let text = in_language(language, key);
            assert!(cx::found(&text, &cx::words(query)), "{query:?} ({language}) in {key}: {text}");
        }
    }

    #[test]
    fn the_english_words_are_found_in_every_language() {
        for (language, _) in aurora_i18n::available_languages() {
            for (_, query, key) in FOUND.iter().filter(|(l, ..)| *l == ALWAYS) {
                let text = in_language(language, key);
                assert!(cx::found(&text, &cx::words(query)), "{query:?} in {language}'s {key}: {text}");
            }
        }
    }

    /// The counts of every page for a search.
    fn counts(query: &str) -> Counts {
        let ctx = egui::Context::default();
        let p = crate::theme::Theme::default().palette();
        let mut s = Settings::default();
        let d = Settings::default();
        let mut st = OptionsUi {
            query: query.to_owned(),
            ..Default::default()
        };
        let mut r = OptionsResult::default();
        run_ui(&ctx, egui::RawInput::default(), |ui| count(ui, &p, &mut s, &d, &mut st, &mut r));
        st.counts
    }

    /// The pages find their Folderstorm rows by the English words, whatever
    /// the language the tests run in.
    #[test]
    fn the_search_finds_the_folderstorm_rows_on_their_pages() {
        let cases: &[(&str, Page, usize)] = &[
            // Privacy › Local assistant: access, session only, permissions,
            // status, activity, setup (client, copy, check)
            ("MCP", Page::Privacy, 8),
            ("agent", Page::Privacy, 8),
            // Layout: the world viewport's rows, the layouts' rows
            ("dual monitor", Page::Layout, 12),
            ("multi-monitor", Page::Layout, 12),
            ("viewport margins", Page::Layout, 5),
            ("photo mode", Page::Layout, 1),
            ("workspace", Page::Layout, 8),
            ("favorites", Page::Layout, 3),
            ("quick switch", Page::Layout, 2),
            ("undo", Page::Layout, 1),
            // Shortcuts › Other: the layouts' keyboard actions
            ("workspace", Page::Shortcuts, 5),
            ("switcher", Page::Shortcuts, 5),
            // Graphics › Frames per second: other windows of the viewer
            ("multiple accounts", Page::Graphics, 2),
            ("alts", Page::Graphics, 2),
            ("session host", Page::Graphics, 2),
            // Chat › Image uploads
            ("screenshot", Page::Chat, 4),
            ("Zipline", Page::Chat, 4),
            // Inventory: outfit safety
            ("outfit safety", Page::Inventory, 1),
        ];
        for (query, page, at_least) in cases {
            let found = counts(query);
            assert!(found.hits(*page) >= *at_least, "{query:?} on {page:?}: {:?}", found.hits);
        }
    }
}
