//! Gapura's core library.
//!
//! This crate holds everything that does not need a UI framework: the domain
//! model, the encrypted vault, and (in later modules) the SSH layer. The
//! Tauri shell is a thin adapter over these pieces, and a future CLI would
//! reuse them unchanged.
//!
//! Two house rules (design doc, sections 1.3 and 7):
//!
//! 1. This crate must never depend on `tauri`.
//! 2. Secret values only exist inside `SecretString` or the vault crypto
//!    boundary; errors and logs must not carry them.

pub mod error;
pub mod import_ssh_config;
pub mod model;
pub mod paths;
pub mod settings;
pub mod ssh;
pub mod vault;

pub use error::{GapuraError, Result};
