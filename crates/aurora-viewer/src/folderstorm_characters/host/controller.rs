//! The session host's MCP controller, carried out (folderstorm/plans/
//! mcp-routing.md, section 3). The listener and its checks are the bridge
//! crate's (`folderstorm_mcp_bridge::controller`); the rules are the
//! session crate's (`folderstorm_session::controller`). Here, at each of the
//! host's ticks: the listener starts or stops as "Enable controller MCP"
//! says, the calls it handed over are decided and answered, and the
//! questions waiting in the Ask window are decided again.
//!
//! Nothing here touches a viewer's local assistant, its settings or its
//! token, and nothing logs anyone in: every action is one of the desk's own
//! (the host window's clicks).

use super::Host;
use ::folderstorm_session::controller::{self as rules, Admitted, Questions, Refusal, Why};
use folderstorm_mcp_bridge::controller::{self, Controller as Listener, ControllerConfig, HOST_TIMEOUT};
use folderstorm_mcp_bridge::{ErrorCode, Pending, ToolError};
use serde_json::{Value, json};
use std::sync::mpsc::Receiver;
use std::time::Instant;

/// Calls taken per tick at most.
const PER_TICK: usize = 8;

/// `AURORA_DEMO_SESSION_HOST_CONTROLLER=ask|allow`, demo only, with the
/// host's demo: the controller is on for the demo (never saved), every demo
/// character has access, metadata is on, every class asks (`ask`, which
/// also makes one request of its own so that the window shows) or is
/// allowed (`allow`).
pub const DEMO_VARIABLE: &str = "AURORA_DEMO_SESSION_HOST_CONTROLLER";

/// What the host keeps of its controller.
#[derive(Default)]
pub(super) struct Controller {
    running: Option<(Listener, Receiver<Pending>)>,
    /// The calls waiting for the user's answer in the Ask window.
    pub questions: Questions<Pending>,
    /// Why the listener did not start (shown in the settings); tried again
    /// once the controller is turned off and on.
    pub failure: Option<String>,
    /// Tests: no discovery file.
    no_file: bool,
}

impl Controller {
    /// The port it listens on, while it runs.
    pub fn port(&self) -> Option<u16> {
        self.running.as_ref().map(|(l, _)| l.port())
    }

    #[cfg(test)]
    pub fn listener(&self) -> Option<&Listener> {
        self.running.as_ref().map(|(l, _)| l)
    }

    #[cfg(test)]
    pub fn without_file() -> Self {
        Self {
            no_file: true,
            ..Self::default()
        }
    }
}

/// What a refusal says to the assistant.
pub(super) fn error(refusal: Refusal) -> ToolError {
    let (code, message): (ErrorCode, String) = match refusal {
        Refusal::Off => (
            ErrorCode::PermissionDenied,
            "the session host's MCP controller is turned off".into(),
        ),
        Refusal::Never => return controller::never(),
        Refusal::Refused => (ErrorCode::PermissionDenied, "the user refused this in the session host".into()),
        Refusal::TimedOut => (
            ErrorCode::PermissionDenied,
            "the user did not answer in the session host within 60 seconds: refused".into(),
        ),
        Refusal::NotFound => (
            ErrorCode::NotFound,
            "no character the user gave controller access has that profile: call host_characters".into(),
        ),
        Refusal::Restarted => (
            ErrorCode::SessionChanged,
            "that character's viewer was started again since: nothing was done. Call host_characters and look again".into(),
        ),
        Refusal::Invalid(why) => (ErrorCode::InvalidArguments, why.into()),
        Refusal::NotReady(why) => (
            ErrorCode::NotReady,
            match why {
                Why::NotInWorld => "that character is not logged in (host_open_login brings its login screen forward)",
                Why::LoggedIn => "that character is logged in already",
                Why::NoViewer => "that character's viewer is not running",
                Why::Starting => "that character's viewer is starting: try again in a moment",
                Why::Closing => "that character's viewer is closing",
                Why::Unmanaged => "the host does not manage that character's viewer (it runs by itself)",
                Why::Active => "that is the active character: it has no monitor",
                Why::Quitting => "the host is quitting every character",
            }
            .into(),
        ),
    };
    ToolError::new(code, message)
}

/// The tool's name of an action, for the answer.
fn done(host: &Host, slot: Option<usize>, tool: &str) -> Value {
    let metadata = host.desk.controller().metadata;
    let mut answer = json!({"done": true, "tool": tool});
    if let Some(row) = slot.and_then(|slot| rules::row(&host.desk, slot, metadata)) {
        answer["character"] = row;
    }
    answer
}

impl Host {
    /// Once per tick: the listener as the settings say, the calls it handed
    /// over, the questions decided again.
    pub(super) fn tick_controller(&mut self, now: Instant) {
        let settings = self.desk.controller();
        if !settings.enabled {
            self.controller.failure = None;
            if let Some((listener, calls)) = self.controller.running.take() {
                while let Ok(call) = calls.try_recv() {
                    call.answer(Err(error(Refusal::Off)));
                }
                listener.stop();
            }
            for q in self.controller.questions.drain() {
                q.payload.answer(Err(error(Refusal::Off)));
            }
            return;
        }
        if self.controller.running.is_none() && self.controller.failure.is_none() {
            let config = ControllerConfig {
                discovery_dir: (!self.controller.no_file).then(crate::settings::config_dir),
                pid: std::process::id(),
                version: aurora_version::version().to_owned(),
                levels: settings.levels,
                host_timeout: HOST_TIMEOUT,
            };
            match Listener::start(config) {
                Ok(running) => self.controller.running = Some(running),
                Err(e) => {
                    log::warn!("session host: the MCP controller did not start: {e}");
                    self.controller.failure = Some(e.kind().to_string());
                }
            }
        }
        let mut taken = Vec::new();
        if let Some((listener, calls)) = &self.controller.running {
            if listener.levels() != settings.levels {
                listener.set_levels(settings.levels);
            }
            for _ in 0..PER_TICK {
                let Ok(call) = calls.try_recv() else { break };
                taken.push((call, listener.host_session().to_owned()));
            }
        }
        for (call, session) in taken {
            self.controller_take(call, &session, now);
        }
        for (q, why) in self.controller.questions.review(&settings, &self.desk, now) {
            log::info!("session host: an MCP controller question was refused ({why:?})");
            q.payload.answer(Err(error(why)));
        }
    }

    /// A call handed over by the listener: decided with what is in force.
    fn controller_take(&mut self, call: Pending, host_session: &str, now: Instant) {
        if call.session.as_deref().is_some_and(|pinned| pinned != host_session) {
            return call.answer(Err(controller::host_changed(host_session)));
        }
        let request = match rules::parse(&call.tool, &call.arguments) {
            Ok(request) => request,
            Err(why) => return call.answer(Err(error(why))),
        };
        let settings = self.desk.controller();
        match rules::admit(&settings, &self.desk, &request) {
            Err(why) => call.answer(Err(error(why))),
            Ok(Admitted::List) => call.answer(Ok(rules::list(&settings, &self.desk))),
            Ok(Admitted::Act { slot, ask: false }) => {
                rules::apply(&mut self.desk, slot, request.action, now);
                let answer = done(self, slot, &call.tool);
                call.answer(Ok(answer));
            }
            Ok(Admitted::Act { slot, ask: true }) => {
                let client = call.client.clone();
                self.controller.questions.ask(&self.desk, slot, request, client, call, now);
            }
        }
    }

    /// The user answered question `number` in the Ask window. Allowed, it
    /// is decided once more, then carried out.
    pub(super) fn controller_decide(&mut self, number: u64, allow: bool, now: Instant) {
        let Some(q) = self.controller.questions.take(number) else {
            return;
        };
        if !allow {
            log::info!("session host: an MCP controller question was refused by the user");
            return q.payload.answer(Err(error(Refusal::Refused)));
        }
        let settings = self.desk.controller();
        if let Err(why) = rules::still(&settings, &self.desk, &q, now) {
            return q.payload.answer(Err(error(why)));
        }
        rules::apply(&mut self.desk, q.slot, q.request.action, now);
        let answer = done(self, q.slot, &q.payload.tool);
        q.payload.answer(Ok(answer));
    }

    /// Demo only ([`DEMO_VARIABLE`]): the controller on, every character
    /// with access; `ask` also asks to put the second character in
    /// Economy, so that the Ask window shows.
    pub(super) fn demo_controller(&mut self, value: &str, now: Instant) {
        use folderstorm_mcp_bridge::Level;
        use folderstorm_mcp_bridge::controller::HostLevels;
        let level = match value.trim().to_ascii_lowercase().as_str() {
            "ask" | "1" => Level::Ask,
            "allow" => Level::Allow,
            _ => return,
        };
        self.desk.set_controller(rules::Settings {
            enabled: true,
            metadata: true,
            levels: HostLevels::all(level),
        });
        let slots: Vec<usize> = self.desk.slots().map(|(slot, _)| slot).collect();
        for slot in &slots {
            self.desk.set_controller_access(*slot, true);
        }
        if level == Level::Ask
            && let Some(profile) = slots
                .get(1)
                .and_then(|slot| self.desk.slot(*slot))
                .map(|s| s.profile.shown().to_owned())
        {
            // (its answer goes nowhere: the demo's own request)
            let (call, _) = Pending::detached(
                "host_background_mode",
                json!({"profile": profile, "mode": "economy"}),
                true,
                Some("Demo assistant"),
            );
            if let Ok(request) = rules::parse(&call.tool, &call.arguments) {
                self.controller
                    .questions
                    .ask(&self.desk, slots.get(1).copied(), request, call.client.clone(), call, now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::chat_ui::tests::host;
    use super::*;
    use ::folderstorm_session::ipc::Standby;
    use folderstorm_mcp_bridge::Level;
    use folderstorm_mcp_bridge::client::{Client, ClientError};
    use folderstorm_mcp_bridge::controller::{HostClass, HostLevels};
    use std::sync::mpsc;
    use std::time::Duration;

    /// A call from another thread, over the real socket, while the host
    /// ticks; `between` runs at each tick (to answer the Ask window).
    fn call(host: &mut Host, client: &Client, tool: &str, args: Value, mut between: impl FnMut(&mut Host)) -> Result<Value, ClientError> {
        let (tx, rx) = mpsc::channel();
        let (client, tool) = (client.clone(), tool.to_owned());
        let session = host.controller.listener().map(|l| l.host_session().to_owned());
        std::thread::spawn(move || {
            let _ = tx.send(client.call_in(&tool, args, session.as_deref()));
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            host.tick(Instant::now());
            between(host);
            if let Ok(answer) = rx.try_recv() {
                return answer;
            }
            assert!(Instant::now() < deadline, "no answer");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn code(result: Result<Value, ClientError>) -> ErrorCode {
        match result {
            Err(ClientError::Tool(e)) => e.code,
            other => panic!("not refused: {other:?}"),
        }
    }

    #[test]
    fn calls_over_the_socket_reach_the_desk_by_the_host_s_rules() {
        let mut host = host();
        // off: nothing listens
        host.tick(Instant::now());
        assert!(host.controller.port().is_none());
        let mut levels = HostLevels::all(Level::Allow);
        levels.set(HostClass::Activate, Level::Ask);
        levels.set(HostClass::Logout, Level::Never);
        host.desk.set_controller(rules::Settings {
            enabled: true,
            metadata: false,
            levels,
        });
        host.desk.set_controller_access(1, true);
        host.tick(Instant::now());
        let listener = host.controller.listener().expect("listening");
        let client = Client::new(listener.port(), listener.token().clone()).with_client_name("Claude Code");
        // the list: only demo-2 has access
        let list = call(&mut host, &client, "host_characters", json!({}), |_| {}).expect("listed");
        let rows = list["characters"].as_array().expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["profile"], "demo-2");
        // Allow: carried out at once
        let done = call(
            &mut host,
            &client,
            "host_background_mode",
            json!({"profile": "demo-2", "mode": "economy"}),
            |_| {},
        )
        .expect("done");
        assert_eq!(done["done"], true);
        assert_eq!(host.desk.slot(1).expect("demo-2").standby, Standby::Economy);
        // a character without access: as if it did not exist
        let refused = call(
            &mut host,
            &client,
            "host_background_mode",
            json!({"profile": "demo-1", "mode": "economy"}),
            |_| {},
        );
        assert_eq!(code(refused), ErrorCode::NotFound);
        assert_eq!(host.desk.slot(0).expect("demo-1").standby, Standby::Warm);
        // Never: refused by the listener, nothing reaches the host
        assert_eq!(
            code(call(&mut host, &client, "host_logout", json!({"profile": "demo-2"}), |_| {})),
            ErrorCode::PermissionDenied
        );
        // Ask: the question waits for the window's answer (demo-2 must be
        // in the world to come forward: it is not, so the host refuses it)
        let refused = call(&mut host, &client, "host_activate", json!({"profile": "demo-2"}), |_| {});
        assert_eq!(code(refused), ErrorCode::NotReady);
        let mut asked = None;
        host.desk.set_controller({
            let mut s = host.desk.controller();
            s.levels.set(HostClass::BackgroundMode, Level::Ask);
            s
        });
        let done = call(
            &mut host,
            &client,
            "host_background_mode",
            json!({"profile": "demo-2", "mode": "warm"}),
            |host| {
                if let Some(q) = host.controller.questions.front() {
                    asked = Some((q.client.clone(), q.request.action));
                    let number = q.number;
                    host.controller_decide(number, true, Instant::now());
                }
            },
        )
        .expect("allowed in the window");
        assert_eq!(done["done"], true);
        assert_eq!(
            asked,
            Some((Some("Claude Code".to_owned()), rules::Action::Background(Standby::Warm)))
        );
        assert_eq!(host.desk.slot(1).expect("demo-2").standby, Standby::Warm);
        // refused in the window
        let refused = call(
            &mut host,
            &client,
            "host_background_mode",
            json!({"profile": "demo-2", "mode": "economy"}),
            |host| {
                if let Some(number) = host.controller.questions.front().map(|q| q.number) {
                    host.controller_decide(number, false, Instant::now());
                }
            },
        );
        assert_eq!(code(refused), ErrorCode::PermissionDenied);
        assert_eq!(host.desk.slot(1).expect("demo-2").standby, Standby::Warm);
        // turned off while a question waits: refused, and nothing listens
        let (tx, rx) = mpsc::channel();
        let (c, session) = (client.clone(), host.controller.listener().map(|l| l.host_session().to_owned()));
        std::thread::spawn(move || {
            let _ = tx.send(c.call_in(
                "host_background_mode",
                json!({"profile": "demo-2", "mode": "economy"}),
                session.as_deref(),
            ));
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        while host.controller.questions.is_empty() {
            host.tick(Instant::now());
            assert!(Instant::now() < deadline, "never asked");
            std::thread::sleep(Duration::from_millis(10));
        }
        host.desk.set_controller(rules::Settings::default());
        host.tick(Instant::now());
        assert!(host.controller.port().is_none() && host.controller.questions.is_empty());
        let refused = rx.recv_timeout(Duration::from_secs(20)).expect("answered");
        assert_eq!(code(refused), ErrorCode::PermissionDenied);
        assert_eq!(host.desk.slot(1).expect("demo-2").standby, Standby::Warm);
    }
}
