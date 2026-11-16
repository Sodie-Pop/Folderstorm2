//! Folderstorm: the local assistant's pose presets and pose files
//! (folderstorm/plans/mcp-gestures.md), through the poser's own format and
//! rules: a file is the poser's `Document`, checked by `Document::valid`,
//! applied by `Document::apply` in rotations mode and written by
//! `Document::capture`; a hand preset is the poser's Hands preset
//! (`presets.json`, set as `controls::hand_preset` sets it, on both hands).
//! The assistant loads a preset only when it holds rotations alone: no
//! joint moved, scaled or pinned to the world, the pelvis left as it is.

use super::super::data::Session;
use super::{Document, LoadMode};
use crate::ik_edit::poser::SubjectPose;
use crate::scene::anim::Rig;
use glam::{EulerRot, Mat4, Quat, Vec3};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// The poser's hand presets, in the order of its Hands control.
pub(crate) const HAND_PRESETS: [&str; 5] = ["Relaxed", "Tight_Grip", "Flat", "Pointing", "Okay"];
/// The priority the poser gives a new pose (`open_poser`).
const PRIORITY: i32 = 4;

fn hand_table() -> &'static HashMap<String, HashMap<String, [f32; 3]>> {
    static TABLE: OnceLock<HashMap<String, HashMap<String, [f32; 3]>>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(include_str!("../presets.json")).unwrap_or_default())
}

/// The hand preset `name` on both hands of `pose`, as the poser's Hands
/// control sets it: each finger joint's complete rotation, its origin the
/// rest's. None: no such preset, or the pose would not be valid.
pub(crate) fn hand_preset(rig: &Rig, pose: &SubjectPose, name: &str) -> Option<SubjectPose> {
    let preset = hand_table().get(name)?;
    let mut out = pose.clone();
    for (joint, xyz) in preset {
        if (joint.ends_with("Left") || joint.ends_with("Right"))
            && let Some(&bone) = rig.names.get(joint)
        {
            out.rotations[bone] = Quat::from_euler(EulerRot::ZYX, xyz[2], xyz[1], xyz[0]);
            out.origin[bone] = rig.local_rot[bone];
            out.locks.remove(&bone);
        }
    }
    out.valid(rig).then_some(out)
}

/// The joints a hand preset sets.
pub(crate) fn hand_preset_joints(name: &str) -> usize {
    hand_table().get(name).map_or(0, HashMap::len)
}

fn same(a: Quat, b: Quat) -> bool {
    a.dot(b).abs() > 0.9999
}

/// A pose file's name, and why the assistant may not load it (None: it
/// may). Err: not a pose file the poser accepts (`Document::valid`).
pub(crate) fn inspect(rig: &Rig, bytes: &[u8]) -> Result<(String, usize, Option<&'static str>), ()> {
    let document: Document = serde_json::from_slice(bytes).map_err(|_| ())?;
    if !document.valid(rig) {
        return Err(());
    }
    let mut why = None;
    for joint in &document.joints {
        let rotation = Quat::from_array(joint.rotation);
        if Vec3::from_array(joint.translation).abs().max_element() > 1e-5 {
            why = Some("it moves joints");
        } else if (Vec3::from_array(joint.scale) - Vec3::ONE).abs().max_element() > 1e-5 {
            why = Some("it scales joints");
        } else if joint.locked {
            why = Some("it pins joints to the world");
        } else if rig.names.get(&joint.name) == Some(&rig.pelvis)
            && !same(rotation, if joint.change { Quat::IDENTITY } else { rig.local_rot[rig.pelvis] })
        {
            why = Some("it turns the pelvis");
        }
        if why.is_some() {
            break;
        }
    }
    Ok((document.name, document.joints.len(), why))
}

/// The pose session made of `base` (captured) and `pose` (now), as the
/// poser keeps it.
fn session(base: &Arc<SubjectPose>, pose: &Arc<SubjectPose>) -> Session {
    Session {
        base: base.clone(),
        pose: pose.clone(),
        enabled: true,
        undo: Vec::new(),
        redo: Vec::new(),
    }
}

/// A rotations-only pose file applied to `pose`, as the poser's Load does
/// in rotations mode. None: not a file the assistant may load.
pub(crate) fn apply_file(rig: &Rig, base: &Arc<SubjectPose>, pose: &Arc<SubjectPose>, bytes: &[u8]) -> Option<SubjectPose> {
    if !matches!(inspect(rig, bytes), Ok((_, _, None))) {
        return None;
    }
    let document: Document = serde_json::from_slice(bytes).ok()?;
    let mut session = session(base, pose);
    // no lock in such a file: the model matrix is never read
    if !document.apply(rig, &mut session, LoadMode::Rotations, Mat4::IDENTITY) {
        return None;
    }
    Some((*session.pose).clone())
}

/// A new pose file of `pose` (captured as `base`), named `name`, in the
/// poser's format: what its Save writes.
pub(crate) fn file_bytes(rig: &Rig, base: &Arc<SubjectPose>, pose: &Arc<SubjectPose>, name: &str) -> Option<Vec<u8>> {
    let document = Document::capture(rig, &session(base, pose), name.to_owned(), PRIORITY);
    if !document.valid(rig) {
        return None;
    }
    serde_json::to_vec_pretty(&document).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::avatar::AvatarLibrary;

    fn rest(rig: &Rig) -> Arc<SubjectPose> {
        Arc::new(SubjectPose {
            rotations: rig.local_rot.clone(),
            positions: rig.local_pos.clone(),
            scales: rig.scale.clone(),
            locks: HashMap::new(),
            origin: rig.local_rot.clone(),
        })
    }

    #[test]
    fn hand_presets_set_both_hands_and_files_round_trip_rotations_only() {
        let library = AvatarLibrary::load();
        let rig = &library.rig;
        let base = rest(rig);
        for name in HAND_PRESETS {
            assert!(hand_preset_joints(name) > 0, "{name}");
            let posed = hand_preset(rig, &base, name).expect("preset");
            assert!(posed.valid(rig));
        }
        assert!(hand_preset(rig, &base, "Nope").is_none());
        // a turned head saved, then listed as loadable and loaded back
        let head = rig.names["mHead"];
        let mut turned = (*base).clone();
        turned.rotations[head] = rig.local_rot[head] * Quat::from_rotation_z(0.4);
        turned.origin[head] = rig.local_rot[head];
        let turned = Arc::new(turned);
        let bytes = file_bytes(rig, &base, &turned, "Look").expect("bytes");
        let (name, joints, why) = inspect(rig, &bytes).expect("a pose file");
        assert_eq!((name.as_str(), why), ("Look", None));
        assert!(joints > 0);
        let loaded = apply_file(rig, &base, &base, &bytes).expect("loaded");
        assert!(same(loaded.rotations[head], turned.rotations[head]));
        // a file that moves a joint is listed with why, never loaded
        let mut document: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        for joint in document["joints"].as_array_mut().expect("joints") {
            if joint["name"] == "mHead" {
                joint["translation"] = serde_json::json!([0.0, 0.0, 0.1]);
            }
        }
        let moved = serde_json::to_vec(&document).expect("bytes");
        assert_eq!(inspect(rig, &moved).expect("a pose file").2, Some("it moves joints"));
        assert!(apply_file(rig, &base, &base, &moved).is_none());
        assert!(inspect(rig, b"{\"version\":2}").is_err());
        assert!(inspect(rig, b"not json").is_err());
    }
}
