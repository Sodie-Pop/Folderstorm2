//! The links of a line of the shared chat (session-host plan,
//! checkpoint 11): where they are, and what a click on one may do from the
//! host. Pure: the host's window draws the pieces and asks the viewer's
//! own judge of a web address (`link_trust`) before anything opens.
//!
//! - **A web page** (`http://`, `https://`, or an address that starts with
//!   `www.`): the only kind the host opens, in the system's browser, and
//!   only an `http` or `https` address ([`Piece::Web::target`]).
//! - **A grid's link** (`secondlife://`, `hop://`,
//!   `x-grid-location-info://`): never followed by the host. It belongs to
//!   the character whose line it is; the host offers to switch to that
//!   character, whose own window shows it (and follows it by its rules,
//!   or not at all).
//!
//! The lines the viewers tell already show their app links, places and
//! names by their labels (the viewer's `resolved_text`), so what is left
//! is mostly web addresses. The search follows the viewer's (`next_link`,
//! `ui/chat.rs`): the link that starts first, up to the next space, the
//! sentence's punctuation left out at its end.

/// The schemes of a grid's links.
const GRID: [&str; 3] = ["secondlife://", "hop://", "x-grid-location-info://"];
/// The schemes of a web page.
const WEB: [&str; 2] = ["https://", "http://"];

/// A part of a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece<'a> {
    Text(&'a str),
    /// A web page: `shown` as written, `target` the address opened
    /// (`http://` before an address written from `www.`).
    Web {
        shown: &'a str,
        target: String,
    },
    /// A grid's link, never followed by the host.
    Grid(&'a str),
}

/// Where a scheme starts at `at`, if one of `schemes` does (any case).
fn starts_with_any(text: &str, schemes: &[&str]) -> bool {
    schemes
        .iter()
        .any(|scheme| text.get(..scheme.len()).is_some_and(|head| head.eq_ignore_ascii_case(scheme)))
}

/// Whether a link may start at byte `i` of `text`: a scheme, or `www.` at
/// the start of a word with a dot further on.
fn link_at(text: &str, i: usize) -> bool {
    let rest = &text[i..];
    if starts_with_any(rest, &WEB) || starts_with_any(rest, &GRID) {
        // the word must go on after the scheme
        let scheme = rest.find("://").map_or(0, |at| at + 3);
        return rest[scheme..].chars().next().is_some_and(|c| !c.is_whitespace());
    }
    let word_start = !text[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_');
    word_start
        && rest.get(..4).is_some_and(|head| head.eq_ignore_ascii_case("www."))
        && rest[4..]
            .split(char::is_whitespace)
            .next()
            .is_some_and(|host| host.trim_end_matches(|c: char| !c.is_alphanumeric()).contains('.'))
}

/// The length of the link that starts the text: up to the next space,
/// without the punctuation that ends a sentence.
fn link_len(rest: &str) -> usize {
    let mut end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    while end > 0 && rest[..end].ends_with(['.', ',', '!', '?', ')', ']', ';', ':', '"', '\'', '>']) {
        end -= 1;
    }
    end
}

/// The line cut into text and links, in order. Nothing is lost: the
/// pieces put end to end give the line back.
pub fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut plain = 0;
    let mut i = 0;
    while i < text.len() {
        if !text.is_char_boundary(i) || !link_at(text, i) {
            i += 1;
            continue;
        }
        let len = link_len(&text[i..]);
        let link = &text[i..i + len];
        // a scheme and nothing after it (cut by the punctuation) is text
        let body = link.find("://").map_or(link, |at| &link[at + 3..]);
        if body.is_empty() {
            i += 1;
            continue;
        }
        if plain < i {
            out.push(Piece::Text(&text[plain..i]));
        }
        out.push(if starts_with_any(link, &GRID) {
            Piece::Grid(link)
        } else if starts_with_any(link, &WEB) {
            Piece::Web {
                shown: link,
                target: link.to_owned(),
            }
        } else {
            Piece::Web {
                shown: link,
                target: format!("http://{link}"),
            }
        });
        i += len;
        plain = i;
    }
    if plain < text.len() {
        out.push(Piece::Text(&text[plain..]));
    }
    out
}

/// Whether a line has a link at all (most have none: drawn as one text).
pub fn has_link(text: &str) -> bool {
    pieces(text).iter().any(|piece| !matches!(piece, Piece::Text(_)))
}

/// Whether `target` may be handed to the system's browser: an `http` or
/// `https` address with a host, and nothing that a system could read as a
/// program or a file. The viewer's judge of the site (trusted, unknown,
/// dangerous) is asked after this.
pub fn web_target_allowed(target: &str) -> bool {
    let Some((scheme, rest)) = target.split_once("://") else {
        return false;
    };
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && rest.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '[')
        && !target.chars().any(|c| c.is_control() || c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(pieces: &[Piece<'_>]) -> String {
        pieces
            .iter()
            .map(|piece| match piece {
                Piece::Text(t) | Piece::Grid(t) => *t,
                Piece::Web { shown, .. } => shown,
            })
            .collect()
    }

    #[test]
    fn a_line_without_a_link_is_one_text() {
        for text in [
            "",
            "hello there",
            "www",
            "see www.",
            "http://",
            "mail me at jane@example.org",
            "a.www.b",
        ] {
            let found = pieces(text);
            assert!(found.iter().all(|p| matches!(p, Piece::Text(_))), "{text}: {found:?}");
            assert_eq!(joined(&found), text);
            assert!(!has_link(text));
        }
    }

    #[test]
    fn web_addresses_are_found_without_the_sentence_s_punctuation() {
        let text = "look (https://example.org/a?b=1), then www.example.net/x. Or HTTP://EXAMPLE.COM!";
        let found = pieces(text);
        assert_eq!(joined(&found), text, "nothing lost");
        let webs: Vec<(&str, &str)> = found
            .iter()
            .filter_map(|p| match p {
                Piece::Web { shown, target } => Some((*shown, target.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            webs,
            [
                ("https://example.org/a?b=1", "https://example.org/a?b=1"),
                ("www.example.net/x", "http://www.example.net/x"),
                ("HTTP://EXAMPLE.COM", "HTTP://EXAMPLE.COM"),
            ]
        );
        // "www." inside a word is not one
        assert!(!has_link("awww.example.org"));
        // written in another script around it
        let text = "voir https://example.org/été — merci";
        assert!(pieces(text).contains(&Piece::Web {
            shown: "https://example.org/été",
            target: "https://example.org/été".into()
        }));
    }

    #[test]
    fn grid_links_are_kept_apart_and_never_web_pages() {
        let text = "tp: secondlife://Ahern/128/128/30, or secondlife:///app/agent/00000000-0000-0000-0000-000000000001/im and hop://grid.example:8002/Region/1/2/3";
        let found = pieces(text);
        assert_eq!(joined(&found), text);
        let grid: Vec<&str> = found
            .iter()
            .filter_map(|p| match p {
                Piece::Grid(link) => Some(*link),
                _ => None,
            })
            .collect();
        assert_eq!(
            grid,
            [
                "secondlife://Ahern/128/128/30",
                "secondlife:///app/agent/00000000-0000-0000-0000-000000000001/im",
                "hop://grid.example:8002/Region/1/2/3"
            ]
        );
        assert!(found.iter().all(|p| !matches!(p, Piece::Web { .. })));
    }

    #[test]
    fn only_a_web_page_may_go_to_the_system_s_browser() {
        for allowed in ["http://example.org", "https://example.org/a", "HTTPS://[::1]/"] {
            assert!(web_target_allowed(allowed), "{allowed}");
        }
        for refused in [
            "secondlife:///app/wear_folder/?folder_id=1",
            "file:///C:/Windows/system32/calc.exe",
            "javascript:alert(1)",
            "C:\\Windows\\notepad.exe",
            "https://",
            "https:///etc/passwd",
            "https://example.org/\u{0}",
            "https://example.org/a b",
            "ftp://example.org",
        ] {
            assert!(!web_target_allowed(refused), "{refused}");
        }
        // what the search calls a web page always passes
        for piece in pieces("www.example.org and https://example.org/x") {
            if let Piece::Web { target, .. } = piece {
                assert!(web_target_allowed(&target), "{target}");
            }
        }
    }
}
