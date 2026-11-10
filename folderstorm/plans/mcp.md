# Local assistant (MCP): plan

Port of Folderstorm (C++)'s local assistant bridge (`doc/fs-mcp.md`,
`doc/help.md` "The local assistant bridge", `doc/plans/assistant-*.md`,
`tools/fs-mcp/README.md`) to Folderstorm2. FEATURES.md rows 4.1–4.6.

Phase 1 (this branch) is **read-only**: nothing an assistant asks can change
the inventory, the avatar, the camera or the world.

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
Custom. Phase 1 only shows the Read row under Custom, since nothing else is
gated yet; the other levels are stored so that phase 2 inherits the
user's preset. Permanent delete and Empty Trash are never a class: always
refused. In phase 2, Ask opens a viewer dialog (60 s, refused on timeout) on
the frame thread before the change runs; until then a class set to Ask is
refused like Never.

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

Phase 2 (changes, behind Edit / Move / Wear / Trash and Ask dialogs):
create folders, rename, describe, move, copy, link, Trash and restore, wear
and take off outfits and items, wait for the bake. Phase 3: camera presets
and snapshots (4.4), then FEATURES.md 4.6 one group at a time (navigation,
chat, teleport, uploads, world observation…).

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
| `crates/aurora-viewer/src/mcp_bridge.rs` + `mcp_bridge/` | viewer adapter: lifecycle, session copy, per-frame poll, `tools` (answers from `Inventory`), `setup` (client configurations, connection check) |
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

Not testable here: the look of the page (no display in the sandbox) and
real MCP clients on Windows.
