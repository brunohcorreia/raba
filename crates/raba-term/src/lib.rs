//! Terminal surfaces and playback: the impure half of the renderer.
//!
//! `raba-core` is pure by rule — no I/O, no syscalls — so the code that owns a
//! real terminal cannot live there. Both binaries need it: `raba` to play a
//! splash, `raba-gen` to show an author what they just built. Keeping one copy
//! here rather than one in each is what stops the two previews drifting apart,
//! and the frame-timing and cursor-parking logic is exactly the kind that drifts
//! silently.
//!
//! This crate is std-only and must stay that way. `raba` links it, so anything
//! added here ends up in an initramfs. See `docs/maintenance.md`.

pub mod play;
pub mod surface;

pub use play::{clear, play, DEFAULT_DELAY_MS};
pub use surface::StdoutSurface;
