//! MCP over stdio: JSON-RPC 2.0 messages, one per line
//! (https://modelcontextprotocol.io/specification, "Transports › stdio"),
//! with the methods a tools-only server needs: `initialize`, `ping`,
//! `tools/list`, `tools/call`. Notifications get no answer.

use folderstorm_mcp_bridge::ToolError;
use folderstorm_mcp_bridge::protocol;
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

    /// The name the MCP client gave at `initialize` (`clientInfo.name`),
    /// which the viewer's Ask dialog shows.
    fn client_name(&mut self, _name: &str) {}
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
            "initialize" => {
                if let Some(name) = params["clientInfo"]["name"].as_str() {
                    self.backend.client_name(name);
                }
                result(id, initialize(&params))
            }
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
        if folderstorm_mcp_bridge::protocol::NEVER.contains(&name) {
            let refused = ToolError::new(
                folderstorm_mcp_bridge::ErrorCode::PermissionDenied,
                folderstorm_mcp_bridge::protocol::NEVER_MESSAGE,
            );
            return Ok(json!({"content": [{"type": "text", "text": refused.to_string()}], "isError": true}));
        }
        if folderstorm_mcp_bridge::protocol::tool(name).is_none() {
            return Err(format!("unknown tool: {name}"));
        }
        let arguments = match &params["arguments"] {
            Value::Null => json!({}),
            Value::Object(_) => params["arguments"].clone(),
            _ => return Err("`arguments` must be an object".into()),
        };
        Ok(match self.backend.call(name, arguments) {
            Ok(mut value) => {
                // a picture goes as MCP image content, the rest as text
                let image = take_image(&mut value);
                let text = serde_json::to_string_pretty(&value).unwrap_or_default();
                let mut content = Vec::new();
                if let Some((mime, data)) = image {
                    content.push(json!({"type": "image", "data": data, "mimeType": mime}));
                }
                content.push(json!({"type": "text", "text": text}));
                let mut out = json!({"content": content, "isError": false});
                if value.is_object() {
                    out["structuredContent"] = value;
                }
                out
            }
            Err(e) => json!({"content": [{"type": "text", "text": e.to_string()}], "isError": true}),
        })
    }
}

/// The picture of a result (`protocol::IMAGE_KEY`: its MIME type and its
/// base64 data), taken out of it. Only JPEG and PNG are passed on.
fn take_image(value: &mut Value) -> Option<(String, String)> {
    let object = value.as_object_mut()?;
    let image = object.remove(folderstorm_mcp_bridge::protocol::IMAGE_KEY)?;
    let mime = image["mime_type"].as_str().filter(|m| matches!(*m, "image/jpeg" | "image/png"))?;
    let data = image["data"].as_str().filter(|d| !d.is_empty())?;
    Some((mime.to_owned(), data.to_owned()))
}

fn initialize(params: &Value) -> Value {
    let asked = params["protocolVersion"].as_str().unwrap_or_default();
    let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "folderstorm-mcp", "title": format!("{} local assistant", folderstorm_brand::PRODUCT), "version": env!("CARGO_PKG_VERSION")},
        "instructions": format!(
            "Reads and organises the inventory and outfits of the {} viewer running on this computer, moves its camera and takes pictures. Call viewer_status first. Address items by their id: names are not unique. Each change needs the user's permission, set in the viewer: it may be refused, or the viewer may ask the user first (up to 60 s). Nothing is ever deleted for good: inventory_trash moves to the Trash. After an outfit change, call wait_for_bake before a picture. Frame the avatar with camera_preset or camera_shot, then camera_snapshot returns the picture; inventory_snapshot_thumbnail sets it as an outfit's or folder's picture.",
            folderstorm_brand::PRODUCT
        )
    })
}

fn tools_list() -> Value {
    let tools: Vec<Value> = protocol::all()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": {
                    "readOnlyHint": !t.changes(),
                    "destructiveHint": false,
                    "openWorldHint": false
                }
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
        fn client_name(&mut self, name: &str) {
            self.0.push(format!("client:{name}"));
        }

        fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
            self.0.push(tool.to_owned());
            if tool == "worn_items" {
                Err(ToolError::new(ErrorCode::PermissionDenied, "not allowed"))
            } else if tool == "camera_snapshot" {
                Ok(json!({"width": 2, "height": 1, "image": {"mime_type": arguments["mime"].clone(), "data": "AAAA"}}))
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
        assert_eq!(s.backend.0, vec!["client:t".to_owned()], "the client's name goes to the viewer");
    }

    #[test]
    fn permanent_deletes_are_refused_without_reaching_the_viewer() {
        let mut s = Server::new(Fake(Vec::new()));
        for never in folderstorm_mcp_bridge::protocol::NEVER {
            let a = ask(
                &mut s,
                &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{never}"}}}}"#),
            );
            assert_eq!(a["result"]["isError"], true, "{a}");
            assert!(
                a["result"]["content"][0]["text"]
                    .as_str()
                    .expect("text")
                    .starts_with("permission_denied")
            );
        }
        assert!(s.backend.0.is_empty());
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
        assert_eq!(names.len(), protocol::all().count());
        for name in [
            "inventory_search",
            "inventory_move",
            "inventory_trash",
            "outfit_wear",
            "wait_for_bake",
        ] {
            assert!(names.contains(&json!(name)), "{name}");
        }
        assert!(!names.contains(&json!("inventory_delete")));
        for name in [
            "camera_preset",
            "camera_shot",
            "camera_reset",
            "camera_snapshot",
            "inventory_snapshot_thumbnail",
        ] {
            assert!(names.contains(&json!(name)), "{name}");
        }
        let tools = a["result"]["tools"].as_array().expect("tools");
        let hint = |name: &str| tools.iter().find(|t| t["name"] == name).expect("listed")["annotations"]["readOnlyHint"].clone();
        assert_eq!(hint("inventory_search"), true);
        assert_eq!(hint("wait_for_bake"), true);
        assert_eq!(hint("inventory_rename"), false);
        assert_eq!(hint("camera_preset"), false, "the camera moves");
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
    fn a_picture_is_image_content() {
        let mut s = Server::new(Fake(Vec::new()));
        let a = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"camera_snapshot","arguments":{"mime":"image/jpeg"}}}"#,
        );
        let content = a["result"]["content"].as_array().expect("content");
        assert_eq!(content[0], json!({"type": "image", "data": "AAAA", "mimeType": "image/jpeg"}));
        assert_eq!(content[1]["type"], "text");
        assert!(
            !content[1]["text"].as_str().expect("text").contains("AAAA"),
            "no base64 in the text"
        );
        assert_eq!(a["result"]["structuredContent"], json!({"width": 2, "height": 1}));
        // anything but JPEG or PNG is not passed on as an image
        let a = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"camera_snapshot","arguments":{"mime":"text/html"}}}"#,
        );
        let content = a["result"]["content"].as_array().expect("content");
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
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
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"inventory_teleport"}}"#,
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

    /// Folderstorm: phase 3, part 2 (inventory gaps).
    #[test]
    fn lists_the_inventory_gap_tools_with_their_hints() {
        let mut s = Server::new(Fake(Vec::new()));
        let a = ask(&mut s, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let tools = a["result"]["tools"].as_array().expect("tools");
        let hint = |name: &str| {
            tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name} listed"))["annotations"]["readOnlyHint"].clone()
        };
        for read in [
            "inventory_get_many",
            "inventory_system_folder",
            "inventory_types",
            "inventory_protected_folders",
            "inventory_changes",
            "inventory_landmark",
            "appearance_outfit_items",
        ] {
            assert_eq!(hint(read), true, "{read}");
        }
        for change in [
            "inventory_create_item",
            "inventory_set_favorite",
            "inventory_batch_rename",
            "inventory_set_thumbnail",
        ] {
            assert_eq!(hint(change), false, "{change}");
        }
        assert!(!tools.iter().any(|t| t["name"] == "inventory_next_owner_set"));
        let search = tools.iter().find(|t| t["name"] == "inventory_search").expect("search");
        assert!(search["inputSchema"]["properties"]["filter_links"].is_object());
        // a call reaches the viewer like any other
        let a = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"inventory_types","arguments":{}}}"#,
        );
        assert_eq!(a["result"]["isError"], false);
        assert_eq!(s.backend.0, vec!["inventory_types".to_owned()]);
    }
}
