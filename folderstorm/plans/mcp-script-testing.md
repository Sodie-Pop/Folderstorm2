# Local assistant (MCP): script testing

Group 12 of the MCP parity checklist (FEATURES.md 4.6): the assistant
exercises **scripts the user owns, in test prims the user owns**: it
touches a prim, reads and answers the script dialogs that prim opens,
says a line on a script channel, and runs a frozen test recipe with one
approval. Built on [mcp-objects.md](mcp-objects.md) (which prims, RLVa,
`client_request_id`, receipts) and
[mcp-task-scripts.md](mcp-task-scripts.md) (the Scripts class, the
running state, `script_debug_poll`'s feed).

Sources, read as information: Folderstorm (C++)'s
`doc/plans/mcp-script-testing.md` ("PR D Interactions and repeatable test
sessions", "Gameplay test checkpoint") and
`tools/fs-mcp/internal/mcptools/objects_testing.go` (names, arguments,
bounds); the Legacy Viewer's touch (LLToolGrab: ObjectGrab then
ObjectDeGrab, `NetCommand::Touch`), script dialogs
(LLScriptFloaterManager, `process_script_dialog`,
`callback_script_dialog`: `world/notifications.rs` `Data::Dialog`,
`Data::TextBox`, `World::respond_notification`) and channel chat
(`send_chat_from_viewer`, a negative channel as a ScriptDialogReply:
`NetCommand::Chat`).

## Tools (capability `script_testing: 1`)

| Tool | Class | Asks | What it does |
|---|---|---|---|
| `object_touch` | Interact | **always** | One touch (grab, release) of an owned prim within 64 m, as a click. `object_id`, `client_request_id` (required). Answers `sent`; a touch is never acknowledged: `handled_confirmed: false`. |
| `object_dialog_list` | Read | – | The live llDialog / llTextBox windows a prim the user owns opened and that wait in the viewer: `dialog_id`, `kind`, `channel`, `message`, `buttons` (index, label), `answerable` and why not. Text from the grid: data. |
| `object_dialog_reply` | Interact | level | Answer one listed dialog: `dialog_id`, then `button_index` (llDialog) or `text` (llTextBox, at most 250 bytes, may be empty); `client_request_id` (required). Answered as the user's click on that button: the dialog closes. |
| `object_channel_send` | Interact | **always** | Say `text` (1–1023 bytes) on script channel `channel` (not 0, not DEBUG_CHANNEL) near a prim the user owns (`object_id`, within 20 m). At most 1 a second and 6 a minute per login. `client_request_id` (required). |
| `script_test_begin` | Scripts (+ Read, + Interact with a touch or a channel step) | **always** | Start a frozen recipe: `object_ids` (1–32 owned prims), `steps` (1–64), `timeout_seconds` (1–600), `fail_on_runtime_errors` (default true), `client_request_id` (required). One Ask for the whole recipe; its steps then run without another dialog. Answers `run_id` at once. |
| `script_test_status` | Read | – | The run's state, its steps, their outcomes and what matched. Polling never advances it. |
| `script_test_stop` | none | – | Cancel the run's unsent steps. Needs no class: works after the permissions are revoked. Never stops or resets a script, never deletes anything. |

Same names as Folderstorm (C++). Differences: one capability instead of
`object_interaction` and `test_sessions`; a touch has no face, UV or
local point (left for later); a dialog is named by one `dialog_id` (a
salted hash of the viewer's notification and its object, channel, text
and buttons: a replaced dialog has another id) instead of an id and a
revision; no `assert_contents` step (left for later); a `wait_dialog`
step instead.

## Permission classes and asking (decision)

- **New class Interact**, saved as `interact`, Preferences row "Touch and
  talk to my objects": touch, answer the dialogs of, and speak on script
  channels near objects the user owns. Ask in "Ask before changes",
  Never in "Read only"; settings saved before it follow "Ask before
  changes" when every other change asks (Scripts included), else Never.
  Kept apart from Scripts (as Folderstorm (C++)'s `object_interact` apart
  from `script_run`): an interaction makes the object's scripts react as
  they would to the user, and a channel line reaches every listener
  nearby, someone else's included.
- **Always ask**: `object_touch` (the checklist's "asks": the user sees
  each touch), `object_channel_send` (a broadcast: the dialog says that
  every script listening on that channel within 20 m hears it, not only
  the user's object), `script_test_begin` (the dialog lists the frozen
  recipe; that one approval covers all its steps, as Folderstorm (C++)
  decided). `object_dialog_reply` follows the Interact level: it answers
  a window the user's own object opened on the user's screen, with a
  button that script offered.
- **A test needs Scripts** ("Run my objects' scripts": a test runs them),
  Read (its assertions read the script output feed and the running
  state) and Interact when it touches or speaks. A step checks its
  classes again just before it is sent: any of them on Never stops the
  run, its unsent steps cancelled.
- **Stop needs no class** and stays bound to the login that started the
  run.

## L$ and other prompts (⚠)

`object_dialog_reply` answers only `Data::Dialog` and `Data::TextBox`
entries of the viewer's notifications: never a permission question
(`Data::Permissions`, which is where a debit permission is asked), a
payment, a teleport offer, an inventory offer, a URL or any other
notification. On top of that it refuses:

- a dialog whose text or chosen button mentions L$ ("L$", "Linden
  dollar"): an owned object that already holds a debit permission could
  pay out on such a button; the user answers it;
- a dialog that answers on channel 0 (its reply would be said in public
  chat) or on DEBUG_CHANNEL;
- a dialog of an object the user does not own, or one the object tools'
  rules refuse.

No tool of this group sends money, accepts anything or grants a
permission. The listing marks such dialogs `answerable: false` with why.

## Which prims

The object tools' rules (`Objects::prim`): a loaded volume prim in the
agent's region, not worn, root owned by the agent (the server's owner:
never group-deeded) and modifiable; RLVa `@edit`, `@editobj`,
`@editworld`, `@interact`, `@fartouch`. Then:

- touch: within 64 m of the avatar, RLVa `World::rlva_can_touch`
  (`@touchall`, `@touchworld`, `@touchthis`, `@touchfar`) when planned,
  and the send boundary (`rlva_allows_command` on `NetCommand::Touch`)
  just before it leaves;
- channel: the prim within 20 m (said chat's range), RLVa
  `@sendchannel` / `@sendchannel_except` (`Rlva::allows_channel`) when
  planned and again at the send, which `Rlva::outgoing_chat` filters too;
- dialog: the dialog's prim passes the rules; `rlva_allows_command` on
  each reply the viewer's own answer makes;
- recipe: every prim of `object_ids` at the begin and the step's prim
  again at its send; a `reset` or `running` step names a script of a
  contents snapshot of its prim (`snapshot_id`, `item_id`: 120 s at the
  begin, the prim unchanged since at the send) with the script's Modify
  for the agent, as `object_script_reset`.

## Creators' permissions

`AGENTS.md` § 2: no asset is shown, copied or exported by this group.
Start, stop and reset steps need the script's Modify for the agent
(`actions::task_agent_mask`, through `objects::rights`), as
`object_script_set_running`. Dialog and output text is the grid's: the
answers say it is data; it is never written to a receipt or a log (the
receipts keep summaries with names through the RLVa filter only).

## Test recipes

Step actions (each names an `object_id` of the scope, `delay` excepted):

| Action | Arguments | Meaning |
|---|---|---|
| `touch` | – | a touch, as `object_touch` |
| `send_channel` | `channel`, `text` | a line, as `object_channel_send` (waits for the rate limit) |
| `reset` | `snapshot_id`, `item_id` | `ScriptReset` |
| `running` | `snapshot_id`, `item_id`, `running` | `SetScriptRunning` |
| `wait_output` | `text`, `match` (`contains` / `equals`), `expected`, `timeout_seconds` (1–120) | expected true: an owner-say or error line of that prim matching the text appears; false: none does within the time |
| `wait_dialog` | `text` (optional, contained in the message), `expected`, `timeout_seconds` | a dialog of that prim opens (or none does) |
| `assert_running` | `snapshot_id`, `item_id`, `expected`, `timeout_seconds` | the simulator reports that running state |
| `delay` | `seconds` (1–60) | waits |

- **Frozen**: the begin checks everything and freezes the recipe into its
  arguments (prims, handles, steps, the dialog's lines); the Ask dialog
  lists the steps (12 lines, then "and N more"); the plan at the send
  checks the prims and classes again.
- **One run per login**, 600 s at most; runs are kept for status (8, 30
  minutes); a login change ends them, access off stops them.
- **Windows**: each action takes a mark of the output feed and of the
  notifications just before it is sent; a positive `wait_output` reads
  from the last action's mark and consumes the line it matched (a second
  wait reads after it); an absence covers the last action's window until
  its time ends. A `wait_dialog` counts dialogs opened after that mark.
- **Outcomes**: `passed` (every assertion passed), `failed` (one
  failed, or a script error of a scope prim while `fail_on_runtime_errors`
  is true), `inconclusive` (lines lost from the feed, the feed restarted,
  no running reply, a step refused at its send, the run's time over),
  `stopped` (Stop, access off, a class on Never), `completed_unverified`
  (no assertion). Unsent steps of a run that ends are `cancelled`.
- An action step is `sent` when it leaves: a touch or a line is never
  acknowledged, a reset neither; the assertions tell what happened.

## RLVa

As "Which prims"; `@sendchat` keeps channel-0 dialogs from the viewer at
all; `@recvchat` does not hide owner-say lines (the feed captures after
RLVa, as the Script debug routing).

## Where the code goes

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/protocol/script_testing.rs` (new) | the catalogue, bounds, `needs_client_request_id`, `always_asks`, `extra_classes` |
| `policy.rs` | `Class::Interact`, presets and migration |
| `binding.rs`, `protocol.rs`, `protocol/assistant.rs` | `script_testing: 1`; `pub mod`, `all()`, required id, always asks; the group bound to its login |
| sidecar `jsonrpc.rs` | the instructions' paragraph |
| viewer `mcp_bridge/script_testing.rs` (new) | checks, freeze and plan, dialogs, rate limit, the runs, the demo |
| `mcp_bridge/script_feed.rs` | a mark and the lines since a mark, for the runs |
| `mcp_bridge/desk.rs`, `changes.rs`, `mcp_bridge.rs` | hook lines: classes, reads, `Work::Interact`, `Work::ScriptTest`, observe, the app's entry points, the demo |
| `app/local_assistant.rs` | the sends (touch, channel line, dialog answer, running, reset, query) with their RLVa checks; the demo box's answers |
| Preferences page, catalogues | the Interact row, the Ask sentences and step lines, en/fr/ja |

No upstream file changes beyond the Preferences row (already a
Folderstorm list).

## Demo

`AURORA_DEMO_MCP=script-testing`: Ask before changes; the demo lists the
plaza's cut box, touches it (asked), the box answers with an owner-say
line and a dialog, the demo lists the dialog and answers "Open" (asked),
then begins a recipe (asked once, the dialog lists its steps): touch,
wait for the line, wait for a dialog, send on channel 7, reset the door
script, check it runs; then reads the status until the run ends.

## Tests

Catalogue (classes, required ids, always asks, bounds); policy (Interact
presets and migration); prims (someone else's, too far, RLVa
`@touchall`, `@sendchannel`); dialogs (owned only, L$ refused, channel
0 refused, a replaced dialog stale, text box bounds, permission
questions never listed); rate limit; recipe validation (scope, fields,
bounds) and freeze; runs: windows and consumption, absence, runtime
error fails, gap inconclusive, assert_running, a class on Never stops,
Stop with permissions revoked, deadline; end to end over a real socket
on the demo.

Not testable here: the grid's touch handling, real dialogs, channel
listeners, RLVa on a grid.

## Left for later

- A touch on a face or point (`face`, `uv`, `local_point`).
- `assert_contents`, and rez or take steps with provenance.
- A receive feed for script channels (never planned: owner-say only).

## As built

Branch `claude/mcp-script-testing`, on `claude/mcp-task-scripts`.

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/protocol/script_testing.rs` (new) | the seven tools, bounds, `needs_client_request_id`, `always_asks`, `extra_classes` |
| `policy.rs` | `Class::Interact` (`interact`), presets and migration |
| `binding.rs`, `protocol.rs`, `protocol/assistant.rs`, `protocol/inventory.rs` | `script_testing: 1`; `pub mod`, `all()`, the required id, always asks; the group bound to its login; the catalogue count |
| sidecar `jsonrpc.rs` | `SCRIPT_TESTING_HINT`; `script_test_stop` not read-only |
| viewer `mcp_bridge/script_testing.rs` (new) | prims, dialogs (fingerprint, `dialog_id`, L$ and channel refusals), rate limit, freeze and plan, the runs, the demo |
| `mcp_bridge/script_feed.rs` | `mark` and `since`, for the runs |
| `mcp_bridge/desk.rs`, `changes.rs`, `mcp_bridge.rs` | `Work::Interact`, `Work::ScriptTest`; reads, freeze, plan, extra classes, required id; tick, clear, close; the running replies; the app's entry points; the demo |
| `app/local_assistant.rs` | the sends with their RLVa checks (`Touch`, `Chat`, the notification's own answer, `SetScriptRunning`, `ScriptReset`, `GetScriptRunning`); the demo box's answers |
| Preferences page, catalogues | the Interact row; the Ask sentences and step lines, en/fr/ja |

Differences from the design above:

- A run's action step is `sent` once the app sent it; the runs' sends go
  through the app each frame (`script_test_sends`), and a step refused at
  the send ends the run `inconclusive`.
- A run that ends cancels its steps not sent yet; a delay or an action
  under way counts as cancelled, an assertion under way as inconclusive.
- `wait_output` reads error lines too (a recipe that allows errors can
  wait for one).
- The demo adds its channel step only when the avatar stands within 20 m
  of the box (the demo's start is 26 m away).
