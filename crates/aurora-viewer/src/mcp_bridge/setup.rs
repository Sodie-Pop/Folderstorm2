//! Folderstorm: Preferences › Local assistant › Setup. The client entries
//! *Copy configuration* gives (Folderstorm (C++)'s
//! doc/plans/assistant-settings-and-setup.md): the sidecar beside this
//! viewer's executable and this viewer's settings folder, never the token
//! nor the discovery file's content; and *Check connection*, which runs
//! `folderstorm-mcp --diagnose` off the frame thread.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The assistants *Copy configuration* knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Client {
    #[default]
    ClaudeCode,
    Cursor,
    Codex,
}

/// The sidecar's name in the client's configuration.
fn server_name() -> &'static str {
    folderstorm_brand::SLUG
}

/// The sidecar beside the running executable.
pub fn sidecar_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "folderstorm-mcp.exe" } else { "folderstorm-mcp" };
    Some(exe.parent()?.join(name))
}

/// The entry to add to `client`'s configuration file. Claude Code
/// (`.mcp.json`) and Cursor (`mcp.json`) read a JSON `mcpServers` entry,
/// Codex (`config.toml`) a TOML table.
pub fn configuration(client: Client, sidecar: &Path, discovery_dir: &Path) -> String {
    let command = sidecar.display().to_string();
    let dir = discovery_dir.display().to_string();
    let env = folderstorm_mcp_bridge::discovery::ENV;
    match client {
        Client::ClaudeCode | Client::Cursor => {
            let entry: Value = json!({
                "mcpServers": {
                    server_name(): {
                        "command": command,
                        "args": [],
                        "env": {env: dir},
                    }
                }
            });
            serde_json::to_string_pretty(&entry).unwrap_or_default()
        }
        Client::Codex => format!(
            "[mcp_servers.{name}]\ncommand = {command}\nargs = []\nenv = {{ {env} = {dir} }}\n",
            name = server_name(),
            command = toml_string(&command),
            dir = toml_string(&dir),
        ),
    }
}

/// A TOML basic string: quotes, backslashes and control characters
/// escaped, everything else (Unicode included) as it is.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The last connection check.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Check {
    #[default]
    NotRun,
    Running,
    Passed {
        logged_in: bool,
    },
    Failed(Failure),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// No sidecar beside the viewer.
    Missing,
    /// It could not be started.
    Launch,
    /// It did not finish in time.
    Timeout,
    /// It ran and did not reach the bridge: its stage.
    Stage(String),
}

static CHECK: Mutex<Check> = Mutex::new(Check::NotRun);

/// How long the check may take before the sidecar is stopped.
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

pub fn check() -> Check {
    CHECK.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

pub fn forget_check() {
    let mut check = CHECK.lock().unwrap_or_else(PoisonError::into_inner);
    if *check != Check::Running {
        *check = Check::NotRun;
    }
}

/// Run `folderstorm-mcp --diagnose` against this viewer on a thread; the
/// result comes through [`check`]. No shell: an argument vector.
pub fn start_check(discovery_dir: PathBuf) {
    {
        let mut check = CHECK.lock().unwrap_or_else(PoisonError::into_inner);
        if *check == Check::Running {
            return;
        }
        *check = Check::Running;
    }
    let spawned = std::thread::Builder::new().name("mcp-check".into()).spawn(move || {
        let result = run_check(&discovery_dir);
        *CHECK.lock().unwrap_or_else(PoisonError::into_inner) = result;
    });
    if spawned.is_err() {
        *CHECK.lock().unwrap_or_else(PoisonError::into_inner) = Check::Failed(Failure::Launch);
    }
}

fn run_check(discovery_dir: &Path) -> Check {
    use std::io::Read;
    let Some(sidecar) = sidecar_path().filter(|p| p.is_file()) else {
        return Check::Failed(Failure::Missing);
    };
    let mut command = std::process::Command::new(sidecar);
    command
        .arg("--diagnose")
        .arg("--viewer-pid")
        .arg(std::process::id().to_string())
        .arg("--discovery")
        .arg(discovery_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes over the viewer
        command.creation_flags(0x0800_0000);
    }
    let Ok(mut child) = command.spawn() else {
        return Check::Failed(Failure::Launch);
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < CHECK_TIMEOUT => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Check::Failed(Failure::Timeout);
            }
        }
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.by_ref().take(64 * 1024).read_to_string(&mut out);
    }
    interpret(&out)
}

/// The sidecar's one-line report, as a check result.
fn interpret(report: &str) -> Check {
    let Ok(report) = serde_json::from_str::<Value>(report.trim()) else {
        return Check::Failed(Failure::Stage("output".into()));
    };
    if report["ok"].as_bool() == Some(true) {
        Check::Passed {
            logged_in: matches!(report["session"].as_str(), Some("logged_in" | "demo")),
        }
    } else {
        Check::Failed(Failure::Stage(
            report["stage"].as_str().unwrap_or("unknown").chars().take(32).collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_entries_parse_and_keep_odd_paths() {
        let sidecar = Path::new("C:\\Program Files\\Folder \"storm\"\\folderstorm-mcp.exe");
        let dir = Path::new("/home/ユーザー/settings dir");
        for client in [Client::ClaudeCode, Client::Cursor] {
            let text = configuration(client, sidecar, dir);
            let parsed: Value = serde_json::from_str(&text).expect("valid JSON");
            let entry = &parsed["mcpServers"]["folderstorm2"];
            assert_eq!(entry["command"], sidecar.display().to_string());
            assert_eq!(entry["env"]["FOLDERSTORM2_MCP_DISCOVERY"], dir.display().to_string());
            assert_eq!(entry["args"], json!([]));
        }
    }

    #[test]
    fn toml_entries_escape_quotes_and_backslashes() {
        let sidecar = Path::new("C:\\Program Files\\Folder \"storm\"\\folderstorm-mcp.exe");
        let dir = Path::new("/home/ユーザー/settings dir");
        let text = configuration(Client::Codex, sidecar, dir);
        assert!(text.starts_with("[mcp_servers.folderstorm2]\n"), "{text}");
        assert!(
            text.contains(r#"command = "C:\\Program Files\\Folder \"storm\"\\folderstorm-mcp.exe""#),
            "{text}"
        );
        assert!(
            text.contains(r#"env = { FOLDERSTORM2_MCP_DISCOVERY = "/home/ユーザー/settings dir" }"#),
            "{text}"
        );
        assert_eq!(toml_string("a\tb\u{1}"), "\"a\\tb\\u0001\"");
    }

    #[test]
    fn no_entry_carries_a_token() {
        // the entries are built from paths only: nothing else goes in
        let text = configuration(Client::Codex, Path::new("/x/folderstorm-mcp"), Path::new("/y"));
        assert!(!text.to_lowercase().contains("token"));
    }

    #[test]
    fn reports_are_read() {
        assert_eq!(interpret(r#"{"ok":true,"session":"logged_in"}"#), Check::Passed { logged_in: true });
        assert_eq!(
            interpret(r#"{"ok":true,"session":"logged_out"}"#),
            Check::Passed { logged_in: false }
        );
        assert_eq!(
            interpret(r#"{"ok":false,"stage":"discovery"}"#),
            Check::Failed(Failure::Stage("discovery".into()))
        );
        assert_eq!(interpret("garbage"), Check::Failed(Failure::Stage("output".into())));
    }
}
