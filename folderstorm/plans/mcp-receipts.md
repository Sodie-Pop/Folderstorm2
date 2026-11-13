# Local assistant (MCP): receipts, plans, login binding and operations

Step 3 of the MCP parity order: the foundation that the later tool groups
need (navigation, uploads, content editing, object dialogs, task scripts,
history and undo, session routing). Checkpoint 1 is PR #28 and
checkpoint 2 is built on top of it, checkpoints 3 to 5 on
`claude/mcp-receipts` (see "As built" below). It extends [mcp.md](mcp.md)
(phases 1 and 2) and assumes the two open catalogue PRs, #21 (camera, `claude/mcp-camera`) and #25 (inventory gaps,
`claude/mcp-inventory-gaps`), land first: both touch `protocol.rs`,
`desk.rs` and `mcp_bridge.rs`.

Sources, read as information: Folderstorm (C++)'s `doc/plans/`
(`mcp-async-native-operations.md`, `mcp-expansion-roadmap.md` E0,
`assistant-permissions-and-approvals.md`, `bulk-inventory-review-and-history.md`,
`multi-character-mcp.md`, `mcp-session-control.md`, `mcp-file-uploads.md`)
and `tools/fs-mcp/internal/` (`viewerapi/policy.go` capabilities,
`discover/discover.go` `viewer_session`, `mcptools/async.go`,
`inventory_bulk_review.go`, `navigation.go`); [session-host.md](session-host.md)
for the login generation and one process per character.

## In one page

| Piece | What it is, in plain words | Why it is needed |
|---|---|---|
| **Permission re-check** | A waiting call is checked against the user's permissions again while it waits, when Allow is clicked and just before it is sent | Today a queued Ask still runs after the user sets its class to Never (verified below) |
| **Login binding** (`viewer_session`) | Every call, plan, question and receipt belongs to one exact login of one viewer; another login refuses it | The bridge keeps running across logout and login; the sidecar may reconnect to another viewer |
| **Capability versions** | The viewer and the sidecar each say which tool groups (and which version of them) they know | An older or newer viewer, sidecar or assistant must refuse clearly, never half-work |
| **Receipt** | A bounded, in-memory record of one change the assistant asked for: who asked, what the permissions decided, the Ask answer, what was sent, what the server said | A lost or late answer must be checkable, never retried blindly |
| **`client_request_id`** | An id the assistant chooses; the same id twice returns the first receipt instead of running again | Safe retry after a lost answer |
| **Plan** (preview → execute) | A preview returns a `plan_id` and the exact rows before and after; execute takes the `plan_id`, checks everything again and runs it once, after one Ask | Batch changes the user (and the assistant) can read before anything happens |
| **Operation** | Every change is an operation with an `operation_id`, a state, a deadline, guards checked before each send, and a cancel | Long actions (uploads, teleports) must not hold the connection, and must stop when the world changed |

The ids keep the Folderstorm (C++) names so that assistants and docs carry
over: `viewer_session`, `client_request_id`, `operation_id`, `plan_id`,
`assistant_operation_status`, `inventory_preview_batch_rename`,
`inventory_preview_batch_move`, `inventory_execute_plan`.

## What exists today (verified in the code)

| Area | Today | Gap |
|---|---|---|
| Permission decision | `server.rs` `call_tool` decides Allow / Ask / Never when the call arrives, on the listener thread; Ask sets `Pending::ask` | Decided once, never again (next section) |
| Waiting calls | `mcp_bridge/desk.rs`: questions (`asks`), allowed changes (`ready`), changes sent (`sent`, 30 s), bake waits; one Ask dialog at a time, 60 s | No id, no record once answered |
| The send | `App::local_assistant_run` (`app/local_assistant.rs`): `mcp_bridge::replan` checks the viewer's rules and the inventory again, then the inventory window's submit or `apply_appearance_action` | A single place where every assistant change leaves: the natural guard point |
| Late answers | The bridge waits with `recv_timeout`; an answer after that is dropped (`Pending::answer` uses `try_send`), but the desk still holds the call and may still run it. A change the server does not confirm in 30 s answers `timeout` ("it may still land") and its later answer is ignored | The assistant cannot learn what happened |
| Login | `World::login` is set at login and cleared at logout (`NetEvent::LoggedOut` → `back_to_login` → `World::reset`), in the same process; the bridge and the desk ignore it | A dialog opened before a relog can run against the next login |
| Sidecar | `folderstorm-mcp` handles one JSON-RPC line at a time (`main.rs`); `connect::Viewer::call` looks for the viewer again after a refused connection or token and **sends the same call once more** to whichever viewer it finds | A long call blocks the MCP session; a retry can reach another viewer process |
| Health | `/v1/health`: `api`, `product`, `version`, `read_allowed`; the sidecar refuses another `API_VERSION` | No list of tool groups |
| Unknown fields | Each tool refuses an unknown argument (`changes::known`, `tools::known_argument`); the envelope (`CallRequest`) and `Health` ignore unknown fields (serde defaults) | Good: a tool never ignores a field silently. The envelope needs care |
| Activity | 20 entries in the bridge (`server::record`): time, tool, outcome; Preferences shows the last 5 | Never says what changed |
| Native bulk review (FEATURES 2.4, #7) | `ui/inventory/bulk_review.rs`: `plan_rename`, `plan_move`, `mutation`; Execute plans again and sends only if the rows are identical to the preview | Not reachable from the bridge (private module) |

### Answer: do queued Asks re-check permissions? No.

- `mcp_bridge::frame` copies new levels into the bridge
  (`Bridge::set_permissions`) but never tells the desk.
- `Desk::tick` moves an allowed question to `ready` without looking at the
  levels; `local_assistant_run` → `replan` re-checks the viewer's rules,
  not the user's levels.
- Turning access off does end waiting calls: the desk is reset
  (`rt.desk = Desk::default()`), the bridge answers "the call was dropped"
  and the dialog disappears.

So: with a class changed from Ask to Never while its dialog is open or
queued, the dialog stays and **Allow still runs the change**. A call admitted
under Allow but still in the channel (up to 16 queued, 4 taken per frame)
also runs after Never. And after a relog, an old dialog can still run on the
new login (ids usually no longer resolve, but `inventory_create_folder`
without a parent would create a folder in the new character's My Inventory).
Section 1 fixes all three; it is checkpoint 1, small and independent.

## 1. Re-checking permissions while a call waits

The frame thread becomes the authority: it receives the permissions in
force (`frame` already has them) and decides again. The listener thread's
check stays as a fast refusal only.

| What changes while a call waits | Not shown yet, or in the dialog | Allowed, not sent yet | Already sent |
|---|---|---|---|
| Any class it needs → Never | refused at once (`permission_denied`, "the permissions changed"); the dialog closes | refused | not undone; a long operation sends nothing more |
| Ask → Allow | **still asks**: an Allow never answers an open question | – | – |
| Allow → Ask | asks | goes back to asking | continues |
| Access turned off | refused ("local assistant access was turned off") instead of "dropped" | refused | not undone; nothing more is sent |
| Another login (section 2) | refused (`session_changed`) | refused | not undone; nothing more is sent |
| A class it does not need | nothing | nothing | nothing |

- **When it is checked**: when the frame thread takes the call; on every
  frame while it waits (a comparison of levels, cheap); when Allow is
  clicked; and in `local_assistant_run`, just before the send.
- **Compound classes** (#21's `ToolSpec::also`): any Never refuses; any Ask
  gives one dialog. A call that always asks (#21's `interface: true`
  picture) never runs without a dialog, whatever the levels.
- An approval covers that one call. It never changes the saved levels.
- `Desk::decide` stays idempotent: a second click, or a click after the call
  was refused, does nothing.

## 2. Exact login binding

| Term | What it is |
|---|---|
| **Login generation** | A counter in `mcp_bridge` that moves each time the frame thread sees the login change: logged out ↔ logged in, another agent id or another grid. Read from `View.world` once per frame (`world.login`, the agent id, the grid as the account folders key it, `chat_log::store::Identity`). A login takes many frames, so no change can be missed. The demo session is one generation. Same idea as the session host's login generation (session-host.md § 8): one counter, reused by the host when it lands. |
| **Bridge epoch** | A new random value each time the bridge starts (access turned on, `--mcp-api`), like the token. |
| **`viewer_session`** | `vs-` and 32 hex digits from the OS random source, made for each (epoch, generation). Not the agent id, not the simulator's session id. Not a secret either: the token is still required, and the value is useless elsewhere. |

- `/v1/health` and `viewer_status` give the current `viewer_session`
  (absent when logged out). `viewer_status` also gives `grid` and
  `character` (the avatar name the viewer shows), so the assistant can say
  which character it acts on.
- **The sidecar pins it**: it reads `viewer_session` from health when it
  connects and sends it with every call (`CallRequest.session`). The frame
  thread compares: another value answers `session_changed`, with the current
  `viewer_session` in the error. The sidecar re-pins then, but **never sends
  the call again**: the assistant looks again (its ids may belong to another
  inventory) and decides.
- **Rediscovery**: after a refused connection or token, the sidecar looks
  for a viewer again as today, but sends the call only if that viewer's
  `viewer_session` is the pinned one (after a restart it never is);
  otherwise it answers `session_changed`. The very first call, with nothing
  pinned yet, works as today.
- **Bound to the session**: every operation, Ask question, plan, receipt
  lookup and late server answer. A plan or receipt of another session
  answers `not_found`: nothing tells that it existed.
- **At a login change**: waiting questions and unsent calls are refused
  (section 1), plans are dropped, and the receipts of the old login are
  cleared (Activity lines stay, as today).
- Turning access off and on keeps the login but makes a new epoch, so a new
  `viewer_session`: the assistant's old plans stop working (they died with
  the bridge anyway), while Preferences still shows the login's receipts.

## 3. Capability versions

`Health` and `viewer_status` gain `capabilities`, a map from a tool group to
the version this build implements. Each `ToolSpec` names its group.

| Capability | Version | Covers |
|---|---|---|
| `inventory` | 1 | phases 1 and 2 |
| `camera` | 1 | #21 |
| `inventory_gaps` | 1 | #25 |
| `login_binding` | 1 | `viewer_session`, `session_changed` |
| `receipts` | 1 | `operation_id`, `client_request_id`, `assistant_operation_status` |
| `plans` | 1 | batch preview and execute |
| `operations` | 1 | early answers, `assistant_operation_cancel` |

Later groups add theirs (`navigation`, `world`, `uploads`, `sessions`…).

- **Versioning rule**: an addition keeps `API_VERSION` 1; a field whose
  meaning changes raises its group's version; a broken wire raises
  `API_VERSION` (the sidecar's existing check refuses the other version).
- **Sidecar → viewer**: the envelope carries the sidecar's own map
  (`CallRequest.features`). The viewer answers early (section 5) and uses
  the new error codes only for a sidecar that declares them; for an older
  one it keeps today's behaviour and maps new codes onto old ones
  (`session_changed` → `not_ready`, `stale` → `not_allowed`, `conflict` →
  `invalid_arguments`, `unsupported` → `unknown_tool`), because an older
  sidecar reads an unknown code as "unreadable error".
- **Viewer → sidecar**: before sending a call, the sidecar checks that the
  viewer advertises the tool's group; otherwise it answers `unsupported`
  ("this viewer is older than this sidecar: update the viewer") and sends
  nothing. A viewer without `capabilities` counts as `{inventory: 1}`. The
  sidecar relies on `session` being enforced only when the viewer advertises
  `login_binding`.
- **`tools/list`** keeps listing the sidecar's whole catalogue (as Folderstorm
  (C++) does); the per-call check above gives the clear refusal. See open
  question 4.
- **Assistants that ignore the new fields keep working**: today's tools still
  answer with the outcome; `operation_id` and `state` are extra fields. The
  MCP `protocolVersion` negotiation is unchanged.

## 4. Receipts

A receipt is made for every call of a change tool (`ToolSpec::changes()`)
that the frame thread takes, whatever its end: done, refused in the dialog,
refused by the viewer's rules, failed, not confirmed. Calls the listener
refuses before they reach the viewer (Never, unknown tool, too many calls)
stay Activity lines only. Reads get none.

| Field | Content | Assistant sees | Preferences shows |
|---|---|---|---|
| `operation_id` | `op-` and 32 hex digits, made by the listener when the call arrives (so even its own timeout error can carry it) | yes | – |
| `client_request_id` | the assistant's own id, when given | yes | – |
| `tool`, `client` | the tool, and the name the program gives itself (48 characters) | yes | yes |
| `viewer_session` | its binding | yes | – |
| times | asked, decided, sent, finished | yes | yes |
| `decision` | `allowed`, `approved` (Allow clicked), `refused_by_user`, `ask_timed_out`, `refused_permissions_changed`, `refused_by_viewer` (`not_allowed`), `stale` | yes | yes |
| `summary` | the Ask dialog's sentence, in the user's language | only when Read is Allow | yes |
| `targets` | the inventory ids it touches (50 at most), never asset ids | yes | – |
| `rows` | plans only: id, before, after, outcome | names only when Read is Allow | the counts |
| `server` | `confirmed`, `queued` (a grid without AIS accepted the moves for later, `Reply::moves_queued`), `refused` with the server's reason, `no_answer` | yes | yes |
| `result` | the ids the tool's answer gives (created folder or item ids) | yes | – |
| `state` | its lifecycle (section 5) | yes | yes |
| `plan_id` | when it ran a plan | yes | – |

**The rule**: a receipt never holds more than its Ask dialog showed and its
tool's answer returned. Never the arguments as given (only a hash, to detect
a reused `client_request_id`), asset, texture or thumbnail ids, pictures,
notecard, script or chat text, local file paths, credentials, tokens,
capability URLs or the simulator session id. Names appear only in
`summary` and `rows`, as in the dialog.

**Where it is kept**: in memory only, on the frame thread (the `Runtime` of
`mcp_bridge.rs`). The store itself (ids, bounds, deduplication, redaction,
eviction) is a pure type in the bridge crate, `receipts.rs`, tested without
a viewer. Never written to disk and never logged: log lines keep counts, as
`local_assistant_submit` does today.

| Bound | Value |
|---|---|
| Receipts per login | 100 |
| Rows, all receipts together | 2,000 |
| Text | summary 300 characters, names 128; control characters removed, cut on a character boundary |
| Retention | until logout, another login or quit; a finished receipt also leaves after 2 hours, or when 100 are kept (oldest finished first) |
| Live receipts | never evicted; with 100 live, a new change answers `busy` |
| Retired `client_request_id`s | the last 1,024 kept as hashes, so an evicted id is never taken for a new request |
| Late server answers | a change sent but not confirmed keeps listening for its answer for 10 minutes (16 at most) and updates its receipt |

**`client_request_id`**: optional on every change tool (1 to 64 printable
ASCII characters). The same id with the same arguments in the same session
answers the existing receipt, in its current state, and runs nothing; with
other arguments, or an evicted id, it answers `conflict`. Duplicates
arriving together are safe: the frame thread takes calls one by one. The
sidecar never invents one: the `operation_id` is returned anyway, in results
and in errors, timeouts included. Required later for effects nobody can
look at afterwards (open question 3).

**How the user sees them**: Preferences › Local assistant › Recent activity
keeps one line per call (tool, outcome, time); a line with a receipt shows
its summary and state ("Moved 3 entries to “Clothes/Old” · done", "… · not
confirmed yet") and names the program in its tooltip. Live operations get
a Stop button (checkpoint 5). A full list waits for the history window of
FEATURES 2.5.

**How tools reference them**:

- every change result gains `operation_id` and `state`; an error after
  admission carries `operation_id` (`ToolError` gains an optional field);
- `assistant_operation_status` (no class, like `viewer_status`, so it works
  with Read on Never; names only when Read is Allow): one receipt by
  `operation_id` or `client_request_id`, or the newest 20 of this session;
- `inventory_execute_plan` answers with its operation; FEATURES 2.5's
  `inventory_history` and `inventory_preview_undo` will read the rows of
  plan receipts.

## 5. Operations: async and guarded

Every change call is an operation: the listener gives it its id, its
receipt is its record.

| State | Meaning |
|---|---|
| `waiting` | taken by the frame thread, before its dialog or its turn |
| `asking` | in the Ask dialog |
| `queued` | allowed, waiting for its turn |
| `running` | something was sent; waiting for the server, the teleport, the bake |
| `done` | confirmed |
| `failed` | the server refused it |
| `refused` | by the user, the permissions or the viewer's rules |
| `cancelled` | stopped before anything was sent |
| `stopped` | stopped after some steps were sent; the receipt says which |
| `stale` | a guard found that the world changed before sending |
| `unconfirmed` | sent, no answer in its time; it may still land (and its receipt keeps listening) |

**When a call answers** (`ToolSpec::answer`):

- **Wait** (every tool today): when the operation ends, or at the latest
  just before the listener's own wait runs out, with `state` and
  `operation_id` instead of today's bare `timeout`.
- **Early** (long tools: teleport, uploads, script tests, and
  `inventory_execute_plan` with `wait: false`): right after the frame thread
  took it, with `operation_id` and `state` (often `asking`); the assistant
  polls `assistant_operation_status`. Only for a sidecar that declares
  `operations`; otherwise Wait.

Why: the sidecar handles one call at a time, so a call waiting 60 s for the
dialog plus 30 s for the server blocks the whole MCP session and holds one of
the bridge's 4 connections, and MCP clients often give up on long calls. Newer MCP revisions describe an experimental
task mechanism for long calls; clients do not all support it. Operation ids
work with every client, and the sidecar can map them onto MCP tasks later
without changing the viewer.

**Guards**, checked when an operation is admitted and again before each step
that sends something:

| Guard | Fails as |
|---|---|
| Same `viewer_session` | `session_changed` |
| Access on; no class it needs on Never (an Ask already approved covers the operation) | `permission_denied` |
| Its deadline | `cancelled`, or `unconfirmed` once something was sent |
| The tool's own preconditions: plan rows unchanged, same region, price not higher, the dialog still open… | `stale` |
| The viewer's rules and RLVa (the re-plan of `local_assistant_run`) | `not_allowed` |

Every assistant send leaves from one place on the frame thread
(`App::local_assistant_run`), so the guard is one call there, and a
multi-step operation calls it before each step. Folderstorm (C++) needed
callback leases across its native callback chains; Folderstorm2 does not.
**No replay**: a step that was sent is never sent again automatically and
never rolled back automatically. "Sent" is not "confirmed".

**Cancel**: `assistant_operation_cancel` (`operation_id`; no class, same
session, so it works after the permissions were lowered: it only stops the
assistant's own operation). `waiting`, `asking` (the dialog closes) and
`queued` become `cancelled`; a `running` multi-step operation sends nothing
more and becomes `stopped`; a single change already sent cannot be recalled,
and the answer says so. Preferences' Stop does the same.

| Bound | Value |
|---|---|
| Live operations per session | 8 |
| Questions waiting for the dialog | 16. Today each waiting call holds one of the bridge's 4 connections, which bounds them; an early answer frees its connection, so the desk needs its own bound |
| Lanes | one operation sending at a time per lane, as the windows do: inventory (the inventory window's single pending change), outfit (Appearance), camera; later navigation and uploads. A busy lane answers `busy`, as today: no queue behind the windows in this step |
| Deadline | the tool's: 60 s of dialog plus its own wait; Early tools set their own |

## 6. Plans: preview, then execute

| Tool | Class | Arguments | Returns |
|---|---|---|---|
| `inventory_preview_batch_rename` | read | `items`: 1–50 `{id, name}`; or `ids` (1–50) with `pattern` (`{name}`, `{n}`, as the native review) | `plan_id`, `expires_in` (seconds), `rows` (`id`, `before`, `after`, or `skip` with its reason), counts, `execution` (Edit now: `allow`, `ask` or `never`), `viewer_session` |
| `inventory_preview_batch_move` | read | `ids` (1–50); `folder_id` or `path` | the same, for Move |
| `inventory_execute_plan` | the plan's (Edit or Move), decided by the viewer from the plan | `plan_id`; `client_request_id`; `wait` (default true; false answers early, checkpoint 5) | `operation_id`, `state`, `rows` with their outcome |

- **Preview is a read**: it changes nothing and grants nothing. It needs
  Read (the rows carry names) and shows the plan even when execution is on
  Never (`execution: never`).
- **One planner for the window and the bridge**: `plan_rename`, `plan_move`,
  `mutation`, `Row` and `apply_pattern` move from `ui/inventory/bulk_review.rs`
  to a new `folderstorm/bulk_plan.rs`; the native review calls them from
  there, the bridge too. A rename to explicit names (`items`) is added next
  to the pattern. A plan is therefore never allowed what a single rename or
  move, or the native review, refuses. The bridge's own extras apply on top:
  the Marketplace Listings folder is left alone, RLVa's `@showinv` refuses,
  duplicate ids are refused.
- **Execute checks everything again**, in this order: the plan exists in this
  session, is not expired and was not used; the class, decided now (Never
  refuses, Ask gives one dialog); a new plan from the same request, whose
  rows must equal the preview exactly (the native review's rule) — otherwise
  `stale`, with the new rows, and nothing is sent; the same again after the
  dialog, just before the send.
- **Used once**: the plan is consumed when its execute is admitted. Executing
  it again answers the same operation; nothing is sent twice. A plan refused
  or found stale is gone: preview again.
- **Expiry**: 5 minutes; 8 open plans per session (the oldest unused one
  goes); plans end with the session and when access is turned off.
- **One Ask for the whole plan**: "“Claude Code” asks to move 12 entries to
  “Clothes/Old”", then at most 5 rows, "and 7 more", and the number skipped,
  taken from the plan made when the dialog opens.
- **Outcome**: the eligible rows leave as one inventory change (`mutation`,
  through `local_assistant_submit`), so the server answers for the whole.
  Confirmed: every row `done`. Refused: the rows are `unknown` (a change can
  land in part: `Reply::refresh`), and the answer says to look at the
  inventory before trying again. `moves_queued`: `queued`. The receipt keeps
  each done row's before and after, which FEATURES 2.5's undo needs.
- Kept from the native review: the frozen selection, 50 rows, skipped rows
  with their reason, Execute only for the plan shown. Not now: Folderstorm
  (C++)'s optional "always review 10 rows or more" setting (open question 5).

## 7. How the later groups use this

| Group | `client_request_id` | Plan | Early operation | Guards beyond the common ones |
|---|---|---|---|---|
| Batch preview and execute | optional | yes | with `wait: false` | rows unchanged |
| History and undo (FEATURES 2.5) | optional | an undo plan | with `wait: false` | only `done` rows; the entry still in its recorded after-state |
| File uploads (preview, execute, status, cancel) | required | yes: files, hashes, per-file price, 120 s | yes: sequential files, cancel stops the unsent ones | price not higher; a positive price always asks; file bytes unchanged |
| Navigation (move, teleport, stop) | required for teleport | – | yes | same region; manual input takes over; a teleport is never sent twice |
| Object dialogs (reply) | required | – | – | the dialog still open, same object and channel; never payment, permission, teleport or inventory offers |
| Task scripts (save, compile) | required | a source snapshot (hash) | yes: save, then compile, two outcomes | source unchanged; the script still in the prim |
| Script test sessions | required | the frozen recipe | yes, with cancel | the owned prims still there |
| Chat send | required | – | – | the text frozen in the dialog; rate limits |
| Session routing | – | – | – | `viewer_session` names the viewer (section 8) |

Each group's own plan says the details; this table only shows that the
foundation covers them.

## 8. Several characters later (session host)

One viewer process per character (session-host.md), so:

- each process keeps its own bridge, token, receipts, plans and questions;
  nothing crosses processes, and the host never sees a token;
- each login has its own `viewer_session`; the sidecar learns them through a
  `viewer_list` tool (character, grid, profile, `viewer_session`);
- the discovery files move to a folder all profiles share (today each
  profile would write into its own settings folder, `config_dir()`, where
  the sidecar does not look), read with the existing bounds;
- tools gain an optional `viewer_session` argument, which the sidecar takes
  out to choose the viewer; with several viewers running, a call without one
  is refused (`target_required`), **never** sent to "the active one"
  (session-host.md § 5); a pinned viewer that disappears answers
  `session_changed`, never falls back to the remaining one;
- an Ask for a background character shows in the host's Attention list;
  Review brings that character forward and its own dialog decides; the
  deadline does not grow.

This is the session routing group, after FEATURES 5.1; nothing of it is in
the checkpoints below.

**What checkpoint 2 prepares for it** (nothing more is built): one
`viewer_session` names one login of one process, so it is the address of a
character.

- `--viewer-pid` already picks one process, so one character, today; it
  stays the way to give an assistant one fixed character.
- `connect::find` already returns each viewer's health, `viewer_session`
  included: `viewer_list` is `discovery::find` over the shared folder, one
  health per file (the existing probe), giving each live process's
  `viewer_session` (and the character, once a decision says where it may be
  shown; see "As built").
- The sidecar's single pin becomes one link per `viewer_session`
  (`connect::Viewer` holds one `Link` and one pin today): the call's
  `viewer_session` argument chooses the link and is sent as
  `CallRequest::session`, so each process checks its own login exactly as
  now. The no-resend rule is per link: a process found again on another
  login answers `session_changed`; a vanished one answers `session_changed`
  with no session; neither ever falls back to another process.
- Nothing changes on the bridge side: each process's bridge already refuses
  a session that is not its own.

## Wire changes

| Where | Change | Older other side |
|---|---|---|
| `Health` | + `capabilities`, + `viewer_session` | an older sidecar ignores them; a newer sidecar reads "none" as `{inventory: 1}`, without binding |
| `CallRequest` | + `session`, + `features` | an older viewer ignores both: no early answer, no binding check; the sidecar knows it from `capabilities` |
| `ToolError` | + `operation_id`; `session_changed` carries the current `viewer_session` | an older sidecar shows the message |
| `ErrorCode` | + `session_changed`, `stale`, `conflict`, `unsupported` | sent only to a sidecar that declares them; mapped onto old codes otherwise |
| Change results | + `operation_id`, `state` | additive |
| `viewer_status` | + `viewer_session`, `capabilities`, `grid`, `character` | additive |
| New tools | `assistant_operation_status`, `assistant_operation_cancel`, `inventory_preview_batch_rename`, `inventory_preview_batch_move`, `inventory_execute_plan` | an older viewer answers `unknown_tool`; the sidecar checks capabilities first and says `unsupported` |

`API_VERSION` stays 1.

## Where the code goes

| Where | What |
|---|---|
| `crates/folderstorm-mcp-bridge/src/receipts.rs` (new) | the receipt store: ids, bounds, deduplication, retired ids, redaction, eviction; pure, no thread, no viewer |
| `crates/folderstorm-mcp-bridge/src/protocol.rs` (or its per-group files, as #25 splits them) | capabilities, each tool's group and answer mode, "class from the plan" for `inventory_execute_plan`, the new error codes and their fallback, the new tools |
| `crates/folderstorm-mcp-bridge/src/server.rs` | `operation_id` at arrival, `session` and `features` in the envelope, the new `Health` fields |
| `crates/folderstorm-mcp/src/connect.rs`, `jsonrpc.rs` | pinning, no retry on another session, the capability check, `features`; `operation_id` shown in errors |
| `crates/aurora-viewer/src/mcp_bridge/login.rs` (new) | login generation and `viewer_session` |
| `crates/aurora-viewer/src/mcp_bridge/desk.rs` | the permission re-check, operation states, receipt updates, early answers, cancel, late answers |
| `crates/aurora-viewer/src/mcp_bridge/plans.rs` (new) | the plan store, preview and execute |
| `crates/aurora-viewer/src/folderstorm/bulk_plan.rs` (new) | the planner taken out of `ui/inventory/bulk_review.rs` (a Folderstorm file), which then calls it |
| `crates/aurora-viewer/src/app/local_assistant.rs` | the guard just before each send |
| `crates/aurora-viewer/src/mcp_bridge/ask.rs` | a plan's rows in the dialog |
| `crates/aurora-viewer/src/ui/options/pages/local_assistant.rs` | receipts in Recent activity, Stop |

**Upstream hooks: none new.** Only the existing `fs-mcp-*` block of
`crates/aurora-i18n/locales/{en,fr,ja}.ftl` grows, and `docs/TESTING.md`
gets one line for a new demo value, `AURORA_DEMO_MCP=plan` (the demo makes a
3-entry move plan and executes it, so the dialog shows rows).

## Checkpoints (each one small PR)

| # | PR title | Contents | Needs |
|---|---|---|---|
| 1 | `fix(mcp): recheck permissions for waiting calls` | Section 1, with the login-change refusal of waiting questions (the login generation of section 2, without `viewer_session` yet) | #21 and #25 merged, or a rebase on them |
| 2 | `feat(mcp): bind calls to one login; capability versions` | Sections 2 and 3: `viewer_session`, `session_changed`, sidecar pinning and its no-retry rule, `capabilities` both ways, `unsupported` | 1 |
| 3 | `feat(mcp): receipts and client request ids` | Section 4: `operation_id` from the listener, the store, `client_request_id`, `assistant_operation_status`, the Wait answer with `state`, late answers, Recent activity | 2 |
| 4 | `feat(mcp): batch preview and execute plans` | Section 6: `folderstorm/bulk_plan.rs`, the three tools, the plan's dialog, `AURORA_DEMO_MCP=plan` | 3 |
| 5 | `feat(mcp): guarded operations: early answers and cancel` | Section 5: answer modes, `assistant_operation_cancel`, lanes and bounds, the guard for multi-step operations, Stop in Preferences; first user `inventory_execute_plan` with `wait: false` | 3 (4 for its first user) |

Each changes the viewer, so the human validates it; each runs the checks of
AGENT_RULES § 4 and updates [mcp.md](mcp.md) and FEATURES.md.

## As built

### Checkpoint 2: login binding and capability versions

Branch `claude/mcp-login-binding`, on top of checkpoint 1 (PR #28, branch
`claude/mcp-recheck-permissions`), which is not on `main` yet; #21, #25 and
#27 are not on its base either.

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/binding.rs` (new) | `Capabilities`, this build's map (`CAPABILITIES`), `assumed()` for a viewer without one, `supports`, `missing`; `new_viewer_session`; the per-bridge `Binding` (generation → `viewer_session`); the `session_changed` and `unsupported` errors |
| `folderstorm-mcp-bridge/src/protocol.rs` | the new wire fields; `ErrorCode::SessionChanged` (409) and `Unsupported` (501) with `fallback` / `for_sidecar`; `ToolSpec::group` and `ToolSpec::bound` |
| `folderstorm-mcp-bridge/src/server.rs` | `Bridge::set_login` / `viewer_session`; health's `capabilities` and `viewer_session`; a fast `session_changed` refusal; every error mapped for the call's sidecar; an unknown tool answers `unsupported` to a sidecar that declares capabilities |
| `folderstorm-mcp-bridge/src/client.rs` | `Client::call_in` (with a session); every call declares this build's capabilities; an error with an unknown code keeps its message |
| `folderstorm-mcp/src/connect.rs` | pinning, no resend, the rediscovery rule, the capability check (`unsupported` before sending) |
| `folderstorm-mcp/src/diagnose.rs` | `sidecar_capabilities`, `viewer_capabilities`, `missing_capabilities`, `login_bound` |
| `aurora-viewer/src/mcp_bridge.rs` | tells the bridge the login generation each frame and in `replan`, gives the desk its `viewer_session` |
| `aurora-viewer/src/mcp_bridge/desk.rs` | a bound call pinned to another login is refused when taken; a login change refuses the waiting calls with `session_changed` (checkpoint 1 said `permission_denied`) |
| `aurora-viewer/src/mcp_bridge/tools.rs` | `viewer_status` gives `viewer_session` and `capabilities` |

**Capabilities on this base**: `inventory: 1` (phases 1 and 2, every tool
today) and `login_binding: 1`. When they merge, #21 adds `camera: 1`, #25
`inventory_gaps: 1` and #27 `world: 1`: each sets its tools' `group` and
adds its row to `binding::CAPABILITIES` (the catalogue test fails until
every tool's group is in the map). Later: `receipts`, `plans`,
`operations`.

**Wire, as built** (`API_VERSION` stays 1; every new field is optional and
skipped when empty):

| Where | Field or code | Older other side |
|---|---|---|
| `Health` | `capabilities` (map), `viewer_session` (absent logged out) | an older sidecar ignores them; this sidecar reads a viewer without them as `{inventory: 1}`, unbound |
| `CallRequest` | `session`, `features` (the sidecar's map) | an older viewer ignores both |
| `ToolError` | `viewer_session` (with `session_changed`) | dropped when the code is mapped |
| `ErrorCode` | `session_changed` → `not_ready`, `unsupported` → `unknown_tool` for a sidecar that does not declare `login_binding` | an older sidecar gets today's codes |
| `viewer_status` | `viewer_session` (null logged out), `capabilities` | additive |

**Differences from the design above**:

- The epoch is the bridge instance: a `viewer_session` is drawn from the OS
  random source for each (bridge start, login generation), in the bridge
  crate rather than `login.rs`, because the listener needs it for health and
  for its fast refusal. `login.rs` keeps the generation. Logged in means
  `World::login` set, or the demo.
- `viewer_status` is not bound: it answers on whatever login is current,
  since it is how the assistant looks again after `session_changed`.
  Every tool with a class is bound.
- With nothing pinned (logged out at the first call), the sidecar asks
  health before each call until it sees a login, then pins it; the very
  first call works as before.
- After `session_changed` the sidecar pins the login named in the error;
  when the viewer is logged out it keeps the old pin, so that every bound
  call is refused until the next login, which is reported once more. A
  viewer found again after a lost connection gets the call only on the
  pinned login; otherwise `session_changed` (nothing sent). A viewer older
  than login binding is called unbound, as before (it cannot tell logins
  apart); one found again in place of a binding viewer is reported once.
- `session_changed` and `unsupported` are both tied to `login_binding` for
  the fallback: they came with it, as `features` did.
- **Not built: `grid` and `character` in `viewer_status`.** `viewer_status`
  needs no class (it works with Read on Never), while decision 1 gives names
  to the assistant only when Read is Allow; and the view has no clean source
  for the grid of the login in force (the settings' choice is the next
  login's). Left for `viewer_list` (section 8) or a Read-gated field: the
  human decides.

**Tests**: bridge crate: `viewer_session` form and freshness (new
generation, new bridge, logged out), capabilities both ways, `Health` and
`CallRequest` read by older and newer sides, the error-code fallback, health
fields over the socket, the fast refusal and the frame thread's `session`,
an older sidecar's mapped codes, `unsupported` for an unknown tool. Sidecar
(`tests/binding.rs`, real sockets): pinning and `session_changed` never
followed by a second send, a logout keeping the pin, no pin while logged
out, a viewer restart reported instead of retried, an older viewer (no
capabilities) called unbound, a viewer lacking a group refused with
`unsupported` before sending, a newer viewer used, every tool listed.
Viewer: the desk's refusal at take, `viewer_status`'s fields, checkpoint 1's
login-change tests with the new code, and end to end (sidecar → socket →
bridge → desk) a relog while the dialog is open: `session_changed`, nothing
sent, never resent, the next call runs.

### The studio follows the desk's rules (integration of #21 with #28 and #32)

Branch `claude/mcp-integrated` (every MCP PR merged, then `main`). #21's
`Studio` held camera calls after the desk had let them run: camera moves
waiting for the camera to stop, pictures waiting for it or for their photo
and encoding, the thumbnail waiting for the gallery's snapshot. Access off
dropped them, but neither a login change nor a class set to Never reached
them. Now:

| Where | What |
|---|---|
| `mcp_bridge/desk.rs` | `Desk::login` (the generation in force) and `Desk::review(tool, login)`: under another generation → the desk's own `session_changed` refusal; a class on Never → `PERMISSIONS_CHANGED`; else Run or Ask. `Desk::recheck` now uses it too |
| `mcp_bridge/studio.rs` | every wait keeps the generation it was queued under; `Studio::review(&desk)` applies `Desk::review` to each (section 1's table: what has not acted refuses or goes back to the dialog, what has acted — the camera moved, the photo taken — answers nothing more, a thumbnail sent waits for the server); `Studio::close` refuses as `Desk::close` does; `Studio::thumbnail_send` stops the gallery's snapshot of a refused thumbnail |
| `mcp_bridge.rs` | the studio reviewed each frame after the desk's levels and login; `photo_step` and `thumbnail_send` read the login in force and review just before acting; the calls that ask again go to `Desk::take` |
| `app/local_assistant.rs`, `app/inventory_thumbnail.rs` (one hook) | the check just before the gallery's snapshot is sent; a stopped snapshot ends as a failed one, with Appearance's note `fs-mcp-thumbnail-stopped` |

Tests (`mcp_bridge/camera_tests/recheck.rs`): a queued picture refused
after a relog (nothing captured) and when its photo is being drawn (its
pixels never become an image); a queued picture refused after Camera →
Never; Camera → Ask sends a picture not asked yet back to the dialog while
one already asked keeps its place; a move waiting for the camera to stop
refused after a logout and after Never, but not after Ask; a thumbnail
waiting for its picture refused after a relog and after Never (the
snapshot stopped), one sent left to the server; a thumbnail not sent going
back to the dialog; access off; and end to end over a real socket (sidecar
→ bridge → desk → studio) a relog between the frame and the photo:
`session_changed`, nothing captured, never resent, the next call runs.

### Checkpoints 3 to 5: receipts, plans, operations

Branch `claude/mcp-receipts` on `main` (checkpoints 1, 2 and the studio fix
merged): one commit for checkpoint 3, one for 4, and for 5 the work saved
before a restart and its completion. PR #45 (session routing) is not on its
base: nothing here touches the sidecar's routing or the host controller
beyond one line in `connect.rs` and the instructions and tool list of
`jsonrpc.rs`. Merging both will still meet text conflicts where each adds
to the same list: `binding::CAPABILITIES`, `ErrorCode`, the end of
`server/tests.rs`, `mcp_bridge.rs` and FEATURES 4.6; keep both sides.

**Checkpoint 3, receipts.**

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/receipts.rs` (new, with `receipts/tests.rs`) | `Store` (admit, duplicates and conflicts, retired ids, eviction, retention, row bound, finish from an answer, late settle, redaction in `Receipt::to_json`), `Ledger` (the frame thread's store, shared by the calls it took), `Tracked` (one call's receipt: its answer ends it), the id helpers |
| `protocol.rs`, `protocol/assistant.rs` (new) | `ErrorCode::Conflict`; `ToolError::operation_id` (also in `Display`); `ToolSpec::input_schema` (every change takes `client_request_id`); `ToolSpec::bound` also for the `receipts` and `operations` groups; `assistant_operation_status` |
| `server.rs` | an `operation_id` for each change when it arrives; `Pending::track`, `receipt`, `tracked`; `Pending::answer` ends the receipt and adds `operation_id` and `state`; the listener's own timeout names the operation; `Activity::operation_id`; every change waits the dialog's 60 s on top (a class may go from Allow to Ask while it waits) |
| `binding.rs` | `receipts: 1`; `check_arguments` (a `client_request_id` needs `receipts`) |
| viewer `mcp_bridge/desk.rs` | a receipt for every change taken (after the login check), `client_request_id` taken out of the arguments, duplicates and conflicts answered; states `waiting`, `asking`, `queued`, `running`; the user's decision noted; a change not confirmed in 30 s answers `unconfirmed` and keeps listening 10 minutes (16 at most) for the server, a created item or the Image window; `Outcome::Queued` for `moves_queued`; receipts kept at access off, cleared at a login change; `assistant_operation_status` |
| `mcp_bridge.rs`, `ui/options/pages/local_assistant.rs` | the studio's waits mark their receipts `running`; `Status::receipts`; Recent activity shows a change's line as its summary and state, the program in the tooltip |
| sidecar `connect.rs`, `jsonrpc.rs` | `check_arguments` before sending; `input_schema` in `tools/list`; the instructions' extra paragraph |

Choices and differences:

- **One exit point**: a call's answer ends its receipt (`Pending::answer`
  through `Tracked`), so no path of the desk, the studio or the app can
  forget one. The store is the pure type of the plan, in the bridge crate;
  the `Ledger` handle each taken call carries is only touched on the frame
  thread.
- A call pinned to another login and refused when taken gets **no
  receipt**: it belongs to no login this desk can show. Calls refused by
  the listener stay Activity lines only, as planned.
- Fields beyond the table: `error` (the code it ended with), `times_ms`
  (Unix milliseconds), `row_counts`; decision `refused_session_changed`.
  `server_reason` is given with names only (Read on Allow), since a server's
  message may name an entry.
- **Duplicates** answer `{operation_id, state, duplicate: true, receipt}`,
  whatever the first call's end; **conflict** is `409`, mapped to
  `invalid_arguments` for a sidecar without `receipts`.
- **Queued moves** (`Reply::moves_queued`): the answer now says
  `done: false, queued: true` (it said `done: true`), the receipt
  `unconfirmed` with server `queued`: never `done`.
- **The Wait answer**: the desk already answers a change 30 s after it was
  sent, well inside the listener's wait (10 s + the tool's own + 60 s for
  every change now), with `unconfirmed` and its `operation_id`; the
  listener's own timeout (a frame thread stalled) carries the
  `operation_id` too, and the receipt goes on.
- A camera move, a picture and a thumbnail are `running` from the moment
  the studio holds them (a queued picture included).

**Checkpoint 4, plans.**

| Where | What |
|---|---|
| viewer `folderstorm/bulk_plan.rs` (new) | `Kind`, `Row`, `apply_pattern`, `plan_rename`, `plan_rename_to` (explicit names, new), `plan_move`, `mutation`; `ui/inventory/bulk_review.rs` re-exports what it uses (its tests unchanged) |
| viewer `mcp_bridge/plans.rs` (new) | `Plans` (5 min, 8 per login, used once), the previews' parsing and rows with the bridge's rules, the frozen execute, `plan` (planned again, must equal), the dialog's lines |
| `desk.rs`, `changes.rs`, `ask.rs` | previews answered by the desk; an executed plan answers its operation; the execute takes its plan (receipt rows, targets, `plan_id`), carries it frozen in its arguments and decides by `Pending::plan_class`; `Question::detail` under the sentence |
| bridge `protocol/assistant.rs`, `server.rs` | the three tools; `ErrorCode::Stale` (`not_allowed` for a sidecar without `plans`); `any_decision` (the listener refuses an execute only when Edit and Move are both Never); `plans: 1` |

Choices and differences:

- The frozen plan travels in the call's own arguments, so `changes::plan`
  stays a pure function and the app's re-plan just before the send checks
  the rows again with no extra state.
- `stale` says so and that the plan is gone; it does not carry the new rows
  (the assistant previews again, which shows them).
- The execute's answer gives each row's id and outcome; the names stay in
  the preview and in the receipt (Read-gated).
- Bridge extras beyond the plan: a pattern for several entries must hold
  `{name}` or `{n}` (otherwise every row would get the same name); entries
  under the Marketplace Listings folder are left out with their reason; a
  destination in the Trash (`inventory_move`'s rule) or the Marketplace
  folder is refused.

**Checkpoint 5, operations.**

| Where | What |
|---|---|
| bridge `protocol.rs`, `protocol/assistant.rs`, `server.rs`, `receipts.rs` | `ErrorCode::Cancelled` (`permission_denied` for a sidecar without `operations`); `assistant_operation_cancel`; `answers_early` (`inventory_execute_plan` with `wait: false`); `Pending::early`, set by the listener only for a sidecar declaring `operations`, and `Pending::answer_early`; `LIVE_MAX` (8) and `Store::live`; `operations: 1` |
| viewer `desk.rs` | the early answer once the call is asking or queued; `Desk::cancel` (dialog closes, queued call removed, sent: not recalled); 8 live operations per login, 16 questions waiting; the deadline (the listener's whole wait) checked in `recheck`, just before the send |
| `mcp_bridge.rs`, `ui/options/pages/local_assistant.rs` | `mcp_bridge::cancel`; Recent activity lists first the changes that have no activity line yet (in the dialog, waiting for their turn, or asked by the viewer itself, as the demo's), a live one with a Stop button |

Choices and differences:

- `cancelled` is a new error code (not in the plan's list): a cancelled
  call that still waits gets it. Preferences' Stop says it was the user.
- **Multi-step operations**: none exists yet (a plan is one inventory
  change). The guard is `Desk::recheck`, which every send passes through
  (`local_assistant_run` → `replan`): login, levels, deadline, then the
  viewer's rules and a plan's rows. A later multi-step tool calls it before
  each step; `stopped` is the receipt state of a cancel after a step was
  sent.
- The studio's calls (camera, pictures, thumbnails) are not cancelled: once
  in the studio they have acted or wait for the photo; the cancel answers
  `cancelled: false` with their state.
- Lanes are the busy checks that already exist (the inventory window's
  pending change, Appearance's, the gallery's snapshot); no queue behind
  them, as planned.
- **The early answer** is `{operation_id, state, early: true, next}`; the
  call's end then only updates its receipt (nothing more reaches the
  listener). The listener records the early answer as the call's activity
  line, which Preferences shows with the receipt's state.
- **Bounds**: 8 live operations per login answer `busy` to a new change (a
  retry of one of them still answers its receipt). Since every question in
  the dialog is a live operation, the 16 questions waiting is a backstop
  (checked at compile time to be above 8).
- **Stop in Preferences**: an activity line exists only once a call
  answered, so a change in the dialog had no line to carry Stop. Recent
  activity therefore lists the receipts without a line first: the live ones
  (with Stop) and, newest first, up to five finished ones the viewer asked
  for itself; a receipt older than the oldest line kept had its line once
  and stays out.

**Tests.** Bridge crate: the store (`receipts/tests.rs`: id forms, text
cut, duplicates, conflicts, eviction order, live never evicted, busy,
retention, login change, other login not found, rows bound, every end from
an answer, late settle, redaction with Read on Never, result ids never other
values, a tracked answer's `operation_id`), the catalogue (status bound
without class, `client_request_id` in change schemas, previews as reads,
execute's class from the plan, cancel bound, early only for `wait: false`),
the listener (`operation_id` at arrival, in its timeout and in activity;
`conflict`, `stale`, `cancelled` mapped for older sidecars; execute refused
only when both classes are Never; early only for a sidecar declaring
`operations`). Sidecar (`tests/binding.rs`): a `client_request_id` to a
viewer without `receipts` answers `unsupported`, nothing sent. Viewer
(`change_tests/receipts.rs`, `plans.rs`, `operations.rs`): a receipt for
every end (refused in the dialog, approved and confirmed, refused by the
rules, timed out), one run for a reused id (also while the first waits),
conflict, status on this login only and names only with Read, no asset id
in any receipt (full-permission items, a picture from a texture, an answer
carrying an asset id), a late confirmation, queued moves, access off and a
login change; the preview's rows equal the native review's, the refusals,
execute's class, one dialog with five rows and "N more", stale before the
dialog and before the send, expiry, once, 8 plans, login change, the
server's three answers in the rows; early answers and polling, cancel while
asking, queued and sent, Stop, the bounds, a change whose time ran out
cancelled just before the send, an early call cancelled in the dialog
answering only once (its rows `not_sent`, its plan consumed); end to end over a real socket: a
retry with the same id, a previewed plan executed after the dialog, a plan
with `wait: false` polled to `done`.

## Tests

| Checkpoint | Tests |
|---|---|
| 1 | Ask → Never with the dialog open, and queued behind another: refused, dialog gone, nothing sent; Never between the Allow click and the send; a call admitted under Allow and taken after Never; Allow → Ask before the send: asks; Ask → Allow: still asks, never runs alone; compound classes and the always-asking picture (#21); access off: refused with the new message; login change: waiting questions refused; a click after the refusal, and two clicks: no effect |
| 2 | Login generation (out → in, in → out, another agent, another grid, demo); `viewer_session` form and freshness (new epoch, new generation); `Health` read both ways by older and newer sides; pinning; `session_changed` never re-sends; a viewer restart between two calls reports instead of retrying; `unsupported` before sending; an older viewer without capabilities |
| 3 | Store bounds (count, rows, text cut on a character boundary, control characters), eviction (finished only, oldest first, never live), cleared at a login change; `client_request_id` (same id and arguments: one send; other arguments: `conflict`; evicted id: `conflict`; two at once: one send); redaction with Read on Never; no asset id in any receipt (a full-permission item in the fixture); `operation_id` in the listener's timeout error; a late server answer updating an `unconfirmed` receipt; the Wait answer before the listener's deadline |
| 4 | Rows identical to the native review's for the same request (shared planner); preview needs Read; execute's class from the plan; Never, Ask (one dialog, 5 rows, "N more"), Allow; `stale` after a rename, move, Trash or permission change in between; expiry; used once (a second execute answers the same operation); a session change ends plans; 51 rows and duplicate ids refused; outcomes with `demo_mutate` (done, failed, queued); end to end from the sidecar on the demo inventory |
| 5 | Early answers only for a sidecar that declares `operations`; polling through `asking`, `queued`, `running`, `done`; cancel while asking (the dialog closes), queued and running; guards failing before a second step; deadlines; lane busy; the live limit |

Not testable here: the dialog's and Preferences' looks (no display in the
sandbox), real MCP clients, and the grid's answers (late confirmations,
grids without AIS).

## Risks

| Risk | Mitigation |
|---|---|
| `desk.rs` grows into a tangle | The store, ids and bounds live in pure bridge-crate types; the desk only moves states. One checkpoint per piece |
| Conflicts with #21 and #25 (`protocol.rs`, `desk.rs`, `mcp_bridge.rs`) | Start after both merge; new tools in their own protocol file, as #25 does |
| "Confirmed" claimed when the server only queued (grids without AIS) | A separate `queued` server state; "sent" and "queued" are never "done" |
| The frame thread stalls and the listener times out first | The `operation_id` is made by the listener, so its timeout error still points at the receipt |
| Users puzzled by `session_changed` after turning access off and on | The message says why; the next call works |
| Receipts keep inventory names in memory | Memory only, never logged or saved, cleared at logout, names to the assistant only with Read (open question 1) |
| A newer sidecar's fields silently ignored by an older viewer | Tools already refuse unknown arguments; envelope features are used only when the viewer advertises them |
| Assistants retrying uncertain writes anyway | Tool descriptions and errors say "look first, use `assistant_operation_status`"; `client_request_id` makes an honest retry harmless |

## Decisions (the human, 2026-11-11)

1. **Names in receipts:** a receipt keeps the Ask dialog's sentence and, for
   plans, the before and after names, in memory only, cleared at logout,
   shown in Preferences, given to the assistant only when Read is Allow.
   **Yes.**
2. **Disk:** receipts stay in memory only; no per-login file. **Yes.**
3. **`client_request_id`:** optional for today's inventory, outfit and
   camera tools; **required** for later tools whose effect cannot be looked
   at afterwards (teleport, chat send, dialog replies, uploads, script
   compile). **Yes.**
4. **`tools/list`:** the sidecar lists every tool it knows; a tool the
   running viewer lacks is refused per call with `unsupported` and a message
   telling the user to update. Permission refusals (Never) stay separate
   (`permission_denied`), and those tools stay listed too. **Yes (list all).**
5. **Plans under Allow:** a plan whose class is on Allow runs without a
   dialog, like any change; no "always review 10 entries or more" setting
   for now (it can come with FEATURES 2.5). **Yes.**

Related decision (2026-11-11, PR #27): the world observation tools get their
own **Surroundings** permission class (Never in "Read only", Ask in "Ask
before changes"), separate from Read, after PR #21 merges.
