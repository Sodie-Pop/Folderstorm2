//! Folderstorm2's own features, kept apart from Aurora's code so that
//! upstream merges stay small (folderstorm/AGENT_RULES.md). Aurora's files
//! reach them through one-line hooks marked `// Folderstorm:`.

pub mod bulk_plan;
pub mod chat_upload;
pub mod diagram;
pub mod viewport;
