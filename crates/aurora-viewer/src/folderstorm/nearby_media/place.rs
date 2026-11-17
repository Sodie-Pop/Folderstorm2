//! Folderstorm: where a nearby media source is, for the panel's "Show" list
//! (LLPanelNearByMedia::shouldShow, refreshParcelItems; the `show_combo` of
//! panel_nearby_media.xml) and its zoom button
//! (LLPanelNearByMedia::onClickSelectedMediaZoom / Unzoom, which call
//! LLViewerMediaFocus::focusZoomOnMedia / unZoom).

use super::{Row, Source};
use crate::camera::{Camera, CameraSettings};
use crate::scene::Scene;
use crate::world::World;
use glam::{Mat4, Vec3};
use std::time::Instant;
use uuid::Uuid;

/// The "Show" list (MediaClass, without MEDIA_CLASS_FOCUSED, which the
/// list does not offer). The Legacy Viewer keeps the choice while the
/// viewer runs and does not save it (the combo box has no control name):
/// so does the panel, in egui's memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Filter {
    #[default]
    All,
    /// "In this Parcel" (MEDIA_CLASS_WITHIN_PARCEL).
    WithinParcel,
    /// "Outside this Parcel" (MEDIA_CLASS_OUTSIDE_PARCEL).
    OutsideParcel,
    /// "On other Avatars" (MEDIA_CLASS_ON_OTHERS).
    OnOthers,
}

impl Filter {
    /// The list's order.
    pub const ALL: [Filter; 4] = [Filter::All, Filter::WithinParcel, Filter::OutsideParcel, Filter::OnOthers];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => aurora_i18n::tr!("fs-nearby-media-filter-all"),
            Filter::WithinParcel => aurora_i18n::tr!("fs-nearby-media-filter-within"),
            Filter::OutsideParcel => aurora_i18n::tr!("fs-nearby-media-filter-outside"),
            Filter::OnOthers => aurora_i18n::tr!("fs-nearby-media-filter-others"),
        }
    }

    /// Whether the row is listed: the parcel's media and music with All and
    /// In this Parcel only (refreshParcelItems), a media on an object by
    /// where its object is (shouldShow). As there, the classes overlap: a
    /// media worn by another avatar standing on our parcel is both in this
    /// parcel and on another avatar.
    pub fn shows(self, row: &Row) -> bool {
        match (self, row.source) {
            (Filter::All, _) => true,
            (Filter::WithinParcel, _) => row.place.in_parcel,
            (Filter::OutsideParcel, Source::Prim { .. }) => !row.place.in_parcel,
            (Filter::OnOthers, Source::Prim { .. }) => row.place.on_other_avatar,
            (Filter::OutsideParcel | Filter::OnOthers, _) => false,
        }
    }
}

/// Where a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Place {
    /// On the agent's parcel (LLViewerMediaImpl::isInAgentParcel).
    pub in_parcel: bool,
    /// Worn by another avatar (isAttachedToAnotherAvatar).
    pub on_other_avatar: bool,
    /// The camera can frame it: an object in the world, not a HUD.
    pub zoomable: bool,
    /// The camera is on it (isZoomedOnMedia): its button zooms back out.
    pub zoomed: bool,
}

impl Place {
    /// The parcel's own media and music: in the parcel, nothing to zoom on.
    pub const PARCEL: Place = Place {
        in_parcel: true,
        on_other_avatar: false,
        zoomable: false,
        zoomed: false,
    };
}

/// LLViewerParcelMgr::inAgentParcel for a media's object: a HUD rides with
/// us, so it counts as here; otherwise its position (render space, the
/// agent's region's metres) on the cells of the agent's parcel. Without the
/// parcel's cells or the object's position, not in it, as there.
pub fn in_agent_parcel(hud: bool, position: Option<Vec3>, parcel_cells: Option<&[u8]>) -> bool {
    if hud {
        return true;
    }
    match (position, parcel_cells) {
        (Some(pos), Some(cells)) => crate::world::landmarks::in_parcel(cells, (0, 0), pos.as_dvec3()),
        _ => false,
    }
}

/// The avatar wearing object `idx` (isObjectAttachedToAnotherAvatar walks
/// up the parents the same way).
fn wearer(world: &World, mut idx: usize) -> Option<Uuid> {
    for _ in 0..16 {
        let o = world.objects.get(idx)?;
        if o.is_avatar() {
            return Some(o.full_id);
        }
        idx = world.objects.parent_of(o)?;
    }
    None
}

/// Where the media on `object` is, and whether the camera (focused on
/// `camera_focus`) is on it.
pub fn place_of(world: &World, camera_focus: Option<Uuid>, object: Uuid, now: Instant) -> Place {
    let Some(idx) = world.objects.index_of_uuid(&object) else {
        return Place::default();
    };
    let xf = Scene::object_transform(world, idx, now, 0);
    let hud = xf.is_some_and(|x| x.2);
    let on_other_avatar = wearer(world, idx).is_some_and(|a| a != world.agent_id);
    let in_parcel = in_agent_parcel(hud, xf.map(|x| x.0), world.parcel.as_deref().map(|p| p.bitmap.as_slice()));
    // the camera follows what a zoom on it follows (an attachment: its avatar)
    let target = if hud {
        None
    } else {
        crate::camera::pick_focus_object(world, idx, now).map(|t| t.0)
    };
    Place {
        in_parcel,
        on_other_avatar,
        zoomable: target.is_some(),
        zoomed: target.is_some() && target == camera_focus,
    }
}

/// The box (centre, size) in world space of a box given in an object's own
/// space by its centre and half size, through the object's model matrix.
pub fn world_box(model: Mat4, center: Vec3, half: Vec3) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for i in 0..8 {
        let sign = Vec3::new(
            if i & 1 == 0 { -1.0 } else { 1.0 },
            if i & 2 == 0 { -1.0 } else { 1.0 },
            if i & 4 == 0 { -1.0 } else { 1.0 },
        );
        let p = model.transform_point3(center + half * sign);
        min = min.min(p);
        max = max.max(p);
    }
    ((min + max) * 0.5, max - min)
}

/// focusZoomOnMedia: the camera frames the media's face, `face_box` (centre
/// and size, see [`world_box`]), else its whole object, and keeps its object
/// as the focus so it follows it. Aurora's own zoom on an object
/// (CLICK_ACTION_ZOOM, `Camera::zoom_object`) does the framing; it comes
/// from where the camera is rather than along the face's normal. False when
/// the object is gone or a HUD.
pub fn zoom_on(
    camera: &mut Camera,
    world: &mut World,
    object: Uuid,
    face_box: Option<(Vec3, Vec3)>,
    aspect: f32,
    settings: &CameraSettings,
    now: Instant,
) -> bool {
    let Some(idx) = world.objects.index_of_uuid(&object) else {
        return false;
    };
    let Some((pos, _, hud)) = Scene::object_transform(world, idx, now, 0) else {
        return false;
    };
    if hud {
        return false;
    }
    let (center, extent) = face_box
        .filter(|(c, e)| c.is_finite() && e.is_finite() && e.length() > 0.0)
        .unwrap_or_else(|| (pos, world.visual_scale_at(idx, now)));
    let focus = crate::camera::pick_focus_object(world, idx, now);
    camera.zoom_object(center, extent, focus, aspect, &mut world.agent, settings);
    true
}

/// unZoom (LLPanelPrimMediaControls::resetZoomLevel): the camera's focus
/// back on the avatar, animated.
pub fn unzoom(camera: &mut Camera, world: &mut World, settings: &CameraSettings) {
    camera.set_focus_on_avatar(true, true, true, &mut world.agent, settings);
}
