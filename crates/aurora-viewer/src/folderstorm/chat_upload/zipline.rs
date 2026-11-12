//! Zipline's upload API (v4: `src/server/routes/api/upload/index.ts`,
//! `src/server/middleware/user.ts`, `src/lib/api/errors.ts` of
//! github.com/diced/zipline at v4.7.0):
//!
//! - `POST <host>/api/upload`, `multipart/form-data`, the file in the field
//!   `file`;
//! - `authorization: <token>`, the token as it is (no "Bearer"), or no such
//!   header for an anonymous upload into a folder that allows it, named by
//!   `x-zipline-folder`;
//! - `x-zipline-format: random`: the server picks a random name;
//! - the answer: `{"files": [{"id", "name", "type", "url", "pending"?}],
//!   "deletesAt"?}`; a refusal: `{"error": "E<code>: …", "code",
//!   "statusCode"}`, with 401 for the token (2xxx), 403 for the folder
//!   (3xxx), 404 (4xxx), 413 for the size or quota (5xxx).
//!
//! The request carries neither cookies, nor the local file's name, nor
//! anything about the resident; it never follows a redirect, so the token
//! cannot be carried to another host.

use super::image::Prepared;
use super::token::Auth;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

/// Largest answer read.
pub const MAX_ANSWER: usize = 256 * 1024;
/// Longest link put in a message.
const MAX_LINK: usize = 512;

/// One HTTP request, as the transport sends it.
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl std::fmt::Debug for HttpRequest {
    // never the token, never the image
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(name, _)| *name).collect();
        f.debug_struct("HttpRequest")
            .field("url", &self.url)
            .field("headers", &names)
            .field("body", &self.body.len())
            .finish()
    }
}

pub struct HttpReply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// What carries the request: the network, or a stand-in in tests and in
/// the demo. A transport gives up with `Failure::Cancelled` as soon as
/// `cancel` is set, dropping the request on its way.
pub trait Transport: Send + Sync {
    fn send(&self, request: HttpRequest, cancel: &Cancel) -> Result<HttpReply, Failure>;
}

/// The user's Stop for one upload, shared by the interface (which sets
/// it) and the thread sending the request (which gives up on it).
#[derive(Clone, Default)]
pub struct Cancel(std::sync::Arc<CancelInner>);

#[derive(Default)]
struct CancelInner {
    cancelled: AtomicBool,
    /// The request was about to leave: the host may get the image.
    started: AtomicBool,
    waker: std::sync::Mutex<Option<std::task::Waker>>,
}

impl Cancel {
    /// Stops the upload. Returns whether its request had started (the
    /// host may then have the image anyway).
    pub fn cancel(&self) -> bool {
        self.0.cancelled.store(true, Ordering::SeqCst);
        if let Some(waker) = self.lock_waker().take() {
            waker.wake();
        }
        self.0.started.load(Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// Marks the request as leaving. Returns false when the upload was
    /// stopped first: then nothing is sent. (With `cancel`'s order, one of
    /// the two always sees the other.)
    pub fn start(&self) -> bool {
        self.0.started.store(true, Ordering::SeqCst);
        !self.is_cancelled()
    }

    fn lock_waker(&self) -> std::sync::MutexGuard<'_, Option<std::task::Waker>> {
        self.0.waker.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `work`, unless the upload is stopped first: then `work` is dropped
    /// (with it the connection) and the answer is `Failure::Cancelled`.
    pub async fn or_cancelled<T>(&self, work: impl Future<Output = Result<T, Failure>>) -> Result<T, Failure> {
        let mut work = std::pin::pin!(work);
        std::future::poll_fn(|cx| {
            if self.is_cancelled() {
                return Poll::Ready(Err(Failure::Cancelled));
            }
            *self.lock_waker() = Some(cx.waker().clone());
            // set between the first look and the waker's registration
            if self.is_cancelled() {
                return Poll::Ready(Err(Failure::Cancelled));
            }
            work.as_mut().poll(cx)
        })
        .await
    }
}

/// Why an upload did not give a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The host could not be reached or did not answer in time.
    Network,
    /// The token is refused, or the host needs one (`had_token`: one was sent).
    Token { had_token: bool },
    /// The folder does not exist or does not take uploads.
    Folder,
    /// Too large for the host.
    TooLarge,
    /// The host's storage is full: retry later.
    Quota,
    /// The host refuses this type of file.
    Type,
    /// Too many uploads: retry later.
    RateLimited,
    /// Another refusal (its HTTP status).
    Refused(u16),
    /// An answer that is not Zipline's, or a link that cannot be trusted.
    Malformed,
    /// The user stopped it.
    Cancelled,
}

/// The multipart boundary: random, and checked against the image.
fn boundary(body: &[u8]) -> String {
    loop {
        let candidate = format!("----folderstorm-{}", uuid::Uuid::new_v4().simple());
        if !body.windows(candidate.len()).any(|w| w == candidate.as_bytes()) {
            return candidate;
        }
    }
}

/// The upload request for `image`, to `origin` (already checked).
pub fn request(origin: &str, auth: &Auth, folder: Option<&str>, image: &Prepared) -> HttpRequest {
    let boundary = boundary(&image.bytes);
    let mut body = Vec::with_capacity(image.bytes.len() + 256);
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    // a name of our own: the local file's name is nobody's business
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"image.{}\"\r\nContent-Type: {}\r\n\r\n",
            image.format.extension(),
            image.format.mime()
        )
        .as_bytes(),
    );
    body.extend_from_slice(&image.bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let mut headers = vec![
        ("content-type", format!("multipart/form-data; boundary={boundary}")),
        ("accept", "application/json".to_owned()),
        ("x-zipline-format", "random".to_owned()),
    ];
    if let Some(token) = auth.header() {
        headers.push(("authorization", token.to_owned()));
    }
    if let Some(folder) = folder {
        headers.push(("x-zipline-folder", folder.to_owned()));
    }
    HttpRequest {
        url: format!("{}/api/upload", origin.trim_end_matches('/')),
        headers,
        body,
    }
}

#[derive(Deserialize)]
struct Answer {
    files: Vec<AnswerFile>,
}

#[derive(Deserialize)]
struct AnswerFile {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
}

#[derive(Deserialize)]
struct Refusal {
    #[serde(default)]
    code: u32,
}

/// The link of an upload's answer, or why there is none.
pub fn parse(origin: &str, reply: &HttpReply, had_token: bool) -> Result<String, Failure> {
    if reply.body.len() > MAX_ANSWER {
        return Err(Failure::Malformed);
    }
    if !(200..300).contains(&reply.status) {
        let code = serde_json::from_slice::<Refusal>(&reply.body).map_or(0, |r| r.code);
        return Err(match (reply.status, code) {
            (_, 2000..=2999) | (401, _) => Failure::Token { had_token },
            (_, 3002 | 3011 | 4001) => Failure::Folder,
            (_, 5002) => Failure::Quota,
            (_, 5000 | 5001) | (413, _) => Failure::TooLarge,
            (_, 1006 | 1065) => Failure::Type,
            (429, _) => Failure::RateLimited,
            (403, _) if had_token => Failure::Token { had_token },
            (403, _) => Failure::Folder,
            (status, _) => Failure::Refused(status),
        });
    }
    let answer: Answer = serde_json::from_slice(&reply.body).map_err(|_| Failure::Malformed)?;
    let file = answer.files.first().ok_or(Failure::Malformed)?;
    link(origin, &file.name, &file.url).ok_or(Failure::Malformed)
}

/// The direct link of the picture: `<host>/raw/<name>`, which the previews
/// of chat (3.2) show, when the server's name is a plain file name; else
/// the link the server gave, only on the host itself.
pub fn link(origin: &str, name: &str, url: &str) -> Option<String> {
    let base = reqwest::Url::parse(origin).ok()?;
    let plain = !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if plain {
        return Some(format!("{}/raw/{name}", origin.trim_end_matches('/')));
    }
    let given = reqwest::Url::parse(url).ok()?;
    let same_host = given.scheme() == base.scheme() && given.host_str() == base.host_str() && given.port() == base.port();
    let clean = given.username().is_empty() && given.password().is_none();
    (same_host && clean && given.as_str().len() <= MAX_LINK).then(|| given.to_string())
}

/// How long a whole upload may take: a minute, and a second more per
/// 32 KiB sent (a slow line of 256 kbit/s), at most half an hour.
pub fn total_timeout(body: usize) -> std::time::Duration {
    const MIN: u64 = 60;
    const MAX: u64 = 30 * 60;
    let per_second = 32 * 1024;
    std::time::Duration::from_secs((MIN + (body / per_second) as u64).min(MAX))
}

/// The network, through reqwest: no redirect, no retry, no cookie.
pub struct Network;

impl Transport for Network {
    fn send(&self, request: HttpRequest, cancel: &Cancel) -> Result<HttpReply, Failure> {
        let url = reqwest::Url::parse(&request.url).map_err(|_| Failure::Network)?;
        let loopback = url.scheme() == "http" && url.host_str().is_some_and(super::settings::is_loopback);
        let builder = reqwest::Client::builder()
            .user_agent(format!("{} (chat image upload)", aurora_version::user_agent()))
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(total_timeout(request.body.len()))
            .retry(reqwest::retry::never())
            .redirect(reqwest::redirect::Policy::none())
            .https_only(!loopback);
        let client = if loopback { builder.no_proxy() } else { builder }.build().map_err(|e| {
            log::warn!("chat upload: no HTTP client: {e}");
            Failure::Network
        })?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Failure::Network)?;
        runtime.block_on(cancel.or_cancelled(async move {
            let mut post = client.post(url).body(request.body);
            for (name, value) in request.headers {
                post = post.header(name, value);
            }
            let mut resp = post.send().await.map_err(|e| {
                // without the address's details nor any header
                log::info!("chat upload: the host did not answer: {}", e.without_url());
                Failure::Network
            })?;
            let status = resp.status().as_u16();
            let mut body = Vec::new();
            while let Some(chunk) = resp.chunk().await.map_err(|_| Failure::Network)? {
                body.extend_from_slice(&chunk);
                if body.len() > MAX_ANSWER {
                    return Err(Failure::Malformed);
                }
            }
            Ok(HttpReply { status, body })
        }))
    }
}
