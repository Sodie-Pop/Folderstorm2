//! Names on the mini-map for residents the scene does not show.
//!
//! The simulator tells the viewer where a resident beyond draw distance is (a
//! coarse location) but sends no object for them, and a private parcel hides
//! the object of one who is there. Upstream's `avatar_name` showed "…" for
//! every avatar that `visible_avatar` did not return. The Legacy Viewer names
//! them all: `LLNetMap::handleToolTip` asks `LLAvatarNameCache::get` for the
//! coarse-location avatars. Second Life's "Managing your parcel" article says
//! that parcel avatar privacy does not touch the maps: "The world map and
//! mini-map are unaffected." `ui/minimap.rs` reaches this module through a
//! hook in `avatar_name`; only RLVa and the avatar-name scope hide a name.

use crate::world::World;
use crate::world::names::{AvatarName, NameOptions};
use uuid::Uuid;

/// Shown until a name arrives, and in place of any name the map may not give.
const UNKNOWN: &str = "…";

/// The name of a mini-map avatar that `visible_avatar` did not return: out of
/// draw distance, or hidden by a private parcel (the maps ignore that).
///
/// - When names are allowed
///   (`rlva_can_name` and the avatar-name scope) its name comes from the
///   avatar-name cache, formatted by `format` in the chosen
///   `MiniAvatarNameFormat`, else from the legacy `UUIDNameReply` names. A
///   missing name is requested through the avatar-name cache (`get` queues
///   it, like a visible avatar's name) and "…" shows until it arrives.
/// - When RLVa (or the name scope) hides names, the anonymous name, as for a visible avatar.
pub fn unseen_avatar_name(world: &World, id: &Uuid, format: impl FnOnce(&AvatarName, &NameOptions) -> String) -> String {
    let names = &world.social.avatar_names;
    if !(world.rlva_can_name(*id) && names.shows(crate::world::names::NameScope::World, id)) {
        return names.anonymous(id).display_name.clone();
    }
    // `get` queues a lookup for a missing or expired name.
    names
        .get(id)
        .map(|name| format(name, &names.options))
        .or_else(|| world.social.names.get(id).filter(|name| !name.is_empty()).cloned())
        .unwrap_or_else(|| UNKNOWN.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn world() -> World {
        let mut world = World::new(std::sync::Arc::new(crate::scene::avatar::AvatarLibrary::load()));
        world.agent_id = Uuid::from_u128(1);
        world
    }

    fn cache_name(world: &mut World, id: Uuid) {
        world.social.avatar_names.on_display_names(
            &[aurora_net::AvatarNameData {
                id,
                username: "john.smith".into(),
                display_name: "Captain Jack".into(),
                legacy_first: "John".into(),
                legacy_last: "Smith".into(),
                ..Default::default()
            }],
            &[],
            Some(3600),
        );
    }

    fn complete(name: &AvatarName, options: &NameOptions) -> String {
        name.complete(options)
    }

    #[test]
    fn a_cached_name_shows_beyond_draw_distance_in_the_chosen_format() {
        let mut world = world();
        let id = Uuid::from_u128(42);
        cache_name(&mut world, id);
        assert_eq!(unseen_avatar_name(&world, &id, complete), "Captain Jack (John Smith)");
        assert_eq!(unseen_avatar_name(&world, &id, |n, o| n.display(o)), "Captain Jack");
        assert!(world.social.avatar_names.take_asks().is_empty());
    }

    #[test]
    fn the_legacy_name_stands_in_when_the_display_name_is_not_cached() {
        let mut world = world();
        let id = Uuid::from_u128(42);
        world.social.names.insert(id, "John Smith".into());
        assert_eq!(unseen_avatar_name(&world, &id, complete), "John Smith");
    }

    #[test]
    fn an_unknown_name_shows_an_ellipsis_and_starts_one_lookup() {
        let world = world();
        let id = Uuid::from_u128(42);
        assert_eq!(unseen_avatar_name(&world, &id, complete), "…");
        let mut world = world;
        assert_eq!(world.social.avatar_names.take_asks(), vec![id]);
        cache_name(&mut world, id);
        assert_eq!(unseen_avatar_name(&world, &id, complete), "Captain Jack (John Smith)");
    }

    #[test]
    fn rlva_hiding_names_gives_the_anonymous_name() {
        let mut world = world();
        let (cached, unknown) = (Uuid::from_u128(42), Uuid::from_u128(43));
        cache_name(&mut world, cached);
        world.social.names.insert(unknown, "Other Resident".into());
        world.social.avatar_names.set_rlva_privacy(true, Default::default());
        for id in [cached, unknown] {
            let anonymous = world.social.avatar_names.anonymous(&id).display_name.clone();
            assert_eq!(unseen_avatar_name(&world, &id, complete), anonymous);
            assert_ne!(anonymous, "…");
        }
        let _ = world.social.avatar_names.take_asks();
        unseen_avatar_name(&world, &unknown, complete);
        assert!(world.social.avatar_names.take_asks().is_empty());
    }

    #[test]
    fn an_avatar_hidden_by_a_private_parcel_is_named_on_the_map() {
        let mut world = world();
        for event in crate::demo::events() {
            world.apply(event);
        }
        let center = glam::Vec3::new(130.0, 130.0, 25.0);
        for event in crate::demo::minimap_name_events(center) {
            world.apply(event);
        }
        let luna = crate::demo::u(0x327701);
        let mut settings = crate::settings::Settings::default();
        crate::demo::parcel_visibility::install(&mut world, &mut settings, "always");
        world.agent.get_off(center);
        world.parcel = Some(crate::demo::parcel_visibility::parcel_at(center.x, center.y));
        let _ = world.update_parcel_visibility(&settings.parcel_visibility, Instant::now());
        let index = world.objects.index_of_uuid(&luna).expect("still loaded");
        assert!(!world.parcel_object_visible(index));
        // SL: "The world map and mini-map are unaffected" by parcel privacy.
        assert_eq!(unseen_avatar_name(&world, &luna, |n, o| n.display(o)), "Luna \u{2726}");
        // RLVa still wins.
        world.social.avatar_names.set_rlva_privacy(true, Default::default());
        assert_eq!(
            unseen_avatar_name(&world, &luna, complete),
            world.social.avatar_names.anonymous(&luna).display_name
        );
    }
}
