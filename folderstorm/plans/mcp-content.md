# Local assistant (MCP): content editing

Step 7 of the MCP parity order ("Content editing" in the parity checklist):
read the full text of a notecard or a script of the agent's own inventory,
preview an edit, save a notecard's text, and save and compile a script.
It extends [mcp.md](mcp.md) and builds on
[mcp-receipts.md](mcp-receipts.md): every save is a guarded operation with
a receipt, an `operation_id` and a **required** `client_request_id`
(decision 3 of mcp-receipts.md: script compiles and uploads cannot be
looked at afterwards).

**Stacking.** Built on `claude/mcp-receipts` (PR #47, not merged), on
branch `claude/mcp-content`. PR #49 (navigation) is stacked on #47 too; it
is not touched here. Both add to the same lists (`binding::CAPABILITIES`,
`protocol::all()`, `mcp_bridge.rs` module lines, the `fs-mcp-*` block of
the catalogues, FEATURES 4.6, the sidecar's instructions): keep both sides
at the merge.

Sources, read as information: Folderstorm (C++)'s
`doc/plans/mcp-content-editing.md` and
`tools/fs-mcp/internal/mcptools/content.go`; the Legacy Viewer's
LLPreviewNotecard (`loadAsset`, `saveIfNeeded`) and LLPreviewLSL
(`loadAsset`, `saveIfNeeded`, the compile callbacks), as Aurora already
ports them in `app/inventory.rs`, `ui/inventory/document.rs`,
`app/scripts.rs` and `aurora-net` (`inventory::notecard`, `scripts`).

## Tools

| Tool | Class | Arguments | Returns |
|---|---|---|---|
| `inventory_content_get` | Read | `item_id` (a link is followed) | `item_id`, `name`, `kind` (`notecard` / `script`), `text`, `bytes`, `lines`, `complete`, `editable` (and `not_editable` with why), `snapshot_hash` (only when complete and editable), `embedded` (notecards: each embedded item's marker index, name and type), `preprocessed` (scripts: the source holds the Legacy Viewer's preprocessor block) |
| `inventory_content_preview` | Read | `item_id`, `snapshot_hash`, `text` | `lines_added`, `lines_removed`, `hunks`, `diff` (unified, 16 KiB at most, `diff_truncated`), `unchanged`, `problems` (what a save would refuse), `save` (`allow` / `ask` / `never`: the Content level now), `editor_open` |
| `inventory_update_notecard` | Content (+ Read) | `item_id`, `snapshot_hash`, `text`, `client_request_id` (required) | `item_id`, `source_saved: true`, `lines_added`, `lines_removed`, `embedded_kept`, `snapshot_hash` of the saved text (when it has no embedded item), `operation_id`, `state` |
| `inventory_save_compile_script` | Content (+ Read) | `item_id`, `snapshot_hash`, `text`, `compiler` (`mono`, the default and only value), `client_request_id` (required) | `item_id`, `source_saved`, `compiled`, `compile_checked` (false: the grid did not compile it, as OpenSim does for inventory scripts), `errors` (`line`, `column`, `message`; 64 at most, 8 KiB), `snapshot_hash`, `operation_id`, `state` |
| `inventory_content_operation_status` | Read | `operation_id` or `client_request_id` | the content outcome of a save of this login: `source_saved` (true / false / null for unknown), `compiled`, `compile_checked`, `errors`, `snapshot_hash`, with its receipt's `state` |

Same names as Folderstorm (C++), so that assistants and docs carry over.
Differences: no `expected_asset_id` (Folderstorm2 never gives an asset id
for an item without full permissions, see "Creators' permissions"), so the
snapshot is named by `snapshot_hash` alone; no `viewer_pid` (the sidecar's
`--viewer-pid` picks the viewer).

## Permission class: Content (new)

Saving a notecard or a script changes what an object does or what a
notecard tells people; renaming does not. Folderstorm (C++) made it a
separate `content_edit` class; Folderstorm2 does the same:
`Class::Content`, saved as `content`, Allow / Ask / Never, row "Edit
notecards and scripts" under Custom.

- **Read only**: Never. **Ask before changes**: Ask. Settings saved before
  this class follow the Camera rule: Ask when every other change asks,
  else Never.
- The two saves need Content **and** Read (`ToolSpec::also`), as in
  Folderstorm (C++): Read on Never refuses them (the text was read under
  Read). The reads (`get`, `preview`, `operation_status`) need Read only.
- Ask dialog: "Save the notecard “Instructions” (3 lines added, 1 removed)"
  / "Save and compile the script “Door” (…)", then at most 5 changed lines
  (`+` / `−`, 80 characters each) and "…and N more". en, fr, ja.

## Creators' permissions

The rights are the Legacy Viewer's, through the shared helpers
(`actions::held`, the agent's mask: owner, group or everyone), never a god
level, and no demo exception:

| | Read the text | Save |
|---|---|---|
| Notecard | Copy (LLPreviewNotecard::loadAsset) | Copy and Modify, owned by the agent (`preview::can_edit`), not in the Library |
| Script | Copy and Modify, or Copy for a Library script (LLPreviewLSL::loadAsset; `app/scripts.rs` `permissions`) | Copy and Modify, owned, not in the Library; LSL only (an SLua source is refused: its target is `luau`, not Mono) |

Stricter than the viewer, as for every assistant change: not in the Trash,
not under the Marketplace Listings folder, not while the item is open in
the viewer's notecard window or script editor (`busy`: no unsaved work is
overwritten and no clean editor shows a stale text), not a link's target
outside My Inventory. Items in objects (task inventory) are never read or
saved here; nothing is rezzed, run, reset or attached.

- **No asset id** in any answer, receipt or log: the snapshot hash is a
  SHA-256 of a per-login random salt, the item id, the asset id, the kind
  and the text, so it does not reveal the asset id and changes when either
  the asset or the text does.
- **Text is data**: the get's text is the creator's content, returned as
  it is, never interpreted. It is never written to a receipt, the log or
  Recent activity (receipts keep the dialog's sentence only). The snapshot
  store and the content outcomes live in memory, cleared at a login change.
- **Embedded items** (notecards): their names and types are listed (the
  notecard window shows them), never their item or asset ids. The text
  carries them as the viewer does, as private-use characters from
  U+100000; a save must keep the same markers in the same order, so every
  embedded record is written back byte for byte (`Notecard::encode`). A
  version 1 notecard with embedded items, and a notecard holding a record
  its text does not reference, are readable but not editable (a save
  would not be lossless).

## Snapshots and the stale check

- `inventory_content_get` makes a snapshot when the text is complete and
  editable: 8 per login (the oldest goes), 30 minutes, gone at a login
  change and when access is turned off.
- A save carries `snapshot_hash`. Not in the store (unknown, expired,
  another login): `stale`, nothing saved. The item's asset now differs
  from the snapshot's (saved elsewhere since): `stale`, nothing saved. The
  check runs when the desk takes the call, and again just before the
  upload (`Desk::recheck` → `replan`): the dialog may stay open a minute.
- After a confirmed save, the answer carries a new `snapshot_hash` for the
  saved text (unless the notecard has embedded items: the simulator may
  rewrite their records, so read again).
- The server offers no compare-and-swap: these checks protect against
  edits seen by this viewer, not against another client saving at the
  same moment. The answer and the tool description say so.

## Limits

| What | Limit |
|---|---|
| Text, both kinds | 65,536 UTF-8 bytes (`Notecard::MAX_TEXT_BYTES`; scripts: the same, stricter than Aurora's 1 MiB editor limit, as Folderstorm (C++)); no NUL |
| Read of a larger script | `complete: false`, the text cut at 65,536 bytes on a character boundary, no snapshot |
| Diff in a preview | 16 KiB of text; a changed middle of more than 1,000,000 line pairs (1,000 × 1,000) is shown as one replaced block (the counts stay true) |
| Compile errors | 64 entries, 8 KiB in all |
| Content outcomes | 32 per login |
| Bridge request body | raised from 64 KiB to 512 KiB (a 64 KiB text escaped in JSON) |

## Flow

- **Get**: a Read the desk answers later, as `inventory_landmark` does.
  The app fetches what the waiting calls need once per frame
  (`mcp_bridge::wanted_content`): a notecard through
  `PreviewInventoryItem` (the simulator checks the item's rights), a
  script through `LoadScript` (the script editor's own request). The
  answers come back through `mcp_bridge::observe`. 45 s at most
  (`timeout`). A notecard or script whose asset is nil (new, empty) is
  answered at once with the text the viewer shows.
- **Save**: a change, through the desk as any other: receipt, Ask, queue,
  `replan` just before the send, then `app/local_assistant.rs` sends what
  the viewer's editors send — `SaveInventoryContent` (notecard,
  `UpdateNotecardAgentInventory`, no fee) or `SaveScript` with an
  inventory target (`UpdateScriptAgent`, `mono`, not running, no
  experience). The answer is `InventoryContentSaved` / `ScriptSaved`; 30 s,
  then `unconfirmed` (its receipt keeps listening). A script's compile
  failure is a saved source: `source_saved: true, compiled: false` with
  its errors, never a failed save. An unconfirmed upload is never sent
  again.
- **RLVa**: `@showinv` refuses everything; `@viewnote` refuses reading and
  saving notecards, `@viewscript` scripts; `@edit` refuses both saves.
  Checked when the call is taken and before the send.

## Where the code goes

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/protocol/content.rs` (new) | the catalogue, limits, schemas, `requires_request_id` |
| `folderstorm-mcp-bridge/src/policy.rs` | `Class::Content`, its level, migration, presets |
| `folderstorm-mcp-bridge/src/binding.rs`, `protocol.rs`, `http.rs` | `content: 1`; the catalogue in `all()`; the body limit |
| `folderstorm-mcp/src/jsonrpc.rs` | the instructions' content paragraph |
| `aurora-viewer/src/mcp_bridge/content.rs` (new), `content/diff.rs` (new), `content_tests.rs` (new) | the rules, snapshots, gets, the frozen save, the outcomes, the diff, the demo |
| `aurora-viewer/src/mcp_bridge/{desk,changes,tools}.rs`, `mcp_bridge.rs` | the desk takes and answers the content tools; `Work::Content`; the Ctx's RLVa and open editors |
| `aurora-viewer/src/app/local_assistant.rs` | fetches, sends, the inventory's new asset after a script save, the demo |
| `aurora-viewer/src/ui/options/pages/local_assistant.rs` | the Content row |

**Upstream hooks**: none new. The `fs-mcp-*` block of
`aurora-i18n/locales/{en,fr,ja}.ftl` grows, and `docs/TESTING.md` gets
`AURORA_DEMO_MCP=content`.

## Demo

`AURORA_DEMO_MCP=content`: the session asks before changes; a folder
"Assistant content tests" holds a full-permission notecard, a notecard
with embedded items, a no-modify notecard, a no-copy notecard, a
full-permission script, a no-modify script, and a script that does not
compile once edited (the demo's marker `AURORA_DEMO_SYNTAX_ERROR`). The demo
reads the notecard and the script, then asks to save both: the dialog shows
the name, the counts and the changed lines; the script answers its compile
error.

## Tests

Permissions matrix (owner, group-owned, someone else's, Library, no copy,
no modify, full, link, Trash, Marketplace) for reads and saves; RLVa
(`@showinv`, `@viewnote`, `@viewscript`, `@edit`); stale (unknown hash,
asset changed before the take and before the send); caps (65,536 bytes,
NUL, larger script read incomplete); embedded items kept byte for byte,
markers removed or reordered refused, v1 and orphan records not editable;
the diff; compile result reporting (saved and compiled, saved with errors,
not compiled by the grid, failed, unconfirmed); receipts (required
`client_request_id`, duplicate answers the first receipt, no text in any
receipt); the dialog's lines; open editor `busy`; the catalogue and
`tools/list`; end to end over a real socket (sidecar → bridge → desk →
app's demo answers).

Not testable here: the dialog's look, the grid's real capabilities
(SL and OpenSim compile answers), and editors open on the grid.

## As built

Branch `claude/mcp-content`, on `claude/mcp-receipts` (PR #47, not
merged). Everything above is built; the details that differ or that the
plan left open:

- **The frozen save.** When the desk takes a save the first time
  (`Desk::take`, a call without a receipt yet), `Content::freeze` checks it
  against its snapshot and replaces its arguments by `item_id` and a
  `frozen_content` object: the encoded notecard (base64) or the source, the
  asset it was checked against, the counts and the dialog's lines. The
  plan just before the upload (`content::plan`, through `changes::plan`
  from `mcp_bridge::replan`) is then a pure function of them and of the
  context, as a batch plan is. A call from the sidecar carrying
  `frozen_content` itself is refused (`unknown argument`): only the desk
  freezes. The receipt's argument hash is taken before the freeze, so a
  retry with the same `client_request_id` and the same text is a
  duplicate.
- **One exit, as every change**: the server's answer (`InventoryContentSaved`,
  or `ScriptSaved` with an inventory target) maps to
  `desk::Outcome::Content`, which only a content save's flight takes (and a
  content save takes no other answer); `Desk::finish` gives it to
  `Content::saved`, which writes the answer and the content record. An
  unconfirmed save records `source_saved: null`; its late answer updates the
  record (`Content::settle`) and the receipt.
- **Uncertain errors**: a save error that the editors' own uploads raise
  after the text left (`script-save-unconfirmed`, `script-timeout`,
  `notecard-save-unconfirmed`, "saved but could not read it back"…) answers
  `timeout` (receipt `unconfirmed`, `source_saved: null`); any other error
  answers `failed` (`source_saved: false`).
- **The script's inventory item** gets its new asset from the upload's
  `new_asset` (`Content::hold`, through `mcp_bridge::content_hold` once per
  frame before the calls), since the viewer's `script_saved` only updates
  the items of its own editors. No item read follows, as in the Legacy
  Viewer (LLBufferedAssetUploadInfo::finishUpload): on the grid that read
  could come back with the old asset, and a script's source is read by its
  item's asset, so `inventory_content_get` gave the old text and the old
  snapshot after each save. The new asset stays held while the item shows
  an asset a save replaced (a stale record); it ends when the item goes or
  is saved elsewhere. A notecard's item goes through the viewer's own
  `inventory_content_saved` (a read back checked against `new_asset`).
- **Reads in the demo**: the demo's `PreviewInventoryItem` answers the
  editors directly, so the app hands the demo notecard to the bridge itself
  (`mcp_bridge::content_demo_notecard`); scripts go through `LoadScript` and
  the demo's own script sources, as in the script editor.
- **Compile positions**: the grid's `(line, column)` count from 0; the
  answer gives them from 1 (`line`, `column`) with the grid's line as it
  came (`raw`).
- **Open editors**: any open editor on the item (the inventory's preview, a
  document window, a script editor on the inventory item), dirty or not,
  answers `busy`: the editors' "dirty" state is private to `ui/scripts.rs`
  (upstream), and a clean editor would show a stale text after the save.
- **Upstream hooks**: none. The new code is in Folderstorm files; the only
  upstream files touched are the three catalogues (`fs-mcp-*` block) and
  `docs/TESTING.md`.

Tests: `mcp_bridge/content_tests.rs` (19), `content/diff.rs` (6), the
catalogue (`protocol/content.rs`), the policy (Content and its migration),
the sidecar's `tools/list` and instructions. The end-to-end test runs the
sidecar's `Viewer` over a real socket to the bridge and the desk: read,
preview, save, and the same save again answering its receipt.
