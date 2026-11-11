use super::image::{self as img, Format, Refusal};
use super::token::{Auth, TokenStore, resolve};
use super::zipline::{self, Cancel, Failure, HttpReply};
use super::*;
use std::io::{Read, Write};
use std::sync::mpsc;

// ---- a local stand-in for the host

/// What the loopback server received.
struct Received {
    head: String,
    body: Vec<u8>,
}

impl Received {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|line| {
            let (n, v) = line.split_once(':')?;
            n.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
        })
    }
}

/// A server on this computer answering one request with `status` and
/// `body`; the address and what it received.
fn serve(status: &'static str, body: &'static str) -> (String, mpsc::Receiver<Received>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback port");
    let origin = format!("http://{}", listener.local_addr().expect("address"));
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else { return };
        let mut data = Vec::new();
        let mut buf = [0u8; 8192];
        let head_end = loop {
            let Ok(n) = stream.read(&mut buf) else { return };
            if n == 0 {
                return;
            }
            data.extend_from_slice(&buf[..n]);
            if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
        };
        let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
        let len: usize = head
            .lines()
            .find_map(|l| l.split_once(':').filter(|(n, _)| n.eq_ignore_ascii_case("content-length")))
            .and_then(|(_, v)| v.trim().parse().ok())
            .unwrap_or(0);
        while data.len() < head_end + len {
            let Ok(n) = stream.read(&mut buf) else { break };
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
        }
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = tx.send(Received {
            head,
            body: data[head_end..].to_vec(),
        });
    });
    (origin, rx)
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let picture = ::image::RgbaImage::from_fn(width, height, |x, y| ::image::Rgba([x as u8, y as u8, 128, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut out, ::image::ImageFormat::Png).expect("png");
    out.into_inner()
}

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let picture = ::image::RgbImage::from_fn(width, height, |x, y| ::image::Rgb([x as u8, y as u8, 90]));
    let mut out = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut out, ::image::ImageFormat::Jpeg).expect("jpeg");
    out.into_inner()
}

fn prepared_png() -> img::Prepared {
    img::prepare_bytes(png(4, 3), 1 << 20).expect("a valid png")
}

const OK: &str = r#"{"files":[{"id":"cm1","name":"Ab12Cd.png","type":"image/png","url":"https://elsewhere.example/u/Ab12Cd.png"}]}"#;

// ---- the client

#[test]
fn the_upload_request_is_zipline_s_and_the_link_is_the_raw_one() {
    let (origin, received) = serve("200 OK", OK);
    let image = prepared_png();
    let link = upload(
        &zipline::Network,
        &origin,
        Auth::User("tok-123".into()),
        Some("folder_1"),
        &image,
        &Cancel::default(),
    )
    .expect("a link");
    assert_eq!(link, format!("{origin}/raw/Ab12Cd.png"));
    let r = received.recv().expect("a request");
    assert!(r.head.starts_with("POST /api/upload HTTP/1.1"), "{}", r.head);
    // the token as it is, as Zipline reads it (no "Bearer")
    assert_eq!(r.header("authorization").as_deref(), Some("tok-123"));
    assert_eq!(r.header("x-zipline-folder").as_deref(), Some("folder_1"));
    assert_eq!(r.header("x-zipline-format").as_deref(), Some("random"));
    assert_eq!(r.header("accept").as_deref(), Some("application/json"));
    assert!(r.header("cookie").is_none());
    let content_type = r.header("content-type").expect("content type");
    let boundary = content_type.strip_prefix("multipart/form-data; boundary=").expect("multipart");
    let body = &r.body;
    assert!(body.starts_with(format!("--{boundary}\r\n").as_bytes()));
    assert!(body.ends_with(format!("\r\n--{boundary}--\r\n").as_bytes()));
    let text = String::from_utf8_lossy(body);
    assert!(text.contains("Content-Disposition: form-data; name=\"file\"; filename=\"image.png\""));
    assert!(text.contains("Content-Type: image/png"));
    assert!(body.windows(image.bytes.len()).any(|w| w == image.bytes.as_slice()));
}

#[test]
fn without_a_token_no_authorization_header_is_sent() {
    let (origin, received) = serve("200 OK", OK);
    upload(
        &zipline::Network,
        &origin,
        Auth::None,
        Some("anon"),
        &prepared_png(),
        &Cancel::default(),
    )
    .expect("a link");
    let r = received.recv().expect("a request");
    assert!(r.header("authorization").is_none());
    assert_eq!(r.header("x-zipline-folder").as_deref(), Some("anon"));
}

#[test]
fn a_refused_token_and_a_missing_one_are_told_apart() {
    let refusal = r#"{"error":"E2001: Invalid token","code":2001,"statusCode":401}"#;
    let (origin, _r) = serve("401 Unauthorized", refusal);
    let got = upload(
        &zipline::Network,
        &origin,
        Auth::User("bad".into()),
        None,
        &prepared_png(),
        &Cancel::default(),
    );
    assert_eq!(got, Err(Failure::Token { had_token: true }));
    let (origin, _r) = serve("401 Unauthorized", refusal);
    let got = upload(&zipline::Network, &origin, Auth::None, None, &prepared_png(), &Cancel::default());
    assert_eq!(got, Err(Failure::Token { had_token: false }));
}

#[test]
fn too_large_for_the_host() {
    let (origin, _r) = serve(
        "413 Payload Too Large",
        r#"{"error":"E5001: File is too large","code":5001,"statusCode":413}"#,
    );
    let got = upload(
        &zipline::Network,
        &origin,
        Auth::None,
        Some("f"),
        &prepared_png(),
        &Cancel::default(),
    );
    assert_eq!(got, Err(Failure::TooLarge));
    // a proxy's bare 413
    let (origin, _r) = serve("413 Payload Too Large", "<html>too big</html>");
    let got = upload(
        &zipline::Network,
        &origin,
        Auth::None,
        Some("f"),
        &prepared_png(),
        &Cancel::default(),
    );
    assert_eq!(got, Err(Failure::TooLarge));
}

#[test]
fn a_full_host_and_a_closed_folder() {
    let (origin, _r) = serve(
        "413 Payload Too Large",
        r#"{"error":"E5002: Storage quota exceeded","code":5002,"statusCode":413}"#,
    );
    let got = upload(
        &zipline::Network,
        &origin,
        Auth::None,
        Some("f"),
        &prepared_png(),
        &Cancel::default(),
    );
    assert_eq!(got, Err(Failure::Quota));
    let (origin, _r) = serve(
        "403 Forbidden",
        r#"{"error":"E3002: Folder is not open","code":3002,"statusCode":403}"#,
    );
    let got = upload(
        &zipline::Network,
        &origin,
        Auth::None,
        Some("f"),
        &prepared_png(),
        &Cancel::default(),
    );
    assert_eq!(got, Err(Failure::Folder));
}

#[test]
fn a_malformed_answer_gives_no_link() {
    for body in [
        "not json",
        r#"{"files":[]}"#,
        r#"{"files":[{"name":"../x","url":"https://evil.example/x.png"}]}"#,
    ] {
        let (origin, _r) = serve("200 OK", body);
        let got = upload(
            &zipline::Network,
            &origin,
            Auth::None,
            Some("f"),
            &prepared_png(),
            &Cancel::default(),
        );
        assert_eq!(got, Err(Failure::Malformed), "{body}");
    }
}

#[test]
fn a_redirect_is_never_followed() {
    let (origin, _r) = serve("302 Found", "");
    let got = upload(
        &zipline::Network,
        &origin,
        Auth::User("tok".into()),
        None,
        &prepared_png(),
        &Cancel::default(),
    );
    assert_eq!(got, Err(Failure::Refused(302)));
}

#[test]
fn an_unreachable_host_is_a_network_failure() {
    // a port that was just free
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("port")
        .port();
    let got = upload(
        &zipline::Network,
        &format!("http://127.0.0.1:{port}"),
        Auth::None,
        None,
        &prepared_png(),
        &Cancel::default(),
    );
    assert_eq!(got, Err(Failure::Network));
}

/// A host that takes the request and never answers.
fn silent_host() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback port");
    let origin = format!("http://{}", listener.local_addr().expect("address"));
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else { return };
        let mut buf = [0u8; 8192];
        // read until the client goes away
        while stream.read(&mut buf).is_ok_and(|n| n > 0) {}
    });
    origin
}

#[test]
fn stopping_an_upload_aborts_its_request() {
    let origin = silent_host();
    let cancel = Cancel::default();
    let (tx, rx) = mpsc::channel();
    let stop = cancel.clone();
    std::thread::spawn(move || {
        assert!(stop.start());
        let _ = tx.send(upload(&zipline::Network, &origin, Auth::None, Some("f"), &prepared_png(), &stop));
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(cancel.cancel(), "the request had left");
    let got = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the request was dropped, not left to time out");
    assert_eq!(got, Err(Failure::Cancelled));
}

/// A transport that waits for the Stop.
struct Hang(std::sync::atomic::AtomicBool);

impl zipline::Transport for Hang {
    fn send(&self, _: zipline::HttpRequest, cancel: &Cancel) -> Result<HttpReply, Failure> {
        self.0.store(true, Ordering::SeqCst);
        while !cancel.is_cancelled() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(Failure::Cancelled)
    }
}

fn job(id: u64, state: State) -> Job {
    Job {
        id,
        field: egui::Id::unique(("test-job", id)),
        label: "a.png".into(),
        origin: "https://h.example".into(),
        folder: None,
        state,
        image: Some(Arc::new(prepared_png())),
        texture: None,
        cancel: Cancel::default(),
    }
}

#[test]
fn a_stopped_upload_says_the_host_may_have_the_image() {
    // the request had left: the job stays, saying so, and nothing else
    let mut sending = job(1, State::Uploading);
    let transport = Arc::new(Hang(AtomicBool::new(false)));
    let (tx, rx) = mpsc::channel();
    let (cancel, t) = (sending.cancel.clone(), transport.clone());
    std::thread::spawn(move || {
        assert!(cancel.start());
        let _ = tx.send(upload(&*t, "https://h.example", Auth::None, None, &prepared_png(), &cancel));
    });
    while !transport.0.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(stop(&mut sending));
    assert!(matches!(&sending.state, State::Failed { retry: false, message } if !message.is_empty()));
    assert!(sending.image.is_none());
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).expect("stopped"),
        Err(Failure::Cancelled)
    );
    // a link coming back after the Stop goes nowhere
    let id = u64::MAX - 7;
    uploads().jobs.push(job(id, State::Uploading));
    let cancel = uploads().jobs.last().map(|j| j.cancel.clone()).expect("the job");
    cancel.cancel();
    finish(id, &cancel, Ok("https://h.example/raw/x.png".into()));
    let state = uploads().jobs.iter().find(|j| j.id == id).map(|j| j.state.clone());
    uploads().jobs.retain(|j| j.id != id);
    assert!(matches!(state, Some(State::Uploading)));
    // still waiting for its turn: dropped, and its thread sends nothing
    let mut waiting = job(2, State::Uploading);
    assert!(!stop(&mut waiting));
    assert!(!waiting.cancel.start());
    // a confirmation or a failure is simply dismissed
    assert!(!stop(&mut job(3, State::Confirm)));
    assert!(!stop(&mut job(
        4,
        State::Failed {
            message: String::new(),
            retry: true
        }
    )));
}

#[test]
fn the_link_stays_on_the_host() {
    let origin = "https://images.example.org";
    assert_eq!(
        zipline::link(origin, "a-b_C.9.gif", "").as_deref(),
        Some("https://images.example.org/raw/a-b_C.9.gif")
    );
    // a name to encode: the server's own link, only on the host
    assert_eq!(
        zipline::link(origin, "a b.png", "https://images.example.org/u/a%20b.png").as_deref(),
        Some("https://images.example.org/u/a%20b.png")
    );
    assert_eq!(zipline::link(origin, "a b.png", "https://other.example/u/a.png"), None);
    assert_eq!(zipline::link(origin, "a b.png", "http://images.example.org/u/a.png"), None);
    assert_eq!(zipline::link(origin, "a b.png", "https://user:pw@images.example.org/u/a.png"), None);
    assert_eq!(zipline::link(origin, ".hidden", "javascript:alert(1)"), None);
}

#[test]
fn an_oversized_answer_is_refused() {
    let reply = HttpReply {
        status: 200,
        body: vec![b' '; zipline::MAX_ANSWER + 1],
    };
    assert_eq!(zipline::parse("https://h.example", &reply, false), Err(Failure::Malformed));
}

#[test]
fn the_request_s_debug_print_hides_the_token() {
    let request = zipline::request(
        "https://h.example",
        &Auth::User("secret-token".into()),
        Some("secret-folder"),
        &prepared_png(),
    );
    let printed = format!("{request:?} {:?}", Auth::User("secret-token".into()));
    assert!(!printed.contains("secret-token"), "{printed}");
    assert!(!printed.contains("secret-folder"), "{printed}");
}

// ---- the token and the folder

#[test]
fn only_the_user_s_own_token_is_ever_sent() {
    // a build carries no token: without the user's, no authorization header
    assert_eq!(resolve(Some("mine".into())), Auth::User("mine".into()));
    assert_eq!(resolve(None), Auth::None);
    assert_eq!(resolve(Some("  ".into())), Auth::None);
    assert_eq!(resolve(Some("two words".into())), Auth::None);
    let request = zipline::request(
        &settings::origin(folderstorm_brand::IMAGE_HOST).expect("default host"),
        &resolve(None),
        Some("builtFolder1"),
        &prepared_png(),
    );
    assert!(request.headers.iter().all(|(name, _)| *name != "authorization"));
}

#[test]
fn the_user_s_folder_comes_first_then_the_built_in_one_on_its_host_only() {
    let default = settings::origin(folderstorm_brand::IMAGE_HOST).expect("default host");
    let built = Some("builtFolder1");
    assert_eq!(settings::resolve_folder(&default, Some("mine"), built).as_deref(), Some("mine"));
    assert_eq!(settings::resolve_folder(&default, None, built).as_deref(), Some("builtFolder1"));
    assert_eq!(
        settings::resolve_folder(&default, Some("bad folder!"), built).as_deref(),
        Some("builtFolder1")
    );
    assert_eq!(settings::resolve_folder("https://other.example", None, built), None);
    assert_eq!(
        settings::resolve_folder("https://other.example", Some("mine"), built).as_deref(),
        Some("mine")
    );
    assert_eq!(settings::resolve_folder(&default, None, None), None);
}

#[test]
fn a_folder_is_only_ever_sent_to_the_host_it_was_entered_for() {
    let mut s = UploadSettings {
        host: "https://images.example.org".into(),
        ..UploadSettings::default()
    };
    let first = s.origin().expect("origin");
    s.set_folder(&first, "folder_1".into());
    assert_eq!(s.folder_for(&first), Some("folder_1"));
    // another host: no folder until one is entered for it
    s.host = "https://other.example".into();
    let second = s.origin().expect("origin");
    assert_eq!(s.folder_for(&second), None);
    assert_eq!(settings::resolve_folder(&second, s.folder_for(&second), None), None);
    s.set_folder(&second, "folder_2".into());
    assert_eq!(s.folder_for(&second), Some("folder_2"));
    assert_eq!(s.folder_for(&first), Some("folder_1"));
    s.set_folder(&second, String::new());
    assert!(!s.folders.contains_key(&second));
    // the map round-trips
    let json = serde_json::to_string(&s).expect("serialised");
    let back: UploadSettings = serde_json::from_str(&json).expect("read back");
    assert_eq!(back, s);
    assert!(!json.contains("\"folder\""), "{json}");
}

#[test]
fn an_old_single_folder_moves_to_the_host_set_with_it() {
    let old: UploadSettings =
        serde_json::from_str(r#"{"enabled":true,"host":"https://images.example.org/","folder":" f1 ","max_mib":5}"#).expect("read");
    assert!(old.enabled);
    assert_eq!(old.max_mib, 5);
    assert!(old.confirm);
    assert_eq!(old.folder_for("https://images.example.org"), Some("f1"));
    assert_eq!(old.folders.len(), 1);
    // with an unusable host the old folder goes nowhere
    let lost: UploadSettings = serde_json::from_str(r#"{"host":"http://h.example","folder":"f1"}"#).expect("read");
    assert!(lost.folders.is_empty());
    // a folder already kept for the host wins over the old one
    let both: UploadSettings =
        serde_json::from_str(r#"{"host":"https://h.example","folder":"old","folders":{"https://h.example":"new"}}"#).expect("read");
    assert_eq!(both.folder_for("https://h.example"), Some("new"));
}

#[test]
fn host_addresses() {
    assert_eq!(
        settings::origin(" https://images.sodie.online/ ").as_deref(),
        Some("https://images.sodie.online")
    );
    assert_eq!(
        settings::origin("https://h.example:8443").as_deref(),
        Some("https://h.example:8443")
    );
    assert_eq!(settings::origin("http://h.example"), None);
    assert_eq!(settings::origin("https://h.example/api"), None);
    assert_eq!(settings::origin("https://u:p@h.example"), None);
    assert_eq!(settings::origin("https://h.example?x=1"), None);
    assert_eq!(settings::origin("images.sodie.online"), None);
    assert_eq!(settings::origin("http://127.0.0.1:9000").as_deref(), Some("http://127.0.0.1:9000"));
}

/// A credential store in memory.
#[derive(Default)]
struct Memory(Mutex<HashMap<String, String>>);

impl TokenStore for Memory {
    fn read(&self, origin: &str) -> Option<String> {
        self.0.lock().ok()?.get(origin).cloned()
    }
    fn write(&self, origin: &str, token: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.insert(origin.into(), token.into());
        Ok(())
    }
    fn remove(&self, origin: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.remove(origin);
        Ok(())
    }
}

#[test]
fn the_settings_round_trip_without_the_token() {
    let store = Memory::default();
    let mut s = crate::settings::Settings::default();
    s.folderstorm_chat_upload.enabled = true;
    s.folderstorm_chat_upload.host = "https://images.example.org".into();
    s.folderstorm_chat_upload.max_mib = 5;
    s.folderstorm_chat_upload.confirm = false;
    let origin = s.folderstorm_chat_upload.origin().expect("origin");
    store.write(&origin, "very-secret-token").expect("saved");
    let json = serde_json::to_string(&s).expect("serialised");
    assert!(!json.contains("very-secret-token"));
    let section = serde_json::to_value(&s.folderstorm_chat_upload).expect("section");
    let keys: Vec<&str> = section.as_object().expect("an object").keys().map(String::as_str).collect();
    assert!(!keys.iter().any(|k| k.contains("token")), "{keys:?}");
    let back: crate::settings::Settings = serde_json::from_str(&json).expect("read back");
    assert_eq!(back.folderstorm_chat_upload, s.folderstorm_chat_upload);
    assert_eq!(store.read(&origin).as_deref(), Some("very-secret-token"));
    store.remove(&origin).expect("removed");
    assert_eq!(store.read(&origin), None);
    // an old settings file without the section gets the defaults: off
    let old: crate::settings::Settings = serde_json::from_str("{}").expect("defaults");
    assert_eq!(old.folderstorm_chat_upload, UploadSettings::default());
    assert!(!old.folderstorm_chat_upload.enabled);
    assert!(old.folderstorm_chat_upload.confirm);
    assert_eq!(old.folderstorm_chat_upload.max_bytes(), 10 * 1024 * 1024);
}

// ---- the image

#[test]
fn formats_are_recognised_by_their_content() {
    assert_eq!(img::sniff(&png(1, 1)), Some(Format::Png));
    assert_eq!(img::sniff(&jpeg(8, 8)), Some(Format::Jpeg));
    assert_eq!(img::sniff(b"GIF89a......"), Some(Format::Gif));
    assert_eq!(img::sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(Format::Webp));
    assert_eq!(img::sniff(b"BM......"), None);
    assert_eq!(img::prepare_bytes(b"<svg/>".to_vec(), 1 << 20).err(), Some(Refusal::Unsupported));
}

#[test]
fn the_size_limit_is_checked_first() {
    let bytes = png(64, 64);
    let len = bytes.len() as u64;
    assert_eq!(
        img::prepare_bytes(bytes.clone(), len - 1).err(),
        Some(Refusal::TooLarge(len, len - 1))
    );
    assert!(img::prepare_bytes(bytes, len).is_ok());
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("big.png");
    std::fs::write(&path, png(64, 64)).expect("written");
    assert_eq!(img::prepare_file(&path, 10).err(), Some(Refusal::TooLarge(len, 10)));
    assert!(img::prepare_file(&path, 1 << 20).is_ok());
    assert_eq!(
        img::prepare_file(&dir.path().join("none.png"), 1 << 20).err(),
        Some(Refusal::Unreadable)
    );
    assert_eq!(img::prepare_file(dir.path(), 1 << 20).err(), Some(Refusal::Unreadable));
}

#[test]
fn a_broken_or_huge_picture_is_refused() {
    let mut bytes = png(8, 8);
    bytes.truncate(40);
    assert!(matches!(img::prepare_bytes(bytes, 1 << 20).err(), Some(Refusal::Corrupt)));
    // a header announcing 5000 × 5000: refused before decoding
    let mut huge = png(1, 1);
    huge[16..20].copy_from_slice(&5000u32.to_be_bytes());
    huge[20..24].copy_from_slice(&5000u32.to_be_bytes());
    // the header's checksum, for decoders that check it
    let crc = crc32(&huge[12..29]);
    huge[29..33].copy_from_slice(&crc.to_be_bytes());
    assert_eq!(img::prepare_bytes(huge, 1 << 20).err(), Some(Refusal::TooManyPixels(5000, 5000)));
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// An EXIF segment with an orientation and a made-up GPS-like text.
fn exif_segment(orientation: u16) -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"MM\0*");
    tiff.extend_from_slice(&8u32.to_be_bytes());
    tiff.extend_from_slice(&1u16.to_be_bytes());
    tiff.extend_from_slice(&0x0112u16.to_be_bytes());
    tiff.extend_from_slice(&3u16.to_be_bytes());
    tiff.extend_from_slice(&1u32.to_be_bytes());
    tiff.extend_from_slice(&orientation.to_be_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_be_bytes());
    tiff.extend_from_slice(b"GPS 48.8566N 2.3522E");
    let mut seg = vec![0xff, 0xe1];
    seg.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
    seg.extend_from_slice(b"Exif\0\0");
    seg.extend_from_slice(&tiff);
    seg
}

#[test]
fn a_jpeg_loses_its_metadata_but_keeps_its_orientation() {
    let plain = jpeg(16, 8);
    let mut tagged = plain[..2].to_vec();
    tagged.extend_from_slice(&exif_segment(6));
    let comment = b"taken by Jane at home";
    tagged.extend_from_slice(&[0xff, 0xfe]);
    tagged.extend_from_slice(&((comment.len() + 2) as u16).to_be_bytes());
    tagged.extend_from_slice(comment);
    tagged.extend_from_slice(&plain[2..]);
    let prepared = img::prepare_bytes(tagged, 1 << 20).expect("prepared");
    assert!(prepared.stripped);
    assert_eq!((prepared.width, prepared.height), (16, 8));
    let out = &prepared.bytes;
    assert!(!out.windows(3).any(|w| w == b"GPS"));
    assert!(!out.windows(comment.len()).any(|w| w == comment));
    // the orientation alone is back
    let at = out.windows(6).position(|w| w == b"Exif\0\0").expect("an orientation segment");
    assert_eq!(img::exif_orientation(&out[at + 6..]), Some(6));
    ::image::load_from_memory(out).expect("still a picture");
    // a JPEG without metadata is sent as it is
    let clean = img::prepare_bytes(plain.clone(), 1 << 20).expect("prepared");
    assert!(!clean.stripped);
    assert_eq!(clean.bytes, plain);
}

/// A progressive JPEG (16 × 8, several scans) made with Pillow.
const PROGRESSIVE: &[u8] = include_bytes!("testdata/progressive.jpg");

/// The offsets of a JPEG's markers of `kind` (outside entropy-coded data
/// for this file, whose scans hold no such byte pair).
fn markers(bytes: &[u8], kind: u8) -> Vec<usize> {
    (0..bytes.len().saturating_sub(1))
        .filter(|&n| bytes[n] == 0xff && bytes[n + 1] == kind)
        .collect()
}

#[test]
fn nothing_after_the_end_of_a_jpeg_is_sent() {
    let plain = jpeg(16, 8);
    // a phone's second picture (MPF) with its own EXIF and location, then
    // a motion photo's video with a location atom
    let mut tagged = plain.clone();
    tagged.extend_from_slice(&[0xff, 0xd8]);
    tagged.extend_from_slice(&exif_segment(1));
    tagged.extend_from_slice(&plain[2..]);
    tagged.extend_from_slice(b"\0\0\0\x18ftypmp42\0\0\0\0isommp42\0\0\0\x1c\xa9xyz+48.8566+002.3522/");
    let prepared = img::prepare_bytes(tagged, 1 << 20).expect("prepared");
    assert!(prepared.stripped);
    assert_eq!(prepared.bytes, plain);
    assert!(!prepared.bytes.windows(3).any(|w| w == b"GPS"));
    assert!(!prepared.bytes.windows(4).any(|w| w == b"\xa9xyz"));
}

#[test]
fn segments_between_the_scans_of_a_progressive_jpeg_are_left_out() {
    let plain = img::prepare_bytes(PROGRESSIVE.to_vec(), 1 << 20).expect("prepared");
    assert!(!plain.stripped);
    assert_eq!(plain.bytes, PROGRESSIVE);
    let scans = markers(PROGRESSIVE, 0xda);
    assert!(scans.len() > 2, "{scans:?}");
    // before the second and the last scans: EXIF with a location, a comment
    let mut tagged = PROGRESSIVE[..scans[1]].to_vec();
    tagged.extend_from_slice(&exif_segment(6));
    let last = *scans.last().expect("a last scan");
    tagged.extend_from_slice(&PROGRESSIVE[scans[1]..last]);
    let comment = b"taken at home";
    tagged.extend_from_slice(&[0xff, 0xfe]);
    tagged.extend_from_slice(&((comment.len() + 2) as u16).to_be_bytes());
    tagged.extend_from_slice(comment);
    tagged.extend_from_slice(&PROGRESSIVE[last..]);
    let prepared = img::prepare_bytes(tagged, 1 << 20).expect("prepared");
    assert!(prepared.stripped);
    // an orientation found after the first scan comes too late to count
    assert_eq!(prepared.bytes, PROGRESSIVE);
    ::image::load_from_memory(&prepared.bytes).expect("still a picture");
}

#[test]
fn a_jpeg_cut_short_never_panics_nor_hangs() {
    let mut tagged = PROGRESSIVE.to_vec();
    tagged.extend_from_slice(&exif_segment(3));
    for source in [PROGRESSIVE.to_vec(), jpeg(16, 8), tagged] {
        for cut in 0..source.len() {
            let piece = &source[..cut];
            if let Ok((out, _)) = img::strip_jpeg(piece) {
                // closed, and never longer than the piece and its EOI
                assert!(out.ends_with(&[0xff, 0xd9]), "cut at {cut}");
                assert!(out.len() <= piece.len() + 2, "cut at {cut}");
            }
            let _ = img::prepare_bytes(piece.to_vec(), 1 << 20);
        }
    }
    // stuffed bytes and restart markers are picture data, not segments
    let mut scan = PROGRESSIVE[..markers(PROGRESSIVE, 0xda)[0]].to_vec();
    scan.extend_from_slice(&[0xff, 0xda, 0, 8, 1, 1, 0, 0, 0x3f, 0]);
    scan.extend_from_slice(&[0x12, 0xff, 0x00, 0x34, 0xff, 0xd0, 0x56, 0xff, 0xff, 0xd9, 1, 2, 3]);
    let (out, stripped) = img::strip_jpeg(&scan).expect("walked");
    assert!(stripped);
    assert!(out.ends_with(&[0x12, 0xff, 0x00, 0x34, 0xff, 0xd0, 0x56, 0xff, 0xd9]));
}

#[test]
fn a_jfif_thumbnail_is_left_out() {
    let plain = jpeg(16, 8);
    // JFIF with a 2 × 1 thumbnail, then a JFXX thumbnail
    let mut jfif = vec![0xff, 0xe0, 0, 16 + 6];
    jfif.extend_from_slice(b"JFIF\0\x01\x02\0\0\x01\0\x01\x02\x01");
    jfif.extend_from_slice(&[9, 9, 9, 8, 8, 8]);
    let mut jfxx = vec![0xff, 0xe0, 0, 2 + 6 + 3];
    jfxx.extend_from_slice(b"JFXX\0\x13");
    jfxx.extend_from_slice(&[7, 7, 7]);
    let mut tagged = plain[..2].to_vec();
    tagged.extend_from_slice(&jfif);
    tagged.extend_from_slice(&jfxx);
    let body_at = 2 + usize::from(u16::from_be_bytes([plain[4], plain[5]])) + 2;
    let skip = if plain[3] == 0xe0 { body_at } else { 2 };
    tagged.extend_from_slice(&plain[skip..]);
    let prepared = img::prepare_bytes(tagged, 1 << 20).expect("prepared");
    assert!(prepared.stripped);
    let out = &prepared.bytes;
    assert_eq!(&out[2..4], &[0xff, 0xe0]);
    assert_eq!(&out[4..6], &[0, 16]);
    assert_eq!(&out[6..20], b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0");
    assert!(!out.windows(4).any(|w| w == b"JFXX"));
    ::image::load_from_memory(out).expect("still a picture");
}

#[test]
fn a_png_loses_its_text_chunks() {
    let plain = png(5, 5);
    let mut chunk = Vec::new();
    let data = b"Author\0Jane Resident";
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(data);
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    // after the signature and IHDR (8 + 25 bytes)
    let mut tagged = plain[..33].to_vec();
    tagged.extend_from_slice(&chunk);
    tagged.extend_from_slice(&plain[33..]);
    let prepared = img::prepare_bytes(tagged, 1 << 20).expect("prepared");
    assert!(prepared.stripped);
    assert_eq!(prepared.bytes, plain);
}

#[test]
fn a_webp_loses_its_exif_chunk() {
    let picture = ::image::RgbaImage::from_fn(6, 4, |x, _| ::image::Rgba([x as u8 * 40, 10, 10, 255]));
    let mut encoded = Vec::new();
    ::image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
        .encode(picture.as_raw(), 6, 4, ::image::ExtendedColorType::Rgba8)
        .expect("webp");
    // RIFF + the encoder's chunks + an EXIF chunk
    let mut chunks = encoded[12..].to_vec();
    chunks.extend_from_slice(b"EXIF");
    chunks.extend_from_slice(&5u32.to_le_bytes());
    chunks.extend_from_slice(b"Jane\0\0");
    let mut tagged = b"RIFF".to_vec();
    tagged.extend_from_slice(&((chunks.len() + 4) as u32).to_le_bytes());
    tagged.extend_from_slice(b"WEBP");
    tagged.extend_from_slice(&chunks);
    let prepared = img::prepare_bytes(tagged, 1 << 20).expect("prepared");
    assert!(prepared.stripped);
    assert_eq!(prepared.bytes, encoded);
}

#[test]
fn a_pasted_picture_becomes_a_new_png() {
    let picture = demo::picture();
    let prepared = img::prepare_pasted(&picture, 4 << 20).expect("prepared");
    assert_eq!(prepared.format, Format::Png);
    assert_eq!((prepared.width, prepared.height), (320, 200));
    let back = ::image::load_from_memory(&prepared.bytes).expect("a png").to_rgba8();
    assert_eq!(back.dimensions(), (320, 200));
    assert!(prepared.thumb.is_some());
    assert!(matches!(img::prepare_pasted(&picture, 100), Err(Refusal::TooLarge(_, 100))));
}

// ---- the draft

#[test]
fn the_link_goes_in_at_the_cursor_with_spaces() {
    let link = "https://h.example/raw/a.png";
    assert_eq!(insert_link("", 0, link), (format!("{link} "), link.len() + 1));
    assert_eq!(insert_link("look", 4, link), (format!("look {link} "), 5 + link.len() + 1));
    assert_eq!(insert_link("look here", 5, link), (format!("look {link} here"), 5 + link.len() + 1));
    assert_eq!(insert_link("lookhere", 4, link), (format!("look {link} here"), 5 + link.len() + 1));
    // characters, not bytes
    assert_eq!(insert_link("été ok", 4, link), (format!("été {link} ok"), 4 + link.len() + 1));
    // a cursor past the end goes to the end
    assert_eq!(insert_link("hi", 99, link).0, format!("hi {link} "));
}

#[test]
fn the_link_follows_the_field_s_cursor() {
    let ctx = egui::Context::default();
    let field = egui::Id::unique("test-field");
    let mut text = "one two".to_owned();
    // never focused: at the end
    insert_into_field(&ctx, field, &mut text, "L1");
    assert_eq!(text, "one two L1 ");
    // after the cursor, which moves past the link
    let mut state = egui::TextEdit::load_state(&ctx, field).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(3))));
    egui::TextEdit::store_state(&ctx, field, state);
    insert_into_field(&ctx, field, &mut text, "L2");
    assert_eq!(text, "one L2 two L1 ");
    let cursor = egui::TextEdit::load_state(&ctx, field)
        .and_then(|s| s.cursor.char_range())
        .expect("a cursor");
    assert_eq!(cursor.primary.index.0, 6);
    // a selection is kept: the link goes after it
    let mut state = egui::TextEdit::load_state(&ctx, field).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(3),
    )));
    egui::TextEdit::store_state(&ctx, field, state);
    insert_into_field(&ctx, field, &mut text, "L3");
    assert_eq!(text, "one L3 L2 two L1 ");
}

#[test]
fn the_demo_uploader_answers_like_zipline() {
    let image = prepared_png();
    let link = upload(
        &demo::Fake(demo::Mode::On),
        demo::HOST,
        Auth::None,
        None,
        &image,
        &Cancel::default(),
    )
    .expect("a link");
    assert!(link.starts_with(&format!("{}/raw/demo", demo::HOST)), "{link}");
    let failed = upload(
        &demo::Fake(demo::Mode::Fail),
        demo::HOST,
        Auth::None,
        None,
        &image,
        &Cancel::default(),
    );
    assert_eq!(failed, Err(Failure::TooLarge));
    let token = upload(
        &demo::Fake(demo::Mode::Token),
        demo::HOST,
        Auth::None,
        None,
        &image,
        &Cancel::default(),
    );
    assert_eq!(token, Err(Failure::Token { had_token: false }));
}

#[test]
fn a_drop_goes_to_the_field_under_the_pointer_then_to_the_focused_one() {
    let (bar, conversation) = (egui::Id::unique("bar"), egui::Id::unique("conversation"));
    let dropped = |fields: Vec<(egui::Id, bool, bool)>| Dropped {
        ctx: egui::Context::default(),
        frame: 1,
        paths: vec![PathBuf::from("a.png")],
        fields,
    };
    // the focused bar is drawn first, the pointer is over the conversation
    assert_eq!(
        dropped(vec![(bar, false, true), (conversation, true, false)]).target(),
        Some(conversation)
    );
    assert_eq!(dropped(vec![(bar, false, true), (conversation, false, false)]).target(), Some(bar));
    assert_eq!(dropped(vec![(bar, false, false), (conversation, false, false)]).target(), None);
}
