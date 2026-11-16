//! Folderstorm: what the local assistant's gesture playback needs of the
//! playback manager (folderstorm/plans/mcp-gestures.md). The assistant
//! plays a gesture under a key of its own, as the gesture editor's Preview
//! does; its stop releases only the animations no other playback (a
//! manual gesture) still holds, so a gesture of the user's is never cut by
//! the assistant's.

use super::{Effect, Manager};
use std::collections::BTreeSet;
use uuid::Uuid;

impl Manager {
    /// Stop playback `key`, like [`Manager::stop`], except for the
    /// animations another playback still requested or plays.
    pub fn stop_owned(&mut self, key: Uuid) -> Vec<Effect> {
        let Some(stopped) = self.playing.remove(&key) else {
            return Vec::new();
        };
        let held: BTreeSet<Uuid> = self
            .playing
            .values()
            .flat_map(|other| other.requested.iter().chain(&other.observed))
            .copied()
            .collect();
        stopped
            .requested
            .union(&stopped.observed)
            .filter(|anim| !held.contains(anim))
            .map(|anim| Effect::Animation(*anim, false))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Effect, Manager};
    use aurora_assets::gesture::{Gesture, Step};
    use std::collections::BTreeSet;
    use std::time::Instant;
    use uuid::Uuid;

    fn animation(anim: Uuid) -> Gesture {
        Gesture {
            steps: vec![Step::Animation {
                name: "wave".into(),
                asset: anim,
                flags: 0,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn the_assistant_stop_keeps_an_animation_another_gesture_plays() {
        let (shared, own) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut manager = Manager::default();
        let (manual, assistant) = (Uuid::from_u128(10), Uuid::from_u128(11));
        manager.play(manual, animation(shared), None);
        let mut both = animation(shared);
        both.steps.extend(animation(own).steps);
        manager.play(assistant, both, None);
        let now = Instant::now();
        let started = manager.tick(now, &BTreeSet::new());
        assert!(started.contains(&Effect::Animation(shared, true)));
        assert!(started.contains(&Effect::Animation(own, true)));
        // only its own animation stops; the manual gesture goes on
        assert_eq!(manager.stop_owned(assistant), [Effect::Animation(own, false)]);
        assert!(manager.is_playing(manual) && !manager.is_playing(assistant));
        assert!(manager.stop_owned(assistant).is_empty());
        // alone, it stops all of its animations
        manager.play(assistant, animation(shared), None);
        let _ = manager.stop(manual);
        let _ = manager.tick(now, &BTreeSet::new());
        assert_eq!(manager.stop_owned(assistant), [Effect::Animation(shared, false)]);
    }
}
