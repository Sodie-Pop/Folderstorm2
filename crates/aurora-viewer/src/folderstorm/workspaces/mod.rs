//! Folderstorm: layouts (FEATURES.md row 1.3; folderstorm/plans/workspaces.md).
//!
//! Folderstorm (C++) saved the placement of its bars around the viewport as
//! named profiles: apply, rename, delete, Reset (doc/help.md "Wide-screen
//! layout"), and kept window places in its workspaces. Here a **layout**
//! says where things go, in two optional parts: the world viewport with the
//! bars' place, and the position and size of each window of an allowlist.
//! Which windows are open is a workspace's business (a later checkpoint),
//! and so is the viewer window itself.
//!
//! - [`store`]: what `settings.json` keeps, its limits, the list.
//! - [`windows`]: the allowlist.
//! - [`fit`]: a saved place in an interface of another size or scale.
//!
//! A layout never holds what a window contains: no message, conversation,
//! Inventory folder or item, filter or selection has a field to go in.

pub mod fit;
pub mod store;
pub mod windows;

pub use store::Store;
