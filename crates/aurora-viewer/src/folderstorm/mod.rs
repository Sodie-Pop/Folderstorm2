//! Folderstorm2's own features, kept apart from Aurora's code so that
//! upstream merges stay small (folderstorm/AGENT_RULES.md). Aurora's files
//! reach them through one-line hooks marked `// Folderstorm:`.

pub mod background_alerts;
pub mod backgrounds;
pub mod bulk_plan;
pub mod chat_upload;
pub mod desktop;
pub mod diagram;
pub mod folder_upload;
pub mod gesture_boards;
pub mod keys;
pub mod login_screen;
pub mod photo_viewport;
pub mod rez_overrides;
pub mod viewport;
pub mod workspaces;
