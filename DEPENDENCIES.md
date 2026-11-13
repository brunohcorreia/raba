# Dependencies

Every direct dependency of `raba` needs an entry here before it is added. This is the
budget from [docs/maintenance.md](docs/maintenance.md) made concrete.

The two binaries have opposite constraints and separate budgets.

## `raba` — the boot binary

**Budget: 2 direct dependencies.** It runs as root, early in boot, inside an initramfs.
Every crate here is code that can break a boot on someone else's release schedule.

| crate | what for | why not `std` | removable? |
|---|---|---|---|
| `raba-core` | format, layout, rendering | ours | no — it is the program |
| `raba-term` | stdout surface, frame timing | ours | no — it is the program |
| `rustix` | `TIOCGWINSZ`, the VT ioctls, `SO_PEERCRED` | `std` cannot make an ioctl, and has no peer credentials | no — without it the size is a guess |
| `libc` | `sigprocmask`, `sigtimedwait` | `std` has no signal API at all | no — without it `SIGTERM` kills mid-frame |

**Spent: 2 of 2.**

M4 added two feature flags to it — `net` for `SO_PEERCRED` and `process` for `getuid`
— and no crates. The socket itself is `std::os::unix::net`, which is why the control
protocol costs nothing: the only thing `std` cannot answer is *who is on the other end*,
and that question is the entire access check.

It was unspent through M0 on the reasoning that the terminal size was "a question it
can mostly guess" from `COLUMNS` and `LINES`. That reasoning was wrong, and a user
found it immediately: a shell sets `COLUMNS` as a *shell* variable and does not export
it, so the guess was not mostly right, it was always 80x25, and `--preview` drew into a
corner of any terminal larger than that. There is no guessing a window size.

`rustix` over `libc` as planned: on Linux it goes through `linux-raw-sys` and makes the
syscalls directly, so the boot binary has no C dependency and stays easy to build
against musl. The whole tree it brings is `bitflags` and `linux-raw-sys`, neither of
which does any I/O of its own. The release binary is 311 KB.

`VT_OPENQRY` and the rest of the VT ioctls arrive at M3 through the same crate, so the
budget does not have to move again.

Not here, deliberately:

- **no JSON parser** — `.raba` is a fixed binary layout. This is the main reason the
  format is ours.
- **no decompression** — the gzip-bomb failure mode cannot be expressed.
- **no async runtime** — a frame loop is a loop. Banned in `deny.toml`. This is the one
  to watch, because a control socket is exactly the shape of problem people reach for
  one to solve. RABA's is polled once a frame, beside the signals, in the loop that was
  already there.

### The second one, and what it cost

The budget moved from one to two at M4, for signals. `std` has no signal API, so there
was no version of "handle `SIGTERM`" that did not add a crate — and the alternative to
handling it is a splash that is killed mid-frame and leaves the display on a terminal
nobody asked to be on.

`libc` rather than `signal-hook`, which would have been the obvious choice: `signal-hook`
depends on `signal-hook-registry`, which depends on `arc-swap`, and `signal-hook` depends
on `libc` as well. Four crates for the same work.

And it costs **nothing**, which was measured rather than assumed. The release binary is
414392 bytes with it and 414392 bytes without — the same number, because the boot binary
is statically linked against musl and the libc these bindings bind to was already in the
file. The crate is bindings, not code.

The previous entry here read "**no `libc`** — `rustix` reaches the kernel through
`linux-raw-sys` instead, which is why the boot binary builds against musl without a C
toolchain". The second half is still true and is why this was cheap. The first half is
no longer true, and is recorded as changed rather than quietly edited away.

## `raba-core` — the pure core

**Budget: 0.** Pure data transformation, no I/O, no syscalls. If something here needs a
dependency, it is probably not pure and belongs in `raba-term`.

## `raba-term` — terminal surfaces and playback

**Budget: 0, and it inherits `raba`'s constraints.** This is the impure half of the
renderer: the thing that owns a real terminal, writes a frame in one call and sleeps
between them. It exists because both binaries need it — `raba` to play a splash and
`raba-gen` to show an author what they just built — and `raba-core` is pure by rule, so
it cannot live there.

The alternative was a copy in each binary. The frame-timing and cursor-parking logic is
exactly the kind that drifts silently when there are two of it, and a preview that no
longer matches what boot does is worse than no preview.

`raba` links this, so **anything added here lands in an initramfs**. Treat its budget as
`raba`'s, not `raba-gen`'s.

## `raba-gen` — the build tool

**No budget.** It never enters an initramfs and is never on the boot path. A crate that
breaks here is a build failure someone fixes at leisure, not an unbootable machine.

Spent: `serde` + `toml` for recipes, `serde_json` + `flate2` for `.dur` import. A `.dur`
is gzipped JSON, so both of those are the format's requirement rather than a choice;
neither can reach the boot path, because nothing on it reads `.dur`.

## Dev-dependencies

**Free.** `proptest`, `cargo-fuzz`, pty helpers — none of it ships.

## Adding one

1. Add a row above with a real answer in every column.
2. State what breaks if it is removed.
3. For `raba`, say why the standard library is not enough.
4. `cargo deny check` must pass.
5. Check the binary size gate.
