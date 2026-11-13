//! Command results. [`GapuraError`] serializes itself as
//! `{code, message, ...}` (impl lives in gapura-core), so Tauri can ship it
//! to the webview directly and the UI can branch on the stable code.

pub use gapura_core::GapuraError;

pub type CommandResult<T> = Result<T, GapuraError>;
