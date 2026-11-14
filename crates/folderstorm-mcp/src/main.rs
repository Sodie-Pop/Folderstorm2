//! `folderstorm-mcp`: the MCP server an assistant starts (stdio), or
//! `--diagnose` for a one-shot connection check. Logs go to stderr so that
//! stdout carries the protocol only; the token is never printed.

use folderstorm_mcp::connect::{Scope, Viewer};
use folderstorm_mcp::host::Host;
use folderstorm_mcp::jsonrpc::{Backend, MAX_LINE, Server};
use folderstorm_mcp_bridge::{ToolError, discovery};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::PathBuf;

struct Args {
    diagnose: bool,
    pid: Option<u32>,
    /// `--profile`: that character's viewer only.
    profile: Option<String>,
    discovery: Option<PathBuf>,
}

impl Args {
    fn scope(&self) -> Scope {
        match (&self.profile, self.pid) {
            (Some(profile), _) => Scope::Profile(profile.clone()),
            (None, Some(pid)) => Scope::Pid(pid),
            (None, None) => Scope::Any,
        }
    }
}

fn usage() -> String {
    format!(
        "folderstorm-mcp {}: {}'s local assistant (MCP over stdio)\n\n\
         Usage: folderstorm-mcp [--profile NAME | --viewer-pid PID] [--discovery DIR]\n       \
         folderstorm-mcp --diagnose [--profile NAME | --viewer-pid PID] [--discovery DIR]\n\n\
         --profile NAME talks to that character's viewer only (main: the main profile); without it, with several\n\
         viewers running, the assistant chooses one with viewer_select.\n\
         DIR holds the viewers' folderstorm2-mcp-<pid>.json (default: ${}, else the viewer's settings folder).",
        env!("CARGO_PKG_VERSION"),
        folderstorm_brand::PRODUCT,
        discovery::ENV
    )
}

fn parse(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        diagnose: false,
        pid: None,
        profile: None,
        discovery: None,
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--diagnose" => parsed.diagnose = true,
            "--viewer-pid" => {
                let value = args.next().ok_or("--viewer-pid needs a value")?;
                parsed.pid = Some(value.parse().map_err(|_| format!("bad --viewer-pid: {value}"))?);
            }
            "--profile" => {
                let value = args.next().ok_or("--profile needs a name")?;
                let ok =
                    !value.is_empty() && value.len() <= 32 && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
                if !ok {
                    return Err(format!("bad --profile: {value}"));
                }
                parsed.profile = Some(value);
            }
            "--discovery" => parsed.discovery = Some(PathBuf::from(args.next().ok_or("--discovery needs a folder")?)),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if parsed.profile.is_some() && parsed.pid.is_some() {
        return Err("give --profile or --viewer-pid, not both".into());
    }
    Ok(parsed)
}

/// The viewer chosen, and the session host's controller.
struct Live {
    viewer: Viewer,
    host: Host,
}

impl Backend for Live {
    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
        match tool {
            "viewer_list" => self.viewer.list(),
            "viewer_select" => self.viewer.select(&arguments),
            _ if tool.starts_with("host_") => self.host.call(tool, arguments),
            _ => self.viewer.call(tool, arguments),
        }
    }

    fn client_name(&mut self, name: &str) {
        self.viewer.set_client_name(name);
        self.host.set_client_name(name);
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.iter().any(|a| a == "-h" || a == "--help") {
        println!("{}", usage());
        return;
    }
    if raw.iter().any(|a| a == "-V" || a == "--version") {
        println!("folderstorm-mcp {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let args = match parse(raw.into_iter()) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("folderstorm-mcp: {e}\n\n{}", usage());
            std::process::exit(2);
        }
    };
    let dir = args.discovery.clone().or_else(discovery::default_dir);
    if args.diagnose {
        let (report, ok) = folderstorm_mcp::diagnose::run(dir.as_deref(), &args.scope());
        if writeln!(std::io::stdout(), "{report}").is_err() {
            std::process::exit(2);
        }
        std::process::exit(if ok { 0 } else { 1 });
    }
    let scope = args.scope();
    let mut server = Server::new(Live {
        viewer: Viewer::scoped(dir.clone(), scope),
        host: Host::new(dir),
    });
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut stdout = std::io::stdout().lock();
    let mut line = String::new();
    loop {
        line.clear();
        match std::io::Read::take(&mut input, MAX_LINE as u64 + 1).read_line(&mut line) {
            Ok(0) => break,
            Ok(n) if n > MAX_LINE => {
                eprintln!("folderstorm-mcp: a message longer than {MAX_LINE} bytes; stopping");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("folderstorm-mcp: stdin: {e}");
                break;
            }
        }
        if let Some(answer) = server.handle_line(&line)
            && (writeln!(stdout, "{answer}").is_err() || stdout.flush().is_err())
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn arguments() {
        let a = args(&["--diagnose", "--viewer-pid", "12", "--discovery", "/tmp/x"]).expect("parsed");
        assert!(a.diagnose);
        assert_eq!(a.pid, Some(12));
        assert_eq!(a.discovery, Some(PathBuf::from("/tmp/x")));
        assert!(args(&[]).is_ok_and(|a| !a.diagnose && a.pid.is_none()));
        assert!(args(&["--viewer-pid", "x"]).is_err());
        assert!(args(&["--viewer-pid"]).is_err());
        assert!(args(&["--token", "x"]).is_err());
        let a = args(&["--profile", "Alt1"]).expect("parsed");
        assert_eq!(a.scope(), Scope::Profile("Alt1".into()));
        assert_eq!(args(&["--viewer-pid", "7"]).expect("parsed").scope(), Scope::Pid(7));
        assert_eq!(args(&[]).expect("parsed").scope(), Scope::Any);
        for bad in [
            &["--profile"][..],
            &["--profile", "../x"],
            &["--profile", ""],
            &["--profile", "a", "--viewer-pid", "3"],
        ] {
            assert!(args(bad).is_err(), "{bad:?}");
        }
    }
}
