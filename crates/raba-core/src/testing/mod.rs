//! Test doubles and oracles. Gated behind the `testing` feature; never in `raba`.
//!
//! These are here rather than in a `tests/` directory because more than one crate
//! needs them: the pty tests in `raba` drive the real binary and then have to
//! interpret what it wrote, and the only honest interpreter is the same strict
//! terminal model `raba-core`'s own renderer tests use. A second copy would be a
//! second thing to keep correct.
//!
//! The feature is opt-in, so the boot binary links a core that does not contain
//! any of this. See `docs/maintenance.md`.

mod buffer;
mod terminal;

pub use buffer::BufferSurface;
pub use terminal::Terminal;
