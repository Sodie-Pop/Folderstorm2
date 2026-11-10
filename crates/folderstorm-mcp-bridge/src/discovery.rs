//! The discovery file: how the sidecar finds a running viewer. Folderstorm
//! (C++) writes `fs-mcp-<pid>.json` with the port and the bearer token in
//! its user_settings folder; Folderstorm2 writes
//! `folderstorm2-mcp-<pid>.json` in its settings folder, readable by the
//! user only where the OS supports it, and deletes it when the bridge stops.

use crate::token::Token;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The environment variable naming the folder that holds the discovery
/// files (Folderstorm's `FIRESTORM_MCP_DISCOVERY`), when it is not the
/// default settings folder: a demo run, a custom settings folder.
pub const ENV: &str = "FOLDERSTORM2_MCP_DISCOVERY";

/// Format of the file; a reader skips another.
pub const SCHEMA: u32 = 1;

/// The largest discovery file a reader accepts.
const MAX_FILE: u64 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Discovery {
    pub schema: u32,
    pub product: String,
    pub pid: u32,
    pub port: u16,
    pub token: String,
    /// Unix seconds when the bridge started.
    pub started: u64,
}

impl Discovery {
    pub fn new(pid: u32, port: u16, token: &Token) -> Discovery {
        Discovery {
            schema: SCHEMA,
            product: folderstorm_brand::PRODUCT.to_owned(),
            pid,
            port,
            token: token.secret().to_owned(),
            started: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        }
    }

    pub fn token(&self) -> Token {
        Token::from_secret(self.token.clone())
    }
}

fn prefix() -> String {
    format!("{}-mcp-", folderstorm_brand::SLUG)
}

/// `folderstorm2-mcp-<pid>.json`.
pub fn file_name(pid: u32) -> String {
    format!("{}{pid}.json", prefix())
}

/// The folder the sidecar reads by default: the environment override, else
/// the product's settings folder (where the viewer keeps settings.json).
pub fn default_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    folderstorm_brand::project_dirs().map(|d| d.config_dir().to_path_buf())
}

/// Write the file for `discovery` into `dir`: a temporary file made
/// readable by the user only, then renamed into place, so that a reader
/// never sees half a file nor a moment of wider permissions.
pub fn write(dir: &Path, discovery: &Discovery) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name(discovery.pid));
    let temp = dir.join(format!(".{}.tmp", file_name(discovery.pid)));
    let body = serde_json::to_vec_pretty(discovery).map_err(std::io::Error::other)?;
    let _ = std::fs::remove_file(&temp);
    let result = (|| {
        let mut file = private_file(&temp)?;
        file.write_all(&body)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map(|()| path)
}

/// A new file only its owner can read and write (Unix mode 0600). Windows
/// has no mode: the user's AppData folder is already private to the user.
fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Delete the file; a file already gone is not an error.
pub fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Read one discovery file, bounded in size; None when it is not one.
pub fn read(path: &Path) -> Option<Discovery> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let found: Discovery = serde_json::from_str(&text).ok()?;
    (found.schema == SCHEMA && found.port != 0 && !found.token.is_empty()).then_some(found)
}

/// The discovery files of `dir`, newest first, only `pid`'s when given.
/// Files left by a viewer that crashed are still listed: the caller skips
/// those whose port does not answer.
pub fn find(dir: &Path, pid: Option<u32>) -> Vec<(PathBuf, Discovery)> {
    let prefix = prefix();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(".json")
        })
        .filter_map(|e| read(&e.path()).map(|d| (e.path(), d)))
        .filter(|(path, d)| {
            // the name and the content agree, or the file was renamed by hand
            path.file_name().is_some_and(|n| n.to_string_lossy() == file_name(d.pid))
        })
        .filter(|(_, d)| pid.is_none_or(|pid| d.pid == pid))
        .collect();
    found.sort_by(|a, b| b.1.started.cmp(&a.1.started).then(b.1.pid.cmp(&a.1.pid)));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(pid: u32, started: u64) -> Discovery {
        let token = Token::generate().expect("random source");
        Discovery {
            started,
            ..Discovery::new(pid, 40_000 + pid as u16, &token)
        }
    }

    #[test]
    fn written_then_read_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let d = sample(7, 100);
        let path = write(dir.path(), &d).expect("written");
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("folderstorm2-mcp-7.json"));
        assert_eq!(read(&path), Some(d.clone()));
        assert_eq!(find(dir.path(), None), vec![(path.clone(), d)]);
        // no temporary file is left beside it
        assert_eq!(std::fs::read_dir(dir.path()).expect("listed").count(), 1);
        remove(&path).expect("removed");
        assert!(!path.exists());
        remove(&path).expect("removing twice is fine");
    }

    #[cfg(unix)]
    #[test]
    fn only_the_user_may_read_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write(dir.path(), &sample(8, 1)).expect("written");
        let mode = std::fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn newest_first_and_filtered_by_pid() {
        let dir = tempfile::tempdir().expect("temp dir");
        let old = sample(10, 100);
        let new = sample(11, 200);
        write(dir.path(), &old).expect("written");
        write(dir.path(), &new).expect("written");
        let pids: Vec<_> = find(dir.path(), None).into_iter().map(|(_, d)| d.pid).collect();
        assert_eq!(pids, vec![11, 10]);
        let only: Vec<_> = find(dir.path(), Some(10)).into_iter().map(|(_, d)| d.pid).collect();
        assert_eq!(only, vec![10]);
        assert!(find(dir.path(), Some(12)).is_empty());
    }

    #[test]
    fn other_files_are_skipped() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("settings.json"), "{}").expect("written");
        std::fs::write(dir.path().join("folderstorm2-mcp-1.json"), "not json").expect("written");
        std::fs::write(dir.path().join("folderstorm2-mcp-2.json"), vec![b' '; 5000]).expect("written");
        // a file whose name does not match its pid
        let d = sample(3, 1);
        std::fs::write(dir.path().join("folderstorm2-mcp-4.json"), serde_json::to_string(&d).expect("json")).expect("written");
        let mut wrong = sample(5, 1);
        wrong.schema = 99;
        std::fs::write(dir.path().join(file_name(5)), serde_json::to_string(&wrong).expect("json")).expect("written");
        assert!(find(dir.path(), None).is_empty());
        assert!(find(&dir.path().join("missing"), None).is_empty());
    }
}
