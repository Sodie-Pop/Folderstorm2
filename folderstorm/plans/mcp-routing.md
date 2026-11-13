# Local assistant (MCP): session routing and the host's MCP controller

FEATURES.md 4.6 ("session routing (multi-character)") with 5.1 (the session
host). Builds on the login binding of [mcp-receipts.md](mcp-receipts.md)
(section 2, "As built") and its section 8 ("Several characters later"), on
the session host of [session-host.md](session-host.md) (section 8, the
link; checkpoint 10, the settings window) and on the human's notes of
2026-11-12 on Folderstorm (C++)'s host page "MCP controller (host access,
separate authority)".

Folderstorm (C++) did both (`doc/plans/multi-character-mcp.md`,
`doc/plans/mcp-session-control.md`, `tools/fs-mcp/internal/controller/`):
`viewer_list` / `viewer_select` over a shared discovery folder, an exact
`viewer_session` for every account call, never a fall-back to the remaining
viewer; and a separately authenticated host controller (its own discovery
namespace, random instance, Off by default, metadata Never, each write class
Ask, every profile opted out) with `sessions_list`, `session_activate`,
`session_background_mode`, `session_open_login`, `session_restart`,
`session_logout`, `session_detach` and monitor tools. This plan keeps those
rules and fits them to what Folderstorm2 already has: one HTTP bridge per
viewer process, the login binding, the host's pure desk.

## In one page

1. **Every viewer writes its discovery file in one folder**, the main
   profile's settings folder, whatever its profile. Its health says its
   profile, whether a host manages it and, only when its Read class is
   Allow, the character's name.
2. **The sidecar chooses one viewer and stays with it.** One viewer running:
   it is chosen, as today. Several: a call is refused (`target_required`)
   until the assistant chooses with `viewer_select` (after `viewer_list`),
   or the sidecar was started with `--profile <name>` (or `--viewer-pid`).
   The choice is a profile; the calls stay pinned to its login
   (`viewer_session`). A chosen viewer that logs out, logs in again or
   restarts answers `session_changed` once, as today; one that is gone
   answers `unavailable`; **never** does a call go to another character.
3. **The host has its own endpoint, the MCP controller**, off by default,
   turned on in the host's settings ("MCP controller" section). Its own
   token, its own discovery file, its own instance (`host_session`), its own
   permissions: Allow profile and monitor metadata; Activate characters,
   Change background mode, Manage monitor previews, Open login windows,
   Restart characters, Log out characters, Stop managing characters, each
   Ask / Allow / Never (Ask by default); and per character "Allow controller
   access" (off by default). Ask opens a dialog of the host, in front,
   centred on the host's window.
4. **The two authorities never mix.** The controller never turns a viewer's
   local assistant on, never reads its permissions and never logs anyone
   in; a viewer's local assistant grants nothing on the host. Nothing here
   types, reads, stores or sends a credential.

## 1. Discovery: one folder for every profile

Today a named profile's viewer writes `folderstorm2-mcp-<pid>.json` in its
own settings folder (`profiles/<name>/`), where the sidecar does not look.

- `mcp_bridge::discovery_dir()` becomes the main profile's settings folder
  (`folderstorm_characters::profile::main_config_dir()`), for every profile.
  The pid in the name keeps the files apart. The sidecar's default folder
  is unchanged (`FOLDERSTORM2_MCP_DISCOVERY`, else the settings folder),
  so a sidecar configured today finds every character.
- The file itself is unchanged (schema 1: pid, port, token, started): the
  profile is in the health answer, behind the token, not in the file.
- **Health** (`/v1/health`) gains three optional fields:
  `profile` (absent for the main profile: an older viewer without the field
  counts as the main one, the only one that wrote here), `managed` (a
  session host manages this viewer) and `character` (the name the viewer
  shows for its own avatar, **only while Read is Allow**: decision 1 of
  mcp-receipts.md; the same in `viewer_status`). Never the grid, the agent
  id or the account name.
- The host's controller writes `folderstorm2-controller-<pid>.json` in the
  same folder (same schema, same `0600` file, same removal): a reader of
  one kind never reads the other.

## 2. Choosing a viewer in the sidecar

| Sidecar started with | What it talks to |
|---|---|
| nothing (today's configuration) | the one viewer running; with several, nothing until `viewer_select` |
| `--profile <name>` | that profile's viewer only (the main profile: `main`); `viewer_select` can choose no other |
| `--viewer-pid <pid>` | that process only, as today; `viewer_select` can choose no other |

Two tools of the sidecar's own (never sent to a viewer, need no class):

| Tool | Arguments | Returns |
|---|---|---|
| `viewer_list` | none | each viewer that answers in the folder (within `--profile` / `--viewer-pid`): `profile`, `pid`, `managed`, `logged_in`, `viewer_session` (absent logged out), `character` (when its Read is Allow), `selected`; and `selection_required` |
| `viewer_select` | exactly one of `profile`, `viewer_session`, `pid` | the chosen viewer's row; from now on every call goes to it, pinned to that login (`viewer_session`) or, logged out, to the first login seen |

- **The choice is a profile**, with the pid it was found at. A lost
  connection looks again **for that profile only** (a restart of that
  character is found; another character never is): found on the pinned
  login, the call goes; found on another one, `session_changed`, nothing
  sent, then pinned (today's rule); not found, `unavailable` ("the chosen
  character's viewer is not running"), and the choice stays. With
  `--viewer-pid` the pid is the choice, as today.
- **Several viewers and no choice**: every viewer tool (and
  `viewer_status`) answers `target_required`, with the profiles running in
  the message, so that the assistant asks the user which character to use.
  The sidecar never picks "the active one" or "the newest".
- **One viewer and no choice**: it is chosen at the first call (its
  profile), as today. A second viewer started later changes nothing.
- The error code `target_required` is made by the sidecar only; no viewer
  sends it.
- Each sidecar process has one choice. Two assistants (or two
  configurations, one `--profile` each) talk to two characters at once.

## 3. The host's MCP controller

### 3.1 Endpoint

- The host (`--session-host`) starts a listener only while "Enable
  controller MCP" is on: `127.0.0.1` on a port the system picks, a new
  32-byte token, the `Origin` and `Host` checks, the bounds of the viewer's
  bridge (8 KiB head, 64 KiB body, 4 connections, 5 s socket timeouts, 16
  calls queued). Same code as the bridge's checks (`server::check_head`).
- `/v1/health` gives `api`, `product`, `version`, `capabilities`
  (`{controller: 1}`) and `host_session` (`hc-` and 32 hex digits, new each
  time the controller starts). `/v1/call` takes the bridge's `CallRequest`;
  its `session` is the `host_session` the sidecar pinned.
- Its listener threads only authenticate, refuse a class on Never at once,
  and hand the call to the host's window thread (a bounded channel, polled
  at each of the host's ticks, 200 ms), where everything is decided again.
- Turning the controller off refuses every waiting call
  (`permission_denied`), closes its question, stops the listener and
  deletes the file. Quitting the host does the same.
- The sidecar finds it in the same folder, pins its `host_session` at the
  first call and, after a host restart, answers `session_changed` once
  without sending the call (the viewers' rule).

### 3.2 Tools

All named `host_*`, listed by the sidecar with the others (MCP clients see
one server). Characters are named by `profile` (unique on a host, stable
when cards swap places); `slot` (1–5) is shown, never used to address.
An optional `launch` (from `host_characters`) refuses the call when that
character's viewer was started again since (`session_changed`).

| Tool | Class | What it does | Refused when |
|---|---|---|---|
| `host_characters` | none (controller on) | the characters with controller access: `slot`, `profile`, `state` (`stopped`, `starting`, `login`, `loading`, `in_world`, `ended`, `crashed`, `unmanaged`), `active`, `background_mode`, `launch`, `closing`; with metadata: `character` (logged-in name), `alias`, `colour`, `monitor` (open, size, rate, what it shows), and the cycling monitor | — |
| `host_activate` | Activate characters | that character comes forward (the card's Switch) | not in the world (at its login screen: `host_open_login`) |
| `host_background_mode` | Change background mode | `mode`: `warm` / `economy` | — |
| `host_monitor` | Manage monitor previews | `open`: true / false, for a character (`profile`) or the cycling monitor (`cycling: true`) | the active character (no monitor), not in the world (open) |
| `host_open_login` | Open login windows | a character not logged in: its viewer started, or its login screen brought forward (the bar's "Open a login"); never fills or sends anything | logged in, starting, closing, unmanaged |
| `host_restart` | Restart characters | its viewer logs out and closes (the card's Close), then starts again at its login screen; one that ended starts again | unmanaged, starting, already closing |
| `host_logout` | Log out characters | its viewer logs out and closes (the card's Close) | no viewer, closing |
| `host_stop_managing` | Stop managing characters | the host lets that viewer go (it goes on by itself, as with a host closed) | no managed viewer |

- **Metadata off** (the default): no names, aliases, colours or monitor
  details; the profile name stays, because it is how a character is
  addressed and the user typed it in the host. A character without
  controller access is not listed, and naming it answers `not_found`, as
  for a profile that does not exist.
- An action answers once the desk has taken it (`done`, with the
  character's row): a viewer starting, logging out or closing does that in
  its own time; `host_characters` shows where it stands.
- None of these sends anything to the grid, starts a login, enables a
  viewer's local assistant or changes its permissions, voice or input
  beyond what the same click in the host does.

### 3.3 Permissions and Ask

- Saved in `session-host.json`: `controller` (`enabled` false, `metadata`
  false, one level per class, Ask by default) and per character
  `controller` (false). Absent fields read as these defaults, so a file of
  checkpoint 10 reads unchanged.
- Decided twice: by the listener (Never refuses at once) and by the host
  for the call it takes. A call in the dialog is decided again at every
  tick: the controller off, the class on Never, the character's access
  removed, the character gone or started again (another launch), or the
  action no longer possible (it logged in, it is closing) refuses it; an
  Ask turned to Allow still asks.
- **Ask** opens the host's "MCP controller request" window, in front and
  centred on the host's window (its last place when the window is hidden
  behind a docked bar): the program's name (`clientInfo.name`, "the name it
  gives itself"), what it asks in plain words ("Log out “Sodie” (character
  2)"), a 60 s countdown, Allow and Refuse, neither the default; closing
  the window or the end of the 60 s refuses. One question at a time,
  oldest first. An approval covers that one call.
- The sidecar's and the listener's waits grow by the 60 s of a question.

## 4. Questions of a background character (viewer side)

A managed viewer's local-assistant Ask dialog opens in its own window, which
is behind or minimised while the character is in the background. The open
question is added to the viewer's Attention list (`AttentionKind::Assistant`:
the program's name and the dialog's sentence), so that the host's inbox and
the bar's count show it; "Switch to answer" brings that character forward
and its own dialog decides. The deadline does not grow. Listed whatever
the Attention kinds chosen (it is no chat and no offer), for characters
included in Attention. The link's protocol becomes 6 (host and viewers are
one program: the handshake already refuses another number).

## 5. Capability versions

| Where | Capability | Covers |
|---|---|---|
| viewer bridge (`binding::CAPABILITIES`) | `sessions: 1` | `profile`, `managed` and `character` in health and `viewer_status`; the shared discovery folder |
| host controller (`controller::CAPABILITIES`) | `controller: 1` | the endpoint, `host_session`, the eight `host_*` tools |

`API_VERSION` stays 1. An older viewer (no `sessions`) is listed as the main
profile with no name. `host_*` tools need a host with `controller: 1`
(`unsupported` otherwise, nothing sent); `viewer_list` / `viewer_select`
need nothing of a viewer. `ErrorCode::TargetRequired` is the sidecar's
own and never crosses the wire from a viewer.

## 6. Where the code goes

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/binding.rs` | `SESSIONS`; `random_id` |
| `folderstorm-mcp-bridge/src/protocol.rs` | `Health.profile/managed/character`; `ErrorCode::TargetRequired` |
| `folderstorm-mcp-bridge/src/discovery.rs` | `Kind` (viewer, controller) |
| `folderstorm-mcp-bridge/src/server.rs` | `Bridge::set_identity`; `check_head` shared |
| `folderstorm-mcp-bridge/src/controller.rs` (new) | host classes, the `host_*` catalogue, `ControllerHealth`, the controller's listener |
| `folderstorm-mcp/src/connect.rs` | the choice (profile / pid), `viewer_list`, `viewer_select`, `target_required`, no fall-back |
| `folderstorm-mcp/src/host.rs` (new) | the controller's client, its pin |
| `folderstorm-mcp/src/jsonrpc.rs`, `main.rs` | the local tools and `host_*` in `tools/list`; `--profile` |
| `folderstorm-session/src/controller.rs` (new) | pure: settings, decisions, arguments, eligibility, the listing and its metadata gate, the questions and their re-check |
| `folderstorm-session/src/host.rs` | `controller` settings, per-character access, `Desk::restart`, `Desk::release` |
| `folderstorm-session/src/settings.rs` | `Section::Controller` |
| `folderstorm-session/src/ipc.rs` | protocol 6, `AttentionKind::Assistant` |
| `aurora-viewer/src/mcp_bridge.rs`, `tools.rs` | the shared folder, the identity in health and `viewer_status`, the open question for Attention |
| `aurora-viewer/src/folderstorm_characters/host/controller.rs` (new) | the controller's run in the host: start/stop, calls, answers |
| `aurora-viewer/src/folderstorm_characters/host/ui/controller.rs` (new) | the settings section and the Ask window |
| `aurora-viewer/src/folderstorm_characters/chat_export.rs` | the assistant's question in the Attention list |

**Upstream hooks: none new.** The locales (`fs-mcp-*` and `folderstorm-*`
blocks) and `docs/TESTING.md` get lines.

## 7. Demo

- `AURORA_DEMO=1 AURORA_DEMO_SESSION_HOST=3 AURORA_DEMO_MCP=1`: each demo
  viewer starts its bridge (the variable is inherited); every discovery
  file is in the demo's settings folder; a sidecar started with
  `FOLDERSTORM2_MCP_DISCOVERY=<that folder>` lists three viewers.
- `AURORA_DEMO_SESSION_HOST_CONTROLLER=ask|allow` (with the host's demo):
  the controller is on for the demo (not saved), every demo character has
  access, metadata on, every class Ask (`ask`) or Allow (`allow`); `ask`
  also makes one request of its own ("Activate demo-2") so that the dialog
  shows.

## 8. Acceptance tests

Pure (`folderstorm-session`, the bridge crate, the sidecar):

- choosing: one viewer chosen at the first call; several refused with
  `target_required` until `viewer_select`; `--profile` and `--viewer-pid`
  limit the list and the choice; selection by profile, by `viewer_session`
  (exact login), by pid; a stale `viewer_session` refused;
- binding to the choice: a relog answers `session_changed` once; a viewer
  that quits answers `unavailable` while another character still runs,
  and nothing reaches the other one; the chosen profile restarted is found
  again (`session_changed`, then pinned);
- health: `profile`, `managed`; `character` only while Read is Allow;
  an older health read as the main profile;
- the controller: off answers nothing (no file); classes Never / Ask /
  Allow; a character without access unlisted and `not_found`; metadata
  gating of every field; each action's eligibility; arguments refused
  (unknown mode, both `profile` and `cycling`, a stale `launch`);
  `host_session` pinning and a host restart; questions refused when the
  controller goes off, a class goes to Never, access is removed, the
  character starts again or ends, or 60 s pass; Ask → Allow still asks;
- settings: defaults off, a checkpoint 10 file reads unchanged, saved and
  read back;
- `Desk::restart` closes then starts again; `Desk::release` lets go;
- Attention: the assistant's question listed and counted, with the kinds
  off, and not for a character left out.

End to end over real sockets:

- sidecar → two real bridges in one folder (two profiles): choose one, its
  calls reach it only; it quits: `unavailable`, the other never called;
- sidecar → the host's controller → the host's desk (the `Host` struct
  ticked by hand): list, an Allow action carried out, an Ask answered by
  the dialog's decision, Never refused at the listener.

Not testable here: the Ask window's look, real MCP clients on Windows, two
real characters on the grid.
