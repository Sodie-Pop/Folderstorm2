//! The startup splash (`splash/`) of a character's viewer: it says which
//! character is starting, in that character's language, and the splashes of
//! several characters the session host starts at once do not stack exactly
//! on top of each other (session-host plan, follow-up "a splash per
//! character").
//!
//! - The viewer passes its splash process `--profile=<name>` (named
//!   profiles only): the splash then reads that profile's settings (its
//!   language) and its window title and label name it
//!   ("Folderstorm2 — Alt1"). The main profile's splash and the updater's
//!   window get nothing more: they stay as upstream draws them.
//! - The host passes each viewer `--folderstorm-slot=<n>`; a managed viewer
//!   passes it on to its splash, which is moved by the slot from the centre
//!   of the screen. The viewer's own link to the host still takes its slot
//!   from the host's bootstrap only: this argument is never more than where
//!   a splash sits.
//!
//! Both arguments are single `--name=value` words: the viewer's command-line
//! parser skips them (a value on its own would be read as a startup link).

use ::folderstorm_session::ipc::MAX_SLOTS;
use ::folderstorm_session::profile::ProfileName;

/// The host's slot of a managed viewer, and of its splash.
const SLOT_ARG: &str = "--folderstorm-slot=";

/// How far, in points, each slot moves a splash right and down.
const STEP: f64 = 36.0;

/// The argument the host gives the viewer of `slot`.
pub fn slot_argument(slot: usize) -> String {
    format!("{SLOT_ARG}{slot}")
}

/// What the viewer gives its splash process beyond upstream's `--title` and
/// `--language`: nothing for the main profile.
pub fn arguments() -> Vec<String> {
    arguments_of(super::profile::current(), viewer_slot())
}

fn arguments_of(profile: Option<&ProfileName>, slot: Option<usize>) -> Vec<String> {
    let Some(profile) = profile else {
        return Vec::new();
    };
    let mut args = vec![format!("--profile={}", profile.shown())];
    if let Some(slot) = slot {
        args.push(slot_argument(slot));
    }
    args
}

/// The host's slot of this viewer: a managed viewer of a named profile only.
fn viewer_slot() -> Option<usize> {
    let cli = crate::cli::get();
    if !cli.managed || super::profile::current().is_none() {
        return None;
    }
    slot_in(std::env::args())
}

/// The slot named on a command line, if a valid one.
fn slot_in(args: impl Iterator<Item = String>) -> Option<usize> {
    args.filter_map(|a| a.strip_prefix(SLOT_ARG).and_then(|n| n.parse::<usize>().ok()))
        .find(|slot| *slot < MAX_SLOTS)
}

/// The name on the splash: the product, and the character's profile.
pub fn title() -> String {
    title_of(super::profile::current())
}

fn title_of(profile: Option<&ProfileName>) -> String {
    match profile {
        None => folderstorm_brand::PRODUCT.to_owned(),
        Some(name) => aurora_i18n::tr!(
            "folderstorm-splash-title",
            product = folderstorm_brand::PRODUCT,
            name = name.shown().to_owned()
        ),
    }
}

/// How far the splash moves from the centre of the screen, in pixels of a
/// screen of that `scale`: by its slot, for a character the host started.
pub fn offset(scale: f64) -> (i32, i32) {
    offset_of(slot_in(std::env::args()), scale)
}

fn offset_of(slot: Option<usize>, scale: f64) -> (i32, i32) {
    let Some(slot) = slot else {
        return (0, 0);
    };
    let step = (slot as f64 * STEP * scale).round() as i32;
    (step, step)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> ProfileName {
        ProfileName::parse(text).expect("a valid profile name")
    }

    fn words(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn a_character_s_splash_gets_its_profile_and_slot() {
        let alt = name("Alt1");
        assert_eq!(arguments_of(Some(&alt), None), ["--profile=Alt1"]);
        assert_eq!(arguments_of(Some(&alt), Some(2)), ["--profile=Alt1", "--folderstorm-slot=2"]);
        // (one word each: the command-line parser takes no value from them)
        assert!(
            arguments_of(Some(&alt), Some(2))
                .iter()
                .all(|a| a.starts_with("--") && a.contains('='))
        );
    }

    #[test]
    fn the_main_profile_s_splash_and_the_updater_s_window_get_nothing_more() {
        assert!(arguments_of(None, None).is_empty());
        // a slot without a named profile is never passed on
        assert!(arguments_of(None, Some(3)).is_empty());
        // the updater's window names nothing: no slot, no move
        assert_eq!(slot_in(words(&["--splash"])), None);
        assert_eq!(offset_of(None, 1.5), (0, 0));
        assert_eq!(title_of(None), folderstorm_brand::PRODUCT);
    }

    #[test]
    fn the_title_names_the_character() {
        let title = title_of(Some(&name("Alt1")));
        assert!(title.contains(folderstorm_brand::PRODUCT) && title.contains("Alt1"), "{title}");
    }

    #[test]
    fn each_slot_moves_its_splash() {
        assert_eq!(offset_of(Some(0), 1.0), (0, 0));
        assert_eq!(offset_of(Some(1), 1.0), (36, 36));
        assert_eq!(offset_of(Some(2), 1.5), (108, 108));
        let moves: Vec<_> = (0..MAX_SLOTS).map(|slot| offset_of(Some(slot), 1.0)).collect();
        let mut distinct = moves.clone();
        distinct.dedup();
        assert_eq!(distinct.len(), MAX_SLOTS);
    }

    #[test]
    fn the_slot_is_read_from_its_own_word_only() {
        let host = slot_argument(4);
        assert_eq!(slot_in(words(&["--splash", "--profile=Alt1", &host])), Some(4));
        for bad in [
            "--folderstorm-slot=5",
            "--folderstorm-slot=-1",
            "--folderstorm-slot=",
            "--folderstorm-slot",
        ] {
            assert_eq!(slot_in(words(&[bad])), None, "{bad}");
        }
        // the host's argument is the one the splash reads
        assert_eq!(slot_in(std::iter::once(slot_argument(1))), Some(1));
    }
}
