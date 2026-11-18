//! The media plugins pack is downloaded from Aurora Viewer's releases (the
//! fork publishes none), while viewer updates stay on the fork's.

use super::*;

#[test]
fn the_pack_comes_from_aurora_releases() {
    let source = Source::default();
    let url = source
        .media_pack_url(&PACK.tag(), &format!("{}.zip", PACK.folder()))
        .expect("pack url");
    let v = PACK.version;
    assert_eq!(
        url,
        format!("https://github.com/Aurora-Viewer/Aurora-Viewer/releases/download/media-plugins-{v}/Aurora-Media-Plugins-{v}-{SYSTEM}.zip")
    );
    source.validate_url(&url).expect("trusted host");
    // where github.com redirects the download (checked on every redirect)
    source
        .validate_url("https://release-assets.githubusercontent.com/github-production-release-asset/1/2")
        .expect("trusted redirect host");
}

#[test]
fn viewer_updates_still_come_from_the_fork() {
    let url = Source::default().asset_url("v1.0.0", "update.zip").expect("asset url");
    assert_eq!(url, "https://github.com/Sodie-Pop/Folderstorm2/releases/download/v1.0.0/update.zip");
    assert!(crate::feed::RELEASES.contains(folderstorm_brand::REPOSITORY));
}

#[test]
fn a_test_feed_serves_the_pack_itself() {
    let source = Source::local("http://127.0.0.1:8080/releases").expect("loopback feed");
    let url = source.media_pack_url("media-plugins-1", "pack.zip").expect("pack url");
    assert_eq!(url, "http://127.0.0.1:8080/download/media-plugins-1/pack.zip");
    source.validate_url(&url).expect("loopback asset");
}
