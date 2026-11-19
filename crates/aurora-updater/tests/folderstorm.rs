//! Folderstorm: the fork's release packages. Shipped builds hold
//! `Folderstorm2.exe` and the MCP sidecar beside it, in a folder named after
//! the brand (release plan, findings F1 to F3): the updater must take such a
//! package, replace the sidecar with the viewer, and refuse Aurora's.

use aurora_updater::{Cancel, install, package};
use folderstorm_brand::{EXE, MCP_EXE, RELEASE_PREFIX};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("folderstorm-updater-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).expect("test directory");
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        assert!(self.0.parent() == Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn zip(entries: &[(String, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, value) in entries {
        writer
            .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .expect("ZIP member");
        writer.write_all(value).expect("ZIP data");
    }
    writer.finish().expect("ZIP").into_inner()
}

/// The Windows package of `v0.3.0`, under the folder `root`.
fn windows_package(root: &str) -> Vec<u8> {
    zip(&[
        (format!("{root}/{EXE}"), b"new viewer"),
        (format!("{root}/{MCP_EXE}"), b"new sidecar"),
        (format!("{root}/assets/emoji/LICENSE-OFL.txt"), b"font licence"),
        (format!("{root}/LICENSE"), b"license"),
        (format!("{root}/NOTICE.md"), b"notice"),
        (format!("{root}/README.md"), b"readme"),
        (format!("{root}/aurora-install.json"), br#"{"schema":1,"version":"0.3.0"}"#),
    ])
}

#[test]
fn the_forks_windows_package_extracts_with_its_sidecar() {
    let temp = Temp::new();
    let archive = temp.0.join(format!("{RELEASE_PREFIX}-v0.3.0-windows-x64.zip"));
    fs::write(&archive, windows_package(&format!("{RELEASE_PREFIX}-v0.3.0-windows-x64"))).expect("ZIP file");
    let root = package::extract(&archive, &temp.0.join("prepared"), "v0.3.0", &Cancel::default()).expect("extracted");
    assert_eq!(fs::read(root.join(EXE)).expect("viewer"), b"new viewer");
    assert_eq!(fs::read(root.join(MCP_EXE)).expect("sidecar"), b"new sidecar");
}

#[test]
fn a_package_under_auroras_name_is_refused() {
    let temp = Temp::new();
    let archive = temp.0.join("package.zip");
    fs::write(&archive, windows_package("Aurora-Viewer-v0.3.0-windows-x64")).expect("ZIP file");
    assert!(package::extract(&archive, &temp.0.join("prepared"), "v0.3.0", &Cancel::default()).is_err());
    assert!(!temp.0.join("prepared").exists());
}

/// The viewer's executable as this system's package names it: the swap
/// runs on any system, the Windows name is checked above.
fn exe() -> &'static str {
    if cfg!(windows) { EXE } else { "aurora-viewer" }
}

fn installation(path: &Path, version: &[u8], sidecar: bool) {
    fs::create_dir_all(path.join("assets")).expect("assets");
    fs::write(path.join(exe()), version).expect("viewer");
    fs::write(path.join("LICENSE"), b"license").expect("license");
    fs::write(path.join("aurora-install.json"), version).expect("manifest");
    if sidecar {
        fs::write(path.join(MCP_EXE), version).expect("sidecar");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path.join(exe()), fs::Permissions::from_mode(0o755)).expect("executable");
    }
}

#[test]
fn an_update_replaces_the_sidecar_with_the_viewer() {
    for installed_with_sidecar in [true, false] {
        let temp = Temp::new();
        let path = temp.0.join("install");
        installation(&path, b"old", installed_with_sidecar);
        let work = install::Workspace::new(&path).expect("workspace");
        installation(&work.root.join("prepared"), b"new", true);
        install::apply(&path, &work.root, || Ok(())).expect("an update holding the sidecar is accepted");
        assert_eq!(fs::read(path.join(exe())).expect("new viewer"), b"new");
        assert_eq!(fs::read(path.join(MCP_EXE)).expect("new sidecar"), b"new");
    }
}

#[test]
fn a_failed_update_restores_the_old_sidecar() {
    let temp = Temp::new();
    let path = temp.0.join("install");
    installation(&path, b"old", true);
    let work = install::Workspace::new(&path).expect("workspace");
    installation(&work.root.join("prepared"), b"new", true);
    assert!(install::apply(&path, &work.root, || anyhow::bail!("no frame")).is_err());
    assert_eq!(fs::read(path.join(exe())).expect("restored viewer"), b"old");
    assert_eq!(fs::read(path.join(MCP_EXE)).expect("restored sidecar"), b"old");
}
