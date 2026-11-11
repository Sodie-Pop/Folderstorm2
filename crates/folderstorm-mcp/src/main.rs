//! `folderstorm-mcp`: the MCP server an assistant starts (stdio), or
//! `--diagnose` for a one-shot connection check. Logs go to stderr so that
//! stdout carries the protocol only; the token is never printed.

use folderstorm_mcp::connect::Viewer;
use folderstorm_mcp::jsonrpc::{Backend, MAX_LINE, Server};
use folderstorm_mcp_bridge::{ToolError, discovery};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::PathBuf;

struct Args {
    diagnose: bool,
    pid: Option<u32>,
    discovery: Option<PathBuf>,
}

fn usage() -> String {
    format!(
        "folderstorm-mcp {}: {}'s local assistant (MCP over stdio)\n\n\
         Usage: folderstorm-mcp [--viewer-pid PID] [--discovery DIR]\n       \
         folderstorm-mcp --diagnose [--viewer-pid PID] [--discovery DIR]\n\n\
         DIR holds the viewer's folderstorm2-mcp-<pid>.json (default: ${}, else the viewer's settings folder).",
        env!("CARGO_PKG_VERSION"),
        folderstorm_brand::PRODUCT,
        discovery::ENV
    )
}

fn parse(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        diagnose: false,
        pid: None,
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
            "--discovery" => parsed.discovery = Some(PathBuf::from(args.next().ok_or("--discovery needs a folder")?)),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(parsed)
}

struct Live(Viewer);

impl Backend for Live {
    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
        self.0.call(tool, arguments)
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
    let dir = args.discovery.or_else(discovery::default_dir);
    if args.diagnose {
        let (report, ok) = folderstorm_mcp::diagnose::run(dir.as_deref(), args.pid);
        if writeln!(std::io::stdout(), "{report}").is_err() {
            std::process::exit(2);
        }
        std::process::exit(if ok { 0 } else { 1 });
    }
    let mut server = Server::new(Live(Viewer::new(dir, args.pid)));
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
    }
}
