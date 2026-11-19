//! Project links shown on the login screen. Empty = not configured yet: the
//! button is shown disabled and nothing is fetched. The README header carries
//! the same Discord and website links: change them there too.

/// Source code repository (GitHub page).
// Folderstorm: the fork's repository
pub const GITHUB: &str = folderstorm_brand::GITHUB;
/// Bug tracker, opened in the browser by "Report a bug" (login screen, top
/// bar, Help menu). The crash window has its own GitHub route.
// Folderstorm: the maintainer's bug tracker, not GitHub issues
pub const NEW_ISSUE: &str = folderstorm_brand::BUG_REPORTS;
/// The help pages: Help › first entry.
// Folderstorm: the maintainer's help pages, not Aurora's wiki
pub const WIKI: &str = folderstorm_brand::HELP;
/// Community Discord invite.
pub const DISCORD: &str = "https://discord.gg/TK4nbMGmKt";
/// Project website.
// Folderstorm: the maintainer's website
pub const WEBSITE: &str = folderstorm_brand::WEBSITE;
/// The privacy policy on the website (Preferences › Privacy › Your data).
// Folderstorm: the maintainer's privacy policy
pub const PRIVACY_POLICY: &str = folderstorm_brand::PRIVACY;
/// News feed: this folder of `main`, hand-edited Markdown files, one
/// `<language>.md` per UI language (`news/` at the repository root). The
/// viewer keeps what it read in its cache as a JSON array of
/// `{"date", "title", "summary", "url"}`.
pub const NEWS_FEED: &str = "https://raw.githubusercontent.com/Aurora-Viewer/Aurora-Viewer/main/news/";
/// Files of the source repository as GitHub serves them unrendered; a branch
/// name and a path follow.
pub const RAW_FILES: &str = "https://raw.githubusercontent.com/Aurora-Viewer/Aurora-Viewer/";

#[cfg(test)]
mod tests {
    // Folderstorm: "Report a bug" opens the brand's bug tracker address.
    #[test]
    fn report_a_bug_opens_the_bug_tracker() {
        assert_eq!(super::NEW_ISSUE, folderstorm_brand::BUG_REPORTS);
    }

    // Folderstorm: website, help and privacy policy are the brand's.
    #[test]
    fn website_help_and_privacy_come_from_the_brand() {
        assert_eq!(super::WEBSITE, folderstorm_brand::WEBSITE);
        assert_eq!(super::WIKI, folderstorm_brand::HELP);
        assert_eq!(super::PRIVACY_POLICY, folderstorm_brand::PRIVACY);
    }
}
