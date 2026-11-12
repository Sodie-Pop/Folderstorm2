//! Folderstorm: the local assistant's camera, picture and thumbnail tools
//! (phase 3, part 1): their arguments, the presets' framing, the picture's
//! size, crop and byte cap, RLVa, and the calls waiting in the studio.

use super::camera::{self, Eye, Move, Pose, Preset, Skeleton, Stage, preset_view};
use super::change_tests::{ctx, demo_inventory};
use super::changes::{Planned, Work, plan};
use super::desk::{Bake, Desk};
use super::picture::{self, Crop, Format, Request, Source};
use super::studio::{PhotoStep, SETTLE_MAX, STILL, Studio};
use crate::world::inventory::Inventory;
use folderstorm_mcp_bridge::protocol::IMAGE_BYTES_MAX;
use folderstorm_mcp_bridge::server::Pending;
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use glam::Vec3;
use serde_json::{Value, json};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use uuid::Uuid;

const AVATAR: Vec3 = Vec3::new(128.0, 128.0, 25.0);
const COP_OUTFIT: Uuid = Uuid::from_u128(703);
const DEMO_FOLDER: Uuid = Uuid::from_u128(8000);
const TEXTURE: Uuid = Uuid::from_u128(8104);
const MARKETPLACE: Uuid = Uuid::from_u128(8002);
const COF: Uuid = Uuid::from_u128(701);

fn stage() -> Stage {
    Stage {
        in_world: true,
        camera_locked: false,
        avatar: Some(AVATAR),
        reach: 64.0,
    }
}

fn try_plan(inv: &Inventory, stage: Stage, tool: &str, args: Value) -> Result<Planned, ToolError> {
    let mut cx = ctx(inv);
    cx.stage = stage;
    plan(tool, &args, &cx)
}

fn fails(tool: &str, args: Value) -> ErrorCode {
    try_plan(&demo_inventory(), stage(), tool, args).expect_err("refused").code
}

fn camera_move(tool: &str, args: Value) -> Move {
    match try_plan(&demo_inventory(), stage(), tool, args).expect("planned").work {
        Work::Camera(mv) => mv,
        other => panic!("not a camera move: {other:?}"),
    }
}

fn snapshot(args: Value) -> Request {
    match try_plan(&demo_inventory(), stage(), "camera_snapshot", args).expect("planned").work {
        Work::Snapshot(request) => request,
        other => panic!("not a picture: {other:?}"),
    }
}

#[test]
fn camera_tool_arguments_are_checked() {
    assert_eq!(
        camera_move("camera_preset", json!({"preset": "portrait"})),
        Move::Preset(Preset::Portrait)
    );
    assert_eq!(fails("camera_preset", json!({"preset": "selfie"})), ErrorCode::InvalidArguments);
    assert_eq!(fails("camera_preset", json!({})), ErrorCode::InvalidArguments);
    assert_eq!(
        fails("camera_preset", json!({"preset": "back", "zoom": 2})),
        ErrorCode::InvalidArguments
    );
    assert_eq!(camera_move("camera_reset", json!({})), Move::Reset);
    assert_eq!(fails("camera_reset", json!({"preset": "back"})), ErrorCode::InvalidArguments);
    // an exact shot: region coordinates, or offsets from the avatar
    assert_eq!(
        camera_move("camera_shot", json!({"position": [130.0, 128.0, 26.0], "target": [128, 128, 25.5]})),
        Move::Shot {
            eye: Eye::Region {
                position: Vec3::new(130.0, 128.0, 26.0),
                target: Vec3::new(128.0, 128.0, 25.5)
            },
            fov: None
        }
    );
    let Move::Shot { eye, fov } = camera_move("camera_shot", json!({"camera_offset": [3, 0, 1], "fov_degrees": 40})) else {
        panic!("a shot");
    };
    assert_eq!(
        eye,
        Eye::Avatar {
            camera: Vec3::new(3.0, 0.0, 1.0),
            focus: Vec3::ZERO
        }
    );
    assert!((fov.expect("a field of view") - 40f32.to_radians()).abs() < 1e-6);
    for bad in [
        json!({"position": [1, 2, 3]}),
        json!({"target": [1, 2, 3]}),
        json!({"focus_offset": [1, 2, 3]}),
        json!({"position": [1, 2, 3], "target": [1, 2, 4], "camera_offset": [1, 0, 0]}),
        json!({"position": [1, 2], "target": [1, 2, 4]}),
        json!({"position": [1, 2, "3"], "target": [1, 2, 4]}),
        json!({"position": [1, 2, 1e9], "target": [1, 2, 4]}),
        json!({"camera_offset": [3, 0, 1], "fov_degrees": 2}),
        json!({"camera_offset": [3, 0, 1], "fov_degrees": 150}),
        json!({"camera_offset": [1, 1, 1], "focus_offset": [1, 1, 1]}),
        json!({}),
    ] {
        assert_eq!(fails("camera_shot", bad.clone()), ErrorCode::InvalidArguments, "{bad}");
    }
}

#[test]
fn an_exact_shot_stays_within_reach_of_the_avatar() {
    // as the pstcampos command refuses a camera beyond the draw distance
    assert_eq!(
        fails("camera_shot", json!({"position": [228.0, 128.0, 25.0], "target": [128, 128, 25]})),
        ErrorCode::NotAllowed
    );
    assert_eq!(fails("camera_shot", json!({"camera_offset": [70, 0, 0]})), ErrorCode::NotAllowed);
    let mut far = stage();
    far.reach = 256.0;
    assert!(try_plan(&demo_inventory(), far, "camera_shot", json!({"camera_offset": [70, 0, 0]})).is_ok());
}

#[test]
fn the_camera_needs_the_world() {
    let inv = demo_inventory();
    let outside = Stage {
        in_world: false,
        ..stage()
    };
    for (tool, args) in [
        ("camera_preset", json!({"preset": "front"})),
        ("camera_reset", json!({})),
        ("camera_snapshot", json!({})),
    ] {
        assert_eq!(
            try_plan(&inv, outside, tool, args).expect_err("not in the world").code,
            ErrorCode::NotReady,
            "{tool}"
        );
    }
    let nowhere = Stage { avatar: None, ..stage() };
    assert_eq!(
        try_plan(&inv, nowhere, "camera_reset", json!({})).expect_err("no avatar").code,
        ErrorCode::NotReady
    );
}

#[test]
fn an_rlv_camera_restriction_refuses_the_moves_but_not_the_pictures() {
    let inv = demo_inventory();
    let locked = Stage {
        camera_locked: true,
        ..stage()
    };
    for (tool, args) in [
        ("camera_preset", json!({"preset": "portrait"})),
        ("camera_shot", json!({"camera_offset": [2, 0, 1]})),
        ("camera_reset", json!({})),
    ] {
        assert_eq!(
            try_plan(&inv, locked, tool, args).expect_err("locked").code,
            ErrorCode::NotAllowed,
            "{tool}"
        );
    }
    assert!(try_plan(&inv, locked, "camera_snapshot", json!({})).is_ok());
}

fn rlva_with(rules: &str) -> crate::world::rlva::Rlva {
    let mut rlva = crate::world::rlva::Rlva::default();
    rlva.configure(true);
    assert!(rlva.receive(&aurora_net::ChatMessage {
        from_name: "Camera test object".to_owned(),
        source_id: Uuid::from_u128(1),
        owner_id: Uuid::from_u128(2),
        source_type: aurora_net::ChatSourceType::Object,
        chat_type: aurora_net::ChatType::OwnerSay,
        position: AVATAR,
        message: rules.to_owned(),
    }));
    rlva
}

#[test]
fn rlva_camera_rules_and_their_aliases_lock_the_camera() {
    assert!(!camera::rlva_locked(&crate::world::rlva::Rlva::default()));
    for rules in [
        "@setcam=n",
        "@camunlock=n",
        "@setcam_unlock=n",
        "@camdistmax:2=n",
        "@setcam_avdistmin:1=n",
        "@setcam_fovmax:0.9=n",
        "@setcam_eyeoffset:1/0/0=n",
        "@setcam_mouselook=n",
    ] {
        assert!(camera::rlva_locked(&rlva_with(rules)), "{rules}");
    }
    // a restriction elsewhere leaves the camera free
    assert!(!camera::rlva_locked(&rlva_with("@showinv=n")));
    // RLVa off: nothing is in force
    let mut off = rlva_with("@setcam=n");
    off.configure(false);
    assert!(!camera::rlva_locked(&off));
}

fn skeleton(yaw: f32) -> Skeleton {
    Skeleton {
        head: AVATAR + Vec3::new(0.0, 0.0, 0.7),
        pelvis: AVATAR,
        foot_z: AVATAR.z - 0.95,
        yaw,
    }
}

#[test]
fn presets_frame_the_avatar_from_their_side() {
    let fov = 60f32.to_radians();
    let s = skeleton(0.0);
    let (eye, target) = preset_view(Preset::Portrait, &s, fov);
    assert!(target.distance(s.head) < 0.1, "the portrait looks at the head");
    assert!(eye.x > target.x && (eye.y - target.y).abs() < 1e-4, "in front of the face (+X)");
    assert!(
        (0.7..1.2).contains(&eye.distance(target)),
        "head and shoulders: {}",
        eye.distance(target)
    );
    let (eye, target) = preset_view(Preset::FullBody, &s, fov);
    assert!(eye.distance(target) >= 2.19);
    assert!(eye.z > target.z, "the whole body from a little above");
    assert!(target.z < s.head.z && target.z > s.foot_z);
    let side = |p: Preset| {
        let (eye, target) = preset_view(p, &s, fov);
        (eye - target).normalize()
    };
    assert!(side(Preset::Front).x > 0.9);
    assert!(side(Preset::Back).x < -0.9);
    assert!(side(Preset::Left).y > 0.9, "the avatar's left is +Y");
    assert!(side(Preset::Right).y < -0.9);
    // the frame turns with the avatar
    let turned = skeleton(std::f32::consts::FRAC_PI_2);
    let (eye, target) = preset_view(Preset::Front, &turned, fov);
    assert!((eye - target).normalize().y > 0.9, "facing +Y: the front camera is on +Y");
    // a narrower lens stands farther back
    let (wide, at) = preset_view(Preset::FullBody, &s, 90f32.to_radians());
    let (narrow, _) = preset_view(Preset::FullBody, &s, 30f32.to_radians());
    assert!(narrow.distance(at) > wide.distance(at));
}

#[test]
fn avatar_offsets_follow_its_facing() {
    let eye = Eye::Avatar {
        camera: Vec3::new(2.0, 0.0, 1.0),
        focus: Vec3::new(0.0, 0.0, 0.5),
    };
    let (camera, focus) = eye.resolve(AVATAR, std::f32::consts::PI);
    assert!(camera.distance(AVATAR + Vec3::new(-2.0, 0.0, 1.0)) < 1e-4);
    assert!(focus.distance(AVATAR + Vec3::new(0.0, 0.0, 0.5)) < 1e-4);
}

#[test]
fn snapshot_arguments_are_checked() {
    let r = snapshot(json!({}));
    assert_eq!(r.source, Source::Window);
    assert_eq!((r.width, r.height, r.max_edge), (None, None, 1024));
    assert_eq!(r.crop, Crop::WHOLE);
    assert!(!r.interface && !r.huds && !r.save);
    assert_eq!(r.format, Format::Jpeg);
    let r = snapshot(
        json!({"source": "world", "width": 512, "height": 256, "huds": true, "format": "png", "save": true,
        "crop": {"x": 0.25, "y": 0.0, "width": 0.5, "height": 1.0}}),
    );
    assert_eq!(r.source, Source::World);
    assert_eq!((r.width, r.height), (Some(512), Some(256)));
    assert!(r.huds && r.save);
    assert_eq!(r.format, Format::Png);
    assert_eq!(r.crop.width, 0.5);
    for bad in [
        json!({"source": "desktop"}),
        json!({"width": 32}),
        json!({"height": 4096}),
        json!({"max_edge": 0}),
        json!({"width": 512.5}),
        json!({"format": "gif"}),
        json!({"interface": "yes"}),
        json!({"interface": true, "source": "world"}),
        json!({"crop": {"x": 0.5, "y": 0, "width": 0.6, "height": 1}}),
        json!({"crop": {"x": 0, "y": 0, "width": 0, "height": 1}}),
        json!({"crop": {"x": 0, "y": 0, "width": 1}}),
        json!({"crop": {"x": 0, "y": 0, "width": 1, "height": 1, "z": 0}}),
        json!({"crop": [0, 0, 1, 1]}),
        json!({"quality": 90}),
    ] {
        assert_eq!(fails("camera_snapshot", bad.clone()), ErrorCode::InvalidArguments, "{bad}");
    }
}

fn request(args: Value) -> Request {
    camera::snapshot_request(&args).expect("valid")
}

#[test]
fn a_picture_of_both_sides_is_centre_cropped_to_them() {
    // a 1600 x 900 window, a square of 512
    let p = picture::plan(&request(json!({"width": 512, "height": 512})), (1600, 900), None);
    assert_eq!(p.out, (512, 512));
    // drawn so that the square part has 512 pixels: 910 x 512
    assert_eq!(p.render.size, (910, 512));
    let [x, y, w, h] = p.cut;
    assert_eq!((w, h), (512, 512));
    assert_eq!(y, 0);
    assert!((x as i32 - (910 - 512) / 2).abs() <= 1, "centred: {x}");
}

#[test]
fn one_side_lets_the_other_follow_the_source() {
    let p = picture::plan(&request(json!({"width": 800})), (1600, 900), None);
    assert_eq!(p.out, (800, 450));
    assert_eq!(p.render.size, (800, 450));
    assert_eq!(p.cut, [0, 0, 800, 450]);
    let p = picture::plan(&request(json!({"height": 300})), (1600, 900), None);
    assert_eq!(p.out, (533, 300));
}

#[test]
fn without_a_size_the_long_edge_is_capped() {
    let p = picture::plan(&request(json!({})), (1600, 900), None);
    assert_eq!(p.out, (1024, 576));
    let p = picture::plan(&request(json!({"max_edge": 2048})), (1600, 900), None);
    assert_eq!(p.out, (1600, 900), "never larger than the window shows it");
    let p = picture::plan(&request(json!({"max_edge": 256})), (900, 1600), None);
    assert_eq!(p.out, (144, 256));
}

#[test]
fn the_world_source_follows_the_world_viewport() {
    // a viewport of the right two thirds of a 1800 x 900 window
    let p = picture::plan(&request(json!({"source": "world"})), (1800, 900), Some((1200, 900)));
    assert_eq!(p.out, (1024, 768));
    assert_eq!(p.render.size, (1024, 768));
    // without one, the window
    let p = picture::plan(&request(json!({"source": "world"})), (1800, 900), None);
    assert_eq!(p.out, (1024, 512));
}

#[test]
fn a_crop_keeps_its_part_and_draws_it_at_the_size_sent() {
    // the right half of a 1600 x 900 window, at its size in the window
    let r = request(json!({"crop": {"x": 0.5, "y": 0.0, "width": 0.5, "height": 1.0}}));
    let p = picture::plan(&r, (1600, 900), None);
    assert_eq!(p.out, (800, 900));
    assert_eq!(p.render.size, (1600, 900));
    assert_eq!(p.cut, [800, 0, 800, 900]);
    // a small crop asked large: drawn larger, within the render limit
    let r = request(json!({"width": 2048, "height": 2048, "crop": {"x": 0.45, "y": 0.45, "width": 0.1, "height": 0.1}}));
    let p = picture::plan(&r, (1600, 900), None);
    assert_eq!(p.out, (2048, 2048));
    assert!(p.render.size.0.max(p.render.size.1) <= picture::RENDER_MAX);
    let [x, y, w, h] = p.cut;
    assert!(x + w <= p.render.size.0 && y + h <= p.render.size.1, "inside the photo");
    assert!((w as i32 - h as i32).abs() <= 1, "square");
}

#[test]
fn the_interface_is_drawn_at_the_window_size() {
    let r = request(json!({"interface": true, "width": 640}));
    let p = picture::plan(&r, (1600, 900), Some((1000, 900)));
    assert!(p.render.interface && !p.render.huds);
    assert_eq!(p.render.size, (1600, 900));
    assert_eq!(p.out, (640, 360));
}

fn gradient(w: u32, h: u32) -> Vec<u8> {
    (0..h)
        .flat_map(|y| (0..w).flat_map(move |x| [(x * 255 / w) as u8, (y * 255 / h) as u8, ((x ^ y) & 0xff) as u8, 255]))
        .collect()
}

#[test]
fn a_picture_is_cut_scaled_and_encoded() {
    let r = request(json!({"width": 256, "height": 256}));
    let p = picture::plan(&r, (400, 200), None);
    let e = picture::finish(
        gradient(p.render.size.0, p.render.size.1),
        p.render.size,
        &p,
        Format::Jpeg,
        IMAGE_BYTES_MAX,
    )
    .expect("encoded");
    assert_eq!(e.size, (256, 256));
    assert_eq!(&e.bytes[..2], &[0xff, 0xd8], "a JPEG");
    assert_eq!(e.quality, Some(90));
    let decoded = image::load_from_memory(&e.bytes).expect("decodes");
    assert_eq!((decoded.width(), decoded.height()), (256, 256));
    let png = picture::finish(
        gradient(p.render.size.0, p.render.size.1),
        p.render.size,
        &p,
        Format::Png,
        IMAGE_BYTES_MAX,
    )
    .expect("png");
    assert_eq!(&png.bytes[1..4], b"PNG");
    // a photo back at another size than planned: the cut follows
    let half = (p.render.size.0 / 2, p.render.size.1 / 2);
    let e = picture::finish(gradient(half.0, half.1), half, &p, Format::Jpeg, IMAGE_BYTES_MAX).expect("encoded");
    assert_eq!(e.size, (256, 256));
    // pixels that do not match their size are refused
    assert!(picture::finish(vec![0; 10], (64, 64), &p, Format::Jpeg, IMAGE_BYTES_MAX).is_err());
}

#[test]
fn a_picture_over_the_byte_cap_steps_down_then_shrinks() {
    let noise: Vec<u8> = (0..512u32 * 512)
        .flat_map(|i| {
            let v = i.wrapping_mul(2_654_435_761);
            [(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8]
        })
        .collect();
    let image = image::RgbImage::from_raw(512, 512, noise).expect("image");
    let full = picture::encode_capped(image.clone(), Format::Jpeg, usize::MAX).expect("encoded");
    assert_eq!(full.quality, Some(90));
    // a little less room: a lower quality, the same size
    let lower = picture::encode_capped(image.clone(), Format::Jpeg, full.bytes.len() - 1).expect("encoded");
    assert!(lower.quality < Some(90));
    assert_eq!(lower.size, (512, 512));
    assert!(lower.bytes.len() < full.bytes.len());
    // far less: smaller too, always within the cap
    let cap = full.bytes.len() / 8;
    let small = picture::encode_capped(image.clone(), Format::Jpeg, cap).expect("encoded");
    assert!(small.size.0 < 512 && small.bytes.len() <= cap);
    let png = picture::encode_capped(image.clone(), Format::Png, 60_000).expect("png");
    assert!(png.bytes.len() <= 60_000 && png.size.0 < 512);
    // nothing fits: an error, not an endless loop
    assert!(picture::encode_capped(image, Format::Png, 10).is_err());
}

#[test]
fn saved_pictures_take_a_free_name_in_the_photo_folder() {
    let dir = tempfile::tempdir().expect("dir");
    let folder = dir.path().join("photos");
    let encoded = picture::Encoded {
        bytes: vec![1, 2, 3],
        size: (1, 1),
        format: Format::Png,
        quality: None,
    };
    let first = picture::save(&folder, "2026-11-11-120000", &encoded).expect("saved");
    assert_eq!(first, folder.join("assistant-2026-11-11-120000.png"));
    let second = picture::save(&folder, "2026-11-11-120000", &encoded).expect("saved");
    assert_eq!(second, folder.join("assistant-2026-11-11-120000-2.png"));
    assert_eq!(std::fs::read(second).expect("read"), vec![1, 2, 3]);
}

#[test]
fn the_thumbnail_goes_through_the_gallery_on_entries_that_may_change() {
    let inv = demo_inventory();
    for id in [COP_OUTFIT, DEMO_FOLDER, TEXTURE] {
        let planned = try_plan(&inv, stage(), "inventory_snapshot_thumbnail", json!({"id": id})).expect("planned");
        assert!(matches!(planned.work, Work::Thumbnail(t) if t == id), "{id}");
        assert!(!planned.summary.is_empty());
    }
    let by_path = try_plan(
        &inv,
        stage(),
        "inventory_snapshot_thumbnail",
        json!({"path": crate::mcp_bridge::tools::path(&inv, COP_OUTFIT)}),
    );
    assert!(matches!(by_path.expect("by path").work, Work::Thumbnail(t) if t == COP_OUTFIT));
    // the Library, the Current Outfit, My Inventory itself, the Marketplace
    let library = inv
        .items
        .values()
        .find(|it| crate::world::inventory::actions::library(&inv, it.id))
        .map_or(inv.lib_root, |it| it.id);
    for (id, code) in [
        (inv.lib_root, ErrorCode::NotAllowed),
        (library, ErrorCode::NotAllowed),
        (COF, ErrorCode::NotAllowed),
        (inv.root, ErrorCode::NotAllowed),
        (MARKETPLACE, ErrorCode::NotAllowed),
        (Uuid::from_u128(0xdead), ErrorCode::NotFound),
    ] {
        let e = try_plan(&inv, stage(), "inventory_snapshot_thumbnail", json!({"id": id})).expect_err("refused");
        assert_eq!(e.code, code, "{id}");
    }
    for bad in [json!({}), json!({"id": COP_OUTFIT, "path": "x"}), json!({"id": "nope"})] {
        assert_eq!(
            try_plan(&inv, stage(), "inventory_snapshot_thumbnail", bad.clone())
                .expect_err("refused")
                .code,
            ErrorCode::InvalidArguments,
            "{bad}"
        );
    }
}

fn call(tool: &str, args: Value, ask: bool) -> (Pending, Receiver<Result<Value, ToolError>>) {
    Pending::detached(tool, args, ask, Some("Claude Code"))
}

fn bake() -> Bake {
    Bake {
        outfit_change_pending: false,
        cof_version: 1,
        appearance_cof_version: 1,
        server_bakes: false,
    }
}

#[test]
fn a_camera_move_asks_in_plain_words_then_runs() {
    let inv = demo_inventory();
    let mut cx = ctx(&inv);
    cx.stage = stage();
    let now = Instant::now();
    let mut desk = Desk::default();
    // the levels the calls were asked under (the desk decides again with them)
    desk.set_permissions(folderstorm_mcp_bridge::Preset::AskBeforeChanges.permissions());
    let (c, answer) = call("camera_preset", json!({"preset": "full_body"}), true);
    desk.take(c, &cx, now);
    let q = desk.question(now).expect("a question");
    assert_eq!(q.client.as_deref(), Some("Claude Code"));
    assert!(q.summary.contains(aurora_i18n::tr!("fs-mcp-camera-full-body")), "{}", q.summary);
    desk.decide(q.number, true);
    let ready = desk.tick(&bake(), now);
    assert_eq!(ready.len(), 1);
    assert!(answer.try_recv().is_err(), "not answered before it runs");
    // a refusal by RLVa needs no question
    cx.stage.camera_locked = true;
    let (c, answer) = call("camera_reset", json!({}), true);
    desk.take(c, &cx, now);
    assert!(desk.question(now).is_none());
    assert_eq!(
        answer.try_recv().expect("answered").expect_err("refused").code,
        ErrorCode::NotAllowed
    );
}

fn pose(x: f32) -> Pose {
    Pose {
        position: Vec3::new(x, 0.0, 2.0),
        target: Vec3::ZERO,
        fov: 1.0,
    }
}

#[test]
fn a_camera_move_answers_once_the_camera_stops() {
    let mut studio = Studio::default();
    let t0 = Instant::now();
    studio.observe(pose(0.0), t0);
    let (c, answer) = call("camera_preset", json!({}), false);
    studio.wait_move(c, json!({"preset": "portrait", "done": true}), 0, t0);
    // the camera glides
    for i in 1..10 {
        let t = t0 + Duration::from_millis(16 * i);
        studio.observe(pose(i as f32 * 0.1), t);
        studio.tick(&pose(i as f32 * 0.1), t);
        assert!(answer.try_recv().is_err(), "still moving at frame {i}");
    }
    let stopped = t0 + Duration::from_millis(160);
    studio.observe(pose(0.9), stopped);
    studio.tick(&pose(0.9), stopped + STILL / 2);
    assert!(answer.try_recv().is_err());
    studio.observe(pose(0.9), stopped + STILL);
    studio.tick(&pose(0.9), stopped + STILL);
    let reply = answer.try_recv().expect("answered").expect("done");
    assert_eq!(reply["preset"], "portrait");
    assert_eq!(reply["camera"], json!([0.9, 0.0, 2.0]));
    assert_eq!(reply["target"], json!([0.0, 0.0, 0.0]));
    assert!(reply["fov_degrees"].as_f64().is_some_and(|f| (f - 57.296).abs() < 0.01));
    // a camera that never stops (it follows a walking avatar): 3 s at most
    let (c, answer) = call("camera_reset", json!({}), false);
    studio.wait_move(c, json!({"done": true}), 0, t0);
    let mut t = t0;
    while t < t0 + SETTLE_MAX {
        t += Duration::from_millis(100);
        studio.observe(pose(t.duration_since(t0).as_secs_f32()), t);
        studio.tick(&pose(0.0), t);
    }
    assert!(answer.try_recv().expect("answered").is_ok());
}

#[test]
fn a_picture_waits_for_the_camera_and_other_photos_then_comes_back_as_an_image() {
    let mut studio = Studio::default();
    let t0 = Instant::now();
    studio.observe(pose(0.0), t0);
    let r = request(json!({"width": 128, "height": 64, "save": false}));
    let (c, answer) = call("camera_snapshot", json!({}), false);
    studio.queue_photo(c, r, 0, t0);
    assert_eq!(studio.photo_step(false, t0), PhotoStep::Idle, "the camera just moved");
    let still = t0 + STILL;
    studio.observe(pose(0.0), still);
    assert_eq!(studio.photo_step(true, still), PhotoStep::Idle, "another photo is being drawn");
    assert_eq!(studio.photo_step(false, still), PhotoStep::Start(r));
    let plan = picture::plan(&r, (400, 200), None);
    studio.photo_started(plan);
    assert_eq!(studio.photo_step(true, still), PhotoStep::Idle, "its own photo is being drawn");
    studio.photo_drawn(
        plan.render.size,
        gradient(plan.render.size.0, plan.render.size.1),
        std::env::temp_dir(),
        "unused".into(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let reply = loop {
        studio.tick(&pose(0.0), still);
        if let Ok(reply) = answer.try_recv() {
            break reply.expect("a picture");
        }
        assert!(Instant::now() < deadline, "the worker answers");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!((reply["width"].clone(), reply["height"].clone()), (json!(128), json!(64)));
    assert_eq!(reply["format"], "jpeg");
    assert_eq!(reply["image"]["mime_type"], "image/jpeg");
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(reply["image"]["data"].as_str().expect("base64"))
        .expect("decodes");
    assert_eq!(reply["bytes"], bytes.len());
    let decoded = image::load_from_memory(&bytes).expect("an image");
    assert_eq!((decoded.width(), decoded.height()), (128, 64));
    assert!(reply.get("saved_to").is_none());
    assert_eq!(studio.counts(), (0, 0, 0, false));
}

#[test]
fn a_picture_whose_photo_is_lost_or_late_fails() {
    let mut studio = Studio::default();
    let t0 = Instant::now();
    studio.observe(pose(0.0), t0);
    let r = request(json!({}));
    let (c, answer) = call("camera_snapshot", json!({}), false);
    studio.queue_photo(c, r, 0, t0);
    // the camera keeps moving: it starts after 3 s all the same
    let late = t0 + SETTLE_MAX;
    studio.observe(pose(1.0), late);
    assert_eq!(studio.photo_step(false, late), PhotoStep::Start(r));
    studio.photo_started(picture::plan(&r, (400, 200), None));
    // the photo job ended without its pixels (the GPU was lost)
    assert_eq!(studio.photo_step(false, late), PhotoStep::Idle);
    assert_eq!(answer.try_recv().expect("answered").expect_err("lost").code, ErrorCode::Failed);
    // one that never starts: given up before the bridge's wait ends
    let (c, answer) = call("camera_snapshot", json!({}), false);
    studio.queue_photo(c, r, 0, t0);
    studio.tick(&pose(0.0), t0 + super::studio::PHOTO_DEADLINE);
    assert_eq!(answer.try_recv().expect("answered").expect_err("late").code, ErrorCode::Timeout);
    // pixels arriving for a picture given up are dropped
    studio.photo_drawn((4, 4), vec![0; 64], std::env::temp_dir(), "x".into());
    assert_eq!(studio.counts(), (0, 0, 0, false));
}

#[test]
fn the_thumbnail_answers_when_the_gallery_snapshot_ends() {
    let mut studio = Studio::default();
    let t0 = Instant::now();
    let (c, answer) = call("inventory_snapshot_thumbnail", json!({}), false);
    studio.thumbnail_started(c, json!({"id": COP_OUTFIT}), 0, t0);
    assert!(studio.thumbnail_busy());
    studio.thumbnail_done(None);
    let reply = answer.try_recv().expect("answered").expect("done");
    assert_eq!(reply, json!({"id": COP_OUTFIT, "done": true}));
    // the user's own gallery snapshot ending is not the assistant's
    studio.thumbnail_done(Some("ignored"));
    let (c, answer) = call("inventory_snapshot_thumbnail", json!({}), false);
    studio.thumbnail_started(c, json!({}), 0, t0);
    studio.thumbnail_done(Some("This item can no longer be modified"));
    let e = answer.try_recv().expect("answered").expect_err("failed");
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(e.message.contains("no longer be modified"));
    let (c, answer) = call("inventory_snapshot_thumbnail", json!({}), false);
    studio.thumbnail_started(c, json!({}), 0, t0);
    studio.tick(&pose(0.0), t0 + super::studio::THUMBNAIL_DEADLINE);
    assert_eq!(answer.try_recv().expect("answered").expect_err("late").code, ErrorCode::Timeout);
    assert!(!studio.thumbnail_busy());
}

// Folderstorm: the studio's waiting calls decided again by the desk's rules
#[cfg(test)]
mod recheck;
