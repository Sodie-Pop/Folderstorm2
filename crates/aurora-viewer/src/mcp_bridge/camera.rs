//! Folderstorm: the local assistant's camera and picture tools
//! (folderstorm/plans/mcp.md, "Phase 3, part 1"; Folderstorm (C++)'s
//! `doc/help.md`, "Asking for something"). Their arguments are checked
//! and planned here, on the frame thread, as the changes of phase 2 are
//! (`changes::plan`); the app moves the camera, draws the photo and sets
//! the thumbnail (`app/local_assistant.rs`).
//!
//! The camera moves as the `pstcampos` command and Escape move it
//! (`Camera::set_view`, `Camera::handle_reset_view`); the presets frame the
//! avatar's skeleton as Appearance's editors do
//! (`shape_editor::camera::view`, LLEditWearableDictionary's subparts). Any
//! RLVa camera restriction refuses a move.

use super::changes::{Planned, Work};
use super::picture::{Crop, Format, Request, Source};
use super::tools::{Ctx, Session, err, ready, scope, uuid_arg};
use crate::world::inventory::actions;
use folderstorm_mcp_bridge::protocol::{CAMERA_PRESETS, FOV_DEGREES, SNAPSHOT_EDGE_DEFAULT, SNAPSHOT_EDGE_MAX, SNAPSHOT_EDGE_MIN};
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use glam::{Quat, Vec3};
use serde_json::{Value, json};
use uuid::Uuid;

/// The RLVa behaviours that hold the camera (RlvActions::canChangeCameraPreset,
/// the camera modifiers of RlvBehaviourDictionary): while any is in force,
/// an assistant does not move it. Their aliases (`@camunlock`,
/// `@camdistmax`…) read as these.
pub const RLVA_CAMERA: &[&str] = &[
    "setcam",
    "setcam_unlock",
    "setcam_eyeoffset",
    "setcam_eyeoffsetscale",
    "setcam_focusoffset",
    "setcam_mouselook",
    "setcam_avdistmin",
    "setcam_avdistmax",
    "setcam_origindistmin",
    "setcam_origindistmax",
    "setcam_fovmin",
    "setcam_fovmax",
];

/// Whether an RLVa restriction holds the camera.
pub fn rlva_locked(rlva: &crate::world::rlva::Rlva) -> bool {
    RLVA_CAMERA.iter().any(|b| rlva.rules_for(b, Uuid::nil()).next().is_some())
}

/// What the camera tools read of the world, gathered by the frame thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stage {
    /// The world is shown (logged in or the demo, past the loading screen).
    pub in_world: bool,
    /// An RLVa camera restriction is in force ([`rlva_locked`]).
    pub camera_locked: bool,
    /// The avatar's centre, in region metres.
    pub avatar: Option<Vec3>,
    /// How far from the avatar an exact shot may put the camera.
    pub reach: f32,
}

/// Where the camera is.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub target: Vec3,
    /// Vertical field of view, radians.
    pub fov: f32,
}

impl Pose {
    /// The pose as an answer gives it: region metres, degrees.
    pub fn json(&self) -> Value {
        let v = |p: Vec3| json!([round(p.x), round(p.y), round(p.z)]);
        json!({"camera": v(self.position), "target": v(self.target), "fov_degrees": round(self.fov.to_degrees())})
    }
}

fn round(v: f32) -> f64 {
    (f64::from(v) * 1000.0).round() / 1000.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Portrait,
    FullBody,
    Front,
    Back,
    Left,
    Right,
}

impl Preset {
    pub fn parse(name: &str) -> Option<Preset> {
        Some(match name {
            "portrait" => Preset::Portrait,
            "full_body" => Preset::FullBody,
            "front" => Preset::Front,
            "back" => Preset::Back,
            "left" => Preset::Left,
            "right" => Preset::Right,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Preset::Portrait => "portrait",
            Preset::FullBody => "full_body",
            Preset::Front => "front",
            Preset::Back => "back",
            Preset::Left => "left",
            Preset::Right => "right",
        }
    }

    fn label(self) -> &'static str {
        use aurora_i18n::tr;
        match self {
            Preset::Portrait => tr!("fs-mcp-camera-portrait"),
            Preset::FullBody => tr!("fs-mcp-camera-full-body"),
            Preset::Front => tr!("fs-mcp-camera-front"),
            Preset::Back => tr!("fs-mcp-camera-back"),
            Preset::Left => tr!("fs-mcp-camera-left"),
            Preset::Right => tr!("fs-mcp-camera-right"),
        }
    }
}

/// The avatar as drawn: its head and pelvis joints, the height of its feet
/// and the way it faces (radians around Z), in region metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Skeleton {
    pub head: Vec3,
    pub pelvis: Vec3,
    pub foot_z: f32,
    pub yaw: f32,
}

/// A preset's camera and focus for `skeleton`, with a vertical field of
/// view `fov`. The whole body is framed as Appearance frames it
/// (`shape_editor::camera::view`: its height and 60 % of the frame, 2.2 m
/// at least); the portrait as the face's subparts, from a little farther
/// so that the shoulders show.
pub fn preset_view(preset: Preset, skeleton: &Skeleton, fov: f32) -> (Vec3, Vec3) {
    let half = (fov * 0.5).tan().max(0.1);
    let face = Quat::from_rotation_z(skeleton.yaw);
    let height = (skeleton.head.z - skeleton.foot_z + 0.35).max(1.0);
    let body = skeleton.pelvis.with_z((skeleton.head.z + skeleton.foot_z) * 0.5 + 0.07);
    let whole = (height * 0.6 / half).max(2.2);
    let (target, direction, distance) = match preset {
        // head and shoulders: about 0.7 m of the frame
        Preset::Portrait => (skeleton.head + Vec3::Z * 0.06, Vec3::new(1.0, 0.0, 0.05), (0.35 / half).max(0.7)),
        Preset::FullBody => (body, Vec3::new(2.5, 0.0, 0.7), whole),
        Preset::Front => (body, Vec3::new(1.0, 0.0, 0.1), whole),
        Preset::Back => (body, Vec3::new(-1.0, 0.0, 0.1), whole),
        Preset::Left => (body, Vec3::new(0.0, 1.0, 0.1), whole),
        Preset::Right => (body, Vec3::new(0.0, -1.0, 0.1), whole),
    };
    (target + face * direction.normalize() * distance, target)
}

/// Where an exact shot puts the camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Eye {
    /// Region metres.
    Region { position: Vec3, target: Vec3 },
    /// Metres from the avatar's centre, in its frame (x forward, y left,
    /// z up).
    Avatar { camera: Vec3, focus: Vec3 },
}

impl Eye {
    /// The camera and its focus in region metres, the avatar at `avatar`
    /// facing `yaw`.
    pub fn resolve(&self, avatar: Vec3, yaw: f32) -> (Vec3, Vec3) {
        match *self {
            Eye::Region { position, target } => (position, target),
            Eye::Avatar { camera, focus } => {
                let face = Quat::from_rotation_z(yaw);
                (avatar + face * camera, avatar + face * focus)
            }
        }
    }
}

/// A camera move the app makes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
    Preset(Preset),
    /// `fov`: radians, None keeps the view's.
    Shot {
        eye: Eye,
        fov: Option<f32>,
    },
    Reset,
}

/// Is this one of the tools planned here?
pub fn handles(tool: &str) -> bool {
    matches!(
        tool,
        "camera_preset" | "camera_shot" | "camera_reset" | "camera_snapshot" | "inventory_snapshot_thumbnail"
    )
}

/// Plan one call of the camera, picture or thumbnail tools.
pub fn plan(tool: &str, args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    match tool {
        "camera_preset" | "camera_shot" | "camera_reset" => {
            let (mv, summary) = camera_move(tool, args, cx)?;
            let mut reply = json!({"done": true});
            if let Move::Preset(preset) = mv {
                reply["preset"] = json!(preset.name());
            }
            Ok(planned(Work::Camera(mv), summary, reply))
        }
        "camera_snapshot" => {
            in_world(cx)?;
            let request = snapshot_request(args)?;
            let summary = snapshot_summary(&request);
            Ok(planned(Work::Snapshot(request), summary, json!({})))
        }
        "inventory_snapshot_thumbnail" => thumbnail(args, cx),
        _ => Err(err(ErrorCode::UnknownTool, "unknown tool")),
    }
}

fn planned(work: Work, summary: String, reply: Value) -> Planned {
    Planned {
        work,
        summary,
        reply,
        created: None,
    }
}

fn invalid(message: impl Into<String>) -> ToolError {
    err(ErrorCode::InvalidArguments, message)
}

fn known(args: &Value, keys: &[&str]) -> Result<(), ToolError> {
    match args.as_object() {
        Some(a) if a.keys().all(|k| keys.contains(&k.as_str())) => Ok(()),
        _ => Err(invalid("unknown argument")),
    }
}

fn in_world(cx: &Ctx) -> Result<(), ToolError> {
    if cx.session == Session::LoggedOut || !cx.stage.in_world {
        return Err(err(ErrorCode::NotReady, "the viewer is not in the world"));
    }
    Ok(())
}

/// The checks every camera move shares: in the world, the avatar there, no
/// RLVa restriction on the camera.
fn movable(cx: &Ctx) -> Result<Vec3, ToolError> {
    in_world(cx)?;
    if cx.stage.camera_locked {
        return Err(err(ErrorCode::NotAllowed, "an RLV restriction holds the camera"));
    }
    cx.stage
        .avatar
        .ok_or_else(|| err(ErrorCode::NotReady, "the avatar is not in a region yet"))
}

/// Three finite numbers, of 100 km at most.
fn vector(args: &Value, key: &str) -> Result<Option<Vec3>, ToolError> {
    let value = &args[key];
    if value.is_null() {
        return Ok(None);
    }
    let bad = || invalid(format!("`{key}` must be three numbers"));
    let list = value.as_array().filter(|l| l.len() == 3).ok_or_else(bad)?;
    let mut v = [0.0f32; 3];
    for (slot, n) in v.iter_mut().zip(list) {
        let n = n.as_f64().filter(|n| n.is_finite() && n.abs() <= 100_000.0).ok_or_else(bad)?;
        *slot = n as f32;
    }
    Ok(Some(Vec3::from(v)))
}

fn camera_move(tool: &str, args: &Value, cx: &Ctx) -> Result<(Move, String), ToolError> {
    use aurora_i18n::tr;
    match tool {
        "camera_preset" => {
            known(args, &["preset"])?;
            let name = args["preset"].as_str().ok_or_else(|| invalid("`preset` is required"))?;
            let preset = Preset::parse(name).ok_or_else(|| invalid(format!("`preset` is one of {}", CAMERA_PRESETS.join(", "))))?;
            movable(cx)?;
            Ok((Move::Preset(preset), tr!("fs-mcp-ask-camera-preset", preset = preset.label())))
        }
        "camera_shot" => {
            known(args, &["position", "target", "camera_offset", "focus_offset", "fov_degrees"])?;
            let eye = match (
                vector(args, "position")?,
                vector(args, "target")?,
                vector(args, "camera_offset")?,
                vector(args, "focus_offset")?,
            ) {
                (Some(position), Some(target), None, None) => Eye::Region { position, target },
                (None, None, Some(camera), focus) => Eye::Avatar {
                    camera,
                    focus: focus.unwrap_or(Vec3::ZERO),
                },
                _ => {
                    return Err(invalid(
                        "give `position` and `target`, or `camera_offset` (and `focus_offset`), not both",
                    ));
                }
            };
            let fov = match &args["fov_degrees"] {
                Value::Null => None,
                v => {
                    let degrees = v
                        .as_f64()
                        .filter(|d| (f64::from(FOV_DEGREES.0)..=f64::from(FOV_DEGREES.1)).contains(d))
                        .ok_or_else(|| invalid(format!("`fov_degrees` must be {} to {}", FOV_DEGREES.0, FOV_DEGREES.1)))?;
                    Some((degrees as f32).to_radians())
                }
            };
            let avatar = movable(cx)?;
            // the frame turns with the avatar: its distances are the same
            let (camera, focus) = eye.resolve(avatar, 0.0);
            if camera.distance(focus) < 0.01 {
                return Err(invalid("the camera and the point it looks at are the same"));
            }
            let reach = cx.stage.reach;
            if camera.distance(avatar) > reach {
                // as the pstcampos command refuses it (cmdline_apply_camera)
                return Err(err(
                    ErrorCode::NotAllowed,
                    format!("the camera would be farther than {} m from the avatar", reach.round()),
                ));
            }
            Ok((Move::Shot { eye, fov }, tr!("fs-mcp-ask-camera-shot").to_owned()))
        }
        _ => {
            known(args, &[])?;
            movable(cx)?;
            Ok((Move::Reset, tr!("fs-mcp-ask-camera-reset").to_owned()))
        }
    }
}

fn edge(args: &Value, key: &str) -> Result<Option<u32>, ToolError> {
    match &args[key] {
        Value::Null => Ok(None),
        v => v
            .as_u64()
            .filter(|n| (u64::from(SNAPSHOT_EDGE_MIN)..=u64::from(SNAPSHOT_EDGE_MAX)).contains(n))
            .map(|n| Some(n as u32))
            .ok_or_else(|| invalid(format!("`{key}` must be {SNAPSHOT_EDGE_MIN} to {SNAPSHOT_EDGE_MAX} pixels"))),
    }
}

fn flag(args: &Value, key: &str) -> Result<bool, ToolError> {
    match &args[key] {
        Value::Null => Ok(false),
        Value::Bool(b) => Ok(*b),
        _ => Err(invalid(format!("`{key}` is true or false"))),
    }
}

/// A `camera_snapshot`'s arguments, checked.
pub fn snapshot_request(args: &Value) -> Result<Request, ToolError> {
    known(
        args,
        &[
            "source",
            "width",
            "height",
            "max_edge",
            "crop",
            "interface",
            "huds",
            "format",
            "save",
        ],
    )?;
    let source = match &args["source"] {
        Value::Null => Source::Window,
        Value::String(s) if s == "window" => Source::Window,
        Value::String(s) if s == "world" => Source::World,
        _ => return Err(invalid("`source` is `window` or `world`")),
    };
    let format = match &args["format"] {
        Value::Null => Format::Jpeg,
        Value::String(s) if s == "jpeg" => Format::Jpeg,
        Value::String(s) if s == "png" => Format::Png,
        _ => return Err(invalid("`format` is `jpeg` or `png`")),
    };
    let crop = match &args["crop"] {
        Value::Null => Crop::WHOLE,
        Value::Object(c) => {
            if c.keys().any(|k| !matches!(k.as_str(), "x" | "y" | "width" | "height")) {
                return Err(invalid("`crop` holds x, y, width and height"));
            }
            let part = |key: &str| {
                c.get(key)
                    .and_then(Value::as_f64)
                    .filter(|v| (0.0..=1.0).contains(v))
                    .map(|v| v as f32)
                    .ok_or_else(|| invalid(format!("`crop.{key}` must be a fraction from 0 to 1")))
            };
            let crop = Crop {
                x: part("x")?,
                y: part("y")?,
                width: part("width")?,
                height: part("height")?,
            };
            if crop.width <= 0.0 || crop.height <= 0.0 || crop.x + crop.width > 1.0001 || crop.y + crop.height > 1.0001 {
                return Err(invalid("`crop` must keep a part inside the picture"));
            }
            crop
        }
        _ => return Err(invalid("`crop` is an object")),
    };
    let interface = flag(args, "interface")?;
    if interface && source == Source::World {
        return Err(invalid("the interface is only in a picture of the whole window (`source: window`)"));
    }
    Ok(Request {
        source,
        width: edge(args, "width")?,
        height: edge(args, "height")?,
        max_edge: edge(args, "max_edge")?.unwrap_or(SNAPSHOT_EDGE_DEFAULT),
        crop,
        interface,
        huds: flag(args, "huds")?,
        format,
        save: flag(args, "save")?,
    })
}

fn snapshot_summary(request: &Request) -> String {
    use aurora_i18n::tr;
    let what = if request.interface {
        tr!("fs-mcp-ask-snapshot-interface")
    } else if request.huds {
        tr!("fs-mcp-ask-snapshot-huds")
    } else {
        tr!("fs-mcp-ask-snapshot")
    };
    if request.save {
        format!("{what} {}", tr!("fs-mcp-ask-snapshot-saved"))
    } else {
        what.to_owned()
    }
}

/// The Outfit Gallery's Take snapshot on any entry the item image window
/// may change (`actions::mutable`, the rule `ui/inventory/thumbnail.rs`
/// applies): not the Library, the Current Outfit nor Received Items.
fn thumbnail(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["id", "path"])?;
    ready(cx)?;
    in_world(cx)?;
    let inv = cx.inv;
    let id = match (uuid_arg(args, "id")?, args["path"].is_null()) {
        (Some(_), false) => return Err(invalid("give `id` or `path`, not both")),
        (Some(id), true) => id,
        (None, false) => scope(&json!({"path": args["path"]}), inv)?.ok_or_else(|| err(ErrorCode::NotFound, "no such folder"))?,
        (None, true) => return Err(invalid("give the entry as `id` or `path`")),
    };
    if !inv.items.contains_key(&id) && !inv.folders.contains_key(&id) {
        return Err(err(ErrorCode::NotFound, "no such item or folder"));
    }
    if let Some(root) = crate::world::marketplace::root(inv)
        && actions::under(inv, id, root)
    {
        return Err(err(
            ErrorCode::NotAllowed,
            "the Marketplace Listings folder is not changed by an assistant",
        ));
    }
    if id == inv.root || !actions::mutable(inv, id) {
        return Err(err(
            ErrorCode::NotAllowed,
            "this entry's picture cannot be changed: the Library, the Current Outfit and Received Items keep theirs",
        ));
    }
    let name = actions::name(inv, id);
    let summary = aurora_i18n::tr!("fs-mcp-ask-thumbnail", name = name);
    Ok(planned(Work::Thumbnail(id), summary, json!({"id": id})))
}
