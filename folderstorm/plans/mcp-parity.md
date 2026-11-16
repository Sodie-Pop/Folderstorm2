# MCP tool parity: Folderstorm (C++) vs Folderstorm2 (Rust)

Updated 2026-11-15 against origin/main 061cf52 and open PRs #54, #56, #58, #61, #67.

Prepared 2026-11-11. Read-only research; nothing was built or pushed.

**Sources used**

- Folderstorm (C++): `tools/fs-mcp/internal/mcptools/*.go` (every registered tool, the most precise list), `tools/fs-mcp/README.md`, and `doc/plans/mcp-*.md`, `assistant-*.md`, `multi-character-mcp.md`, `windows-mcp-launcher-ownership.md`, `doc/help.md`.
- Folderstorm2 merged: `origin/main` at `061cf52` (tool catalogue in `crates/folderstorm-mcp-bridge/src/protocol.rs` and `protocol/*.rs`; the sidecar's own `viewer_list` / `viewer_select` in `crates/folderstorm-mcp/src/routing.rs`; the session host's `host_*` tools in `crates/folderstorm-mcp-bridge/src/controller.rs`; `folderstorm/plans/mcp*.md`; `folderstorm/FEATURES.md` rows 4.1 to 4.6). Merged MCP branches: mcp-camera, mcp-inventory-gaps, mcp-world-observation, mcp-session-routing (#45), mcp-receipts (#47), mcp-navigation (#49), mcp-content (#51), mcp-objects (#52).
- Folderstorm2 in progress (open PRs, stacked: each branch contains the one before it, so a tool is listed under the PR that adds it): #54 `claude/mcp-script-readback` (a fix to content saves, no new tool), #56 `claude/mcp-task-scripts`, #58 `claude/mcp-script-testing`, #61 `claude/mcp-building`, #67 `claude/mcp-gestures`. The viewer feature 2.11 (folder upload) is PR #65 `claude/folder-upload`, with no MCP tool yet.

## Summary

| | Count |
|---|---|
| Tools in Folderstorm (C++) | 171 (of which 2 are deliberate "never available" stubs: `inventory_empty_trash`, `inventory_purge`) |
| ✅ Done (merged on `origin/main`) | 77 |
| 🔧 In progress (open PRs #56, #58, #61, #67, and `claude/mcp-uploads`) | 42 |
| ⬜ Missing | 37 |
| ➖ Not applicable | 15 (8 shared-pose tools dropped by the human, 2 hard-denied stubs, 2 older-viewer confirm fallbacks, 1 controller discovery tool with no Folderstorm2 equivalent (unsure), 2 plain notecard / script reads superseded by `inventory_content_get`) |

Folderstorm2 ships 75 tool names on `main`: 65 in the viewer bridge catalogue (including `viewer_status`, the receipt tools `assistant_operation_status` / `assistant_operation_cancel`, and the camera tools), 2 sidecar routing tools (`viewer_list`, `viewer_select`) and 8 session-host tools (`host_*`). The open PRs add 36 more: 7 task-script tools (#56), 7 script-testing tools (#58), 8 building tools (#61) and 14 gesture and pose tools (#67); #54 adds none. Several C++ tools map onto one Folderstorm2 tool (for example the batch tools, because the move/copy/link/Trash tools already take 1 to 50 ids, and the monitor and session tools onto `host_*`), which is why the done count is not the same as the tool count. The 171 rows are counted once each: the texture-upload mode of `inventory_snapshot_upload` is a second row of the same tool and is not counted (its thumbnail mode is). Non-tool capabilities (presets, Ask dialogs, setup, diagnose, and so on) are listed in the last group and are **not** in these counts.

Per group (✅ / 🔧 / ⬜ / ➖): 1 Inventory and outfits 32/0/3/0 · 2 Camera 5/1/0/0 · 3 Navigation 5/0/0/0 · 4 World 6/0/0/0 · 5 Objects 9/0/0/0 · 6 Building 0/8/0/0 · 7 Content 6/0/0/2 · 8 Uploads 0/5/0/0 · 9 Gestures and posing 0/14/0/8 · 10 Preferences and workspaces 0/0/11/0 · 11 Social 0/0/16/0 · 12 Script testing 0/7/0/0 · 13 Task scripts 0/7/0/0 · 14 Session routing 12/0/7/1 · 15a Bridge tools 2/0/0/4.

**Legend:** ✅ done (merged) · 🔧 in progress · ⬜ missing · ➖ not applicable (reason given)

**Flags in the Notes column**

- ⚠ PERMS: touches creators' permissions (shows, copies, edits, exports or reuses someone's asset or asset id). Must go through the shared permission helpers (AGENT_RULES rule 3; read `AGENTS.md` § 2 "Creators' permissions" first).
- ⚠ L$: can spend money, or must quote a price and ask.
- ⚠ CRED: near credentials, tokens or login. Needs extra care (AGENT_RULES rule 1 and 2).

**Two prerequisites that apply to many rows**

1. Resolved on main: the exact login (`viewer_session`, #45) and per-request receipts (`client_request_id`, `operation_id`, `assistant_operation_status`, #47) now exist, with capability versions. Every new tool group plugs into them.
2. Several C++ MCP tools sit on top of a viewer feature that is not merged yet: undo history 2.5, safe link replacement 2.12, folder-preserving bulk upload 2.11 (now PR #65). Workspaces 1.4/1.5 and bulk review 2.4 are on main, but the workspace and preferences MCP tools are not started. Check `FEATURES.md` for the matching row before starting the tool.

---

## 1. Inventory and outfits

Folderstorm2 (merged): reads, create folder, rename, describe, move, copy, link, Trash, restore, wear and take off, `wait_for_bake`, and the inventory-gaps and receipts groups (`inventory_get_many`, `inventory_changes`, `inventory_create_item`, favorites, batch rename, preview and execute plans, thumbnails, outfit items). Folderstorm2 uses different tool names, and its move/copy/link/Trash tools take 1 to 50 ids, so C++ single and batch tools both map to the same tool.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `inventory_get` | Read one item or folder by UUID | ✅ `inventory_item` | Folder lookup is `inventory_list_folder`. ⚠ PERMS: `asset_id` is shown only if the viewer would show it (full permissions). Done to the stricter rule. |
| `inventory_get_many` | Read up to 50 items by UUID in one call | ✅ `inventory_get_many` | Up to 50 ids; ids not found are listed in `missing`. Same ⚠ PERMS rule as `inventory_item`. |
| `inventory_list` | List the direct children of a folder | ✅ `inventory_list_folder` | Takes a folder id or a path. |
| `inventory_search` | Search by name, description, creator, type, worn, permissions, favorite, date | ✅ `inventory_search` | Now has the C++ filters too: description, creator, worn, copyable, modifiable, favorite, created before/after, `filter_links`, `linked_id` (plus name, folder or path, type, library, trash). |
| `inventory_system_folder` | Find the UUID of a system folder (Textures, Objects, Landmarks...) | ✅ `inventory_system_folder` | Merged (inventory gaps). |
| `inventory_types` | List the folder and asset type names the viewer accepts | ✅ `inventory_types` | Merged (inventory gaps). Also lists system folder names, wearable types and creatable item types. |
| `inventory_resolve_path` | Return every UUID that matches a path (names are not unique) | ✅ via `path` arguments | An ambiguous path returns an error listing candidate ids instead of a separate tool. |
| `inventory_protected_folders` | List protected folders (read only) | ✅ `inventory_protected_folders` | Merged (inventory gaps): system, locked, pinned and Marketplace Listings folders, with the reason. |
| `inventory_changes` | Poll what changed since a generation number | ✅ `inventory_changes` | Merged (inventory gaps): generation cursor, `reset` when history is lost. |
| `inventory_create_folder` | Create a folder | ✅ `inventory_create_folder` | |
| `inventory_create_item` | Create a notecard, script, gesture, material, sky/water/day cycle, wearable or landmark | ✅ `inventory_create_item` | Merged (inventory gaps). Notecard, script, gesture, material, sky, water, day cycle, landmark of where the avatar stands. Class Edit. |
| `inventory_rename` | Rename an item or folder | ✅ `inventory_rename` | |
| `inventory_set_description` | Set an item or folder description | ✅ `inventory_describe` | |
| `inventory_set_thumbnail` | Point a folder or item thumbnail at an existing texture UUID | ✅ `inventory_set_thumbnail` | Merged (inventory gaps). ⚠ PERMS done: the texture must allow copy and transfer (as the Image window), links, Library and no-Modify items keep theirs. |
| `inventory_set_favorite` | Set or clear the favorite flag | ✅ `inventory_set_favorite` | Merged (inventory gaps). Library and Trash entries cannot be marked. |
| `inventory_link` | Make a link to an item or folder | ✅ `inventory_link` | |
| `inventory_replace_links` | Create replacement links first, then move old links to Trash | ⬜ | Depends on viewer feature 2.12 (safe link replacement), still ⬜. |
| `inventory_move` | Move one item or folder | ✅ `inventory_move` | Takes `ids` (1 to 50). |
| `inventory_batch_move` | Move up to 50 ids into one folder | ✅ `inventory_move` | Same tool. |
| `inventory_batch_rename` | Rename up to 50 items in one call | ✅ `inventory_batch_rename` | Merged (inventory gaps). Up to 50, all or nothing. |
| `inventory_copy` | Copy an item or folder, with a copy policy for no-copy items | ✅ `inventory_copy` | ⚠ PERMS: Folderstorm2 refuses items without Copy. The C++ policies `default` / `strict` / `copyable_only` and "move unique no-copy items after approval" are ⬜ (no copy policy argument on main). |
| `inventory_batch_copy` | Copy up to 50 items | ✅ `inventory_copy` | Same tool (`ids`). |
| `inventory_trash` | Move an object to Trash (restorable) | ✅ `inventory_trash` | |
| `inventory_restore` | Move an object out of Trash | ✅ `inventory_restore` | |
| `inventory_preview_batch_rename` | Dry-run a bulk rename and return a plan | ✅ `inventory_preview_batch_rename` | Merged with receipts (#47): plan of 1 to 50 rows (explicit names or a pattern), `plan_id` valid 5 minutes, no change made. |
| `inventory_preview_batch_move` | Dry-run a bulk move and return a plan | ✅ `inventory_preview_batch_move` | Merged with receipts (#47). |
| `inventory_execute_plan` | Run a previewed plan exactly once | ✅ `inventory_execute_plan` | Merged with receipts (#47): runs a plan once, planned again first (`stale` if anything changed), may ask once for the whole plan. Also on main, with no C++ twin named so: `assistant_operation_cancel`. |
| `inventory_history` | List this session's assistant inventory outcomes | ⬜ | Viewer feature 2.5 is still ⬜ in `FEATURES.md`; the plan receipts keep each row's before and after, which this would build on. |
| `inventory_preview_undo` | Prepare a limited undo of confirmed history rows | ⬜ | Same (2.5). |
| `appearance_outfits` | List outfit folders under My Outfits | ✅ `outfits_list` | Also marks the current base outfit. |
| `appearance_outfit_items` | List the items in one outfit folder with wearable type and worn flag | ✅ `appearance_outfit_items` | Merged (inventory gaps): links followed, name, type, wearable type, worn now. |
| `appearance_worn` | Show the Current Outfit, with attachment points and a "dirty" flag | ✅ `worn_items` | The "dirty" flag is replaced by `wait_for_bake`. |
| `appearance_wear_outfit` | Wear an outfit (replace or add) | ✅ `outfit_wear` | Ask dialog applies. |
| `appearance_wear_items` | Wear one or several items | ✅ `items_wear` | |
| `appearance_detach` | Take off or detach items | ✅ `items_take_off` | Folderstorm2 also has `outfit_take_off` (extra, no C++ tool). |
| (wait for the avatar to finish baking) | Wait until the outfit is baked before taking a picture | n/a in C++ (the C++ client polls `appearance_worn`) | Folderstorm2 has an extra `wait_for_bake`. |

## 2. Camera and snapshots

Camera rows are merged (`claude/mcp-camera`). The `camera` permission class sits in "Ask before changes".

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `camera_get` | Read the camera position, focus and distance | 🔧 `camera_get` (`claude/mcp-uploads`) | Read class, own capability `camera_read`; also the distance, field of view and the avatar's position. |
| `camera_set_pose` | Camera preset: portrait, full body, front, back, left, right | ✅ `camera_preset` | Same six presets. RLVa camera restrictions refuse it (`not_allowed`). |
| `camera_set` | Exact camera position and focus point (region coordinates) | ✅ `camera_shot` | Also accepts an offset from the avatar and a field of view. |
| `camera_reset` | Return to the normal third-person camera | ✅ `camera_reset` | |
| `camera_snapshot` | Take a picture of the window or the world view and return it | ✅ `camera_snapshot` | Size, crop, source, JPEG or PNG, optional save to the Photos folder. `interface: true` (shows chat and windows) always asks, even with Camera on Allow. Privacy: pictures can show private chat. |
| `inventory_snapshot_upload` (thumbnail) | Take a picture and set it as an outfit, folder or item thumbnail (free, 256 px) | ✅ `inventory_snapshot_thumbnail` | Needs Edit and Camera together. ⚠ PERMS (the result becomes inventory content). |
| `inventory_snapshot_upload` (texture) | Take a picture, upload it as a real inventory texture, then set it as thumbnail | 🔧 `inventory_snapshot_upload` (`claude/mcp-uploads`) | Two calls in Folderstorm2: the picture (Camera) becomes a one-file plan, then `inventory_upload_execute` (Upload, + Edit for the thumbnail) uploads it, always asking with the price above L$0. ⚠ L$: costs money. C++ rule: positive price always asks and shows the amount; a price increase needs a fresh confirmation; Allow uploads silently only at L$0. Explicitly "not in this part" in the Folderstorm2 plan. Needs Camera + Create + Edit together. |

## 3. Navigation

Merged (`claude/mcp-navigation`, #49). Excluded from C++ too: indirect teleport, autopilot, sit, touch.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `navigation_status` | Read location and the result of the last assistant-started move | ✅ `navigation_status` | Merged (#49). Read class (C++: opt-in World read). Location as `world_location`, `active`, `last` (outcome, reason, distance left), teleport wait; positions hidden under RLVa `@showloc`. |
| `navigation_move_to` | Walk or fly to a point in the same region (at most 64 m) | ✅ `navigation_move_to` | Merged (#49). New "Move my avatar" class (Ask in Ask before changes). Viewer's own autopilot; early answer with operation_id. Stops on timeout, manual input, another walk, teleport, login change, Never, access off, stop; reason reported. RLVa as the double click (`@tplocal`, `@sittp`, `@unsit`), `@fly` for flight. |
| `navigation_flying_set` | Take off or land | ✅ `navigation_flying_set` | Merged (#49). Fly button's rules (`can_takeoff`: no-fly land, RLVa `@fly`, sitting). |
| `navigation_teleport` | Submit one global-coordinate teleport | ✅ `navigation_teleport` | Merged (#49). Always asks (even on Allow); `client_request_id` required; never re-sent; one in flight, 15 s apart; RLVa at the send boundary; ends observed (arrived / at_target, failed, cancelled, unconfirmed after 60 s). |
| `navigation_stop` | Stop one assistant-started movement by its operation id | ✅ `navigation_stop` | Merged (#49). No class, bound to the login; stops only the assistant's own autopilot; a submitted teleport only gets a best-effort cancel (`cancel_requested`). |

## 4. World observation

Read-only, opt-in "World read" permission. Merged (`claude/mcp-world-observation`), read-only through the RLVa filters.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `world_location` | Region, position and facing of this login | ✅ `world_location` | Native visibility restrictions (RLVa) apply. |
| `world_parcel` | Current parcel and a few read-only flags | ✅ `world_parcel` | |
| `world_nearby_avatars` | Bounded list of nearby avatars | ✅ `world_nearby_avatars` | Names can be unknown. RLVa can refuse the whole list. Privacy-sensitive. |
| `world_nearby_objects` | Nearby root objects with ids and positions | ✅ `world_nearby_objects` | No contents, no properties. |
| `world_audio_sources` | Recent sounds heard and where they came from | ✅ `world_audio_sources` | |
| `world_media_sources` | Parcel and object media flags (URLs omitted) | ✅ `world_media_sources` | |

## 5. Object inspection and lifecycle

All "owned objects only" in C++: owned, modifiable prims. Merged (`claude/mcp-objects`, #52), with exact snapshots and request ids. `object_rez_identify` answers `not_found` on main until the rez tools of #61 call `Objects::track_rez` (that call is on the building branch).

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `object_contents_list` | List the contents of one owned prim (with a 120-second snapshot id) | ✅ `object_contents_list` | Merged (#52). Read class; owner checked through the server's properties; fresh server listing; 120 s snapshot, 16 per login; names only, through the RLVa filter; no asset ids. |
| `object_contents_put` | Copy an owned inventory item (script, object, notecard, sound, texture) into a prim | ✅ `object_contents_put` | Merged (#52). ⚠ PERMS done: own item, Copy required (stricter than the Contents tab), `contents_drop::accepts`; script stopped (RezScript enabled false); class Objects; required `client_request_id`; confirmed only when the new task item's asset equals the source's (compared in the viewer only), else unconfirmed with candidates. |
| `object_contents_reconcile` | Link an unconfirmed copy receipt to the item that appeared in the prim | ✅ `object_contents_reconcile` | Merged (#52). associated_unverified, immutable, sends nothing; refuses an item present before, of another type, or with a different known asset. |
| `object_contents_rename` | Rename an item inside a prim | ✅ `object_contents_rename` | Merged (#52). task item Modify (`task_agent_mask`); confirmed by the next listing. |
| `object_contents_remove` | Permanently remove an item from a prim (no Trash) | ✅ `object_contents_remove` | Merged (#52). Always asks (even on Allow); Copy and Modify required (stricter than the Contents tab); class Objects (no separate class: the dialog shows every removal). |
| `object_operation_status` | Read the receipt of a contents or script operation | ✅ `object_operation_status` | Merged (#52). Read class; receipt plus the facts seen (task item, candidates, association, arrival, removal). |
| `object_take` | Take an owned object into inventory (removes it from the world) | ✅ `object_take` | Merged (#52). Always asks; class Take; root, owned, modify, move, nobody seated, not worn, RLVa @rez; outcome: object item arrived in the folder (by folder, type, name: `item_id_confirmed: false`) and the object gone from the world. |
| `object_take_copy` | Take a copy of an owned object into inventory | ✅ `object_take_copy` | Merged (#52). ⚠ PERMS: OBJECT_COPY required; class Take, follows its level. |
| `object_rez_identify` | Link a tracked rez receipt to a nearby new object the assistant chose | ✅ `object_rez_identify` | Merged (#52). Checks built and tested (new owned root absent from the rez baseline, immutable, `chosen`, `object_id_confirmed: false`); no rez tool exists yet, so every call answers not_found until the building group calls `Objects::track_rez`. |

## 6. Building

All on PR #61 `claude/mcp-building` (stacked on #56 and #58), not merged.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `build_object_get` | Read an owned root object with a 120-second revision token | 🔧 #61 `claude/mcp-building` | |
| `build_object_transform` | Move, rotate or resize an owned object | 🔧 #61 `claude/mcp-building` | Owned, modifiable, movable, non-physical root only. |
| `build_object_surface` | Change texture, colour, etc. on faces | 🔧 #61 `claude/mcp-building` | ⚠ PERMS: inventory textures need full owner permissions. |
| `build_object_material` | Edit materials and PBR overrides | 🔧 #61 `claude/mcp-building` | ⚠ PERMS: same rule as textures. Material assignment only; PBR overrides left to native Build (as C++). |
| `build_rez_primitive` | Rez a cube, sphere, cylinder or cone | 🔧 #61 `claude/mcp-building` | Buildable parcel, within 64 m. Does not guess the new UUID. |
| `build_rez_inventory` | Rez a copyable inventory object into the world | 🔧 #61 `claude/mcp-building` | ⚠ PERMS (needs Copy). |
| `inventory_permissions_get` | Read an owned item's permission masks and revision | 🔧 #61 `claude/mcp-building` | ⚠ PERMS. |
| `inventory_next_owner_set` | Change next-owner copy / modify / transfer flags | 🔧 #61 `claude/mcp-building` | ⚠ PERMS: changes creators' rights flags. Uses native Properties rules. Highest care. Always asks; object items refused for now (slam flag question). |

## 7. Content editing

Notecard and script reads and edits in the agent's own inventory (the earlier C++ reads were hardened and are listed here).

Merged (#51, `claude/mcp-content`), plan `folderstorm/plans/mcp-content.md`. A follow-up fix, PR #54 `claude/mcp-script-readback` (script read-back after a save), is open and adds no tool. New permission class **Content** (saves need Content and Read), capability `content: 1`, snapshot hash instead of the C++ `expected_asset_id` (no asset id is ever given), required `client_request_id` on saves. Demo `AURORA_DEMO_MCP=content`.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `inventory_read_notecard` | Read notecard text (capped at 64 KiB) | ➖ superseded | Folderstorm2 has only `inventory_content_get` (Folderstorm (C++) superseded the plain reads with it too). ⚠ PERMS: Copy needed. |
| `inventory_read_script` | Read script source (capped) | ➖ superseded | Same: `inventory_content_get`. ⚠ PERMS: Copy and Modify (Copy for a Library script). |
| `inventory_landmark` | Read a landmark's region name and position | ✅ (inventory gaps, #25) | Cheap read. |
| `inventory_content_get` | Read full notecard or script text with a versioned edit snapshot | ✅ `inventory_content_get` | Merged (#51). ⚠ PERMS: the editors' rules through `actions::held`, no god level; text is data (a `note` says so); embedded items listed by name and type, never ids; `snapshot_hash` = salted SHA-256 of item, asset, kind and text; scripts over 64 KiB `complete: false`, not editable; v1 notecards with items and orphan records not editable. |
| `inventory_content_preview` | Preview a diff of a planned edit | ✅ `inventory_content_preview` | Merged (#51). Read class; counts, unified diff (16 KiB), problems a save would refuse, `save` allow/ask/never, `editor_open`. |
| `inventory_update_notecard` | Replace the text of an owned notecard, keeping embedded items | ✅ `inventory_update_notecard` | Merged (#51). ⚠ PERMS: Copy and Modify, owned, not Library/Trash/Marketplace, no editor open (`busy`). Markers kept in order, records byte for byte. `stale` when the asset changed (checked at take and just before the upload). Content + Read classes; Ask dialog with name, counts and changed lines. |
| `inventory_save_compile_script` | Save raw LSL to an owned script and compile (Mono) | ✅ `inventory_save_compile_script` | Merged (#51). ⚠ PERMS as above, LSL only (SLua refused). `source_saved` and `compiled` / `compile_checked` / `errors` (line, column from 1; 64 max, 8 KiB) are separate. Mono only; not running; nothing rezzed or reset. Open fix 🔧 #54 (script read-back after a save). |
| `inventory_content_operation_status` | Read what happened after a save or timeout | ✅ `inventory_content_operation_status` | Merged (#51). Read class; by `operation_id` or `client_request_id`, this login only; `source_saved` true/false/null; refused if the item is no longer readable. |

## 8. File uploads

Local files only, on the machine running the viewer. No remote paths or URLs. Built on `claude/mcp-uploads` (stacked on #67 with PR #65 merged in), with its own runner on the uploader's codecs, prices and upload request; new class "Upload local files".

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `inventory_upload_preview` | Check files or a folder, quote the cost, return a plan (120 s) | 🔧 | ⚠ L$: quotes per-file price. Limits: 32 files, 64 MiB each. Depends on viewer feature 2.11. |
| `inventory_upload_execute` | Upload the previewed plan | 🔧 | ⚠ L$ and ⚠ PERMS. Always asks if total is above L$0. Separate "Upload local files" permission. |
| `inventory_upload_status` | Check progress and outcomes | 🔧 | |
| `inventory_upload_cancel` | Stop unsent uploads | 🔧 | Cannot undo accepted uploads or charges. |
| `inventory_upload_mesh_review` | Open the native mesh importer for a Collada file (no upload) | 🔧 | ⚠ L$: the importer shows the fee. Resident finishes it by hand. |

## 9. Gestures and posing

Shared posing was dropped by the human on 2026-11-10 (`FEATURES.md`, "Not porting"). The rest is on PR #67 `claude/mcp-gestures` (stacked on #61), not merged.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `gesture_list` | List active gestures | 🔧 `gesture_list` (#67 `claude/mcp-gestures`) | Read class; active gestures only (others through `inventory_search`); names and triggers as data. |
| `gesture_status` | Inspect one gesture and its animations (asset hash) | 🔧 `gesture_status` (#67 `claude/mcp-gestures`) | ⚠ PERMS done: own item, Copy (stricter than the editor), not Library/Trash; loaded through the window's path, never activated; steps by name and kind, salted `revision` instead of the C++ asset id and hash (no asset id ever given). |
| `gesture_play` | Play an inspected gesture (asks) | 🔧 `gesture_play` (#67 `claude/mcp-gestures`) | New class Gestures (+ Read); required `client_request_id`; under the assistant's own key (Preview path), no activation; 64 steps, one at a time, 120 s; a gesture with a chat line always asks and lists its lines (no chat class yet: review with group 11); viewer-command lines and lines RLVa would block/change refuse it; `@sendgesture`. |
| `gesture_stop` | Stop an assistant-started gesture | 🔧 `gesture_stop` (#67 `claude/mcp-gestures`) | Releases only animations no other playback holds (`Manager::stop_owned`); revocation, login change, access off, teleport and 120 s stop it too. |
| `pose_status` | Read the assistant's own pose session | 🔧 `pose_status` (#67 `claude/mcp-gestures`) | Read class; no `target`: own avatar only (the sidecar pins the login). |
| `pose_joints` | Read joint rotations of the own avatar | 🔧 `pose_joints` (#67 `claude/mcp-gestures`) | Rotations relative to each joint's rest; the session's or the avatar's pose now. |
| `pose_presets` | List pose presets that are rotation-only | 🔧 `pose_presets` (#67 `claude/mcp-gestures`) | Hand presets + the poser's library files (64, 64 KiB, worker thread), opaque ids, no path; loadable only rotation-only. |
| `pose_start` | Start a private rotation-only pose session | 🔧 `pose_start` (#67 `claude/mcp-gestures`) | New class Pose (+ Read); local only (the scene's poser layer); refused while the viewer's poser poses you, ended if it takes over. |
| `pose_update` | Apply rotations to up to 64 joints | 🔧 `pose_update` (#67 `claude/mcp-gestures`) | Integer revision; not the pelvis; unit quaternions (0.002). |
| `pose_undo` | Undo the last rotation change | 🔧 `pose_undo` (#67 `claude/mcp-gestures`) | 64 steps. |
| `pose_reset` | Release the joint overlay | 🔧 `pose_reset` (#67 `claude/mcp-gestures`) | Back to the captured pose; history kept. |
| `pose_stop` | Stop this source's pose motion | 🔧 `pose_stop` (#67 `claude/mcp-gestures`) | Ends the session; teleport, login change, access off, Pose on Never end it too. |
| `pose_load` | Load a rotation-only preset | 🔧 `pose_load` (#67 `claude/mcp-gestures`) | Applies the listing's frozen content (`Document::apply`, rotations mode). |
| `pose_save` | Save the overlay as a new pose file | 🔧 `pose_save` (#67 `claude/mcp-gestures`) | Always asks; required `client_request_id`; `create_new` (never an overwrite), confined name; the poser's own format. |
| `pose_shared_sessions` | List nearby shared-pose sessions | ➖ | Shared poser dropped 2026-11-10. |
| `pose_shared_grants` | List shared-pose editing grants | ➖ | Same. |
| `pose_shared_joints` | Read another avatar's shared-pose joints | ➖ | Same. |
| `pose_shared_update` | Edit another avatar's pose with their consent | ➖ | Same. |
| `pose_shared_cancel` | Cancel a shared-pose edit | ➖ | Same. |
| `pose_shared_operation_status` | Read a shared-pose receipt | ➖ | Same. |
| `pose_start_sharing` | Publish your own pose to others | ➖ | Same. |
| `pose_stop_sharing` | End sharing your pose | ➖ | Same. |

## 10. Viewer preferences (and workspaces)

Workspace tools are not in your list of groups; they are grouped here because they change viewer settings. The viewer's workspaces (1.4, 1.5) are on main (checkpoint 5), but no `preferences_*` or `workspace_*` MCP tool exists on main or on an open branch.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `preferences_schema` | Read the closed list of curated settings, types and ranges | ⬜ | Never exposes arbitrary debug settings, permissions, credentials or paths. |
| `preferences_get` | Read effective, saved, default and unsaved values (up to 16) | ⬜ | Opt-in read. |
| `preferences_status` | Read the current preview and its cleanup state | ⬜ | |
| `preferences_preview` | Try up to 16 settings for at most 300 s | ⬜ | Saved values unchanged until accept. |
| `preferences_accept` | Save the previewed settings | ⬜ | |
| `preferences_cancel` | Discard the preview, restore previous values | ⬜ | |
| `workspace_list` | List saved workspaces and layouts | ⬜ | Depends on viewer features 1.4 to 1.7. |
| `workspace_status` | Active workspace, Previous availability, restore report | ⬜ | |
| `workspace_preview` | Show what switching would change (no change) | ⬜ | |
| `workspace_apply` | Switch to a workspace or layout | ⬜ | Asks. Workspaces with camera or worn-HUD groups also need Camera and Wear approval. |
| `workspace_previous` | Return to the previous arrangement | ⬜ | |

## 11. Social and events

**Shelved by the human (2026-11-15): very low priority, maybe one day. Do not start.**

All chat text, names, profiles and notifications are untrusted data in C++ (AGENT_RULES rule 4). Privacy-sensitive: each read is a separate opt-in. Chat sending is a network effect on other people.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `chat_conversations` | List open conversations (no mark-read, no focus) | ⬜ | Opt-in chat read. |
| `chat_history` | Read retained chat messages of one conversation (live login only) | ⬜ | Never reads disk logs. |
| `chat_search` | Search retained chat text | ⬜ | |
| `friends_list` | List friends, online first | ⬜ | Opt-in friends read. |
| `resident_search` | Search residents by name (max 100) | ⬜ | |
| `resident_profile` | Read a public profile by UUID | ⬜ | Private notes excluded. |
| `chat_open_im` | Open an IM to a resident (no send) | ⬜ | |
| `chat_draft_get` | Read the assistant's own draft | ⬜ | |
| `chat_draft_set` | Replace the assistant's draft (no send) | ⬜ | |
| `chat_send` | Send frozen text to one IM, group, conference or nearby chat | ⬜ | Asks. 1023 bytes, 1 per second. Speaks for the user to other people. |
| `chat_mark_read` | Mark one IM read | ⬜ | |
| `chat_close` | Hide a conversation from assistant views | ⬜ | |
| `events_poll` | Poll chat, contact and notification events with a cursor | ⬜ | Independent cursor per client. |
| `notifications_list` | List active offers and notices | ⬜ | |
| `notifications_get` | Read one notification | ⬜ | |
| `notification_review` | Open the native UI to review one notification | ⬜ | Never accepts inventory, permissions or teleport offers by itself. |

## 12. Script testing

For testing scripts the user owns, on owned test prims. All on PR #58 `claude/mcp-script-testing` (stacked on #56), not merged.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `object_touch` | Touch an owned prim (within 64 m) | 🔧 #58 `claude/mcp-script-testing` | Asks. |
| `object_dialog_list` | List live game dialogs from an owned prim | 🔧 #58 `claude/mcp-script-testing` | |
| `object_dialog_reply` | Answer a live game dialog by button or text | 🔧 #58 `claude/mcp-script-testing` | Never answers permission, payment, teleport or inventory offers. ⚠ L$: must never answer a payment request. |
| `object_channel_send` | Say text on a script channel (6 per minute) | 🔧 #58 `claude/mcp-script-testing` | Broadcast to everyone listening. |
| `script_test_begin` | Start a frozen test recipe (up to 32 prims, 64 steps, 600 s) | 🔧 #58 `claude/mcp-script-testing` | One approval for all steps. |
| `script_test_status` | Read test progress and assertions | 🔧 #58 `claude/mcp-script-testing` | |
| `script_test_stop` | Cancel unsent steps | 🔧 #58 `claude/mcp-script-testing` | Works even after permissions are revoked. |

## 13. Task scripts (scripts inside objects)

All on PR #56 `claude/mcp-task-scripts`, not merged. PR #54 (a fix for content saves) is related but adds no tool.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `object_script_get` | Read an installed script's source and hash | 🔧 `object_script_get` (#56 `claude/mcp-task-scripts`) | ⚠ PERMS (Copy and Modify, `task_agent_mask`). Read class; owned modifiable prims only; editor's `LoadScript` + Experience metadata; snapshot hash, no asset id; SLua or unknown Experience: readable, not editable. |
| `object_script_preview` | Preview an edit against the installed source | 🔧 (#56 `claude/mcp-task-scripts`) | Read class; diff, problems, save decision. |
| `object_script_save_compile` | Save and compile Mono LSL in an installed script | 🔧 (#56 `claude/mcp-task-scripts`) | ⚠ PERMS (Copy and Modify). Classes Content + Objects (+ Read; + Scripts with `running: true`); Experience association sent back unchanged (unknown: not editable); source_saved and compiled apart; upload's `new_asset` as the new snapshot (lagging listings ignored); `client_request_id` required. |
| `object_script_status` | Query running state | 🔧 (#56 `claude/mcp-task-scripts`) | Read class; `GetScriptRunning`, 20 s. |
| `object_script_set_running` | Start or stop a script | 🔧 (#56 `claude/mcp-task-scripts`) | New class Scripts ("Run my objects' scripts", Ask in Ask before changes; not always-ask); script's Modify; done when the state reply matches. |
| `object_script_reset` | Reset a script | 🔧 (#56 `claude/mcp-task-scripts`) | Class Scripts; `reset_confirmed: false` always (grid has no ack). |
| `script_debug_poll` | Read runtime errors and owner-say output (cursor) | 🔧 (#56 `claude/mcp-task-scripts`) | Read class; `owner_id` == agent; captured in `World::on_chat` before the Script debug routing (one upstream hook); 1,024 lines / 1 MiB, gaps counted; cleared on access off, Read Never, login change. |

## 14. Session routing and multi-character

Merged with #45 (`claude/mcp-session-routing`): `viewer_list` and `viewer_select` in the sidecar (plus `--profile`), and the session host's own MCP controller as eight `host_*` tools (list, activate, background mode, monitors, login window, restart, log out, stop managing), behind a separate permission the user turns on in the host's settings. They never log anyone in. Where a C++ tool has no matching call the row stays ⬜ or is marked unsure.

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `viewer_list` | List running viewers, with an exact login reference (`viewer_session`) | ✅ `viewer_list` | Merged (#45): answered by the sidecar itself, lists every viewer within scope with its profile and an exact login reference (`viewer_session`). |
| `viewer_select` | Choose one viewer when several run | ✅ `viewer_select` | Merged (#45): by profile, login or process; calls then stay with that character and are never sent to another (`target_required` / `session_changed` otherwise). `--profile` in the sidecar does the same up front. |
| `controller_list` | Discover the session-host controller | ➖ | (unsure: no tool of that name. The sidecar finds the session host's controller itself and pins a `host_session`; the `host_*` tools need the host's MCP controller turned on in its settings. ⚠ CRED: separate authenticated controller endpoint.) |
| `sessions_list` | List host profiles (including stopped ones) | ✅ `host_characters` | Merged (#45): slot, profile, state (stopped, starting, login, loading, in_world, closing, ended, crashed, unmanaged), active, background mode, launch number. Names and colours only when the user allows profile metadata. |
| `session_status` | Read one profile's state | ✅ `host_characters` | (unsure: `host_characters` lists every character; there is no single-profile read.) |
| `controller_operation_status` | Read a controller request receipt | ⬜ | (unsure: the `host_*` calls answer in one round trip, with no receipt or operation id on main.) |
| `session_activate` | Make one character the active one | ✅ `host_activate` | Merged (#45). The user may be asked first; host class Activate. |
| `session_background_mode` | Warm or Economy mode for an inactive character | ✅ `host_background_mode` | Merged (#45). warm or economy. |
| `session_open_login` | Open the native login window of a stopped profile | ✅ `host_open_login` | Merged (#45). ⚠ CRED: never logs in, never fills or sends anything; the user logs in. |
| `session_restart` | Close, release and reopen login for a profile | ✅ `host_restart` | Merged (#45). ⚠ CRED: the user logs in again. |
| `session_logout` | Ordinary logout of one character | ✅ `host_logout` | Merged (#45). |
| `session_detach` | Stop managing a character (does not log out) | ✅ `host_stop_managing` | Merged (#45): the viewer goes on by itself, logged in. |
| `monitor_open` | Open a read-only preview of an inactive character | ✅ `host_monitor` | Merged (#45): `open: true` with `profile`. |
| `monitor_close` | Close a preview | ✅ `host_monitor` | Merged (#45): `open: false`. |
| `monitor_list` | List previews and cycle state | ⬜ | (unsure: `host_characters` shows monitor details only when the user allows monitor metadata; no cycle state is reported.) |
| `monitors_open_all` | Open previews of all eligible characters | ⬜ | (unsure: `host_monitor` takes one profile or the cycling monitor; no open-all call.) |
| `monitors_arrange` | Arrange preview windows in a grid or row | ⬜ | No arrange call on main. |
| `monitor_cycle_configure` | Set up a cycling preview | ⬜ | (unsure: `host_monitor` with `cycling: true` opens or closes the cycling monitor; it has no configuration.) |
| `monitor_cycle_pause` | Pause the cycling preview | ⬜ | |
| `monitor_cycle_next` | Advance the cycling preview | ⬜ | |

## 15. Bridge, permissions and setup

### 15a. Tools in this group (counted)

| Capability / tool | What it does (one line, plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| `viewer_status` | Health check: login, inventory ready, fetch progress, change generation | ✅ `viewer_status` | Needs a valid token but no permission class, so Check connection works with Read on Never. |
| `assistant_operation_status` | Read the receipt of a slow create or outfit change | ✅ `assistant_operation_status` | Merged with receipts (#47): state (waiting, asking, queued, running, done, failed, refused, cancelled, stopped, stale, unconfirmed), by `operation_id` or `client_request_id`; without arguments the newest 20 on this login. |
| `confirm_action` | Older-viewer fallback: confirm a stored wear or link plan | ➖ | Folderstorm2 has no older viewers; Ask dialogs are always in the viewer. |
| `inventory_confirm_copy` | Older-viewer fallback: confirm a no-copy move plan | ➖ | Same. |
| `inventory_empty_trash` | Permanently empty Trash (hard-denied, never works) | ➖ | Deliberately refused in both. Folderstorm2 refuses `inventory_delete`, `inventory_purge`, `empty_trash` with `permission_denied` whatever the levels. |
| `inventory_purge` | Permanently delete one object (hard-denied, never works) | ➖ | Same. |

### 15b. Non-tool capabilities (not in the counts)

| Capability | What it does (plain words) | Folderstorm2 status | Notes |
|---|---|---|---|
| Local assistant bridge, off by default | Loopback-only (127.0.0.1) listener; nothing runs until enabled | ✅ | `--mcp-api` forces on for one session without saving. FEATURES 4.1. |
| Bearer token and discovery file | Per-launch random token in a `0600` file; sidecar finds the viewer from it | ✅ | ⚠ CRED: token never logged, shown, copied or printed. Folderstorm2 file is `folderstorm2-mcp-<pid>.json`. |
| Browser and DNS-rebinding defence | Refuse `Origin` and wrong `Host` headers; size and time limits | ✅ | |
| Sidecar program (stdio MCP server) | `fs-mcp` in C++ (Go); `folderstorm-mcp` in Folderstorm2 (Rust, no SDK) | ✅ | FEATURES 4.2 says "packaging beside the viewer to do". |
| Permission classes with Allow / Ask / Never | One level per kind of action | 🔧 | Merged: read, edit, move, wear, trash, camera, navigation, content, objects, take, plus the host's own controller classes (activate, background mode, monitors, open login, restart, log out, stop managing). Open PRs add Scripts (#56), Gestures and Pose (#67), and Upload (`claude/mcp-uploads`). C++ also has: create, nocopy (move no-copy items), links, uploads, chat read/state/send, friends, profile, notifications, preferences, workspaces. Each new tool group adds its own. |
| Presets: Read only, Ask before changes, Custom | One-click permission sets; Custom shows every row | ✅ | Derived from the levels, never stored. Camera and Navigation ("Move my avatar") are in "Ask before changes". |
| "Use these settings for this session only" | Session copy in memory, saved settings return at restart | ✅ | |
| Ask dialogs (60 s, refuse on timeout) | Viewer dialog names the program and says what it wants to change | ✅ | Neither button is the default. C++ approval text is richer (names with folder context, counts, omitted rows) and re-checks queued asks when permissions change; confirm Folderstorm2 matches that (⬜ unsure). |
| Permanent delete never possible | No setting enables purge or empty Trash | ✅ | |
| Creators' permissions on `asset_id` | Show asset ids only if the viewer would | ✅ | Folderstorm2 is stricter (no demo exception; RLVa `@showinv` blocks every read). |
| Setup page: Copy configuration | Codex (TOML), Cursor and Claude Code (JSON) entries; no token copied | ✅ | FEATURES 4.5. |
| Check connection | Runs the real sidecar against this viewer; token-free result | ✅ | |
| `--diagnose` | Terminal check with fixed failure stages, JSON, exit codes 0/1/2 | ✅ | |
| Recent assistant activity in Preferences | Tool name and outcome only, never arguments or results | ✅ | Layout fix merged with the camera branch. |
| Per-request receipts (`client_request_id`, `operation_id`) | Safe retry: same id returns the original receipt; uncertain writes are never replayed | ✅ | Merged (#47): required `client_request_id` on changes, `operation_id`, `assistant_operation_status` / `assistant_operation_cancel`; the same id returns the original receipt and an uncertain write is never replayed. |
| Exact login binding (`viewer_session`) | Every account tool is pinned to one exact login, not a PID | ✅ | Merged (#45): `viewer_session` pinned at the first call, `session_changed` after a logout or another login; `viewer_list` / `viewer_select` and `--profile` pick the character. |
| Capability versions (for example `navigation_tools: 1`) | Tools appear only if viewer and sidecar both advertise the capability | ✅ | Merged (#45, #47): `binding::CAPABILITIES` per group (inventory, login_binding, camera, inventory_gaps, world, sessions, receipts, plans, operations, navigation, content, objects); a tool of a missing capability answers `unsupported`. |
| Async guarded native operations | Long actions (create item, wear) keep their authority after the HTTP reply; cancelled on logout, relogin, revoke; no replay | ✅ | Re-planned as the receipts and operations of #47 (waiting, asking, queued, running, done, failed, refused, cancelled, stopped, stale, unconfirmed; cancelled on logout, relogin, revoke; no replay). Not a copy of the C++ plan. |
| Windows launcher ownership | Each install's own `fs-mcp.exe` is the command; no shared ProgramData alias | ➖ | `FEATURES.md` "Not porting" lists the Windows MCP link cleanup utility. Folderstorm2 Copy configuration already uses the actual executable. Revisit with 0.3 and 4.2 (packaging). |
| Client timeout setting (`FIRESTORM_MCP_TIMEOUT`) | Let the sidecar's wait match a longer client timeout | ⬜ | Unsure it is needed; Folderstorm2 has its own timeout table per tool. |
| Presets with explicit descriptors (E0 rule) | New private reads must never be silently opted in by a read-only preset | ⬜ | Rule to apply when social and preference reads arrive (world reads are merged; check how they are preset). |

---

## Suggested porting order (groups)

Done (merged on main): camera and snapshots (except `camera_get` and the texture upload), inventory gaps, receipts and capability handshake, navigation, world observation, content editing, object inspection and lifecycle, session routing (`viewer_list`, `viewer_select`, `host_*`).

In progress (open PRs, stacked, merge in this order): #54 content read-back fix, #56 task scripts, #58 script testing, #61 building (also wires `object_rez_identify` to the rez tools), #67 gestures and own posing. `inventory_next_owner_set` (in #61) is the highest-care tool: review it last.

Left, in a sensible order:

1. **Small leftovers of finished groups**: `camera_get`; the inventory copy policies (`default` / `strict` / `copyable_only`, no-copy moves after approval); `inventory_replace_links` (needs viewer feature 2.12); `inventory_history` and `inventory_preview_undo` (need viewer feature 2.5).
2. **Camera texture upload** (`inventory_snapshot_upload`, texture mode): ⚠ L$, asks with the price; needs Camera + Create + Edit together. Can share the upload design with group 3.
3. **File uploads** (5 tools, ⚠ L$ and ⚠ PERMS): build on viewer feature 2.11 (PR #65); merge that first. Separate "Upload local files" permission.
4. **Viewer preferences** (6 tools: curated catalogue, preview, accept, cancel) and **workspace tools** (5 tools; the viewer's workspaces 1.4 / 1.5 are on main already).
5. **Social and events** (16 tools; privacy and untrusted-text rules; each read a separate opt-in; plan separately). Last of the large groups.
6. **Session host extras** (7 rows still ⬜ or unsure: `controller_operation_status`, `monitor_list`, `monitors_open_all`, `monitors_arrange`, and the three cycling-monitor rows): only if the human wants them; `host_*` already covers the rest.

## Capabilities I was unsure how to classify

- `controller_list`: marked ➖ with an unsure note. No tool of that name; the sidecar finds the controller itself. Change to ✅ if the human counts that discovery as parity.
- `session_status`: marked ✅ on `host_characters`, which lists every character, but there is no single-profile read.
- `controller_operation_status`: marked ⬜; the `host_*` calls are synchronous with no receipt on main. May be ➖ if receipts are not wanted for host calls.
- `monitor_list`, `monitors_open_all`, `monitor_cycle_configure`: marked ⬜; `host_characters` shows monitor details only with metadata permission, `host_monitor` takes one profile or the cycling monitor and only opens or closes it. A looping client could cover open-all.
- `appearance_outfit_items`: now ✅ (`appearance_outfit_items` is on main with the worn flag, which settles the earlier doubt).
- `inventory_resolve_path`: marked ✅ because paths are accepted everywhere and an ambiguous path returns the candidate ids; a dedicated tool does not exist.
- `inventory_empty_trash` and `inventory_purge`: counted as ➖ because the C++ tools exist only to say "not available" and Folderstorm2 refuses the same actions.
- Workspace tools: placed under "Viewer preferences" because they are not in your list of groups.
- Ask dialog contents and "re-check queued asks when permissions change": not verified in Folderstorm2 code; listed as unsure.
- `inventory_snapshot_upload`: the thumbnail mode (✅, `inventory_snapshot_thumbnail`) is counted; the texture mode is a separate ⬜ row that is not counted as a second tool.
- Stacked PRs: each of #58, #61 and #67 contains the earlier branches' commits, so a tool is attributed to the PR that introduces it, not to every branch that contains it.
- Folderstorm2 extras with no C++ equivalent: `outfit_take_off`, `wait_for_bake`, `assistant_operation_cancel`.
