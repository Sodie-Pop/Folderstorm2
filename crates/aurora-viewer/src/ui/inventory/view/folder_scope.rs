//! Folderstorm: a custom Inventory tab scoped to chosen folders (feature
//! 2.2, `custom_tabs/folders.rs`), its pinned items and its flat list
//! (feature 2.13, `custom_tabs/items.rs`, `custom_tabs/flat.rs`), in the
//! search.
//!
//! A child module of `view`, so that it reaches the cache's private fields
//! without widening them. `custom_tabs::folder_ui::before_list` hands the
//! scope over once a frame: none for every list but a custom tab.

use super::*;

/// What a custom tab narrows the search to.
#[derive(Default, PartialEq, Eq)]
pub(in crate::ui::inventory) struct Scope {
    /// The live folders kept, with all they hold (None: no folder scope).
    roots: Option<Vec<Uuid>>,
    /// The live pinned items, shown at the top whatever their folder.
    pins: IdSet,
    /// Items only, in one list (`custom_tabs/flat.rs`).
    flat: bool,
}

impl ViewCache {
    /// The live folders the next searches keep, with all they hold (None:
    /// no scope). A change searches again.
    #[cfg(test)]
    pub(in crate::ui::inventory) fn set_folder_scope(&mut self, roots: Option<Vec<Uuid>>) {
        self.set_tab_scope(roots, Vec::new(), false);
    }

    /// The folders kept (None: no folder scope), the pinned items shown at
    /// the top, and whether the list is flat. A change searches again.
    pub(in crate::ui::inventory) fn set_tab_scope(&mut self, roots: Option<Vec<Uuid>>, pins: Vec<Uuid>, flat: bool) {
        let scope = Scope {
            // pinned items alone scope the tab to themselves
            roots: if roots.is_none() && !pins.is_empty() {
                Some(Vec::new())
            } else {
                roots
            },
            pins: pins.into_iter().collect(),
            flat,
        };
        if self.folder_scope != scope {
            self.folder_scope = scope;
            self.query = None;
        }
    }

    /// The folders searched, cut to the scope and everything below it
    /// (`LLInventoryFilter::passesViewFolders`): the result tree adds their
    /// parents as the path, as for any result.
    pub(super) fn within_folder_scope(&self, inv: &Inventory, mut scope: IdSet) -> IdSet {
        if let Some(roots) = &self.folder_scope.roots {
            let under = within(inv, roots);
            scope.retain(|id| under.contains(id));
        }
        scope
    }

    /// An entry is searched when its folder is in `scope`, or it is pinned.
    pub(super) fn in_scope(&self, scope: &IdSet, entry: &Entry) -> bool {
        scope.contains(if entry.folder { &entry.id } else { &entry.parent })
            || (!entry.folder && self.folder_scope.pins.contains(&entry.id))
    }

    /// A folder chosen for the scope is shown even where the search hides
    /// it otherwise: the outfit folders (and a pinned item in one). The
    /// Trash and the Library are never a scope
    /// (`custom_tabs::folders::status`).
    pub(super) fn folder_scope_query(&self, query: Query) -> Query {
        if self.folder_scope.roots.is_some() {
            Query { outfits: true, ..query }
        } else {
            query
        }
    }

    /// A result whose folder path is not added to the tree: a pinned item
    /// (shown at the top) and, in a flat list, every result.
    pub(super) fn skips_path(&self, id: &Uuid) -> bool {
        self.folder_scope.flat || self.folder_scope.pins.contains(id)
    }

    /// Once the result tree is built: the pinned items at the top of the
    /// list, in the order of the sort; a flat list keeps only the items,
    /// pinned first, all directly under `root`.
    pub(super) fn quick_access_tree(&mut self, root: Uuid) {
        let scope = &self.folder_scope;
        if !scope.flat && scope.pins.is_empty() {
            return;
        }
        let pinned: Vec<Uuid> = self.results.iter().filter(|id| scope.pins.contains(*id)).copied().collect();
        if scope.flat {
            let folders = &self.folders;
            self.results.retain(|id| !folders.contains(id));
            let mut list = pinned;
            list.extend(self.results.iter().filter(|id| !scope.pins.contains(*id)));
            self.result_children.clear();
            if !list.is_empty() {
                self.result_children.insert(root, list);
            }
            self.holding = None;
        } else if !pinned.is_empty() {
            self.result_children.entry(root).or_default().splice(0..0, pinned);
        }
    }

    /// The list is flat (no folder rows).
    pub(in crate::ui::inventory) fn flat(&self) -> bool {
        self.folder_scope.flat
    }
}
