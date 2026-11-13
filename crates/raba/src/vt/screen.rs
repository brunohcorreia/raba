//! The seam between the boot loop and the kernel's virtual terminals.
//!
//! Two traits and one implementation, and the implementation is [`Vt`]. Nothing
//! here adds a capability: [`Screen`] is the set of things [`Vt`] can already do
//! and [`Kernel`] is the three constructors already on it. What they add is a
//! place to stand.
//!
//! # Why a trait for something with one implementation
//!
//! `boot.rs` is the loop with the most consequence in this program. It runs as
//! root, in an initramfs, on a machine belonging to somebody who has never heard
//! of RABA, and if it goes wrong the symptom is a computer that will not finish
//! booting. It is also, measured honestly, the part that was least tested: 6% of
//! its regions, because every path through it needed a real console, root, and
//! a `VT_OPENQRY` that does not exist on a laptop running `cargo test`.
//!
//! The virtual-machine tier does exercise it, and asserts thirty things about
//! the result. But a behavioural assertion says the whole ran; it does not say
//! which of the branches inside ran, and the branches that matter most in this
//! loop are the ones nothing normal reaches — the console that changes size
//! under the splash, the frame that is five seconds late, the terminal that goes
//! away between being taken and being drawn on.
//!
//! So the type of one field, `Session::vt`, was the thing nailing all of that to
//! hardware. [`raba_core::surface::Surface`] was already a trait and
//! [`raba_term::Player`] was already generic over it; this finishes the job for
//! the handful of VT-specific things the loop also needs.
//!
//! # What is still not covered by this
//!
//! Everything below the trait: [`Vt`] itself, `vt::ioctl`, and the real
//! `VT_OPENQRY`/`VT_ACTIVATE`/`KDGETMODE` calls. That is deliberate — the point
//! of a seam is that one side of it stays small enough to read line by line —
//! and it is measured separately, by `scripts/vm-test.sh --coverage`, which
//! brings the guest's profile back out over the serial console.

use super::{Displaced, Error, Vt};
use raba_core::surface::Surface;
use std::io;

/// A terminal the boot loop has borrowed.
///
/// Everything [`crate::boot`] needs from a console beyond drawing on it. The
/// methods are [`Vt`]'s own, named the same, so that reading the loop against
/// the real implementation needs no translation.
pub trait Screen: Surface {
    /// Which terminal this is. `/dev/tty<this>`.
    fn number(&self) -> u16;

    /// Which terminal was displayed before, and will be again.
    fn previous(&self) -> u16;

    /// Has something else taken the screen? See [`Vt::displaced`].
    fn displaced(&self) -> Option<Displaced>;

    /// Give the terminal up without taking the screen back. See
    /// [`Vt::relinquish`].
    ///
    /// # Errors
    ///
    /// The first step that failed, after every step has been attempted.
    fn relinquish(&mut self, why: Displaced) -> io::Result<()>;

    /// Let go without putting anything back. See [`Vt::forget`].
    fn forget(&mut self);
}

impl Screen for Vt {
    fn number(&self) -> u16 {
        Vt::number(self)
    }

    fn previous(&self) -> u16 {
        Vt::previous(self)
    }

    fn displaced(&self) -> Option<Displaced> {
        Vt::displaced(self)
    }

    fn relinquish(&mut self, why: Displaced) -> io::Result<()> {
        Vt::relinquish(self, why)
    }

    fn forget(&mut self) {
        Vt::forget(self);
    }
}

/// Where a [`Screen`] comes from.
///
/// The three ways RABA can end up with a terminal, which are three different
/// situations and not three options: adopting is what a resumed instance does,
/// taking is what `Boot.Tty=<n>` means, and acquiring is the ordinary boot.
pub trait Console {
    /// What this console hands out.
    type Screen: Screen;

    /// Continue on the terminal a previous instance left behind, clearing
    /// nothing. See [`Vt::adopt`].
    ///
    /// # Errors
    ///
    /// If the terminal named in the state file cannot be opened.
    fn adopt(&mut self, terminal: u16, previous: u16) -> Result<Self::Screen, Error>;

    /// Take the terminal that was asked for by name, whatever is on it. See
    /// [`Vt::take`].
    ///
    /// # Errors
    ///
    /// Anything that means "do not draw".
    fn take(&mut self, number: u16) -> Result<Self::Screen, Error>;

    /// Take a free terminal and switch the display to it. See [`Vt::acquire`].
    ///
    /// # Errors
    ///
    /// Anything that means "do not draw".
    fn acquire(&mut self) -> Result<Self::Screen, Error>;
}

/// The kernel's virtual terminals: the only [`Console`] that ships.
///
/// A unit struct rather than a free function so that the thing the boot loop
/// holds has a name, and so the fake in `crates/raba/tests/boot.rs` is a peer of
/// it rather than a special case.
#[derive(Debug, Clone, Copy, Default)]
pub struct Kernel;

impl Console for Kernel {
    type Screen = Vt;

    fn adopt(&mut self, terminal: u16, previous: u16) -> Result<Vt, Error> {
        Vt::adopt(terminal, previous)
    }

    fn take(&mut self, number: u16) -> Result<Vt, Error> {
        Vt::take(number)
    }

    fn acquire(&mut self) -> Result<Vt, Error> {
        Vt::acquire()
    }
}
