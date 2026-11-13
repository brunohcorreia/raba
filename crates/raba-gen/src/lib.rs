//! Theme generation: art in, `.raba` out.
//!
//! Build-time only. This never enters an initramfs, so it may depend on whatever is
//! convenient and may be as strict and as talkative as it likes — every error here is
//! one the boot path will never have to survive. See `docs/maintenance.md`.

pub mod ansi;
pub mod art;
pub mod build;
pub mod compose;
pub mod cp437;
pub mod dur;
pub mod durwrite;
pub mod generate;
pub mod import;
pub mod layout;
pub mod logos;
pub mod palette;
pub mod recipe;
pub mod text;
