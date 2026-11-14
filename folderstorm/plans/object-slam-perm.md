# Object rez overrides from the Properties window (`SLAM_PERM` & co.)

Fork branch: `claude/object-slam-perm`. Written so that it can be sent
upstream to Aurora as an issue or a PR description.

## What

When the owner changes the permissions or the sale terms of an **object**
inventory item in the Properties window, Aurora saves the new masks on the
item but does not set the inventory flags that tell the simulator to apply
them on rez. The item then shows the new next-owner rights, while a copy
rezzed from it (or given and rezzed by the next owner) keeps the rights saved
in the object's asset: the old ones.

## Why (Legacy Viewer reference)

`LLSidepanelItemInfo::updatePermissions` and `updateSaleInfo`
(`indra/newview/llsidepaneliteminfo.cpp`; the same code is in
`llfloaterproperties.cpp`) add, on an item of type `AT_OBJECT` and on top of
the flags the item already has:

| Changed term | Flag (`llinventorydefines.h`) | Value |
|---|---|---|
| next owner mask | `II_FLAGS_OBJECT_SLAM_PERM` | `0x100` |
| everyone mask | `II_FLAGS_OBJECT_PERM_OVERWRITE_EVERYONE` | `0x080000` |
| group mask | `II_FLAGS_OBJECT_PERM_OVERWRITE_GROUP` | `0x040000` |
| sale type or price | `II_FLAGS_OBJECT_SLAM_SALE` | `0x1000` |

`llinventorydefines.h` explains: "Normally, if no permissions slam or
overwrite flags are set, the asset's permissions are used and the
inventory's permissions are ignored. If any of these flags are set, the
inventory's permissions take precedence." The item is then sent whole
(`update_inventory_item` → `asLLSD()` to AIS, or `UpdateInventoryItem`), so
the flags travel with the masks. For an item inside an object, the same
`new_item` goes out with `LLViewerObject::updateInventory`
(`UpdateTaskInventory`).

Aurora already does this in one place: the inventory bulk permissions
(`ui/inventory/bulk_permissions.rs`, "stored objects must carry these
overrides so rez applies the new masks"). The Properties windows do not:

- agent inventory: `ui/inventory/properties.rs` `show` builds an AIS patch
  with `name`, `desc`, `permissions`, `sale_info` and no `flags`;
- object contents: `build/ui/item_properties.rs` `save_item_properties`
  copies the item (its flags unchanged) and sends `UpdateTaskInventory`.

A second, smaller bug sits in the read-back: `inventory/operations.rs`
`confirmed` compares each patched field with the item fetched back, but its
table of observed item fields has no `flags`. A patch that carries `flags`
(today: the bulk permissions on an object) can therefore never be confirmed
and reports "the server did not confirm every change" although AIS applied
it.

Existing flags are not lost today: AIS `PATCH` only changes the fields it is
given, and the UDP update copies the item it knows. The fix keeps it that
way: it sends the item's flags with the new bits added, and only when a bit
is added.

## The change

- A pure function computes the new flags from the item's terms before and
  after the edit (only for `AT_OBJECT`; unchanged terms add nothing; the
  existing flags, including the attachment point in the low byte, are kept).
- `properties.rs`: add `flags` to the patch when that function adds a bit.
- `item_properties.rs`: apply it to the task item before
  `UpdateTaskInventory`.
- `operations.rs` `confirmed`: add `"flags" => it.flags as i32` to the
  observed item fields.

On grids without AIS, the agent-inventory Properties window is unchanged:
`udp_edits` already refuses a patch with `permissions` there.

In Folderstorm2 the function lives in
`crates/aurora-viewer/src/folderstorm/rez_overrides.rs` and the upstream files
get one-line hooks; upstream, it would sit naturally next to
`bulk_permissions.rs`, which could share it.

## Test on the grid

1. Rez a new box, take it into inventory. Open the item's Properties and
   untick Next owner: Modify (and change Everyone: Copy, if you like).
2. Rez the item again, select the copy and look at its Next owner
   permissions in the Build floater (General tab): it must show the new
   rights, not the ones the box had when taken. Same after giving the item to
   an alt and letting the alt rez it.
3. Same with an object in a prim's contents: edit its next-owner rights in
   its Properties window, then take a copy or let a script rez it.
4. The Properties window must not report "the server did not confirm every
   change" after the edits above (a refused AIS request is logged with the
   patched fields, for example "patch item [desc, flags, name, permissions,
   sale_info]").
5. Inventory bulk permissions on an object item must now finish without
   that message too.
