//! The Folderstorm2 product identity, in one place.
//!
//! Folderstorm2 is a fork of Aurora Viewer. Aurora writes its name and its
//! identifiers directly where they are used; the fork changes those places
//! with one-line hooks (marked `// Folderstorm:`) that read from here, so
//! that an upstream merge touches as little as possible.
//!
//! Two goals:
//! - the user sees Folderstorm2 (window, menus, About, the grid's channel);
//! - Folderstorm2 never shares anything with an Aurora Viewer installed on
//!   the same computer: settings, cache, logs, saved passwords, the
//!   `secondlife://` link handler, notification and link hand-over names.
//!
//! Aurora's copyright and licence notices stay as they are (GPL sections 4
//! and 5): the About window says that Folderstorm2 is based on Aurora Viewer.

/// The product name as a literal, for `concat!` and attributes that need
/// one. Everything else reads [`PRODUCT`].
#[macro_export]
macro_rules! product {
    () => {
        "Folderstorm2"
    };
}

/// The name the user sees: window title, menus, About, notifications.
pub const PRODUCT: &str = product!();

/// Lower-case identifier for files and system ids.
pub const SLUG: &str = "folderstorm2";

/// Where Aurora credits itself, the fork names its origin.
pub const BASED_ON: &str = "Aurora Viewer";

/// `directories::ProjectDirs::from` arguments: the settings, cache, data and
/// log folders (Aurora's are "org", "Aurora", "AuroraViewer").
pub const DIRS_QUALIFIER: &str = "org";
pub const DIRS_ORGANIZATION: &str = "Folderstorm";
pub const DIRS_APPLICATION: &str = "Folderstorm2";

/// The settings, cache, data and log folders of the system for this
/// product: what Aurora gets from
/// `directories::ProjectDirs::from("org", "Aurora", "AuroraViewer")`.
pub fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from(DIRS_QUALIFIER, DIRS_ORGANIZATION, DIRS_APPLICATION)
}

/// Folder made inside a cache location the user picks (Aurora's:
/// "AuroraViewerCache").
pub const CACHE_ROOT_NAME: &str = "Folderstorm2Cache";

/// Sub-folder of the system's Pictures folder for snapshots.
pub const SNAPSHOT_FOLDER: &str = PRODUCT;

/// Services of the system's credential store. They must differ from
/// Aurora's, or each viewer would read and overwrite the other's saved
/// passwords and cache key.
pub const KEYRING_LOGIN: &str = "Folderstorm2";
pub const KEYRING_MFA: &str = "Folderstorm2 MFA";
pub const KEYRING_CACHE: &str = "Folderstorm2 cache";
pub const KEYRING_GITHUB: &str = "Folderstorm2 GitHub";

/// The viewer channel sent to the grid at login and in viewer statistics
/// (LLVersionInfo::getChannel). The Second Life third-party viewer policy
/// asks a viewer to identify itself.
pub const GRID_CHANNEL: &str = PRODUCT;

/// Windows notification identity (AppUserModelID, Aurora's "Aurora.Viewer").
pub const WINDOWS_AUMID: &str = "Folderstorm2.Viewer";

/// Name of the Windows pipe that hands a `secondlife://` link to a running
/// viewer: `\\.\pipe\<prefix><session>…` (Aurora's "aurora-viewer-slurl-").
pub const SLURL_PIPE_PREFIX: &str = "folderstorm2-slurl-";

/// Window name prefix of the Windows link receiver (Aurora's
/// "Aurora.SLurl.Receiver.2.").
pub const SLURL_RECEIVER_PREFIX: &str = "Folderstorm2.SLurl.Receiver.2.";

/// Desktop entry written on Linux for the link handler (Aurora's
/// "aurora-viewer-slurl.desktop").
pub const LINUX_SLURL_DESKTOP_ID: &str = "folderstorm2-slurl.desktop";

/// Folder of the named profiles (`--profile <name>`, session-host plan
/// section 5): `<settings folder>/profiles/<name>`, and the caches of the
/// profiles beside the main cache, in `<cache folder>-profiles/<name>`.
pub const PROFILES_FOLDER: &str = "profiles";

/// What a second launch of a named profile hands to the viewer already
/// running it, through the `secondlife://` hand-over: bring the window
/// forward. Not a link: nothing else reads it.
pub const PROFILE_RAISE_REQUEST: &str = "folderstorm2:raise-profile";

/// Name prefix of the endpoint a managed viewer reports to its session host
/// on: a Windows pipe `\\.\pipe\<prefix><session>-<host pid>-<slot>-<random>`,
/// a socket file of that name elsewhere (session-host plan, section 8).
pub const SESSION_PIPE_PREFIX: &str = "folderstorm2-session-";

/// The session host's settings file, in the main settings folder, and the
/// lock that keeps one host per settings folder.
pub const SESSION_HOST_FILE: &str = "session-host.json";
pub const SESSION_HOST_LOCK: &str = "session-host.lock";

/// The fork's GitHub repository: updates, issues, source links.
pub const REPOSITORY: &str = "Sodie-Pop/Folderstorm2";
pub const GITHUB: &str = "https://github.com/Sodie-Pop/Folderstorm2";
pub const NEW_ISSUE: &str = "https://github.com/Sodie-Pop/Folderstorm2/issues/new";
pub const RELEASES_PAGE: &str = "https://github.com/Sodie-Pop/Folderstorm2/releases";
pub const RELEASES_API: &str = "https://api.github.com/repos/Sodie-Pop/Folderstorm2/releases?per_page=100";

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing that decides where data lives or which system entries the
    /// viewer owns may equal Aurora Viewer's.
    #[test]
    fn identity_never_collides_with_aurora() {
        let aurora = [
            "Aurora",
            "AuroraViewer",
            "AuroraViewerCache",
            "Aurora Viewer",
            "Aurora Viewer MFA",
            "Aurora Viewer cache",
            "Aurora Viewer GitHub",
            "Aurora.Viewer",
            "aurora-viewer-slurl-",
            "Aurora.SLurl.Receiver.2.",
            "aurora-viewer-slurl.desktop",
        ];
        for ours in [
            DIRS_ORGANIZATION,
            DIRS_APPLICATION,
            CACHE_ROOT_NAME,
            KEYRING_LOGIN,
            KEYRING_MFA,
            KEYRING_CACHE,
            KEYRING_GITHUB,
            GRID_CHANNEL,
            WINDOWS_AUMID,
            SLURL_PIPE_PREFIX,
            SLURL_RECEIVER_PREFIX,
            LINUX_SLURL_DESKTOP_ID,
            PROFILE_RAISE_REQUEST,
            SESSION_PIPE_PREFIX,
        ] {
            assert!(!aurora.contains(&ours), "{ours} is Aurora Viewer's");
        }
        // a link never reaches the session host's pipes, nor the reverse
        assert!(!SESSION_PIPE_PREFIX.starts_with(SLURL_PIPE_PREFIX) && !SLURL_PIPE_PREFIX.starts_with(SESSION_PIPE_PREFIX));
    }

    #[test]
    fn links_point_at_the_fork() {
        for link in [GITHUB, NEW_ISSUE, RELEASES_PAGE, RELEASES_API] {
            assert!(link.contains(REPOSITORY), "{link}");
        }
        assert_eq!(concat!(product!(), "/1"), "Folderstorm2/1");
    }
}
