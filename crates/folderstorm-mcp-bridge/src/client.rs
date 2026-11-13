//! The sidecar's side of the wire: one HTTP request per connection to the
//! bridge on 127.0.0.1, with the bearer token of the discovery file.

use crate::discovery::Discovery;
use crate::http;
use crate::protocol::{CallRequest, ErrorCode, Health, ToolError};
use crate::server::{CLIENT_HEADER, PROBE_HEADER};
use crate::token::Token;
use serde_json::Value;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::time::Duration;

/// Longer than the bridge's wait for the viewer, so that its own timeout
/// error arrives first. A tool that may take longer (a change, the Ask
/// dialog, `wait_for_bake`) adds its time, as the bridge does.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub enum ClientError {
    /// Nothing listens on the port (the viewer quit, or the file is stale).
    Connect(std::io::Error),
    /// The connection broke or timed out.
    Io(std::io::Error),
    /// The answer is not the bridge's.
    Protocol(String),
    /// The bridge answered with an error.
    Tool(ToolError),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Connect(e) => write!(f, "the viewer is not reachable: {e}"),
            ClientError::Io(e) => write!(f, "the connection to the viewer failed: {e}"),
            ClientError::Protocol(what) => write!(f, "unexpected answer from the viewer: {what}"),
            ClientError::Tool(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ClientError {}

#[derive(Clone, Debug)]
pub struct Client {
    port: u16,
    token: Token,
    timeout: Duration,
    probe: bool,
    client: Option<String>,
}

impl Client {
    pub fn new(port: u16, token: Token) -> Client {
        Client {
            port,
            token,
            timeout: DEFAULT_TIMEOUT,
            probe: false,
            client: None,
        }
    }

    pub fn from_discovery(d: &Discovery) -> Client {
        Client::new(d.port, d.token())
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Client {
        self.timeout = timeout;
        self
    }

    /// Mark the requests as a diagnostic probe: the viewer does not list
    /// them as an assistant's activity.
    pub fn probe(mut self) -> Client {
        self.probe = true;
        self
    }

    /// The MCP client's name (`clientInfo.name`), sent with each call for
    /// the viewer's Ask dialog. Only printable characters are sent.
    pub fn with_client_name(mut self, name: &str) -> Client {
        let name: String = name
            .chars()
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .take(crate::server::CLIENT_NAME_MAX)
            .collect();
        self.client = (!name.trim().is_empty()).then(|| name.trim().to_owned());
        self
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn health(&self) -> Result<Health, ClientError> {
        let value = self.request("GET", "/v1/health", None, self.timeout)?;
        serde_json::from_value(value).map_err(|e| ClientError::Protocol(e.to_string()))
    }

    /// Call a tool, unbound.
    pub fn call(&self, tool: &str, arguments: Value) -> Result<Value, ClientError> {
        self.call_in(tool, arguments, None)
    }

    /// Call a tool on the login `session` names (None: unbound). Every call
    /// declares this build's capabilities, so that the viewer may answer
    /// with the newer error codes.
    pub fn call_in(&self, tool: &str, arguments: Value, session: Option<&str>) -> Result<Value, ClientError> {
        let body = serde_json::to_vec(&CallRequest {
            tool: tool.to_owned(),
            arguments,
            session: session.map(str::to_owned),
            features: Some(crate::binding::capabilities()),
        })
        .map_err(|e| ClientError::Protocol(e.to_string()))?;
        let extra = crate::protocol::tool(tool).map_or(Duration::ZERO, |spec| {
            spec.wait
                + if spec.changes() {
                    crate::protocol::ASK_TIMEOUT
                } else {
                    Duration::ZERO
                }
        });
        self.request("POST", "/v1/call", Some(&body), self.timeout + extra)
    }

    fn request(&self, method: &str, path: &str, body: Option<&[u8]>, timeout: Duration) -> Result<Value, ClientError> {
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port);
        let mut stream = TcpStream::connect_timeout(&address.into(), Duration::from_secs(2)).map_err(ClientError::Connect)?;
        stream.set_read_timeout(Some(timeout)).map_err(ClientError::Io)?;
        stream.set_write_timeout(Some(self.timeout)).map_err(ClientError::Io)?;
        let body = body.unwrap_or_default();
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.port,
            self.token.secret(),
            body.len()
        );
        if self.probe {
            head.push_str(PROBE_HEADER);
            head.push_str(": 1\r\n");
        }
        if let Some(client) = &self.client {
            head.push_str(CLIENT_HEADER);
            head.push_str(": ");
            head.push_str(client);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes()).map_err(ClientError::Io)?;
        stream.write_all(body).map_err(ClientError::Io)?;
        stream.flush().map_err(ClientError::Io)?;
        let (_, body) = http::read_message(&mut stream, http::MAX_RESPONSE_BODY).map_err(|e| match e {
            http::HttpError::Io(e) => ClientError::Io(e),
            other => ClientError::Protocol(other.to_string()),
        })?;
        let mut answer: Value = serde_json::from_slice(&body).map_err(|e| ClientError::Protocol(e.to_string()))?;
        match answer["ok"].as_bool() {
            Some(true) => Ok(answer["result"].take()),
            Some(false) => {
                let error = answer["error"].take();
                Err(ClientError::Tool(serde_json::from_value(error.clone()).unwrap_or_else(|_| {
                    // a code this build does not know (a newer viewer that
                    // missed the fallback): keep its message
                    let message: String = error["message"].as_str().unwrap_or_default().chars().take(500).collect();
                    let code: String = error["code"]
                        .as_str()
                        .unwrap_or_default()
                        .chars()
                        .filter(char::is_ascii_graphic)
                        .take(48)
                        .collect();
                    ToolError::new(
                        ErrorCode::Internal,
                        if message.is_empty() {
                            "unreadable error from the viewer".to_owned()
                        } else {
                            format!("{message} (viewer error `{code}`)")
                        },
                    )
                })))
            }
            None => Err(ClientError::Protocol("no `ok` field".into())),
        }
    }
}
