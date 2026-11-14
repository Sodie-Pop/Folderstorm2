# Local assistant (MCP): building

Group 6 of the MCP parity checklist (FEATURES.md 4.6): the assistant reads
and edits **objects the user owns** (move, turn, resize, a face's colour
and texture, a face's PBR material), rezzes a primitive or a copyable
object from the inventory on land where the user may build, and reads and
changes an inventory item's next-owner rights. Built on
[mcp-objects.md](mcp-objects.md) (which objects, RLVa, `client_request_id`,
receipts, `object_rez_identify`).

Sources, read as information: Folderstorm (C++)'s `doc/plans/mcp-building.md`,
`tools/fs-mcp/internal/mcptools/building.go` and the native listener
`indra/newview/fsbuildlistener.cpp` (names, arguments, bounds, rules). The
viewer's own paths reused: `aurora_net::build::BuildCmd` (`Transform` =
MultipleObjectUpdate, `SetTextures` = ObjectImage, `Add` = ObjectAdd as
`rezplat` sends it, `RezInventory` = RezObject as the inventory drag
packs it, `Cap` = ModifyMaterialParams as `build/materials.rs` posts it),
`build::shapes`, `World::build_right_at` / `agent_can_build`
(LLViewerParcelMgr::allowAgentBuild), `World::group_for_rez`,
`world::inventory::actions` (`agent_mask`, `next_owner_editable`,
`mutable`, `patch`: the Properties window's rules and its save).

## Tools (capability `building: 1`)

| Tool | Class | Asks | What it does |
|---|---|---|---|
| `build_object_get` | Read | – | One root prim the user owns and may modify: region position, rotation `[x,y,z,w]`, scale, flags (physical, phantom, temporary, may move, may copy), linked prims count, up to 64 faces (colour, glow, fullbright, repeats, offset, rotation, `texture` as `default` / `blank` / `transparent` / `other`, `pbr_material` true/false). Returns a `revision` valid 120 s. Never an asset id. |
| `build_object_transform` | Build (+ Read) | level | Move, turn or resize that root at its revision: `position` (region metres), `rotation` (normalised quaternion), `scale`; at least one. A linkset moves and turns as one; resizing a linkset is refused (native Build). |
| `build_object_surface` | Build (+ Read) | level | One face (`face` 0–63) at the revision: `color` RGBA 0–1, `glow` 0–1, `fullbright`, `uv_scale`, `uv_offset`, `uv_rotation` (radians), `texture_item` (an inventory texture). A face with a PBR material is refused (native Build). |
| `build_object_material` | Build (+ Read) | level | Put an inventory PBR material (`material_item`) on one face at the revision, clearing that face's overrides. |
| `build_rez_primitive` | Build (+ Read) | level | Rez a cube, sphere, cylinder or cone with explicit `position`, `rotation`, `scale`. |
| `build_rez_inventory` | Build (+ Read) | level | Rez a copy of one of the user's copyable inventory objects (`item_id`) at `position`; the item stays in the inventory. |
| `inventory_permissions_get` | Read | – | An item's masks (base, owner, group, everyone, next owner) as copy / modify / transfer, whether the next owner's rights may be changed and why not, and a `revision`. |
| `inventory_next_owner_set` | Edit (+ Read) | **always** | Set the next owner's `copy`, `modify`, `transfer` of one item at its revision, with the Properties window's rules. |

Every change needs a `client_request_id` (a rez can never be looked at
reliably afterwards; decision 3 of mcp-receipts.md). Same names as
Folderstorm (C++); no `viewer_session` (the sidecar pins the login);
revisions are `revision`, not `expected_revision`.

## Permission classes and asking (decision)

- **New class Build**, saved as `build`, Preferences row "Build with my
  objects": move, turn, resize and repaint objects the user owns, and rez
  new objects on land where the user may build. Ask in "Ask before
  changes", Never in "Read only"; settings saved before it follow "Ask
  before changes" when every other change asks (Interact included), else
  Never. One class for edit and rez (Folderstorm (C++) had `build_edit`
  and `build_rez`): both put the user's objects in the world as the user
  sees them; one row is enough control, the dialog shows each change.
- Every building change also needs **Read** (`ToolSpec::also`), as
  Folderstorm (C++) needed World read: the revision comes from a read.
- **`inventory_next_owner_set` uses Edit and always asks**, even on
  Allow: it changes what the next owner may do with the creator's work.
  Kept in Edit (the inventory's own edits) rather than a class of its own:
  the dialog shows every change, and Never on Edit still refuses it.
- Rezzing does not always ask (Folderstorm (C++) decided the same): it
  is bounded to buildable land within 64 m and asks through the Build
  level.

## Which objects

The object tools' rules (`Objects::prim`): a loaded volume prim of the
agent's region, not worn, root owned by the agent (the server's owner:
never group-deeded) and modifiable; RLVa `@edit`, `@editobj`,
`@editworld`, `@interact`, `@fartouch`. Then the **root** of its linkset
(a child is refused with the root's id), and for every change: `OBJECT_MOVE`
and not physical (Folderstorm (C++)'s rule), at most 64 faces. The owner
comes from the server's properties: `build_object_get` asks for them when
the viewer has none, as `object_contents_list` does.

Positions: in the agent's region, `z` from 0 to 4096, within 64 m of the
avatar (and of `@fartouch`'s limit). Scales: the region's prim limits
(`World::prim_limits`), at most 64 m.

## Rezzing (the parcel)

- On the **agent's own parcel** (the position inside the parcel the agent
  stands on, as the viewer knows it) where `World::build_right_at` says
  Allowed (LLViewerParcelMgr::allowAgentBuild); unknown land is refused.
- RLVa: `@rez` and `@edit` refuse (RlvActions::canRez, canEdit), `@interact`
  too; `@showloc` refuses (the answer would tell where the avatar is).
- Under `World::group_for_rez(true)` (RezUnderLandGroup, on by default in
  both viewers), not selected, raycast bypassed at the exact position.
- **The new object's id is never guessed.** The rez answers `unconfirmed`
  with `object_id_confirmed: false` and `candidates`: owned roots that were
  not there before the send and appeared within 10 s within 10 m of the
  position. The receipt is tracked for `object_rez_identify`
  (`Objects::track_rez`): the assistant chooses, the viewer records the
  choice, never proof.
- Primitive: the Create tool's shapes (`build::shapes`: cube, cylinder,
  cone, sphere), wood, the simulator's default permissions.
- Inventory object: the agent's own item (`agent_mask`), an object (asset
  type 6), not a link, not worn, not in the Library, Trash or Marketplace;
  **Copy** for the agent: `RemoveItem` is always false, the source stays.
  Its permissions are packed by `RezInventory::message` as the inventory
  drag packs them (pack_permissions_slam).

## Creators' permissions

`AGENTS.md` § 2, through the shared helpers:

- **No asset id** leaves: a face's texture is named by kind, a material by
  a flag; revisions are random ids or salted hashes. Asset ids are
  compared inside the viewer only (to confirm a surface change).
- **Textures and materials from the inventory** need the agent's own item
  with **full rights** (copy, modify, transfer: `actions::FULL` on
  `agent_mask`), not a link, not in the Library or Trash, of the right type
  (texture 0, material 57). Stricter than the Build window (which lets a
  no-copy texture move into the object): as Folderstorm (C++), assigning
  an asset never weakens how its rights travel. No arbitrary asset id is
  accepted.
- **Rez from inventory**: Copy (above); the server applies the item's
  rights to the new object.
- **Permissions read**: the item's masks, owner items only (no Library,
  Trash or link: the link's original is not followed).
- **Next owner** (the Properties window's rules,
  `ui/inventory/properties.rs` with `actions::next_owner_editable` and
  `actions::mutable`): the agent's own item, not a link, calling card,
  landmark or folder link, not in the Library, the Current Outfit or
  Received Items; the agent's owner mask has Transfer. Asked rights within
  the item's **base and owner** masks (Folderstorm (C++)'s stricter
  rule; the window checks base only); `copy: false` needs `transfer: true`
  (the window locks Transfer on without Copy; never changed silently: the
  call is refused); an environment setting's Copy cannot change. The other
  bits of the next owner mask, the group and everyone masks and the sale
  stay as they are: the patch is the window's `permissions` block with
  only the next owner's C/M/T changed.
- **Objects are left out of `inventory_next_owner_set` (decision, to
  review).** The Legacy Viewer (LLSidepanelItemInfo) and Folderstorm (C++)
  set the object's `II_FLAGS_OBJECT_SLAM_PERM` flag when an object item's
  next owner rights change, so that a rezzed copy takes them; Aurora's
  Properties window sends no flag. Until that is settled upstream, the tool
  refuses object items rather than guess which rule applies; the user
  changes them in Properties.

## Revisions and stale checks

- An object revision: object, a fingerprint of what the read showed
  (position, rotation, scale, flags, linked prims, faces, PBR materials),
  taken; 16 per login, 120 s, gone at a login change or access off. A
  change is `stale` ("read it again") when the revision is unknown,
  expired, of another object, the object's fingerprint now differs, or the
  assistant changed it since. Checked when the call is taken and again just
  before the send (the 120 s only when taken).
- An item revision: a SHA-256 of a per-login salt, the item, its masks,
  flags, owner and sale: `stale` when it differs now. No expiry needed.

## Outcomes (never retried, never claimed)

| Tool | Confirmed (`done`) | Otherwise |
|---|---|---|
| transform | an object update shows the position, rotation and scale asked (within 1 cm, 0.1°, 1 mm) | `unconfirmed` after 30 s; a later update settles the receipt for 10 minutes |
| surface | the face shows what was asked (colour and glow as bytes, the texture compared inside the viewer) | the same |
| material | the face's PBR material is the item's (compared inside the viewer) | the same |
| rez | never: `unconfirmed`, `object_id_confirmed: false`, `candidates` | – |
| next owner | the inventory's answer, as every inventory change (the desk's path) | the inventory change's own outcomes |

## RLVa (the viewer decides)

As above; just before the send, `World::rlva_allows_command` checks the
message (`Transform`, `SetTextures`, `Add`, `RezInventory`, `Cap`,
`EditInventory`), as the viewer's own sends do.

## Where the code goes

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/protocol/building.rs` (new) | the catalogue, bounds, `needs_client_request_id`, `always_asks` |
| `policy.rs` | `Class::Build`, presets and migration |
| `binding.rs`, `protocol.rs`, `protocol/inventory.rs` | `building: 1`; `pub mod`, `all()`, required id, always asks; the catalogue count |
| sidecar `jsonrpc.rs` | the instructions' paragraph |
| viewer `mcp_bridge/building.rs` (new) | checks, revisions, plans, watches, candidates, the demo |
| `mcp_bridge/objects.rs` | a root lookup that asks for the owner; rez receipts in `object_operation_status` |
| `mcp_bridge/desk.rs`, `changes.rs`, `mcp_bridge.rs` | hook lines: reads, `Work::Build`, plan and replan, tick, clear, close, the app's entry point, the demo |
| `app/local_assistant.rs` | the sends with their RLVa checks; the owner's properties as for the object tools |
| Preferences page, catalogues | the Build row, the Ask sentences, en/fr/ja |

No upstream file changes beyond the Preferences row (already a
Folderstorm list).

## Demo

`AURORA_DEMO_MCP=building`: Ask before changes; the demo reads the plaza's
cut box, moves it up 0.5 m (asked; the demo region echoes it: `done`),
rezzes a cube 2 m from the avatar (asked; the demo answers with a new
object: a candidate), then reads the demo notecard's permissions and asks
to make its next owner copy and modify only.

## Tests

Catalogue (classes, required ids, always asks, bounds); policy (Build
presets and migration); objects (someone else's, group, no modify, no
move, physical, a child, worn, RLVa `@edit`, `@rez`, `@interact`);
vectors (non-finite, quaternion not normalised, scale limits, 64 m,
region); parcel (not buildable, another parcel, unknown); revisions
(expired, changed, own change, another object); textures and materials
(not full rights, link, Library, wrong type, PBR face refused); rez
inventory (no copy, not an object, worn); next owner (rules matrix, base
and owner bounds, no-copy needs transfer, settings, objects refused,
stale revision, always asks even on Allow, the patch keeps the other
bits); outcomes (transform confirmed, timeout unconfirmed then late,
candidates, `object_rez_identify` on a rez receipt); no asset id in any
answer; end to end over a real socket on the demo.

Not testable here: the grid's echo timing, ModifyMaterialParams on a grid,
the server's handling of next owner rights, the dialog's look.

## Left for later

- PBR overrides (Folderstorm (C++) left them to native Build too).
- Resizing a linkset (its children's transforms reviewed together).
- Next owner rights of objects (the slam flag question above).
