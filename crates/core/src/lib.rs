//! OCB Inspector core: BIOS schema extraction, lossless OCB handling, reviewed setting
//! recipes, validation, migration and verified writing. Used by the `ocbi` CLI and the desktop app.

pub mod approvals;
pub mod bytes;
pub mod catalog;
pub mod contracts;
pub mod engine;
pub mod error;
pub mod firmware;
pub mod format;
pub mod io;
pub mod settings;

pub use error::{Error, Result};
