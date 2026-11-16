//! Folderstorm: Inventory folder windows as a workspace sees them
//! (`crate::folderstorm::workspaces`, FEATURES.md row 1.4;
//! folderstorm/plans/workspaces.md, checkpoint 6 part 2). Folderstorm (C++):
//! "Enable Remember Inventory folders before saving … to include the folder
//! root … of each single-folder Inventory window" (doc/help.md
//! "Workspaces").
//!
//! A folder window (`InventoryUi::open_folder_window`) has a random id each
//! time it is opened, so a workspace knows it by its folder. This file only
//! lists them, opens one and tells its window's id, and closes one: nothing
//! of what a window shows is read.

use super::{InventoryUi, floater_id};
use uuid::Uuid;

impl InventoryUi {
    /// The folder windows that are open: each window's id (as its floater
    /// is made with) and its folder.
    pub(crate) fn folderstorm_folder_windows(&self) -> Vec<(String, Uuid)> {
        self.windows
            .iter()
            .filter(|w| w.open && !w.additional && !w.root.is_nil())
            .map(|w| (floater_id(w.id), w.root))
            .collect()
    }

    /// Opens a window on the folder `root`, as *Open in a new window* does;
    /// the id of its floater, so that it can be placed.
    pub(crate) fn folderstorm_open_folder(&mut self, root: Uuid) -> Option<String> {
        self.open_folder_window(root);
        self.windows.last().filter(|w| w.root == root).map(|w| floater_id(w.id))
    }

    /// Closes the folder window `window`, as its cross does.
    pub(crate) fn folderstorm_close_folder(&mut self, window: &str) {
        for w in &mut self.windows {
            if !w.additional && !w.root.is_nil() && floater_id(w.id) == window {
                w.open = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_windows_are_listed_opened_and_closed_by_their_folder() {
        let ctx = egui::Context::default();
        let mut st = InventoryUi::default();
        st.open_additional_window(&ctx);
        assert!(st.folderstorm_folder_windows().is_empty(), "not the additional window");
        let (a, b) = (Uuid::from_u128(8000), Uuid::from_u128(8001));
        let first = st.folderstorm_open_folder(a).expect("opened");
        let second = st.folderstorm_open_folder(b).expect("opened");
        assert_ne!(first, second);
        assert!(first.starts_with("inventory_"));
        assert_eq!(st.folderstorm_folder_windows(), [(first.clone(), a), (second.clone(), b)]);
        st.folderstorm_close_folder(&first);
        assert_eq!(st.folderstorm_folder_windows(), [(second, b)]);
        assert!(st.additional_open(), "the additional window is not a folder window");
    }
}
