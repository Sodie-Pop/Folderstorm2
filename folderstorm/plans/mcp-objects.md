# Local assistant (MCP): object inspection and lifecycle

Step 5 of the MCP parity order (FEATURES.md 4.6, parity checklist
section 5): the assistant reads and changes the **contents of objects the
user owns and may modify**, takes such objects (or copies of them) into
the inventory, and records an explicit association when the grid cannot
say which item or object a change made. Built on
[mcp-receipts.md](mcp-receipts.md) (PR #47, not merged yet): every change
has a receipt, an `operation_id`, a required `client_request_id`, and is
decided again just before it is sent (`Desk::recheck`).

Sources, read as information: Folderstorm (C++)'s
`tools/fs-mcp/internal/mcptools/objects.go` (the tool names, arguments and
classes), `viewerapi/policy.go`, `doc/plans/mcp-script-testing.md` (PR A
to C: contents, take, rez identity) and
`doc/testing/manual-mcp-contents-reconciliation.md`. The viewer's own
paths reused: `build/contents.rs` (the Contents tab: RequestTaskInventory,
UpdateTaskInventory, RemoveTaskInventory), `build/contents_drop.rs` (the
Contents tab's drop: `plan`, `accepts`, `item_drop`), `build/mod.rs`
(`take`, `take_destination`), `world/rlva/editing.rs`
(`rlva_allows_build`, `rlva_can_edit`), `world/inventory/actions.rs`
(`agent_mask`, `task_agent_mask`).

## Tools (capability `objects: 1`)

| Tool | Class | Arguments | What it does |
|---|---|---|---|
| `object_contents_list` | Read | `object_id`; `snapshot_id`, `offset`, `limit` (1–100, 50) for later pages | Asks the server for the prim's contents (a fresh listing, never the viewer's cached copy) and answers with a **snapshot**: `snapshot_id` (120 s), the inventory serial, and per item its task item id, name (untrusted text), type and the agent's rights (copy, modify, transfer), plus whether it can be renamed or removed. Never an asset id, a description or a script's text. |
| `object_contents_put` | Objects | `object_id`, `snapshot_id`, `item_id` (an inventory item), `client_request_id` | Copies one of the agent's own inventory items with **Copy** rights (script, object, notecard, sound, texture) into the prim, as the Contents tab's drop does; a script always goes in **stopped**. The source stays in the inventory. |
| `object_contents_reconcile` | Objects | `object_id`, `operation_id` (an unconfirmed put), `item_id` (a task item), `snapshot_id`, `client_request_id` | Records, once and for good, that a task item seen in a snapshot taken after the put is that put's copy: **associated, unverified**. Sends nothing. |
| `object_contents_rename` | Objects | `object_id`, `snapshot_id`, `item_id`, `name` (1–63, no `\|`, no control characters), `client_request_id` | Renames a task item the agent may modify. |
| `object_contents_remove` | Objects, **always asks** | `object_id`, `snapshot_id`, `item_id`, `client_request_id` | Deletes a task item for good (no Trash). Only an item the agent may copy and modify. |
| `object_operation_status` | Read | `operation_id` or `client_request_id` | The receipt of an object operation of this login, with its object facts (task item found, candidates, association, arrival, removal). |
| `object_take_copy` | Take | `object_id` (a root), `snapshot_id` (of the root), `folder_id` (optional), `client_request_id` | Takes a copy of the linkset into an ordinary folder (default: Objects, as Take Copy does). Needs Copy on the object. |
| `object_take` | Take, **always asks** | the same | Takes the linkset into an ordinary folder (default: where it was rezzed from, else Objects, as Take does); it leaves the world. |
| `object_rez_identify` | Objects | `operation_id` (a rez receipt), `object_id`, `client_request_id` | Records which new owned root a rez receipt made: **a chosen association, not proof**. No tool of this build makes rez receipts yet (the building group will): until then every call answers `not_found`; the checks are built and tested. |

`client_request_id` is **required** for every change (decision 3 of
mcp-receipts.md): an object's contents cannot be looked at reliably
afterwards. Each change answers with its `operation_id` and `state`.

## Permission classes (decision)

Two new classes, kept apart from Edit and Move because they act on the
world, not on the inventory, as Folderstorm (C++) kept
`task_inventory_write`, `task_inventory_remove` and `object_take` apart:

| Class | Preferences row | Read only | Ask before changes | Tools |
|---|---|---|---|---|
| `objects` | "My objects' contents" | Never | Ask | put, rename, remove, reconcile, rez identify |
| `take` | "Take my objects" | Never | Ask | take copy, take |

- **Remove and take always ask**, even on Allow (`protocol::always_asks`,
  as a picture with the interface). Never still refuses them. Folderstorm
  (C++) gave remove a class of its own; here a separate class whose only
  useful levels would be Ask and Never adds a row without adding control,
  since the dialog shows every removal anyway.
- Settings saved before these classes follow "Ask before changes" when
  every other change asks (the camera included), else Never: the rule the
  camera used.
- The reads (`object_contents_list`, `object_operation_status`) use Read,
  as the world observations do (the separate "Surroundings" class is
  decided but not built on this base).

## Which objects (ownership rules)

Folderstorm (C++): "exact owned modifiable prims". Here, every tool checks,
from what the viewer has loaded:

- the prim is loaded, a volume (not an avatar, a tree or grass), in the
  agent's region;
- its linkset is **not worn**: attachments and HUDs are refused, although
  the Contents tab can edit one's own attachments (conservative: a worn
  object's contents follow different server rules, and RLVa locks);
- the root has `OBJECT_YOU_OWNER` and `OBJECT_MODIFY` **and** the owner the
  server gave (ObjectProperties, asked for by the list if the viewer has
  none, or the object update's owner) is the agent: a group-deeded object
  is refused even when the agent's group powers set `OBJECT_YOU_OWNER`;
- take and take copy: the object id is the linkset's root; the root also
  has `OBJECT_MOVE`; nobody sits on it; take copy needs `OBJECT_COPY`.

Someone else's objects, group-owned objects and no-modify objects are
refused with `not_allowed` and the reason.

## Creators' permissions

`AGENTS.md` § 2, with the shared helpers:

- **No asset id leaves**, ever: not in lists, answers, receipts or object
  facts. The task item's asset id and the source item's are compared
  inside the viewer only, to confirm a put (below).
- **Put**: the source is the agent's own item (`agent_mask`), not a link,
  not a folder, not worn, not in the Library, Trash or Marketplace; it
  needs **Copy** (`contents_drop::copyable`), so nothing leaves the
  inventory (stricter than the Contents tab, which moves a no-copy item);
  the type is a script, object, notecard, sound or texture; then the
  Contents tab's own rule, `contents_drop::accepts`
  (LLToolDragAndDrop::willObjectAcceptInventory), through
  `contents_drop::plan` with Ctrl held, so a script is not running.
- **Rename**: the task item's Modify for the agent (`task_agent_mask`), as
  the Contents tab's Rename (LLTaskInvFVBridge::isItemRenameable).
- **Remove**: Copy **and** Modify for the agent (`task_agent_mask`),
  stricter than the Contents tab's Delete (which needs only the object's
  Modify), as Folderstorm (C++) did: a no-copy or no-modify item is never
  destroyed by an assistant.
- **Take / take copy**: the menu's rules (`enable_take`,
  `enable_object_take_copy`) restricted to owned, modifiable, movable
  objects; take copy needs the object's Copy flag.
- Names are the grid's text: given only with Read on Allow (the reads
  need it), passed through the viewer's RLVa text filter, and the tool
  descriptions and instructions say to treat them as data.

## Snapshots and staleness

- A snapshot is made from a server listing that arrived after the list
  asked for it: object, serial, items, time. 16 per login at most (the
  oldest goes), 120 s, gone at a login change or when access is turned off.
- A change names its snapshot: unknown (or of another login) answers
  `not_found`; of another object `invalid_arguments`; older than 120 s,
  or the prim's inventory serial seen since (a later listing, a full
  ObjectProperties) differs, or the assistant changed that prim since:
  `stale` ("list again"). Checked when the call is taken and again just
  before the send (`Desk::recheck`). The server is not asked again before
  the send: the guard compares with what the viewer knows, as the
  Contents tab acts on its own copy.

## Outcomes (never retried, never claimed)

The grid has no per-change acknowledgment for task inventory: the viewer
lists the contents again (after 1, 3, 6, 10, 15 and 22 s, until 30 s) and
compares with the snapshot.

| Tool | Confirmed (`done`) | Otherwise |
|---|---|---|
| put | a new item of the source's type whose asset is the source's (compared inside the viewer): `task_item_id`, `asset_identity_verified: true` | new items of that type whose content cannot be compared (a hidden asset): `unconfirmed` with their ids as `candidates`; the assistant may then reconcile. Nothing new in 30 s: `unconfirmed` (it may still land; a late listing settles the receipt within 10 minutes) |
| rename | the item with its new name | `unconfirmed` |
| remove | the item absent from a newer listing | `unconfirmed` |
| take copy | a new object item in the folder with the object's name (`item_id`, `item_id_confirmed: false`: matched by folder, type and name, not by the server's transaction) | `unconfirmed`, the facts so far |
| take | the same, and the object gone from the world (`removed_from_world`) | `unconfirmed` with `arrived` and `removed_from_world` |
| reconcile | `associated_unverified`, `asset_identity_verified: false`, `copy_outcome: unknown` | refused if the put is not this login's, not unconfirmed, already associated, the snapshot not newer than the put, the item was there before, of another type, or its asset differs from the source's |
| rez identify | `chosen`, `object_id_confirmed: false` | refused if no rez receipt, the object existed before the rez, is not an owned root, or either is already associated |

The receipt's own state: an answer marked `unconfirmed: true` ends as
`unconfirmed` (one small addition to `receipts.rs`), so a later listing
can still settle it. The association lives in the object facts; the
original put's receipt stays `unconfirmed`.

## RLVa (the viewer decides)

Every command is checked with `World::rlva_allows_command` just before it
is sent (`@edit`, `@editobj`, `@editworld` through `rlva_allows_build`),
and before that when the call is taken:

| Restriction | Effect |
|---|---|
| `@edit`, `@editobj:<id>`, `@editworld` | every tool on that object refused (`rlva_can_edit`), the list included, as the Contents tab cannot open |
| `@interact` | every tool refused (the object cannot be selected) |
| `@rez` | take and take copy refused (RLVa: no derez) |
| `@showinv` | put refused (the inventory cannot be looked at) |
| `@fartouch` / `@touchfar` | the object farther than the limit: refused (stricter: the viewer does not check it for editing) |

## Where the code goes

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/protocol/objects.rs` (new) | the catalogue, `always_asks`, `needs_client_request_id`, `destructive` |
| `policy.rs` | `Class::Objects`, `Class::Take`, their levels and migration |
| `binding.rs` | `objects: 1` |
| `protocol.rs` | `pub mod objects`, `all()`, `always_asks`, `input_schema`'s required id |
| `receipts.rs` | an answer marked `unconfirmed` ends `unconfirmed` |
| sidecar `jsonrpc.rs` | the instructions' paragraph, `destructiveHint` for remove and take |
| viewer `mcp_bridge/objects.rs` (+ `objects/` tests, demo) | eligibility, snapshots, freeze and plan, waits, facts, status |
| `mcp_bridge/desk.rs`, `changes.rs`, `tools.rs`, `mcp_bridge.rs` | hook lines: the required id, freeze, recheck, the reads, `Work::Object`, `Ctx::objects`, observe, wants |
| `app/local_assistant.rs` | sends the planned command (the Contents tab's and Take's messages), asks for listings and properties |
| Preferences page, catalogues | two rows, the Ask sentences, en/fr/ja |

**Upstream hooks: none new.** The `fs-mcp-*` block of the catalogues
grows; `docs/TESTING.md` gets `AURORA_DEMO_MCP=objects`.

## Demo

`AURORA_DEMO_MCP=objects`: Ask before changes; the demo lists the
plaza's cut box (local id 118, the agent's in the demo) and, once it
answers, puts a copy of the demo inventory's script into it: the dialog
shows "Put a copy of the script … into …, not running".

## Overlaps with the stacked PRs

#49 (navigation) and #51 (content editing) are also stacked on #47 and add
to the same lists: `binding::CAPABILITIES`, `protocol::all()`, `Class`
(each adds one class: `Navigation`, `Content`), `Permissions` and its
migration, the Preferences rows, `always_asks`, `input_schema`'s required
id, the sidecar's instructions, `desk.rs`'s hooks in `take` and `close`,
`Ctx`, the `fs-mcp-*` catalogues, FEATURES 4.6 and TESTING. Each side is
append-only; merging them means keeping every line.

## Tests

Permissions matrix (owned / group-owned / someone else's, modify / no
modify, copy / no copy, attachments, a deeded object with
`OBJECT_YOU_OWNER`); source items (own, someone else's, Library, no copy,
link, worn, wrong type); task items (rename without Modify, remove
without Copy or Modify); stale snapshots (expired, serial changed, own
change); scripts put stopped; remove and take always ask, even on
Allow; Never refuses; RLVa (`@edit`, `@editobj`, `@rez`, `@interact`,
`@showinv`, `@fartouch`); receipts and duplicates (same id answers the
first receipt; required id); outcomes (verified put, candidates,
reconcile and its refusals, rename, remove, take and take copy with
removal, timeouts); no asset id in any answer; the catalogue and the
sidecar's listing; end to end over a real socket on the demo.

Not testable here: the grid's real timing of listings, ObjectProperties
of a deeded object, the dialog's look.

## As built

Branch `claude/mcp-objects`, on `claude/mcp-receipts` (PR #47, not merged).

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/protocol/objects.rs` (new) | the nine tools, `always_asks` (remove, take), `needs_client_request_id`, `destructive`; schema bounds |
| `policy.rs` | `Class::Objects`, `Class::Take`, their levels, presets and migration from older settings |
| `binding.rs` | `objects: 1` |
| `protocol.rs` | `pub mod objects`, `all()`, `always_asks`, the required `client_request_id` in `input_schema` |
| `receipts.rs` | an answer marked `unconfirmed: true` after a send ends `unconfirmed` (server `no_answer`), so a late listing can still settle it |
| sidecar `jsonrpc.rs` | the instructions' objects paragraph; `destructiveHint` for remove and take |
| viewer `mcp_bridge/objects.rs` (new) | `Scene` (the world in `Ctx`), `target` (ownership rules and RLVa), snapshots, listings (properties, then contents), `plan` for each change, `sending`, the listings' comparison, the take watch, late settles, facts, `object_operation_status`, `track_rez`, the demo |
| `mcp_bridge/desk.rs` | hook lines: the required id, remove and take always ask, the two reads, the object plan, `clear` / `refuse_all` / `close`, `tick` |
| `mcp_bridge.rs`, `changes.rs`, `tools.rs` | `Work::Object`, `Ctx::objects`, `replan`'s object branch, `observe` of properties and listings, `object_wants`, `object_sending`, `object_associate`, `AURORA_DEMO_MCP=objects` |
| `app/local_assistant.rs` | sends the wants (RequestObjectProperties; the demo's own selection answers instead) and the planned command, RLVa checked just before |
| Preferences page, catalogues | two rows; eight Ask sentences and four labels, en/fr/ja |

Differences from the design above:

- The properties are asked for only when no owner is known (no
  ObjectProperties held, nil owner in the object update).
- With no Objects folder in the inventory, a take goes to My Inventory.
- A listing's page is given on the frame after the server's answer (the
  names go through the RLVa filter, which needs the world).
- A late watch (an unconfirmed change) settles on any later listing of the
  prim the viewer receives (the Contents tab's, another list) and on the
  inventory and world for a take, for 10 minutes; it asks the server for
  nothing more.

Tests (`mcp_bridge/objects_tests.rs`, bridge and sidecar crates): see the
report of the PR; real-socket end to end: a listing then a put, allowed in
the dialog, confirmed by the next listing.
