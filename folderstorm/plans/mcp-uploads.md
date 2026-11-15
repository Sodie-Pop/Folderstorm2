# Local assistant (MCP): camera read and file uploads

Groups 2 (the last two camera rows) and 8 (file uploads) of the MCP parity
checklist (FEATURES.md 4.6). Branch `claude/mcp-uploads`, on
`claude/mcp-gestures` with `claude/folder-upload` (PR #65, FEATURES 2.11)
merged in.

Sources, read as information: Folderstorm (C++)'s
`doc/plans/mcp-file-uploads.md` and `tools/fs-mcp/internal/mcptools/uploads.go`,
`camera.go`, `inventory_extra.go` (names, arguments, bounds, rules). The
viewer's own paths reused: the uploader's codecs (`aurora_assets`
`import_texture`, `import_sound`, `import_animation`, the importer's
`sound::Options`), its prices (`Prices::from_login`, the photo window's
`photo_prices`), its upload request (`NetCommand::UploadAsset`, which sends
the quoted price and stops on `PriceChanged`), its folder creation (one
acknowledged `create_folder` at a time, as `imports/folders.rs`), the
upload permissions of Preferences (`Preferences::permissions_for`), the
studio's picture path (`camera_snapshot`), the Image window's path for a
picture from a texture (`inventory_set_thumbnail`), and the uploader's Mesh
window for Collada review.

## Tools

| Tool | Class | Asks | What it does |
|---|---|---|---|
| `camera_get` | Read | – | Where the camera is: position, focus (target), distance, field of view, and the avatar's position. Read only. |
| `inventory_upload_preview` | Read | – | Checks 1–32 local files, or one local folder, reads and hashes them on a worker, decodes them as the uploader does, prices each file with this account's prices, and keeps the frozen bytes as a plan for 120 s. Answers `plan_id`, rows (name, kind, folder, bytes, SHA-256, size, price), the folders to create, the total, the balance and `execution`. Uploads nothing. |
| `inventory_snapshot_upload` | Camera (+ Read) | level (Camera) | Takes a picture as `camera_snapshot` does (world view, no interface) and keeps it as a one-file upload plan for 120 s, priced as a texture, with an optional entry to set it on as its picture afterwards. Uploads nothing. |
| `inventory_upload_execute` | Upload (+ Read; + Edit when the plan sets a picture) | level; **always** when the total is above L$0 | Uploads a plan once: folders first, then the files one by one, each with its frozen price. Required `client_request_id`. Answers early (operations) with `operation_id`. |
| `inventory_upload_status` | Read | – | The run's per-file outcomes, folders created, L$ spent, by `plan_id` or `operation_id`. |
| `inventory_upload_cancel` | – (bound to the login) | – | Stops the files not sent yet; an upload already sent runs to its answer. Never undoes an accepted upload or its charge. `assistant_operation_cancel` and Preferences' Stop do the same. |
| `inventory_upload_mesh_review` | Upload | level | Opens one local Collada (`.dae`) file in the uploader's Mesh window. Uploads nothing: the window shows the fee, and the user finishes by hand. Refused while the uploader is busy or already holds a mesh. |

Names are Folderstorm (C++)'s; no `viewer_session` argument (the sidecar
pins the login). The C++ `inventory_snapshot_upload` had two modes; the
thumbnail mode is `inventory_snapshot_thumbnail` on main, so this tool is
the texture mode only, in two calls: the picture and its quote (free), then
`inventory_upload_execute` (the paid part, which asks with the price).

## Permission classes and asking (decision)

- **New class Upload** (`upload`, "Upload local files"): execute and mesh
  review. Ask in "Ask before changes", Never in "Read only"; settings saved
  before it get Ask only when every other change already asks (Pose
  included), else Never.
- **Anything that costs L$ always asks and shows the price**, whatever the
  level: an execute whose total is above L$0 always opens the Ask dialog,
  which shows the total, the balance before and after, and the files with
  their price (5 at most, "and N more"). Under Allow, only an L$0 plan runs
  without a dialog. Nothing spends money without the user's click in the
  viewer.
- **Prices**: the account's own (`account_level_benefits`, with the
  economy's single price as the fallback, as the photo window), frozen in
  the plan. An unknown price refuses the preview: never read as free. Just
  before each file is sent, the account's price is read again; a higher one
  stops the run (`stale`: preview again). The server also checks the frozen
  price (`expected_cost`) and refuses a different one (`PriceChanged`): the
  run stops, nothing more is sent.
- **Balance**: a total above the balance (or an unknown balance with a
  positive total) refuses the execute (`not_allowed`), as the uploader's
  Upload button.
- `client_request_id` is required for the execute (an upload cannot be
  looked at afterwards: decision 3 of mcp-receipts.md).

## Files and limits (decision)

| Bound | Folderstorm (C++) | Aurora's uploader | Here |
|---|---|---|---|
| Files per plan | 32 | 256 per selection, 512 drafts | **32** |
| Bytes per file | 64 MiB | 256 MiB read | **64 MiB** |
| Bytes per plan | 128 MiB | 1 GiB retained | **128 MiB** |
| Folder depth | 8 | 64 | **8** |
| Folders created | 64 | – | **64** |
| Plans per login | 4 | – | **4**, 128 MiB staged in all (the oldest unused plan goes) |
| Plan lifetime | 120 s | – | **120 s** |

C++'s bounds are kept: they are within Aurora's, and the plan holds the
bytes in memory until it runs or expires.

- **Local files only**, on the machine running the viewer: absolute paths,
  no URL, no network share (`\\…`, `//…`), no symbolic link anywhere in the
  path or the tree (each component checked without following links), no
  special file. A folder is walked in name order.
- **Types**: PNG, JPEG, TGA, BMP (textures), WAV (sounds up to the
  uploader's limit, 30 s on Second Life, 60 s on other grids; no splitting:
  one file is one item), ANIM and BVH (animations). Anything else refuses
  the whole preview with its name (as Folderstorm (C++): nothing is skipped
  silently); a `.dae` points to the mesh review.
- **Textures** are sized as the uploader sizes them: at most
  `max_texture_dimension` (1024 by default, or 2048), powers of two; the
  price follows the area (the large-texture price above 1024 × 1024).
- **Names**: the file's name without its extension, 1–63 characters, no
  `|` or control character (the uploader's rule); otherwise refused.
- **The bytes are read once**, at the preview, hashed (SHA-256) and kept:
  what is uploaded is exactly what was priced, whatever happens to the
  files afterwards. Released when the plan runs, expires, or the login
  changes.
- **Destination**: `folder_id` or `path`, a folder that takes new items
  (`actions::destination`: not the Library, the Trash, the Current Outfit,
  a protected folder or the Marketplace listings). A folder upload creates
  one new folder named after the disk folder, then its sub-folders that
  hold files (as the uploader's mirror), always new folders, never merged.
- **Permissions of the new items**: the uploads' defaults in Preferences
  (next owner, group, everyone), as the uploader sets them; a snapshot as
  the photo window's (next owner all). Preferences are never changed.

## Run and outcomes

- One run at a time per login (`busy`); the uploader's own queue is not
  touched: the assistant's run has its own requests, and the user's drafts
  stay where they are.
- **Folders first**, one acknowledged request at a time; a refused folder
  stops the run before any file is sent (the folders made stay).
- **Files one by one**: encoded on a worker from the frozen bytes, then
  `UploadAsset` with the frozen price. Before each send: same login, Upload
  not Never, access on, the destination still takes items, the price not
  higher, the balance enough.
- **Outcomes per file**: `done` (the item id), `failed` (the server's
  reason), `unknown` (sent, answer lost or not verified: never resent; look
  at the inventory), `not_sent` (the run stopped before it), `cancelled`.
  A failure or an unknown outcome stops the unsent files (no automatic
  retry, as Folderstorm (C++) and the uploader).
- **Receipt**: the operation's state is `done` when every file is done,
  `stopped` when some were sent and others not, `failed` when the first
  send was refused, `cancelled` when nothing was sent; its rows are the
  files (name, outcome), its `result` the item ids.
- **The picture**: a snapshot plan with an entry to set it on goes on, once
  its texture is uploaded, through the Image window's path
  (`inventory_set_thumbnail`'s); its outcome is a line of the status.
- **Revocation**: Upload set to Never, access turned off or a login change
  stops the unsent files; the upload already sent finishes and is recorded.

## Mesh review

The `.dae` is loaded into the uploader's Mesh window through its own loader
(a `// Folderstorm:` door beside PR #65's `add_folder_path`), which opens
it: LODs, physics, the server's fee quote and the Upload button are the
user's. The tool answers `opened: true`; it never uploads, never returns an
asset id, and never invents a price. Refused (`busy`) while the uploader
loads, uploads, confirms, or holds a mesh already. The importer may read
the textures the Collada file references beside it (local only, as for the
user).

## Creators' permissions

Uploads create new assets from the user's own files; no one else's asset
or asset id is read, shown or reused, and answers never carry an asset id
(the new items are named by their inventory id). The picture set from the
new texture goes through the Image window's path, which needs the target's
Modify as for the user.

## RLVa

No RLVa restriction covers uploads in the Legacy Viewer / RLVa; the upload
request passes through `send`, whose RLVa checks apply as for the
uploader. `@showinv` refuses the previews (they read inventory folders), as
every inventory read. The picture follows `camera_snapshot`'s rules (the
camera restrictions refuse moves, not pictures). `camera_get` reads what
the camera tools already return.

## Where the code goes

| Where | What |
|---|---|
| bridge `protocol/uploads.rs`, `protocol/camera_read.rs` (new) | the catalogues, bounds, required ids, early answer |
| bridge `policy.rs`, `binding.rs`, `protocol.rs`, `protocol/assistant.rs`, `protocol/inventory.rs` | `Class::Upload` and its migration; capabilities `uploads: 1`, `camera_read: 1`; hook lines |
| sidecar `jsonrpc.rs` | the instructions' paragraph |
| viewer `mcp_bridge/uploads.rs`, `uploads/scan.rs`, `uploads/demo.rs` (new) | plans, the worker scan and codecs, the run, status, cancel, the demo |
| `mcp_bridge/desk.rs`, `changes.rs`, `studio.rs`, `mcp_bridge.rs` | hook lines: reads, `Work::Upload`, ask rule, tick, clear, close, cancel, the app's entry points |
| `app/local_assistant.rs` | the sends (folders, uploads, the picture), the mesh review |
| `app.rs` | two calls: an upload's and a folder's answer |
| `imports/files.rs` | `add_mesh_path`, a door to the loader for the mesh review |
| Preferences page, catalogues | one row, the Ask sentences, en/fr/ja |

## Demo

`AURORA_DEMO_MCP=uploads`: Ask before changes; the demo writes a small
folder under `.scratch/` (two textures in a sub-folder, a sound), reads
`camera_get`, previews the folder, executes it (always asked: it costs the
demo's L$), then logs `inventory_upload_status`.
`AURORA_DEMO_MCP=camera-get` only logs `camera_get`.

## Tests

Catalogue (classes, required id, early answer, bound cancel); policy
(presets and migration); the scan (absolute paths, links refused, special
files, 32 files, 64 MiB, depth 8, unsupported types, names, hashes, sorted
order); prices (benefits, large textures, unknown price refused, 2048
option); plans (expiry, used once, 4 plans and the byte bound, login
change); execute (always asks above L$0, Allow at L$0 runs, balance,
higher price stale, folders then files, failure and unknown stop the rest,
cancel, Never mid-run, receipt states and rows, no asset id, no local path
in any receipt); snapshot plan (picture to plan, thumbnail needs Edit);
mesh review (busy, not a `.dae`); `camera_get`; end to end over a real
socket.

Not testable here: real prices and charges on a grid, the server's price
check, the uploader's Mesh window, the dialog's look.

## As built

Branch `claude/mcp-uploads`, on `claude/mcp-gestures` with
`claude/folder-upload` (PR #65) merged in: the PR shows #65's diff until #65
merges.

| Where | What |
|---|---|
| bridge `protocol/uploads.rs`, `protocol/camera_read.rs` (new) | the 7 tools, bounds, the execute's required id and early answer |
| `policy.rs`, `binding.rs` | `Class::Upload` (`upload`), presets and migration; capabilities `camera_read: 1`, `uploads: 1` |
| `protocol.rs`, `protocol/assistant.rs`, `protocol/inventory.rs` | `pub mod`, `all()`, `needs_client_request_id`, `answers_early`, `bound_group` (the cancel); the catalogue count |
| sidecar `jsonrpc.rs` | `UPLOADS_HINT` |
| viewer `mcp_bridge/uploads.rs`, `uploads/scan.rs`, `uploads/demo.rs` (new) | plans, the scan and codecs on workers, the run (`Act`s for the app), status, cancel, the snapshot plan, the mesh review's checks, the demo |
| `mcp_bridge/desk.rs`, `changes.rs`, `studio.rs`, `camera.rs`, `mcp_bridge.rs` | `Work::Upload`, `Work::MeshReview`; the freeze, the always-ask rule, the dialog's lines, the extra Edit class, cancel, clear, close; the studio hands a snapshot's PNG to the plans; `camera::get`; the app's entry points |
| `app/local_assistant.rs` | the account (prices, balance, sound limit, texture options), the sends, the mesh review |
| `app.rs` | two `// Folderstorm:` hooks: the assistant's upload answers and folder answers |
| `imports/files.rs` | `add_mesh_path` (one `// Folderstorm:` door beside #65's `add_folder_path`) |
| Preferences page, catalogues | one row, nine strings, en/fr/ja |

Upstream files touched, each by a small `// Folderstorm:` hook: `app.rs`
(two answer hooks), `imports/files.rs` (`add_mesh_path`).

Differences from the design above:

- The execute takes its plan when the desk first takes it (before its
  classes are decided, since a picture needs Edit too); a plan taken by an
  execute that is then refused in the dialog lasts the dialog's 60 s more,
  then goes.
- The snapshot's picture is the studio's PNG (no interface, not saved),
  read through the same scan as files; its default name is "Snapshot" and
  the date, its default folder the Photo Album, its kind a snapshot texture
  as the photo window's Save to inventory.
- The picture set from the new texture counts as done when the Image
  window's path ends without an error (60 s at most, then `failed` with
  "may still be set").
- `inventory_upload_status` also answers a plan not executed yet
  (`planned`, or `asking` while its execute is in the dialog).
- The receipt's rows map a refused upload to `unknown` (the receipts have
  no `failed` row outcome); the status gives the exact outcome per file.
