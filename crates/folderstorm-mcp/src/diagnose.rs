//! `folderstorm-mcp --diagnose`: can this sidecar reach a viewer, and is
//! it ready? One JSON line of fixed fields (never the token), exit status
//! 0 when the bridge answered, 1 when it did not, 2 for bad arguments.
//! Folderstorm (C++)'s `fs-mcp --diagnose`. Its requests are marked as
//! probes, so the viewer does not list them as an assistant's activity.

use crate::connect::{self, NotFound};
use folderstorm_mcp_bridge::binding;
use serde_json::{Value, json};
use std::path::Path;

/// The report and whether the check passed.
pub fn run(dir: Option<&Path>, pid: Option<u32>) -> (Value, bool) {
    let mut report = json!({
        "ok": false,
        "product": folderstorm_brand::PRODUCT,
        "sidecar_version": env!("CARGO_PKG_VERSION"),
        "sidecar_capabilities": binding::capabilities(),
        "discovery_dir": dir.map(|d| d.display().to_string()),
    });
    let (client, found_pid) = match connect::find(dir, pid, true) {
        Ok(found) => (found.client, found.pid),
        Err(e) => {
            report["stage"] = json!(match e {
                NotFound::NoFolder | NotFound::NoFile => "discovery",
                NotFound::NoAnswer => "connect",
                NotFound::OtherVersion(_) => "api",
            });
            report["message"] = json!(e.to_string());
            return (report, false);
        }
    };
    report["viewer_pid"] = json!(found_pid);
    report["port"] = json!(client.port());
    match client.health() {
        Ok(health) => {
            report["api"] = json!(health.api);
            report["viewer_version"] = json!(health.version);
            report["read_allowed"] = json!(health.read_allowed);
            let capabilities = health.capabilities();
            // what this sidecar knows and the viewer lacks: those tools
            // answer `unsupported` until the viewer is updated
            report["missing_capabilities"] = json!(binding::missing(&capabilities));
            report["viewer_capabilities"] = json!(capabilities);
            report["login_bound"] = json!(health.viewer_session.is_some());
        }
        Err(e) => {
            report["stage"] = json!("health");
            report["message"] = json!(e.to_string());
            return (report, false);
        }
    }
    // the viewer's own state; a viewer that is not logged in still passes
    match client.call("viewer_status", json!({})) {
        Ok(status) => {
            report["session"] = status["session"].clone();
            report["inventory_usable"] = status["inventory"]["usable"].clone();
        }
        Err(e) => {
            report["stage"] = json!("status");
            report["message"] = json!(e.to_string());
            return (report, false);
        }
    }
    report["stage"] = json!("done");
    report["ok"] = json!(true);
    (report, true)
}
