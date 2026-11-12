//! Folderstorm: `inventory_landmark` (folderstorm/plans/mcp.md, "Phase 3,
//! part 2"): where a landmark points, read through the viewer's landmark
//! cache (`world::landmarks`, LLLandmarkList / LLLandmark::getGlobalPos),
//! which the app asks for the asset and the region's handle. The call waits
//! here, polled once per frame, until both are known or
//! [`LANDMARK_WAIT`] passes.
//!
//! As the landmark's About window does, any landmark of the inventory or
//! the Library is read; RLVa's @showloc refuses it (the location is
//! hidden). The answer never carries the landmark's asset id.

use super::tools::{Ctx, err, uuid_arg};
use crate::world::World;
use crate::world::inventory::actions;
use crate::world::landmarks::Resolved;
use folderstorm_mcp_bridge::protocol::inventory::LANDMARK_WAIT;
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::{Value, json};
use std::time::Instant;
use uuid::Uuid;

pub struct Wait {
    call: Pending,
    item: Uuid,
    name: String,
    asset: Uuid,
    deadline: Instant,
}

/// Check a call and find its landmark's asset: it then waits.
pub fn start(call: Pending, cx: &Ctx, now: Instant) -> Option<Wait> {
    match check(&call.arguments, cx) {
        Ok((item, name, asset)) => Some(Wait {
            call,
            item,
            name,
            asset,
            deadline: now + LANDMARK_WAIT,
        }),
        Err(e) => {
            call.answer(Err(e));
            None
        }
    }
}

fn check(args: &Value, cx: &Ctx) -> Result<(Uuid, String, Uuid), ToolError> {
    if args.as_object().is_none_or(|a| a.keys().any(|k| k != "item_id")) {
        return Err(err(ErrorCode::InvalidArguments, "unknown argument"));
    }
    super::tools::ready(cx)?;
    let inv = cx.inv;
    let id = uuid_arg(args, "item_id")?.ok_or_else(|| err(ErrorCode::InvalidArguments, "`item_id` is required"))?;
    if !inv.items.contains_key(&id) {
        return Err(err(ErrorCode::NotFound, "no such item"));
    }
    let original = actions::original(inv, id)
        .and_then(|o| inv.items.get(&o))
        .ok_or_else(|| err(ErrorCode::NotFound, "the link is broken"))?;
    if original.asset_type != 3 || original.asset_id.is_nil() {
        return Err(err(ErrorCode::InvalidArguments, "that item is not a landmark"));
    }
    Ok((original.id, original.name.clone(), original.asset_id))
}

/// The landmark assets the waiting calls still need (the app asks for them).
pub fn wanted(waits: &[Wait]) -> Vec<Uuid> {
    waits.iter().map(|w| w.asset).collect()
}

/// Every waiting call refused with `refusal` (a login change, access
/// turned off: `Desk::refuse_waiting`).
pub fn refuse_all(waits: &mut Vec<Wait>, refusal: &ToolError) {
    for wait in std::mem::take(waits) {
        wait.call.answer(Err(refusal.clone()));
    }
}

/// Once per frame: answer the calls whose landmark is known, failed or out
/// of time.
pub fn tick(waits: &mut Vec<Wait>, world: &World, now: Instant) {
    for wait in std::mem::take(waits) {
        if world.rlva.has("showloc") {
            wait.call
                .answer(Err(err(ErrorCode::NotAllowed, "an RLV restriction (@showloc) hides locations")));
            continue;
        }
        match world.landmarks.peek(wait.asset) {
            Resolved::Ready {
                region_id,
                region_pos,
                global,
            } => {
                let region = world.map.sim_at_global(global.x, global.y).map(|(_, sim)| sim.name.clone());
                let mut out = json!({
                    "item_id": wait.item,
                    "name": wait.name,
                    "region_name": region,
                    "region_position": [region_pos.x, region_pos.y, region_pos.z],
                    "global_position": [global.x, global.y, global.z],
                });
                if !region_id.is_nil() {
                    out["region_id"] = json!(region_id);
                }
                wait.call.answer(Ok(out));
            }
            Resolved::Failed => wait.call.answer(Err(err(ErrorCode::Failed, "the landmark could not be read"))),
            Resolved::Pending if now >= wait.deadline => wait.call.answer(Err(err(
                ErrorCode::Timeout,
                "the landmark's location did not arrive in time; try again",
            ))),
            Resolved::Pending => waits.push(wait),
        }
    }
}
