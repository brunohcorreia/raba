//! The boot splash, as a library, so that the virtual-machine tests can drive its
//! virtual-terminal layer directly.
//!
//! There is nothing here a user of RABA would call. The binary is the product; the
//! library exists because the only honest way to test [`vt`] is on a real kernel
//! with a real console, which means from inside the QEMU guest in
//! `crates/raba-vmtest` — and a test that has to go through the command line can
//! only check that the program did not crash.
//!
//! [`lifecycle`] is here for the opposite reason: it is pure, and exposing it
//! makes it reachable from `cargo test` on a host with no console at all.
//!
//! See `docs/development.md` on the tiers.

pub mod boot;
pub mod conflict;
pub mod control;
pub mod lifecycle;
pub mod signals;
pub mod state;
pub mod vt;
