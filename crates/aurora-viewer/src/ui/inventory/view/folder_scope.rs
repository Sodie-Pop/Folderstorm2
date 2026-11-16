//! Folderstorm: a custom Inventory tab scoped to chosen folders (feature
//! 2.2, `custom_tabs/folders.rs`), in the search.
//!
//! A child module of `view`, so that it reaches the cache's private fields
//! without widening them. `custom_tabs::folder_ui::before_list` hands the
//! scope over once a frame: None for every list but a scoped custom tab.

use super::*;

impl ViewCache {
    /// The live folders the next searches keep, with all they hold (None:
    /// no scope). A change searches again.
    pub(in crate::ui::inventory) fn set_folder_scope(&mut self, roots: Option<Vec<Uuid>>) {
        if self.folder_scope != roots {
            self.folder_scope = roots;
            self.query = None;
        }
    }

    /// The folders searched, cut to the scope and everything below it
    /// (`LLInventoryFilter::passesViewFolders`): the result tree adds their
    /// parents as the path, as for any result.
    pub(super) fn within_folder_scope(&self, inv: &Inventory, mut scope: IdSet) -> IdSet {
        if let Some(roots) = &self.folder_scope {
            let under = within(inv, roots);
            scope.retain(|id| under.contains(id));
        }
        scope
    }

    /// A folder chosen for the scope is shown even where the search hides
    /// it otherwise: the outfit folders. The Trash and the Library are never
    /// a scope (`custom_tabs::folders::status`).
    pub(super) fn folder_scope_query(&self, query: Query) -> Query {
        if self.folder_scope.is_some() {
            Query { outfits: true, ..query }
        } else {
            query
        }
    }
}
