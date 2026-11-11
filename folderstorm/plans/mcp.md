# Local assistant (MCP): plan

Port of Folderstorm (C++)'s local assistant bridge (`doc/fs-mcp.md`,
`doc/help.md` "The local assistant bridge", `doc/plans/assistant-*.md`,
`tools/fs-mcp/README.md`) to Folderstorm2. FEATURES.md rows 4.1–4.6.

Phase 1 is **read-only**: nothing an assistant asks can change the
inventory, the avatar, the camera or the world. Phase 2 adds inventory and
outfit changes, each behind its permission class (see "Phase 2" below).

## Architecture

```
MCP client (Claude Code, Cursor, Codex…)
   │  JSON-RPC 2.0 over stdio
   ▼
folderstorm-mcp            (sidecar binary, next to the viewer executable)
   │  finds the viewer through folderstorm2-mcp-<pid>.json
   │  HTTP/1.1 on 127.0.0.1:<port>, Authorization: Bearer <token>
   ▼
folderstorm-mcp-bridge     (in the viewer process: listener threads)
   │  bounded channel of pending calls
   ▼
aurora-viewer mcp_bridge   (frame thread: polled once per frame, answers
                            from the inventory and the worn items)
```

- The bridge's listener and its connection threads never touch viewer
  state. They authenticate, check the permission policy, then hand the call
  to the frame thread through a bounded channel and wait (with a timeout)
  for the answer on a one-shot channel.
- The frame thread drains at most a few calls per frame with `try_recv`;
  it never blocks. Each answer is bounded (result counts are capped).
- The viewer does not start the sidecar. The MCP client starts it; the
  sidecar finds the running viewer and talks to it.

## Security model

- **Off by default.** Nothing listens until the user turns on
  *Allow local assistant access* (or passes `--mcp-api` for one session).
- **Loopback only**: the listener binds `127.0.0.1:0` (an OS-chosen port),
  never `0.0.0.0` or `::`.
- **Bearer token**: 32 bytes from the OS random source (`getrandom`), hex
  encoded, new each time the bridge starts. Compared in constant time
  (`subtle`). Never logged, never shown in Preferences, never copied by
  *Copy configuration*, never printed by the sidecar (`--diagnose` included).
- **Discovery file** `folderstorm2-mcp-<pid>.json` in the user settings
  directory (`settings::config_dir()`, under `folderstorm_brand::project_dirs()`):
  `{schema, product, pid, port, token, started}`. Written to a temporary
  file created with mode `0600` on Unix, then renamed; deleted when the
  bridge stops (disable, quit). On Windows the per-user AppData ACL already
  limits it to the user. A file left by a crash is harmless: its port no
  longer answers with its token; the sidecar skips it.
- **Browser defence**: a request carrying an `Origin` header is refused, and
  the `Host` header must name `127.0.0.1` or `localhost` with the bridge's
  port (DNS rebinding). The token blocks the rest.
- **Bounds**: request head 8 KiB, body 64 KiB, 4 connections at once (more
  are answered 503 and closed), 5 s socket read/write timeouts, 10 s for the
  viewer's answer, 16 calls queued for the frame thread at most. One request
  per connection (`Connection: close`).
- **What it never exposes**: passwords, session ids, capability URLs, the
  token, chat, and any asset id the viewer itself would hide (see below).
  Activity shown in Preferences keeps the tool name and the outcome only,
  never arguments or results.
- Other programs running under the same OS account can read the discovery
  file: the Preferences text says so, as Folderstorm (C++) does.

## Creators' permissions

`inventory_item` returns an item's `asset_id` only when the viewer would
show it: `actions::held(inv, item) & actions::FULL == actions::FULL`, the
rule of `LLViewerInventoryItem::getProtectedAssetUUID`
(indra/newview/llviewerinventory.cpp) that `actions::asset_uuid_shown`
applies; otherwise the field is absent and `asset_id_hidden` is true. Links
give their target item id (not an asset). Thumbnails (texture ids) are not
returned at all. Search and listing results carry no asset ids. Stricter
than the Legacy Viewer: no demo-mode exception, and RLVa's `@showinv` makes
every inventory read fail (the Legacy Viewer only hides the windows).

## Permission model

Classes, each with a level **Allow / Ask / Never**:

| Class | Phase | Gate for |
|---|---|---|
| `read` | 1 | every inventory and outfit read (Allow or Never only: no Ask) |
| `edit` | 2 | rename, describe, create folders |
| `move` | 2 | move, copy, link |
| `wear` | 2 | wear, take off, detach |
| `trash` | 2 | move to Trash, restore |

`viewer_status` is the health check: it needs a valid token but no class,
so *Check connection* works even with Read set to Never. It returns only
login and inventory readiness.

Presets (derived from the levels, never stored):

| Preset | read | edit, move, wear, trash |
|---|---|---|
| Read only | Allow | Never |
| Ask before changes | Allow | Ask |
| Custom | any other combination | |

Choosing a preset writes all the levels at once; editing a level shows
Custom. Custom shows one row per class (phase 1 showed only Read; the
other levels were stored so that phase 2 inherits the user's preset). Permanent delete and Empty Trash are never a class: always
refused. Ask opens a viewer dialog (60 s, refused on timeout) on the frame
thread before the change runs (phase 2).

**Use these settings for this session only**: the switch and the
permissions edit a session copy kept in memory; the saved settings resume
at the next start. `--mcp-api` starts the session with such a copy, turned
on, and saves nothing.

## Tools

Phase 1 (read):

| Tool | Class | Returns |
|---|---|---|
| `viewer_status` | health | product, version, session state (logged out / logged in / demo), inventory usable, folders fetched / total, items known |
| `inventory_search` | read | items and folders whose name contains `query` (case-insensitive), filters `folder_id` / `path` (subtree), `type`, `include_library`, `include_trash`; `limit` ≤ 200, `truncated` flag |
| `inventory_list_folder` | read | a folder by `folder_id` or `path` (root when neither): its subfolders and items (≤ 500), whether it is fetched |
| `inventory_item` | read | one item: name, description, type, folder and path, creator, creation date, permissions, worn, link target; `asset_id` only per the rule above |
| `outfits_list` | read | outfits under My Outfits, with the current base outfit marked |
| `worn_items` | read | the Current Outfit's clothing and body parts and the attached objects |

Paths are folder names joined with `/`, from My Inventory (`Clothes/Boots`)
or from the Library (`Library/…`). Names are not unique: an ambiguous path
returns an error listing the candidate ids.

Phase 2 (changes): see "Phase 2" below. Phase 3: camera presets and
snapshots (4.4), then FEATURES.md 4.6 one group at a time (navigation,
chat, teleport, uploads, world observation…).

## Phase 2: changes

Every change goes through the path the inventory window or Appearance
uses for the same click, with its rules: `world::inventory::actions`
(`create_folder`, `patch`, `move_selection`, `paste`, `default_folder`)
for the inventory, `world::appearance::Action` (through
`App::apply_appearance_action`) for outfits. No new network message. A
call is checked twice: when it arrives (a refusal needs no dialog) and
again when it runs (the inventory may have changed during the dialog).

| Tool | Class | Arguments | Returns | Refused when |
|---|---|---|---|---|
| `inventory_create_folder` | edit | `name`; `folder_id` or `path` of the parent (My Inventory when neither) | `folder_id`, `name`, `parent_id` | the parent takes no new entry (`actions::destination`: Library, Current Outfit, Received Items, Trash, My Outfits); a name the inventory refuses |
| `inventory_rename` | edit | `id` (item or folder), `name` | `id`, `name` | `actions::renameable` says no: system folders, Library, Current Outfit, Received Items, links, calling cards, items without Modify |
| `inventory_describe` | edit | `item_id`, `description` (≤ 127 characters) | `id` | folders and links; Library, Current Outfit, Received Items; items without Modify (the properties panel's rule) |
| `inventory_move` | move | `ids` (1–50), `folder_id` or `path` | `moved` | `actions::move_selection` says no: system, locked, pinned or worn entries, a folder into itself, a closed destination; the Trash (that is `inventory_trash`) |
| `inventory_copy` | move | `ids` (1–50), `folder_id` or `path` | `copied` | `actions::paste`: an item without Copy (Library items copy), a folder not fully loaded, a closed destination |
| `inventory_link` | move | `ids` (1–50), `folder_id` or `path` | `linked` | `actions::paste` with links: originals in the Library or the Trash, calling cards and other types that are never linked |
| `inventory_trash` | trash | `ids` (1–50) | `trashed` | the Delete rule (`actions::removable` and worn ancestors): system and locked folders, worn items and the folders holding them, the current outfit |
| `inventory_restore` | trash | `ids` (1–50) of entries in the Trash | `restored`: each id with the folder it went to | an entry outside the Trash; "Restore item"'s rule (`default_folder` then `move_selection`) |
| `outfit_wear` | wear | `outfit_id` or `path`, `mode` `replace` (default) or `add` | `done` | the outfit or the Current Outfit not loaded yet, `appearance::plan`'s other refusals, RLVa |
| `outfit_take_off` | wear | `outfit_id` or `path` | `done` | as above; body parts always stay (takeOffOutfit) |
| `items_wear` | wear | `item_ids` (1–50), `mode` `add` (default) or `replace` | `done` | not clothing, a body part or an object; Library and Trash items (copy them first); RLVa |
| `items_take_off` | wear | `item_ids` (1–50) | `done` | an item not worn; body parts cannot be taken off, only replaced |
| `wait_for_bake` | read | `timeout_seconds` (1–120, default 30) | `settled`, `outfit_change_pending`, `cof_version`, `appearance_cof_version` | never: a timeout answers `settled: false` |

- Permanent delete, Purge and Empty Trash are never tools: the names
  `inventory_delete`, `inventory_purge` and `empty_trash` are refused by
  the bridge and the sidecar with `permission_denied`, whatever the levels.
- The Marketplace Listings folder is left alone: a change that touches it
  (source or destination) is refused. Its listing dialogs have no
  assistant counterpart. Stricter than the window.
- One change at a time, like the window: while the inventory window or
  Appearance waits for the server, a change is refused with `busy`.
- An inventory change answers when the server confirmed it (or with
  `timeout` after 30 s, the change may still land); an outfit change when
  the Current Outfit is written. `wait_for_bake` then waits until no outfit
  change is pending and the server's appearance carries the Current
  Outfit's version (in demo mode, or a region that does not bake, the
  first condition alone).
- Refusals by the viewer's rules use the new error code `not_allowed`; the
  user's permission levels keep `permission_denied`. Messages from the
  viewer's own rules are its interface text (in the user's language).

### Ask

A class set to Ask passes the bridge with an `ask` flag. The frame thread
checks the call, then queues a dialog (`widgets::Modal`, Palette, shared
buttons) that says in plain words which program asks what: "“Claude Code”
asks to move “Boots” to “Clothes/Old”". The program's name is the
`clientInfo.name` the MCP client gave the sidecar at `initialize`, passed
on in an `X-Folderstorm-Client` header (printable, 48 characters at most);
the dialog says it is the name the program gives itself. Allow runs the
change; Refuse, Escape, a click outside and 60 s without an answer refuse
it (`permission_denied`). Neither button is the dialog's default, so an
Enter typed for something else allows nothing. Dialogs come one at a time, oldest first; each
call's 60 s count from its arrival. Waiting never blocks the frame: the
call stays in a queue polled once per frame. The bridge's wait for the
viewer grows with the call: 10 s, plus 60 s when it asks, plus the tool's
own time (30 s for a change, 120 s for `wait_for_bake`); the sidecar's
socket timeout follows the same table.

Preferences shows every class under Custom now, with Allow / Ask / Never
(Read: Allow / Never).

## MCP sidecar: no SDK

The official Rust SDK, `rmcp` 3.9 (Apache-2.0, maintained by the MCP
project), exists. It is not used: it requires an async runtime (tokio,
tokio-util, futures), `schemars` and macros, for a server of three methods
(`initialize`, `tools/list`, `tools/call`) and six tools, and its API has
changed incompatibly across major versions. A hand-written newline-delimited
JSON-RPC 2.0 layer on `serde_json` is about 300 lines, synchronous, and
fully unit-tested here. It answers `ping` and ignores notifications, as the
specification requires.

Discovery: `FOLDERSTORM2_MCP_DISCOVERY` (a directory) overrides the default
settings directory; `--viewer-pid` picks one viewer when several run;
otherwise the newest file whose viewer answers is used.
`folderstorm-mcp --diagnose [--viewer-pid N] [--discovery DIR]` prints one
JSON line (stage reached, port, readiness; never the token) and exits 0 on
success, 1 on failure, 2 on bad arguments.

## Crates and files

| Where | What |
|---|---|
| `crates/folderstorm-mcp-bridge/` | no viewer dependency: `policy` (classes, levels, presets, saved settings), `token`, `discovery`, `protocol` (tool catalogue, wire types, error codes), `http` (bounded HTTP/1.1 heads), `server` (listener), `client` (used by the sidecar and the tests) |
| `crates/folderstorm-mcp/` | binary `folderstorm-mcp`: `jsonrpc` (MCP), `diagnose`, `main` |
| `crates/aurora-viewer/src/mcp_bridge.rs` + `mcp_bridge/` | viewer adapter: lifecycle, session copy, per-frame poll, `tools` (answers from `Inventory`), `setup` (client configurations, connection check); phase 2: `changes` (plans each change with the window's rules), `desk` (calls waiting for the user, the app, the server or the bake), `ask` (the dialog) |
| `crates/aurora-viewer/src/app/local_assistant.rs` | phase 2: runs the planned changes through `App::apply_appearance_action` and the inventory window's submit, and tells the desk the request id |
| `crates/aurora-viewer/src/ui/options/pages/local_assistant.rs` | the Preferences group |

New dependencies: `getrandom` 0.4 (OS random source, MIT/Apache-2.0,
already in Cargo.lock) and `subtle` 2.6 (constant-time comparison,
BSD-3-Clause, already in Cargo.lock); `serde`, `serde_json`, `log` as the
workspace uses them; `tempfile` for tests (already in the lock).

## Upstream hooks (each marked `// Folderstorm:`)

- `crates/aurora-viewer/Cargo.toml`: the bridge dependency; the sidecar as a
  dev-dependency (end-to-end test).
- `main.rs`: `mod mcp_bridge;`.
- `cli.rs`: the `mcp_api` field, its parse branch, the help line.
- `settings.rs`: the `local_assistant` field and its default.
- `app.rs`: the per-frame poll and the stop at exit.
- `ui/options/pages.rs`: `pub mod local_assistant;`; `pages/privacy.rs`: one
  call that draws the group; `ui/options.rs`: the demo row
  (`AURORA_DEMO_OPTIONS=privacy:local-assistant`).
- `aurora-i18n/locales/{en,fr,ja}.ftl`: a block of `fs-mcp-*` messages after
  the Go to folder block.
- `README.md`: `--mcp-api` in the command-line section; `docs/TESTING.md`:
  the demo scenario.
- Phase 2, `app.rs`: `mod local_assistant;`, the per-frame call becomes
  `self.local_assistant_frame()`, one `mcp_bridge::observe(&ev)` at the top
  of `on_app_event` (inventory and outfit answers), one
  `mcp_bridge::ask::show(&ctx, &p)` beside the other confirmation dialogs
  of `draw_ui`. The `fs-mcp-*` block of the catalogues grows.

## Tests

- bridge crate: token (length, randomness, constant-time match, redacted
  Debug), discovery (write/read round trip, `0600` on Unix, removal, newest
  first, pid filter, malformed files skipped), policy (presets, derivation,
  Read never denies, Ask refused until phase 2, serde defaults), HTTP head
  parsing limits, server (no token, bad token, Origin, Host, oversized body,
  bridge stopped, permission denied, a call answered, viewer timeout).
- sidecar crate: JSON-RPC (initialize version negotiation, tools/list,
  tools/call success and tool error, unknown method, parse error,
  notifications get no answer, ping), discovery selection, an integration
  test against an in-process bridge over a real localhost socket.
- viewer: each tool against `world::inventory::demo::seed` (search filters,
  paths, ambiguous path, list, item with full and partial permissions,
  link, outfits, worn), RLVa `@showinv`, configuration snippets (JSON and
  TOML escaping of spaces, quotes, backslashes and Unicode), and an
  end-to-end test: sidecar logic → real socket → bridge → the adapter
  answering from the demo inventory.
- demo: `AURORA_DEMO_OPTIONS=privacy:local-assistant` opens the group;
  `AURORA_DEMO_MCP=1` also starts the bridge (session only) so its status
  and port show. Listed in `docs/TESTING.md`.

- phase 2: each change tool on the demo inventory (applied with
  `actions::demo_mutate` / `appearance::demo_mutate`), its refusals
  (system, locked, worn, Current Outfit, Library, Marketplace, no copy, no
  modify, links, body parts), the creators' permissions matrix (owner,
  group-owned, someone else's, Library, no copy, no transfer, no modify,
  full) for rename, describe and copy; the desk (Ask: client and summary,
  refusal, 60 s timeout, queue; Allow runs at once; server answers in and
  after the send, copies, failure, 30 s timeout; `wait_for_bake`); the
  bridge (Ask flag and client name, Never refused, permanent delete never
  allowed, the longer wait of an asking call); the sidecar (client name,
  tools/list annotations, permanent delete refused); an end-to-end move
  allowed and Trash refused in the dialog, from the sidecar to the demo
  inventory.

Not testable here: the look of the page and of the dialog (no display in
the sandbox), real MCP clients on Windows, and the grid's answers (the
bake wait against a real region).

## Phase 3, part 3: world observation

Step 4 of the MCP parity order (first half): the six `world_*` reads of
Folderstorm (C++) (`tools/fs-mcp/internal/mcptools/world.go`,
`doc/plans/mcp-world-observation.md`), with the C++ names. Their catalogue
is its own file (`protocol/world.rs`, listed with `protocol::all()`); the
viewer answers them in `mcp_bridge/world.rs`, from what it has already
loaded and shows, on the frame thread. They are **Read** (Allow / Never,
never ask): they change nothing, send nothing to the server, and keep
nothing between calls (no history of other avatars in the bridge). The
Read row of Preferences now says that it covers the surroundings too.

Folderstorm (C++) put these behind a separate opt-in `world_read` class.
Here they share Read, as the approved order asked; a separate class could
follow once the camera PR's `Class::Camera` has merged (both edit
`policy.rs`).

| Tool | Arguments | Returns | RLVa |
|---|---|---|---|
| `world_location` | none | region (name, maturity, size), parcel name, position in the region, global position, facing (heading, compass point), flying, sitting | `@showloc`: region and parcel replaced by the navigation bar's hidden labels, no coordinates (`location_hidden_by_rlv`) |
| `world_parcel` | none | About Land's summary: local id, name, description, owner (resident or group), area, prims used / max, for sale and price, flags (fly, build, group build, scripts, group scripts, damage, push restricted, landmarks, terraform, voice, local sounds, see avatars, avatar sounds) | `@showloc`: `not_allowed`, unless the user owns the parcel or manages it through a group (`World::rlva_can_view_parcel`'s rule), and then its name still goes through the RLVa text filter (hidden); `@shownames`: an anonymous owner, no id |
| `world_nearby_avatars` | `radius` (1–256 m, 64), `limit` (1–100, 25) | the avatars `radar::sightings` gives (loaded ones, then coarse locations), nearest first: name, id, distance (none when the altitude is unknown), same region, loaded, sitting, friend, offset east / north / up | `@shownearby`: `not_allowed` (People › Nearby is empty); `@shownames`, and `@shownametags` for a hidden tag (stricter than the People list): the anonymous name, no id, no friend flag; `@showminimap`: no offsets (`positions_hidden_by_rlv`) |
| `world_nearby_objects` | `radius`, `limit` | loaded root objects (no avatars, attachments or HUDs; the parcel isolation applies), nearest first: id, distance, offset, prim count, tree; name, description and owner only when the viewer already holds them (Area Search, a hover tip, the selection), else `name_known: false` | names and descriptions through the viewer's RLVa text filter; `@shownames`: an anonymous owner, no id |
| `world_audio_sources` | `radius`, `limit` | the Sound Explorer's history, newest first: kind (`avatar`, `collision`, `llTriggerSound`, `llLoopSound`, `llPlaySound`), owner name, playing, started / stopped seconds ago, distance | `@shownames`: an anonymous owner (`sound_owner_name`) |
| `world_media_sources` | `radius`, `limit` | the parcel's music and media: configured or not, media type, description, size, looping; loaded objects nearby with media on a face (not worn ones): id, distance, faces | descriptions through the RLVa text filter |

- Every list gives `radius`, `limit`, `count`, `total` (rows in the radius)
  and `truncated`; nothing past the limit is computed into the answer.
- Creators' permissions: no asset id at all (sound assets included, even
  where the Sound Explorer would show one), no object contents, no hover
  text, no media or music URL.
- Not ready (`not_ready`): logged out, during a teleport, before the
  region is known; `world_parcel` before the parcel's properties came;
  `world_audio_sources` without the scene's history.
- Unknown is not absence: what the viewer has not loaded is not listed,
  and an empty list proves nothing. Playback state of media is not
  reported (the media manager is not read).
- Upstream hooks: `app/area_search.rs` gives its loaded properties to the
  bridge (`Feed::props`, read only). `app/local_assistant.rs` (ours) adds
  the property caches and the sound history to `mcp_bridge::View`;
  `mcp_bridge::frame` answers the `world_*` calls before the desk.
- Tests (`mcp_bridge/world_tests.rs`, on the demo plaza): each tool, the
  argument bounds, not ready, the 100-row cap and the order, each
  restriction (`@showloc` for the place and the parcel, the owner's
  exception, `@shownearby`, `@shownames` with an exception,
  `@shownametags`, `@showminimap`), no asset id or URL in any answer; the
  catalogue (`protocol/world.rs`) and the sidecar's `tools/list`.
