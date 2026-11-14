# Local assistant (MCP): scripts inside objects

Group 13 of the MCP parity checklist (FEATURES.md 4.6): the assistant reads
and edits a **script inside a prim the user owns and may modify**, saves
and compiles it in place, reads and changes whether it runs, resets it,
and reads the runtime errors and owner-say output of the user's own
objects. Built on [mcp-objects.md](mcp-objects.md) (which prims, contents
snapshots, RLVa, receipts) and [mcp-content.md](mcp-content.md) (snapshot
hash, preview, frozen save, compile errors).

Sources, read as information: Folderstorm (C++)'s
`doc/plans/mcp-script-testing.md` (PR A diagnostics, PR B installed
scripts) and `tools/fs-mcp/internal/mcptools/objects.go` (names,
arguments, classes); the Legacy Viewer's LLLiveLSLEditor as Aurora ports
it (`app/scripts.rs`, `ui/scripts.rs`, `aurora-net` `scripts`,
`scripts/experiences.rs`, `BuildCmd::{GetScriptRunning, SetScriptRunning,
ScriptReset}`) and LLFloaterScriptDebug's routing
(`world/script_debug.rs`).

## Tools (capability `task_scripts: 1`)

| Tool | Class | Arguments | What it does |
|---|---|---|---|
| `object_script_get` | Read | `object_id`, `snapshot_id` (object_contents_list, 120 s), `item_id` | The script's whole source, as the object's script editor loads it (`LoadScript` with the prim as target), and its Experience association (the editor's GetMetadata request). Answers `text`, `bytes`, `lines`, `complete`, `editable` (or `not_editable` and why), `snapshot_hash` (complete and editable only), `preprocessed`, `experience` (`none`, `kept` or `unknown`). Never an asset id. |
| `object_script_preview` | Read | `object_id`, `item_id`, `snapshot_hash`, `text`, `compiler` | What a save would change: counts, a unified diff (16 KiB), the problems a save would refuse, and whether it would run now, ask or be refused. |
| `object_script_save_compile` | Content + Objects (+ Read; + Scripts with `running: true`) | `object_id`, `item_id`, `snapshot_hash`, `text`, `compiler` (`mono`), `running` (default false), `client_request_id` (required) | Saves the source in the task item and compiles it for Mono (`UpdateScriptTask`), keeping the script's Experience association; stopped unless `running: true`. Answers `source_saved` and `compiled` apart, the compile errors, the new `snapshot_hash`, `requested_running`. |
| `object_script_status` | Read | `object_id`, `snapshot_id`, `item_id` | Asks the simulator whether the script runs (`GetScriptRunning`) and answers `running`, `mono` from its reply. |
| `object_script_set_running` | Scripts | `object_id`, `snapshot_id`, `item_id`, `running`, `client_request_id` (required) | Starts or stops the script (`SetScriptRunning`), then asks its state: `done` when a reply shows the state asked for, else `unconfirmed` with the state seen. |
| `object_script_reset` | Scripts | `object_id`, `snapshot_id`, `item_id`, `client_request_id` (required) | Resets the script (`ScriptReset`), then asks its state. The grid never acknowledges a reset: the answer says `reset_confirmed: false` with the state that the next reply showed; runtime output tells the rest. |
| `script_debug_poll` | Read | `start` (`now` or `retained`) or `cursor`; `object_id`, `limit` (1–100, 50) | Script errors and owner-say lines of the user's own objects received since access was turned on, from a cursor: `events` (sequence, time, kind, object id and name, text), `next_cursor`, `gap` and `dropped` when lines were lost. |

Same names as Folderstorm (C++). Differences: no `viewer_session` or
`expected_asset_id` argument (the sidecar pins the login; an asset id is
never given out, the snapshot hash names the read); `running` is a boolean
for the save instead of `running_mode` (`stopped` / `preserve` with a
running snapshot): the assistant says what it wants, and running after a
save needs the Scripts class. A task operation's receipt is read with the
existing `object_operation_status`, which also gives the facts of these
operations (source saved, compiled, running asked and seen).

## Permission classes (decision)

- **New class Scripts**, saved as `scripts`, Preferences row "Run my
  objects' scripts": start, stop and reset scripts in objects the user owns
  and may modify, and let a save leave a script running. Ask in "Ask before
  changes", Never in "Read only". Folderstorm (C++) kept `script_run`
  apart from writing contents too: running a script acts in the world
  (it may give, say, move or ask for permissions), which editing it does
  not.
- **No tool of this group always asks.** On Allow a test loop (edit,
  compile, reset, read the output) runs without a dialog per step, as
  Folderstorm (C++) decided ("no mandatory prompt for every reset when the
  user has selected Allow"). Ask and Never work as for every class.
- **Save and compile** needs Content ("Edit notecards and scripts": it
  changes what the script does) **and** Objects ("My objects' contents":
  it changes what the object holds), as Folderstorm (C++)'s `content_edit`
  plus `task_inventory_write`. The desk adds Read (the source was read
  under Read: Read on Never refuses the save, as for inventory scripts)
  and, when `running` is true, Scripts. The listener's fast refusal knows
  only Content and Objects; the desk decides with all of them, when it
  takes the call and again just before the upload.
- The reads (get, preview, status, debug poll) need Read, as the brief
  decided; they only ever concern the user's own prims and objects.
- Settings saved before Scripts follow "Ask before changes" when every
  other change asks (objects and take included), else Never: the camera's
  rule.

## Which scripts

Every tool locates the prim with the object tools' rules
(`objects::target`): loaded volume prim in the agent's region, not worn,
root owned by the agent (`OBJECT_YOU_OWNER` and the server's owner: never
group-deeded) and modifiable; RLVa `@edit`, `@editobj`, `@editworld`,
`@interact`, `@fartouch`. The task item comes from a contents snapshot of
that prim (`object_contents_list`), must be a script (asset type 10), and
the snapshot is checked as the object tools check it (120 s when the call
is taken, the prim unchanged since).

## Creators' permissions

`AGENTS.md` § 2, through the shared helpers, never a god level:

| | Rule |
|---|---|
| Read the source (get, preview) | the task item's **Copy and Modify** for the agent (`actions::task_agent_mask`), as LLLiveLSLEditor::loadAsset (`app/scripts.rs` `task_permissions` without the god case) |
| Save and compile | the same, and **LSL only** (an SLua source keeps its `luau` target, not Mono: refused), not open in the viewer's script editor (busy: no unsaved work overwritten), Experience association known |
| Start, stop, reset | the script's **Modify** for the agent; stricter than Tools › Scripts (which needs the object's Modify only) |
| Status | the prim's rules and a script of its snapshot |
| Debug poll | lines from objects the server says the agent owns (`owner_id`), after the viewer's mutes, RLVa and filters |

- **No asset id** leaves: the snapshot hash is a SHA-256 of a per-login
  salt, the prim, the item, the asset and the text.
- **Text is data**: the source is the creator's content, the debug lines
  and names are the grid's text; the answers and the tool descriptions say
  so. Neither is written to a receipt or a log.
- **Experience** (decision): the association the editor reads
  (`LoadScriptExperiences`, the GetMetadata capability) is fetched with the
  source and sent back unchanged with the save (`experience` in
  `UpdateScriptTask`'s body). If it cannot be read (no capability: some
  OpenSim grids; an error), the script is readable but **not editable**: a
  save would clear the association. The assistant never sets or clears an
  association.

## Snapshots and stale checks

- A source snapshot: prim, item, asset, Experience, text, made; 8 per
  login, 30 minutes, gone at a login change and when access is turned off.
- **Stale** (nothing saved, "read it again"): unknown hash; a contents
  listing of the prim received after the snapshot shows the item gone or
  with another asset; the assistant saved that script since (its earlier
  snapshots end at the upload).
- **The upload's answer is the truth** (the lesson of PR #54): after a
  save, the new snapshot's asset is the upload's `new_asset`, never a
  re-read. A listing that still shows the asset from before the save is a
  lagging listing and is ignored; only a third asset makes the snapshot
  stale. The viewer lists the prim again after the save and asks the
  script's state.
- The server offers no compare-and-swap: another client saving at the same
  moment is not detected (the answer and the description say so).

## Outcomes

| Tool | Confirmed | Otherwise |
|---|---|---|
| save | the upload's answer: `source_saved: true`, `compiled` true or false with `errors` (64, 8 KiB; lines and columns from 1) | an error the editor's upload raises after the text left (`script-save-unconfirmed`, `script-timeout`) or no answer in 30 s: `timeout` (receipt `unconfirmed`, `source_saved: null`), never sent again; a late answer updates the facts and the receipt. Any other error: `failed`, `source_saved: false` |
| set running | a `ScriptRunning` reply after the send with the state asked for | the reply shows the other state, or none in 30 s: `unconfirmed` with `observed_running` |
| reset | a `ScriptRunning` reply after the send (`reset_confirmed: false`: the grid does not say the reset ran) | no reply in 30 s: `unconfirmed` |
| status | a reply within 20 s | `timeout` |

The state replies carry no request id: any reply for that prim and item
after the send counts, as the script editor reads them.

## Script output feed (`script_debug_poll`)

- **Where it is captured** (one upstream hook): `World::on_chat`, right
  before the Script debug routing, so after the mutes, the LSL bridge,
  RLVa's receive rules and the chat filters, and whatever the user's
  "Show script errors" and owner-say routing say: the feed sees a line
  whether it goes to the nearby chat or the Script debug window, and the
  window may stay closed. Kept: object messages of type Debug (errors) and
  OwnerSay whose `owner_id` is the agent.
- **When**: only while local assistant access is on and Read is Allow. The
  feed is cleared when access is turned off, Read goes to Never, or the
  login changes; a cursor of an earlier feed answers `stale`.
- **Bounds**: 1,024 lines, 1 MiB, 1,024 bytes of text per line, 100 lines
  or 64 KiB per answer. Lost lines are counted: a cursor older than the
  oldest line kept answers `gap: true` and `dropped`.
- No script name or line number: the grid does not say which script spoke.

## RLVa (the viewer decides)

The object tools' rules for the prim, plus: `@viewscript` refuses get,
preview and save (the script editor does not open); the send boundary
(`World::rlva_allows_command`) checks `LoadScript`, `SaveScript`,
`SetScriptRunning` and `ScriptReset` just before they leave.

## Where the code goes

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/protocol/task_scripts.rs` (new) | the catalogue, limits, `needs_client_request_id`, `extra_classes` |
| `policy.rs` | `Class::Scripts`, its level, presets and migration |
| `binding.rs`, `protocol.rs` | `task_scripts: 1`; `pub mod`, `all()`, the required id |
| sidecar `jsonrpc.rs` | the instructions' paragraph |
| viewer `mcp_bridge/task_scripts.rs` (new) | reads, snapshots, freeze and plan, sends, outcomes, facts, the demo |
| viewer `mcp_bridge/script_feed.rs` (new) | the feed and `script_debug_poll` |
| `mcp_bridge/objects.rs` | a few accessors (the prim and its snapshot item), the status filter |
| `mcp_bridge/desk.rs`, `changes.rs`, `mcp_bridge.rs` | hook lines: classes, the reads, `Work::TaskScript`, observe, wants, the demo |
| `app/local_assistant.rs` | fetches and sends, the open task editors, the saved asset in the Contents tab |
| `world/mod.rs` (upstream) | **one hook**: `mcp_bridge::script_feed::capture` before the Script debug routing |
| Preferences page, catalogues | the Scripts row, the Ask sentences, en/fr/ja |

## Demo

`AURORA_DEMO_MCP=task-scripts`: Ask before changes; the demo lists the
plaza's cut box, reads its door script, asks to save an edit (the dialog
shows the script, the object, the counts and the changed lines), then asks
to reset it.

## Tests

Catalogue (classes, required ids, bounds); policy (Scripts presets and
migration); permissions matrix (own / group / someone else's prim, script
without Copy or Modify, SLua, not a script, attachments); RLVa
(`@viewscript`, `@edit`); stale (unknown hash, listing with another asset,
lagging listing ignored, own save ends the old snapshot); Experience kept,
unknown refuses; save outcomes (saved and compiled, saved with errors,
failed, unconfirmed then late); running and reset outcomes; extra classes
(Read Never, Scripts Never or Ask with `running: true`); the feed (owned
only, cursor, gap, clear at login change and access off, bounds); no asset
id in any answer; end to end over a real socket on the demo.

Not testable here: the grid's real replies (SL and OpenSim), Experience
metadata on a grid, the dialog's look.

## Left for later

- `running_mode: preserve` (keep the running state seen by a fresh status)
  if assistants need it; `running: true` covers it explicitly today.
- Script name and line in the feed (the grid does not give them).
- Test sessions, touch and dialog replies (Folderstorm (C++)'s PR D) are
  other groups.
