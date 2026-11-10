//! MCP over stdio: JSON-RPC 2.0 messages, one per line
//! (https://modelcontextprotocol.io/specification, "Transports › stdio"),
//! with the methods a tools-only server needs: `initialize`, `ping`,
//! `tools/list`, `tools/call`. Notifications get no answer.

use folderstorm_mcp_bridge::ToolError;
use folderstorm_mcp_bridge::protocol::TOOLS;
use serde_json::{Value, json};

/// The protocol revisions this server speaks, newest first; a client
/// asking for another gets the newest.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

/// The largest line read from the client.
pub const MAX_LINE: usize = 1024 * 1024;

/// What runs a tool: the viewer, or a fake in the tests.
pub trait Backend {
    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError>;
}

pub struct Server<B: Backend> {
    backend: B,
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

impl<B: Backend> Server<B> {
    pub fn new(backend: B) -> Self {
        Server { backend }
    }

    /// Answer one line; None for a notification (or a response, which a
    /// tools-only server never asked for).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.trim().is_empty() {
            return None;
        }
        let answer = match serde_json::from_str::<Value>(line) {
            Ok(message) => self.handle(message),
            Err(_) => Some(error(Value::Null, PARSE_ERROR, "parse error")),
        };
        answer.map(|a| a.to_string())
    }

    fn handle(&mut self, message: Value) -> Option<Value> {
        let Some(object) = message.as_object() else {
            return Some(error(Value::Null, INVALID_REQUEST, "a message must be one JSON object"));
        };
        let id = object.get("id").cloned();
        let valid_id = id.as_ref().is_none_or(|id| id.is_string() || id.is_i64() || id.is_u64());
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || !valid_id {
            return Some(error(id.unwrap_or(Value::Null), INVALID_REQUEST, "not a JSON-RPC 2.0 request"));
        }
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            // a response to a request we never sent
            return None;
        };
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            // notifications/initialized, notifications/cancelled…
            return None;
        };
        Some(match method {
            "initialize" => result(id, initialize(&params)),
            "ping" => result(id, json!({})),
            "tools/list" => result(id, tools_list()),
            "tools/call" => match self.tools_call(&params) {
                Ok(value) => result(id, value),
                Err(message) => error(id, INVALID_PARAMS, &message),
            },
            _ => error(id, METHOD_NOT_FOUND, "method not found"),
        })
    }

    /// A tool's own failure is a result with `isError` (the assistant reads
    /// it); a call that names no known tool is a protocol error.
    fn tools_call(&mut self, params: &Value) -> Result<Value, String> {
        let name = params["name"].as_str().ok_or("`name` is missing")?;
        if folderstorm_mcp_bridge::protocol::tool(name).is_none() {
            return Err(format!("unknown tool: {name}"));
        }
        let arguments = match &params["arguments"] {
            Value::Null => json!({}),
            Value::Object(_) => params["arguments"].clone(),
            _ => return Err("`arguments` must be an object".into()),
        };
        Ok(match self.backend.call(name, arguments) {
            Ok(value) => {
                let text = serde_json::to_string_pretty(&value).unwrap_or_default();
                let mut out = json!({"content": [{"type": "text", "text": text}], "isError": false});
                if value.is_object() {
                    out["structuredContent"] = value;
                }
                out
            }
            Err(e) => json!({"content": [{"type": "text", "text": e.to_string()}], "isError": true}),
        })
    }
}

fn initialize(params: &Value) -> Value {
    let asked = params["protocolVersion"].as_str().unwrap_or_default();
    let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "folderstorm-mcp", "title": format!("{} local assistant", folderstorm_brand::PRODUCT), "version": env!("CARGO_PKG_VERSION")},
        "instructions": format!(
            "Reads the inventory and outfits of the {} viewer running on this computer. Read-only: nothing can be changed. Call viewer_status first. Address items by their id: names are not unique.",
            folderstorm_brand::PRODUCT
        )
    })
}

fn tools_list() -> Value {
    let tools: Vec<Value> = TOOLS
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": {"readOnlyHint": true, "openWorldHint": false}
            })
        })
        .collect();
    json!({"tools": tools})
}

#[cfg(test)]
mod tests {
    use super::*;
    use folderstorm_mcp_bridge::ErrorCode;

    struct Fake(Vec<String>);

    impl Backend for Fake {
        fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
            self.0.push(tool.to_owned());
            if tool == "worn_items" {
                Err(ToolError::new(ErrorCode::PermissionDenied, "not allowed"))
            } else {
                Ok(json!({"tool": tool, "arguments": arguments}))
            }
        }
    }

    fn ask(server: &mut Server<Fake>, line: &str) -> Value {
        serde_json::from_str(&server.handle_line(line).expect("an answer")).expect("json")
    }

    #[test]
    fn initialize_negotiates_the_version() {
        let mut s = Server::new(Fake(Vec::new()));
        let a = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
        );
        assert_eq!(a["id"], 1);
        assert_eq!(a["result"]["protocolVersion"], "2025-03-26");
        assert!(a["result"]["capabilities"]["tools"].is_object());
        let a = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":"x","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        );
        assert_eq!(a["id"], "x");
        assert_eq!(a["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn notifications_and_responses_get_no_answer() {
        let mut s = Server::new(Fake(Vec::new()));
        assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
        assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","id":3,"result":{}}"#), None);
        assert_eq!(s.handle_line("   "), None);
    }

    #[test]
    fn lists_the_tools() {
        let mut s = Server::new(Fake(Vec::new()));
        let a = ask(&mut s, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let names: Vec<_> = a["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|t| t["name"].clone())
            .collect();
        assert_eq!(names.len(), TOOLS.len());
        assert!(names.contains(&json!("inventory_search")));
        assert_eq!(a["result"]["tools"][0]["inputSchema"]["type"], "object");
    }

    #[test]
    fn calls_a_tool() {
        let mut s = Server::new(Fake(Vec::new()));
        let a = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"inventory_search","arguments":{"query":"boot"}}}"#,
        );
        assert_eq!(a["result"]["isError"], false);
        assert_eq!(a["result"]["structuredContent"]["arguments"]["query"], "boot");
        assert!(a["result"]["content"][0]["text"].as_str().expect("text").contains("boot"));
        let a = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"viewer_status"}}"#,
        );
        assert_eq!(a["result"]["structuredContent"]["arguments"], json!({}));
    }

    #[test]
    fn a_tool_error_is_a_result_the_assistant_reads() {
        let mut s = Server::new(Fake(Vec::new()));
        let a = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"worn_items","arguments":{}}}"#,
        );
        assert_eq!(a["result"]["isError"], true);
        assert_eq!(a["result"]["content"][0]["text"], "permission_denied: not allowed");
    }

    #[test]
    fn protocol_errors() {
        let mut s = Server::new(Fake(Vec::new()));
        assert_eq!(ask(&mut s, "{nope")["error"]["code"], PARSE_ERROR);
        assert_eq!(ask(&mut s, "[1]")["error"]["code"], INVALID_REQUEST);
        assert_eq!(ask(&mut s, r#"{"id":1,"method":"ping"}"#)["error"]["code"], INVALID_REQUEST);
        assert_eq!(
            ask(&mut s, r#"{"jsonrpc":"2.0","id":{},"method":"ping"}"#)["error"]["code"],
            INVALID_REQUEST
        );
        assert_eq!(
            ask(&mut s, r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#)["error"]["code"],
            METHOD_NOT_FOUND
        );
        let a = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"inventory_delete"}}"#,
        );
        assert_eq!(a["error"]["code"], INVALID_PARAMS);
        let a = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"worn_items","arguments":[1]}}"#,
        );
        assert_eq!(a["error"]["code"], INVALID_PARAMS);
        assert_eq!(ask(&mut s, r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#)["result"], json!({}));
        assert!(s.backend.0.is_empty(), "nothing reached the viewer");
    }
}
