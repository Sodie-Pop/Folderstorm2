//! `AURORA_DEMO_CHAT_UPLOAD` (with `AURORA_DEMO`): the uploads work
//! offline. An uploader inside the viewer answers like Zipline after a
//! moment with a made-up link; nothing leaves the computer and the
//! settings stay as they are.
//!
//! - `1`: uploads on, paste, drop or choose an image in a chat field;
//! - `staged`: also puts a made-up picture in the first chat field shown,
//!   once, so the confirmation opens by itself;
//! - `fail`: the uploader refuses every image as too large;
//! - `token`: the uploader refuses as a host that needs a token.

use super::zipline::{Cancel, Failure, HttpReply, HttpRequest, Transport};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    On,
    Staged,
    Fail,
    Token,
}

pub fn mode() -> Option<Mode> {
    static MODE: std::sync::OnceLock<Option<Mode>> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| {
        std::env::var_os("AURORA_DEMO")?;
        match std::env::var("AURORA_DEMO_CHAT_UPLOAD").ok()?.trim() {
            "1" | "on" => Some(Mode::On),
            "staged" => Some(Mode::Staged),
            "fail" => Some(Mode::Fail),
            "token" => Some(Mode::Token),
            other => {
                log::warn!("AURORA_DEMO_CHAT_UPLOAD={other}: unknown");
                None
            }
        }
    })
}

/// The demo's host: a reserved name that can never resolve.
pub const HOST: &str = "https://images.example.invalid";

/// The uploader of the demo.
pub struct Fake(pub Mode);

impl Transport for Fake {
    fn send(&self, request: HttpRequest, cancel: &Cancel) -> Result<HttpReply, Failure> {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        // long enough to see the chip, and to stop it
        for _ in 0..30 {
            if cancel.is_cancelled() {
                return Err(Failure::Cancelled);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let n = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        let extension = if request.body.windows(9).any(|w| w == b"image.gif") {
            "gif"
        } else {
            "png"
        };
        let reply = |status: u16, body: String| {
            Ok(HttpReply {
                status,
                body: body.into_bytes(),
            })
        };
        match self.0 {
            Mode::Fail => reply(
                413,
                r#"{"error":"E5001: File is too large","code":5001,"statusCode":413}"#.to_owned(),
            ),
            Mode::Token => reply(
                401,
                r#"{"error":"E2000: Invalid login session","code":2000,"statusCode":401}"#.to_owned(),
            ),
            Mode::On | Mode::Staged => reply(
                200,
                format!(
                    r#"{{"files":[{{"id":"demo{n}","name":"demo{n}.{extension}","type":"image/png","url":"{HOST}/u/demo{n}.{extension}"}}]}}"#
                ),
            ),
        }
    }
}

/// A made-up picture for `staged`: the brand's gradient, with a stripe.
pub fn picture() -> egui::ColorImage {
    let (w, h) = (320usize, 200usize);
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let t = x as f32 / w as f32;
            let stripe = if (y / 20) % 2 == 0 { 1.0 } else { 0.85 };
            let r = ((40.0 + 120.0 * t) * stripe) as u8;
            let g = ((170.0 - 90.0 * t) * stripe) as u8;
            let b = ((200.0 + 40.0 * t) * stripe) as u8;
            pixels.push(egui::Color32::from_rgb(r, g, b)); // theme-guard: content — a demo picture's pixels
        }
    }
    egui::ColorImage::new([w, h], pixels)
}
