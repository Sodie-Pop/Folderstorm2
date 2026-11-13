//! The little HTTP/1.1 the bridge and its client speak: one request per
//! connection, a bounded head, a body sized by Content-Length only (no
//! chunked encoding), `Connection: close`.

use std::io::{Read, Write};

/// The largest request or response head.
pub const MAX_HEAD: usize = 8 * 1024;
/// The largest request body (a tool call's arguments). Folderstorm: 512 KiB
/// since content editing, so that a whole notecard or script (64 KiB of
/// text) fits however JSON escapes it (mcp-content.md).
pub const MAX_REQUEST_BODY: usize = 512 * 1024;
/// The largest response body the client reads (results are capped well
/// below this by the viewer).
pub const MAX_RESPONSE_BODY: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub enum HttpError {
    TooLarge,
    Malformed(&'static str),
    Io(std::io::Error),
}

impl From<std::io::Error> for HttpError {
    fn from(e: std::io::Error) -> Self {
        HttpError::Io(e)
    }
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::TooLarge => f.write_str("message too large"),
            HttpError::Malformed(what) => write!(f, "malformed HTTP: {what}"),
            HttpError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// A parsed head: the first line split on spaces, then the headers with
/// lower-case names.
#[derive(Debug)]
pub struct Head {
    pub first: Vec<String>,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
    fn count(&self, name: &str) -> usize {
        self.headers.iter().filter(|(n, _)| n == name).count()
    }
}

/// Read one message (head and body) from `stream`; `max_body` bounds the
/// body.
pub fn read_message(stream: &mut impl Read, max_body: usize) -> Result<(Head, Vec<u8>), HttpError> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let end = loop {
        if let Some(at) = find(&buf, b"\r\n\r\n") {
            break at;
        }
        if buf.len() > MAX_HEAD {
            return Err(HttpError::TooLarge);
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err(HttpError::Malformed("connection closed in the head"));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    if end > MAX_HEAD {
        return Err(HttpError::TooLarge);
    }
    let head = parse_head(&buf[..end])?;
    if head.header("transfer-encoding").is_some() {
        return Err(HttpError::Malformed("transfer-encoding is not supported"));
    }
    if head.count("content-length") > 1 {
        return Err(HttpError::Malformed("several content-length headers"));
    }
    let length = match head.header("content-length") {
        Some(v) => v.trim().parse::<usize>().map_err(|_| HttpError::Malformed("bad content-length"))?,
        None => 0,
    };
    if length > max_body {
        return Err(HttpError::TooLarge);
    }
    let mut body = buf.split_off(end + 4);
    if body.len() > length {
        return Err(HttpError::Malformed("more bytes than content-length"));
    }
    let missing = length - body.len();
    body.reserve(missing);
    stream.take(missing as u64).read_to_end(&mut body)?;
    if body.len() != length {
        return Err(HttpError::Malformed("body shorter than content-length"));
    }
    Ok((head, body))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_head(raw: &[u8]) -> Result<Head, HttpError> {
    let text = std::str::from_utf8(raw).map_err(|_| HttpError::Malformed("head is not UTF-8"))?;
    let mut lines = text.split("\r\n");
    let first: Vec<String> = lines
        .next()
        .ok_or(HttpError::Malformed("empty head"))?
        .split(' ')
        .map(str::to_owned)
        .collect();
    if first.len() < 2 {
        return Err(HttpError::Malformed("bad first line"));
    }
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(HttpError::Malformed("bad header line"))?;
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(HttpError::Malformed("bad header name"));
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
    }
    Ok(Head { first, headers })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Error",
    }
}

/// Write a JSON response and flush it.
pub fn write_response(stream: &mut impl Write, status: u16, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        reason(status),
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(raw: &[u8], max: usize) -> Result<(Head, Vec<u8>), HttpError> {
        read_message(&mut std::io::Cursor::new(raw.to_vec()), max)
    }

    #[test]
    fn a_request_with_a_body() {
        let (head, body) = read(b"POST /v1/call HTTP/1.1\r\nHost: 127.0.0.1:5\r\nContent-Length: 2\r\n\r\n{}", 10).expect("parsed");
        assert_eq!(head.first, ["POST", "/v1/call", "HTTP/1.1"]);
        assert_eq!(head.header("host"), Some("127.0.0.1:5"));
        assert_eq!(body, b"{}");
    }

    #[test]
    fn limits_and_malformed_messages() {
        assert!(matches!(
            read(b"POST / HTTP/1.1\r\nContent-Length: 11\r\n\r\n", 10),
            Err(HttpError::TooLarge)
        ));
        let huge = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD));
        assert!(matches!(read(huge.as_bytes(), 10), Err(HttpError::TooLarge)));
        assert!(matches!(
            read(b"GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n", 10),
            Err(HttpError::Malformed(_))
        ));
        assert!(matches!(
            read(b"GET / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n", 10),
            Err(HttpError::Malformed(_))
        ));
        assert!(matches!(
            read(b"POST / HTTP/1.1\r\nContent-Length: 4\r\n\r\n{}", 10),
            Err(HttpError::Malformed(_))
        ));
        assert!(matches!(read(b"GET / HTTP/1.1\r\n", 10), Err(HttpError::Malformed(_))));
        assert!(matches!(read(b"GET / HTTP/1.1\r\nbad\r\n\r\n", 10), Err(HttpError::Malformed(_))));
    }

    #[test]
    fn a_response_is_framed() {
        let mut out = Vec::new();
        write_response(&mut out, 200, b"{\"ok\":true}").expect("written");
        let (head, body) = read(&out, 100).expect("parsed");
        assert_eq!(head.first[1], "200");
        assert_eq!(body, b"{\"ok\":true}");
    }
}
