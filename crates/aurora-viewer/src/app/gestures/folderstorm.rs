//! Folderstorm: the local assistant's gestures go through the Gestures
//! window's own paths (folderstorm/plans/mcp-gestures.md): a gesture is
//! loaded as for the window, never activated (`gesture_request` with
//! nothing to open, play or activate), played under a key of the
//! assistant's own as the gesture editor's Preview plays it
//! (`gesture_preview`), and stopped with only the animations it alone holds
//! (`Manager::stop_owned`), its effects sent by `gesture_effects`.

use super::super::App;
use aurora_assets::gesture::Gesture;
use std::collections::BTreeSet;
use uuid::Uuid;

impl App {
    /// Load gesture `item` (the original, not a link) if it is not loaded.
    pub(in crate::app) fn assistant_gesture_load(&mut self, item: Uuid) {
        self.gesture_request(item, false, false, None, false);
    }

    /// The gestures whose asset could not be loaded: (item, asset).
    pub(in crate::app) fn assistant_gesture_failures(&self) -> &BTreeSet<(Uuid, Uuid)> {
        &self.gesture_state.failed
    }

    /// Play `definition` once under `key` (never a gesture's item id).
    pub(in crate::app) fn assistant_gesture_play(&mut self, key: Uuid, definition: Gesture) {
        self.gesture_preview(key, definition, None);
    }

    /// Stop the playback under `key`, keeping what another one holds.
    pub(in crate::app) fn assistant_gesture_stop(&mut self, key: Uuid) {
        let effects = self.gesture_state.manager.stop_owned(key);
        self.gesture_effects(effects);
    }
}
