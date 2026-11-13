//! Talking to a running instance.
//!
//! `raba`(8) listens, `raba-ctl`(1) connects, and `docs/protocol.md` is the
//! contract between them — written before either end, so that the protocol is
//! something both were built against rather than whatever the first client
//! happened to send.
//!
//! The split here is the same one the rest of the project uses: [`wire`] is pure
//! and holds the grammar, [`server`] holds the file descriptors and the privilege
//! boundary. Every malformed request anybody can think of is a unit test in
//! `wire`; `server` is small enough to read in one sitting, which is what you
//! want from the code that decides who is allowed to draw on a console as root.

pub mod server;
pub mod wire;

pub use server::{Connection, Listener, SOCKET};
pub use wire::{Command, Rejection, Reply, MAX_REQUEST, VERSION};
