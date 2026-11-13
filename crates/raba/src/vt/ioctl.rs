//! The VT ioctls, and the only `unsafe` in the project.
//!
//! Every call here is a raw `ioctl` on a console file descriptor. They are grouped
//! in one small module on purpose: the rest of RABA can be read without thinking
//! about memory safety, and this file can be read line by line by somebody who is.
//!
//! # Why these are "bad" ioctls
//!
//! The VT and KD requests predate `_IOC` by about a decade, so their numbers carry
//! no direction and no argument size — `VT_ACTIVATE` is simply `0x5606`. There is
//! nothing to derive them from; they are copied from `linux/vt.h`, `linux/kd.h`,
//! `linux/tiocl.h` and `asm-generic/ioctls.h`, and must be read against those
//! headers rather than against a macro.
//!
//! # What makes each call sound
//!
//! Three shapes, and the distinction is the whole safety argument.
//!
//! - **Integer requests** (`VT_ACTIVATE`, `KDSETMODE`) pass the value *in* the
//!   argument register. The kernel never dereferences it, so there is no pointer
//!   and nothing to get wrong beyond the number itself.
//! - **Pointer requests** (`VT_OPENQRY`, `VT_GETSTATE`, `KDGETMODE`) hand the
//!   kernel a pointer to write through. Soundness needs the pointed-to type to be
//!   exactly what the kernel writes, which is stated at each one against the
//!   kernel source that writes it.
//! - **`TIOCLINUX`**, which is a family rather than a request: the *subcode* is
//!   the first byte of a buffer we fill in, so `Getter` is wrong for it — that
//!   passes uninitialised storage, and the kernel reads the subcode before it
//!   decides what to do. These use `Updater`, which passes a buffer we filled.
//!
//! `TIOCLINUX` is also where the one genuinely surprising detail is, and it cost
//! two wrong implementations to find. The subcodes do not agree on where the
//! answer goes: `TIOCL_SETKMSGREDIRECT` reads its argument from **byte 1**, but
//! `TIOCL_GETKMSGREDIRECT` writes its answer back over **byte 0** — the subcode
//! byte itself (`vt.c`: `data = vt_get_kmsg_redirect(); return put_user(data, p)`,
//! where `p` is the start of the buffer and `param` is `arg + 1`). A reader that
//! looks at byte 1 by symmetry with the setter compiles, succeeds, and reports
//! zero forever. `tiocl.h` hints at it — "these two don't return a value: they
//! write it back in the type" — against the wrong two subcodes.
//!
//! `fd` being a console is *not* a safety requirement: an ioctl on the wrong kind
//! of file returns `ENOTTY`, which is an error and not undefined behaviour.

#![allow(
    unsafe_code,
    reason = "the VT ioctls are the one thing in RABA that cannot be done safely"
)]

use rustix::fd::AsFd;
use rustix::io::Errno;
use rustix::ioctl::{self, Opcode};

// linux/vt.h
const VT_OPENQRY: Opcode = 0x5600;
const VT_GETSTATE: Opcode = 0x5603;
const VT_ACTIVATE: Opcode = 0x5606;
const VT_WAITACTIVE: Opcode = 0x5607;
const VT_DISALLOCATE: Opcode = 0x5608;

// linux/kd.h
const KDSETMODE: Opcode = 0x4b3a;
const KDGETMODE: Opcode = 0x4b3b;
const KDGKBMODE: Opcode = 0x4b44;

// asm-generic/ioctls.h. One request number for a family: the *subcode* is the
// first byte of the buffer, not part of the number.
const TIOCLINUX: Opcode = 0x541c;

// linux/tiocl.h. `GET` is 17 and not 12 — 12 is `TIOCL_GETFGCONSOLE`, which also
// answers with a terminal number, so swapping them reads the foreground console
// and calls it the redirect.
const TIOCL_SETKMSGREDIRECT: u8 = 11;
const TIOCL_GETKMSGREDIRECT: u8 = 17;

/// `KD_TEXT`: the console draws characters. The only mode RABA ever sets.
pub const KD_TEXT: i32 = 0x00;
/// `KD_GRAPHICS`: something owns the framebuffer and the console draws nothing.
pub const KD_GRAPHICS: i32 = 0x01;

/// `K_XLATE`: keystrokes arrive as characters from the loaded keymap.
pub const K_XLATE: i32 = 0x01;
/// `K_UNICODE`: as `K_XLATE`, but UTF-8. The default on a modern console, so a
/// check that insisted on `K_XLATE` would be asserting a 1990s kernel.
pub const K_UNICODE: i32 = 0x03;

/// Whether a console in this keyboard mode is one a person can type on.
///
/// The modes that are not are `K_RAW` and `K_MEDIUMRAW`, where keystrokes arrive
/// as scancodes for a program that is no longer running, and `K_OFF`, where they
/// arrive nowhere. RABA has no way to set any of them — see
/// [`get_keyboard_mode`] — and this is how the tests say so.
#[must_use]
pub const fn is_typable(mode: i32) -> bool {
    matches!(mode, K_XLATE | K_UNICODE)
}

/// `struct vt_stat` from `linux/vt.h`.
///
/// Three `unsigned short` and nothing else, in this order. `VT_GETSTATE` fills all
/// three with `put_user` on the fields individually, so the layout has to match
/// the header exactly — hence `repr(C)` and no reordering.
#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct VtStat {
    /// The terminal currently displayed.
    pub active: u16,
    /// Which terminals have pending signals. Unused here; read and ignored.
    pub signal: u16,
    /// Bitmask of allocated terminals.
    pub state: u16,
}

/// The number of a free virtual terminal, allocating it.
///
/// `VT_OPENQRY` is the answer to "which terminal may I use", and it is the only
/// correct answer: hardcoding `tty1` takes the terminal a getty is already on, and
/// hardcoding anything else takes one somebody may be logged into.
///
/// Returns `None` when every terminal is in use, which the kernel reports as `-1`
/// rather than as an error.
pub fn open_query<F: AsFd>(fd: F) -> Result<Option<u16>, Errno> {
    // SAFETY: VT_OPENQRY writes one `int` through the argument pointer
    // (`vt_ioctl.c`: `ret = put_user(i, (int __user *)arg)`), so `Getter<_, i32>`
    // hands it a pointer to exactly that much correctly aligned storage.
    let number: i32 = unsafe { ioctl::ioctl(fd, ioctl::Getter::<VT_OPENQRY, i32>::new())? };
    if number < 0 {
        return Ok(None);
    }
    Ok(u16::try_from(number).ok())
}

/// Which terminal is displayed, and which are allocated.
pub fn get_state<F: AsFd>(fd: F) -> Result<VtStat, Errno> {
    // SAFETY: VT_GETSTATE writes a `struct vt_stat` through the argument pointer,
    // field by field (`vt_ioctl.c`: `vt_get_vtstat`). `VtStat` is `repr(C)` with
    // the same three `u16` in the same order, so the kernel writes exactly the
    // bytes this allocation has.
    unsafe { ioctl::ioctl(fd, ioctl::Getter::<VT_GETSTATE, VtStat>::new()) }
}

/// Switch the display to a terminal. Returns as soon as the request is queued.
///
/// Queued, not done: the switch can be refused or delayed by a process that has
/// taken the terminal in `VT_PROCESS` mode. [`wait_active`] is what waits.
pub fn activate<F: AsFd>(fd: F, terminal: u16) -> Result<(), Errno> {
    // SAFETY: VT_ACTIVATE takes its terminal number *in* the argument, not through
    // a pointer (`vt_ioctl.c`: `if (arg == 0 || arg > MAX_NR_CONSOLES)`), so there
    // is nothing for the kernel to dereference.
    unsafe { ioctl::ioctl(fd, ioctl::IntegerSetter::<VT_ACTIVATE>::new_usize(terminal.into())) }
}

/// Block until a terminal is the one being displayed.
pub fn wait_active<F: AsFd>(fd: F, terminal: u16) -> Result<(), Errno> {
    // SAFETY: an integer request, as `activate`.
    unsafe { ioctl::ioctl(fd, ioctl::IntegerSetter::<VT_WAITACTIVE>::new_usize(terminal.into())) }
}

/// Give a terminal back to the kernel.
///
/// Fails with `EBUSY` if the terminal is still open anywhere, which is why the
/// file descriptor is closed before this is called.
pub fn disallocate<F: AsFd>(fd: F, terminal: u16) -> Result<(), Errno> {
    // SAFETY: an integer request, as `activate`.
    unsafe { ioctl::ioctl(fd, ioctl::IntegerSetter::<VT_DISALLOCATE>::new_usize(terminal.into())) }
}

/// Whether a terminal is in text mode or has been taken for graphics.
///
/// The question RABA asks before drawing anything. A terminal in [`KD_GRAPHICS`]
/// belongs to something else — a display server, another splash — and writing
/// characters to it puts them nowhere while fighting for the screen.
pub fn get_mode<F: AsFd>(fd: F) -> Result<i32, Errno> {
    // SAFETY: KDGETMODE writes one `int` through the argument pointer
    // (`vt_ioctl.c`: `setint:` ... `put_user(ucval, (int __user *)arg)`).
    unsafe { ioctl::ioctl(fd, ioctl::Getter::<KDGETMODE, i32>::new()) }
}

/// Set a terminal's mode.
///
/// RABA only ever sets [`KD_TEXT`], and only to put back a mode it found. It never
/// sets [`KD_GRAPHICS`]: a process killed while a console is in graphics mode
/// leaves a machine with no visible console and no way to get one back, and
/// `SIGKILL` cannot be caught. Staying in text mode is what makes "killed
/// mid-frame leaves a usable terminal" true by construction rather than by
/// cleanup.
pub fn set_mode<F: AsFd>(fd: F, mode: i32) -> Result<(), Errno> {
    // SAFETY: KDSETMODE takes its mode *in* the argument (`vt_ioctl.c`:
    // `switch (arg)`), so there is no pointer involved.
    unsafe {
        ioctl::ioctl(
            fd,
            ioctl::IntegerSetter::<KDSETMODE>::new_usize(usize::try_from(mode).unwrap_or(0)),
        )
    }
}

/// Which terminal the kernel prints its messages on.
///
/// `0` means "whichever one is displayed", which is the default and the reason
/// this matters at all: a splash takes the foreground, so by default every
/// `EXT4-fs` and `usb 1-1` line lands on top of the art.
///
/// Note that this is **one global in the kernel**, not a property of `fd`
/// (`vt.c`: `static int kmsg_con` inside `vt_kmsg_redirect`). It is shared with
/// whatever else on the machine has an opinion about it, which is why RABA reads
/// the old value and puts that exact value back rather than assuming zero.
///
/// On a kernel built without `CONFIG_VT_CONSOLE` the whole thing is a no-op that
/// always answers `0` — harmless here, since the splash does not depend on it.
pub fn kmsg_redirect<F: AsFd>(fd: F) -> Result<u8, Errno> {
    let mut buffer = [TIOCL_GETKMSGREDIRECT, 0];
    // SAFETY: TIOCLINUX reads the subcode from the first byte of the buffer, and
    // for this subcode writes one byte back over that same first byte (`vt.c`:
    // `put_user(data, p)`). `Updater` is the pattern for a buffer the kernel both
    // reads and writes, and two bytes is more than this arm touches.
    unsafe { ioctl::ioctl(fd, ioctl::Updater::<TIOCLINUX, [u8; 2]>::new(&mut buffer))? };
    let [terminal, _unused] = buffer;
    Ok(terminal)
}

/// Print kernel messages on a given terminal instead of on the displayed one.
///
/// Needs `CAP_SYS_ADMIN`, and `TIOCLINUX` as a whole needs `CAP_SYS_TTY_CONFIG`
/// when the file descriptor is not the caller's controlling terminal — which
/// RABA's never is, because it opens consoles with `O_NOCTTY`. Both hold for PID
/// 1 in an initramfs and neither holds for a normal user, so the caller treats a
/// failure here as "messages stay where they are", never as a reason not to draw.
pub fn set_kmsg_redirect<F: AsFd>(fd: F, terminal: u8) -> Result<(), Errno> {
    let mut buffer = [TIOCL_SETKMSGREDIRECT, terminal];
    // SAFETY: as `kmsg_redirect`. For this subcode the kernel only reads the
    // second byte (`vt.c`: `get_user(data, p + 1); vt_kmsg_redirect(data)`), so
    // the buffer is written by us and left alone by it.
    unsafe { ioctl::ioctl(fd, ioctl::Updater::<TIOCLINUX, [u8; 2]>::new(&mut buffer)) }
}

/// What the keyboard is doing: [`K_XLATE`] on any console anybody can type on.
///
/// RABA has no setter for this on purpose. A console left in `K_RAW` by a process
/// that died is a machine whose owner cannot type, and since `SIGKILL` cannot be
/// caught the only reliable way not to leave it that way is never to set it. This
/// reader exists so that "never" can be checked rather than asserted.
pub fn get_keyboard_mode<F: AsFd>(fd: F) -> Result<i32, Errno> {
    // SAFETY: KDGKBMODE writes one `int` through the argument pointer
    // (`vt_ioctl.c`: `setint:` ... `put_user(ucval, (int __user *)arg)`), as
    // KDGETMODE does.
    unsafe { ioctl::ioctl(fd, ioctl::Getter::<KDGKBMODE, i32>::new()) }
}
