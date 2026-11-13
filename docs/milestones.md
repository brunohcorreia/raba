# Milestones

Ordered by risk, not by dependency: the cheapest-to-test and most existential questions
first, and the dangerous code only once there is a harness to catch it. See
[development.md](development.md) for the reasoning.

---

## M0 — See a logo animate

**Answers: does ASCII art at console cell size look good enough to justify this project?**

Everything else rests on that, and nobody has tested it. No `.dur`, no VT, no initramfs,
no root, no VM — plain text in, animation on a screen out.

| # | issue | |
|---|---|---|
| 1 | `raba-core`: `.raba` types — `Movie`, `Frame`, `Op`, `Palette` | done |
| 2 | `raba-core`: encoder, `Movie` → bytes | done |
| 3 | `raba-core`: decoder with bounded validation, bytes → `Movie` | done |
| 4 | **The failure table** — every malformed input, exit 0, no panic | done |
| 5 | `raba-core`: `Surface` trait and `BufferSurface` for tests | done |
| 6 | `raba-core`: grid → ANSI encoder, redraw minimisation, **one write per frame** | done |
| 7 | `raba-core`: glyph validator against the console-safe set | done |
| 8 | `raba-gen`: load fastfetch-style text art with `$1`..`$9` colour markers | done |
| 9 | `raba-gen`: compose logo + spinner, least-common-multiple frame count | done |
| 10 | `raba`: `--preview`, rendering to stdout | done |
| 11 | First theme, and the verdict on the premise | built; verdict pending |

**Done when** `raba --preview themes/cachyos.raba` animates a logo in a terminal, and we
have looked at it and decided it is worth continuing.

It animates. **The looking is the part that is left**, and it is not something the code
can do for itself: the tests prove the logo is the right shape, in the right colours,
made of characters a console can draw. Whether that is *good* is a judgement, and it is
the judgement the whole milestone exists to collect.

### Progress

The format is implemented and frozen at version 1, and the renderer turns a grid into
terminal bytes. `docs/format.md` describes what was built rather than what was sketched.

- 132 tests pass; `clippy` is clean under `-D warnings` with the workspace deny lints
- the failure table covers 25 shapes of malformed input, each asserting a specific error
- property tests cover round-trip, determinism, truncation and "arbitrary bytes never
  panic"
- a `cargo-fuzz` target asserts internal consistency and round-trip, not merely absence
  of panics
- `raba` release binary: 310 KB against a 512 KB gate, and still zero dependencies
- the CachyOS theme is 54x24 in three colours: 15652 bytes, 4 frames, 1299 ops — of
  which 1296 are frame 0. The animation costs one cell per frame, which is the delta
  encoding doing the thing it was designed for.

The renderer is checked against a model of a terminal rather than against golden bytes,
so the property from [development.md](development.md) is stated directly: applying
`to_ansi(grid, prev)` to `prev` yields `grid`, for arbitrary pairs of grids. The model
panics on anything outside the renderer's permitted alphabet, which makes "no escape
sequence ever leaks through a glyph" an assertion about the whole output rather than
about the cases a test thought to check.

That property immediately found something the format work had missed: nothing validated
a colour index against the declared palette, so a 16-colour movie could carry index 200
and the renderer would silently show a different colour. The decoder now rejects it —
see [format.md](format.md) on why rejection rather than clamping.

The glyph validator reports three levels rather than a verdict, and the reasoning is in
[glyphs.md](glyphs.md): RABA draws before anyone knows which console font is loaded, so
safe can only mean the intersection of the kernel's built-in CP437 and the fonts a
distribution loads over it. A property ties it to the renderer — a character is
substituted if and only if it is a control character — so the escape-injection defence is
provably not mangling ordinary art as a side effect.

---

## M1 — Theme authoring

Makes themes contributable by someone who does not write Rust.

| | issue | |
|---|---|---|
| 1 | `raba-gen build` from a TOML recipe, full `raba-theme(5)` schema | done |
| 2 | 256 → 16 colour downsampling; clamp backgrounds to 0–7 | done |
| 3 | Spinner styles: `ascii`, `dots`, `bar`, `arc`, `blocks`, `custom` | done |
| 4 | `--check` | done |
| 5 | Vendor fastfetch logo art at a pinned commit | done |
| 6 | `--list-logos`, `raba-gen --preview` | done |
| 7 | Golden render tests | done — 50 |
| 8 | Themes for the most-used distributions | done — 50 |

### Progress

The recipe schema is what `raba-theme(5)` specifies, not a parallel invention. That
was not true at first: the schema was built before the man page was read, and had to be
rewritten to match it. The man page is the spec; where the two disagreed, the code moved.

Where the page was *wrong*, it moved instead. It described the `bar` spinner as "CP437
blocks", and only `▄` and `█` of that set are in CP437 — the eighth blocks `▁▂▃▅▆▇` are
not in any console font. The glyph validator caught it, which is the second time that
check has found something nobody would have found by reading. `bar` and `arc` are kept
and marked terminal-only, and `blocks` was added as the non-ASCII spinner that does work
at boot.

Three places where the page promised something the format cannot carry, now recorded
rather than quietly skipped:

- `canvas.align`, `x_offset` and `y_offset` are run-time placement. The console's size
  is not known at build time and the header has no field for the intent. Accepted and
  reported as not honoured.
- The page's outro example says it plays once, and the page documented no key to say so.
  Added `canvas.repeat`.
- `logo.source = "fastfetch:name"` needed vendored art. It has it now; see below.

Colour fitting is automatic rather than a flag, which is what the page promises: an
index above 15 in a 16-colour theme is an xterm colour the author drew in, and the
console's nearest equivalent is a better answer than an error. Every substitution is
reported, so a typo still surfaces.

Fifty themes ship, chosen by evidence across four sources that measure different
populations — the Steam hardware survey for gamers, the Stack Overflow survey for
developers, W3Techs for servers, and distro.watch's Google Trends and Wikipedia
ranking for the long tail. themes/README.md records why Reddit subscriber counts and
DistroWatch page hits are not among them. Their colours are extracted from fastfetch
rather than chosen: the extractor renders each logo twice, once with the colour slots
forced to known values to recover the `$1`..`$9` markers and once untouched to recover
the real colours, then pairs them by position.

Two things only came out of running it on real art. The Debian logo is made of dollar
signs, which have to be escaped as `$$` or the art will not parse. And **several
fastfetch logos cannot be used at boot at all** — NixOS's is quadrant blocks and
`nixos_small` is sextants, so neither renders on a console; this repository ships the
ASCII `NixOS2` variant instead. The glyph validator refused the build rather than
shipping a logo full of holes, which is three times now that it has caught something
reading would not have.

Two more found while widening the set. `ESC[39m` is "default foreground", and a logo
drawn in it is not uncoloured — on a console the default is grey and bolded it is
white, which is what MX and elementary actually look like; reading it as "no colour"
shipped them grey. And asking fastfetch for a logo it does not have **does not fail**:
it falls back to the logo of the distribution running the command, so
`fastfetch-logo.py bazzite` on a CachyOS machine would have written a CachyOS logo into
`bazzite.txt` in silence. The extractor now refuses an unknown name and suggests near
matches.

Bazzite and Nobara are the two notable absences: Bazzite has no fastfetch logo at all,
and Nobara's is braille. Both need art drawn by hand.

Widening the search to all 51 distributions distro.watch ranks found a hole the
survey-based sources structurally could not. **Omarchy** is ninth worldwide by search
share, ahead of Zorin, NixOS, Manjaro and Pop!_OS, all four of which had already
shipped; it is new enough that no survey has sampled it. Tails, Qubes OS, antiX and
Puppy Linux were the same kind of miss. Seventeen themes came out of that pass, all
seventeen building with no glyph complaints except Mageia, whose cauldron bubbles were
U+00B0 — the same character Garuda used, hand-edited to "o" the same way.

Reddit was tried as a fifth source and abandoned. Reddit blocks unauthenticated access
to the subscriber API, and the aggregators that republish the figures disagree by over
fifty per cent on the same subreddit — r/archlinux appears as 226K, 285K and 344K
across three of them. DistroWatch blocks automated access too, and is a measure of
attention rather than installs; it is worth watching as an early signal, since it
flagged CachyOS about a year before the Steam survey did.

---

## M2 — Import

| # | What | Status |
|---|---|---|
| 1 | `.dur` v7 and v8 → `.raba`, handling the colour-map transposition | done |
| 2 | **The critical test:** identical artwork as v7 and v8 must produce byte-identical `.raba` | done |
| 3 | Fuzz target on the `.dur` parser | done |
| 4 | Canvas placement: centre and crop when the art is not the console's size | done |
| 5 | `generate` for animations that have no source file | done |
| 6 | ANSI art and plain text importers | done |
| 7 | `--also-dur` for display managers that play their own animation | done |

### Progress

`.dur` import was driven by a real file rather than by the specification:
`/etc/ly/blackhole.dur`, a 240x67, 40-frame, 256-colour animation that a user runs
as their Ly splash. It converts exactly — 16,080 cells of frame 0 checked against
the source JSON, zero wrong characters and zero wrong colours.

**It also disproved a worry.** The concern was that delta encoding stops paying
for anything but a logo with a spinner, the wall `docs/3d-logos.md` hit. A black
hole changes only **6.8% of cells per frame**, colour included, so 40 frames of
full-screen animation come to 705 KB rather than the 7.7 MB a dense format would
need. Delta encoding earns its place on real animation, not just on logos.

Three things the real file taught that the specification did not:

- **The version does not predict the key names.** `docs/dur-support.md` says v7
  uses `sizeX`/`sizeY` and v8 uses `columns`/`lines`. This file declares v7 and
  uses `columns`/`lines`, because it was written by `video-to-dur` rather than by
  Durdraw. Both spellings are accepted from both versions.
- **The version does not reliably predict the colour-map orientation either.**
  Where the map's shape is unambiguous — a canvas that is not square — the shape
  decides and the disagreement is reported. The version is consulted only when the
  canvas is square and there is nothing else to go on.
- **Durdraw's 0–15 are in curses order, not ANSI order.** 1 is blue, 4 is red.
  Copying the numbers across unchanged swaps red and blue in every imported file,
  which reads as bad art rather than as a bug.

### Placement

The black hole is 240x67; a console at boot may be 80x25. `raba_core::render::place`
always returns exactly screen-sized output, so the encoder's absolute cursor moves
are always in range — drawing past the last column instead would leave the console
scrolled, which is the one thing a splash must not do.

What it returns depends on the art. Shading art is **resampled**, averaging the
coverage of the block of source cells each output cell stands for; everything else
is centred and cropped. The alphabet alone cannot decide this — a fire and a Game
of Life board are written in the same seven characters — so the `.raba` header
carries a `scaling` byte, `auto` guesses from the alphabet for imported art, and
anything RABA generates states its answer outright. See
[console-size.md](console-size.md).

### Importing art nobody wrote for RABA

Three importers now, and the shape of each follows from what its format actually
is:

- **`.dur`** is a document, so it is parsed. The traps are in the data: colour-map
  orientation and curses colour order.
- **`.ans`** is a *recording of what was sent to a screen*, so the importer is a
  terminal — a cursor, a grid and a loop. Art that draws a shape and jumps back
  inside it to colour it in works because the real thing works that way. The traps
  are CP437 (not UTF-8), the control range being pictures rather than controls, and
  bold meaning bright. See [ansi-support.md](ansi-support.md).
- **plain text** is text. Tabs, encoding fallback, and a deliberate refusal to read
  `$1` as anything but two characters.

The extension is a hint rather than a promise, so a file that is not obviously one
of them is examined and the choice is reported.

Writing `.dur` back out (`--also-dur`) is what lets RABA's outro and Ly's
background be the same art from one source, which is what makes the handoff
seamless. The round trip is checked cell for cell, because a writer validated
against its own idea of the format proves nothing.

---

## M3 — The dangerous part

The VT layer, where a mistake hangs a boot. **The VM harness is built before a line of
this is written.**

| # | What | Status |
|---|---|---|
| 1 | Tier 3a harness: QEMU kernel + initramfs, ~1 second per run, `isa-debug-exit` | done |
| 2 | Pty tests for the escape encoder | done |
| 3 | `VT_OPENQRY` to acquire a free terminal — never hardcode tty1 | done |
| 4 | Draw, and restore on **every** exit path including panic | done |
| 5 | Detect a terminal already in `KD_GRAPHICS` and yield | done |
| 6 | `TIOCL_SETKMSGREDIRECT` to move kernel messages aside; restore exactly | done |
| 7 | `SIGKILL` mid-frame leaves a usable terminal — the test that matters most | done |

### The harness, first

`scripts/vm-test.sh` boots the kernel already on the machine with an initramfs of two
static binaries and one generated `.raba` — about 470 KB, around a second a run. The
guest is `crates/raba-vmtest`, which mounts what it needs, runs its checks against a
real console, reports them as TAP on the serial line, and switches the machine off.

Two decisions worth keeping:

**`console=ttyS0` plus an emulated VGA adapter.** Results leave by the serial line while
the guest still has a real virtual terminal to draw on. `tty0` is under test and `ttyS0`
reports on it; without that split these tests would be exercising a serial console and
proving nothing about a VT.

**The harness proves it can fail before anything trusts it that it passed.**
`--self-test` makes the guest fail one check on purpose and asserts the run comes back
failed. CI runs that before it runs the real checks. A rig that cannot fail reports
success for reasons unrelated to the code, and for the VT layer the first sign of that
would be somebody's machine not booting.

The guest needs no `unsafe`, which keeps the whole workspace inside
`unsafe_code = "forbid"` until the VT layer genuinely needs it: `/dev/port` reaches
`isa-debug-exit`'s I/O port through a file, and `/proc/sysrq-trigger` is the fallback.

### The VT layer

`crates/raba/src/vt` is the only `unsafe` in the project, which is why the workspace
lint went from `forbid` to `deny` — `forbid` cannot be lifted — and why `raba-core`,
`raba-term` and `raba-gen` now each carry `#![forbid(unsafe_code)]` at their own crate
root. A promise is worth more where it is kept than where it is inherited.

**Almost all of the safety is in what it does not do.** It never sets `KD_GRAPHICS`:
a process killed while a console is in graphics mode leaves a machine with no visible
console and no way back, and `SIGKILL` cannot be caught, so no cleanup code would help.
It never touches the keyboard mode, for the same reason. And it draws on a terminal of
its own from `VT_OPENQRY` rather than on the one somebody is using — so anything it does
leave wrong is confined to a terminal that is about to be handed back, and the user's
terminal is untouched apart from being switched away from. "Killed mid-frame leaves a
usable terminal" is then true by construction rather than by careful unwinding.

Nineteen checks in the guest, and they keep finding things. The first run of the VT
checks found two:

- **`O_NOCTTY`.** RABA is PID 1 in an initramfs, which makes it a session leader with no
  controlling terminal — and a session leader that opens a terminal without that flag
  *acquires* it. A splash should not be receiving a console's signals.
- **`VT_DISALLOCATE` refuses for about 600 microseconds after a switch.** Open a terminal
  and close it and the kernel frees it in 50µs; switch away from it first and it is busy
  until the release completes a moment later. Every run was leaking its terminal. The fix
  is to wait, and the guest measures the real number on each run against a 10ms line, so
  the budget stops being a guess the moment a kernel changes it.

### Where the kernel prints

A splash takes the foreground, and by default the kernel prints on whichever
terminal is in the foreground — so without `TIOCL_SETKMSGREDIRECT` every
`EXT4-fs` and `usb 1-1` line lands on top of the art. RABA points them at the
terminal the user came from, and puts the old value back on the way out.

Two things make this different from the other ioctls here. It is **one global in
the kernel**, not a property of a file descriptor, so "restore" means putting
back the value that was there rather than setting it to zero. And it is the one
piece of state that outlives a `SIGKILL`: if RABA is killed, the redirect stays.
Pointing it at the user's own terminal is what makes that harmless — the messages
are waiting on the terminal they land on, rather than being thrown at a terminal
that no longer exists.

It cost two wrong implementations, both of which compiled, succeeded and reported
zero forever. `TIOCL_SETKMSGREDIRECT` reads its argument from byte 1 of the
buffer, but `TIOCL_GETKMSGREDIRECT` writes its answer back over **byte 0** — the
subcode byte itself. Writing the reader by symmetry with the writer is wrong, and
so is assuming it answers in the return value the way `TIOCL_GETFGCONSOLE` does.
Only reading `vt.c` settled it. The check is written so that the ioctl pair is
exercised directly *before* it is exercised through `Vt`, because `Vt` swallows a
failure here on purpose — a splash must still draw when it may not move the
messages — and a check that went only through `Vt` reports "it did not move them"
without being able to say why.

### Killed mid-frame

The claim the whole module is arranged around cannot be tested in-process:
`SIGKILL` cannot be caught, so a test that sent it to itself would have nobody
left to report the answer. The guest therefore re-executes itself with
`--hold-a-terminal`; that child takes a terminal and draws in a loop, the parent
waits until the display is actually on it — polling `VT_GETSTATE` rather than
sleeping, so the victim is killed mid-frame and not mid-startup — and then kills
it and asks the questions that mean "usable" on a console whose owner no longer
exists:

- the screen is still in `KD_TEXT`, so there is something to see;
- the keyboard mode is unchanged and still one a person can type in;
- the orphaned terminal still opens and still accepts a write;
- `VT_ACTIVATE` away from it completes rather than being quietly queued forever;
- the stranded kernel-message redirect points at the terminal the user is on.

This found the one assertion in the set that was wrong: a modern console comes up
in `K_UNICODE`, not `K_XLATE`, so insisting on `K_XLATE` was asserting a 1990s
kernel. The check now holds the stronger and more honest property — the mode is
whatever it was before, and it is one somebody can type in.

### Tier 2 found two bugs on its first run

`crates/raba/tests/pty.rs` drives the real binary against a real pseudo-terminal and
replays the result through the renderer's own strict terminal model. It immediately
caught what a pure model could not: `ONLCR` rewriting an outbound `\n` as `\r\n` — the
mechanism behind the rule that the renderer emits no line feeds — and a cursor park one
line past the bottom of the screen on every single frame, invisible because terminals
clamp an out-of-range `CUP`. See [development.md](development.md).

---

## M4 — Lifecycle

M3 made RABA able to draw on a console and give it back. M4 is everything that
decides *when* — the part somebody else has to be able to drive.

| # | What | Status |
|---|---|---|
| 1 | `docs/protocol.md`: the wire protocol, written before either end of it | done |
| 2 | Message sanitisation — no caller bytes reach a terminal | done |
| 3 | The phase machine, pure and testable: boot loop → outro → handoff | done |
| 4 | Control socket: `0600`, root-owned, `SO_PEERCRED` rejecting non-root | done |
| 5 | `raba-ctl`: `quit`, `deactivate`, `reactivate`, `message`, `ping`, `status` | done |
| 6 | `/run/raba.state`, resumable: being killed stops being an event | done |
| 7 | `raba` as a daemon: phases, `MaxDuration`, signals | done |
| 8 | Timeout **reveals the console** rather than exiting | done |
| 9 | Conflict detection in the binary, not in service files | done |
| 10 | Read `raba.conf`(5), the kernel command line, and `--check` | done |

### What the socket work turned up

Two things that were wrong in the plan rather than in the code.

**`umask` is the wrong tool for a private socket.** The mode has to be right from the
instant the socket is reachable, and `bind` then `chmod` leaves a window — so the
obvious fix is to set `umask` around the bind. It is per *process*, so it races
anything else running at the time, and RABA's own test suite found that within minutes:
several tests binding at once left the process umask at whichever one finished last,
and directories created afterwards came out with no execute bit. The socket is now
bound at `raba.sock.new`, chmodded, and `rename`d into place. Atomic, same inode, and
it replaces a stale socket in the same step.

**`reject any uid != 0` is the right rule written down wrong.** It is what
[security.md](security.md) says, and taken literally it means a user running `raba` as
themselves could not control their own instance while root could. The check is "the
peer is the user that bound the socket", which at boot *is* "root only" and is correct
everywhere else. It also made the check testable: a hardcoded zero cannot be exercised
by a test suite that is not root, and an access check nobody can run is an access check
nobody knows works.

### The daemon is a `for` loop

One loop, three sources of events, no threads and no async runtime. Signals are
blocked and polled with `sigtimedwait`; the listener is non-blocking and polled;
the only thing that sleeps is the gap between frames, sliced at 10ms so a `quit`
is acted on promptly rather than at the end of a frame that may be holding for a
second. A control socket is exactly the shape of problem people reach for an
async runtime to solve, and it turned out to be a `for` loop — which is the one
thing [DEPENDENCIES.md](../DEPENDENCIES.md) says to watch for.

Almost nothing in it decides anything. The phase machine decides, `wire` parses,
`state` validates, and what is left here has a clock, a socket and a console and
does what it is told. That is why the six end-to-end checks found nothing: by the
time they ran, every decision in them had already been unit-tested somewhere that
did not need a virtual machine.

Six checks, running the **real binaries** — `/raba` as a child and `/raba-ctl` as
its client — because the phase machine, the socket and the VT layer had each been
tested alone and never together with a clock:

| | what it proves |
|---|---|
| draw, answer, hand back | the whole `quit --wait` handoff, including that the state file and socket do not outlive the instance |
| a timeout reveals | the console comes back and the process **keeps running**, which is the half an `exit` gets wrong |
| `quit` after `deactivate` | no outro is played onto a terminal the display manager now has |
| killed and resumed | SIGKILL, then a second instance adopts the same terminal at the same frame, with nothing cleared in between |
| a second instance yields | a live socket is a conflict; a socket left by a dead instance is not |
| `Boot.Tty` from a file | the whole chain, file to frames on the terminal the file named |
| `raba.enable=0` | the kill switch takes no terminal, binds no socket, exits 0 |

### A parser that is not allowed to refuse

`raba.conf`(5) is read by a root process, before the root filesystem is mounted,
in a program that must exit 0. So the parser **has no failure mode**: an unknown
key, an unknown section, a value that does not parse, a file that is not UTF-8,
a file that is four megabytes — each is recorded and stepped over, and the
setting keeps the value it had. A section it does not recognise is skipped
whole, which is what lets a file written for a later version boot an earlier one.

That is a lie-friendly rule, and it is paid for twice. `raba --check` reads the
same files with the same parser and is strict: every complaint, a non-zero exit,
and the theme actually decoded rather than merely found — at a moment when the
user is at their keyboard instead of watching a boot. And `raba.debug=1` writes
that same report into `Log.File`, for the case where there is no way to run a
command on the machine at all. One report function behind both, so what is on
the screen and what is in the log cannot drift.

The layering is four deep — compiled-in defaults, the distribution's
`raba.defaults`, the user's `raba.conf`, the kernel command line — and applied
key by key onto one struct rather than merged at the end, so a key you did not
set keeps the distribution's value. The kernel line sits **above both files** on
purpose: it is the layer reachable from a bootloader on a machine that will not
boot, and a kill switch a file could override would not be one.

Two things the parser treats as hostile input even though the file is
root-owned. `Theme=` is pasted onto a directory and opened as root, so a name is
`[A-Za-z0-9._-]` and nothing else — `Theme=../../etc/shadow` is a complaint, not
a read. And `ID=` out of `os-release`(5) becomes a path component the same way,
so it gets the same check; nothing upstream validates it.

### Parsed, and honestly not honoured

Every key `raba.conf`(5) documents is parsed and validated, and not all of them
do anything: there is no progress indicator and no `Handoff.Mode=clear`. Those
keys are in one list in the code, repeated in the man page, and `--check` names
the ones *your* file actually sets — which is the only version of that message
worth printing. Accepting a key and quietly ignoring it is how a configuration
file stops meaning anything, and it is why `raba-ctl progress` still answers
`-ERR not configured` now that the configuration is in fact read.

`Boot.Tty` went the other way. It was the one setting whose effect is visible
from outside the process, and honouring it was a ten-line variation on
`Vt::acquire` — so it is honoured, and its default changed from the `1` the man
page claimed to `auto`, which is what the code has always done and the safer
answer besides. Naming a terminal by number takes it whatever is on it; naming
the one already displayed means kernel messages cannot be moved aside, because
aside is where the user is. Both are now in the manual.

### Fifteen kilobytes of Grisu

The binary grew 28 KB for all of this and would have grown 43: `{}` on an `f64`
links Grisu *and* Dragon4 and their power-of-ten tables, and the only thing it
was buying was the way a duration is spelled in a report. `{}.{:03}s` from
`as_secs()` and `subsec_millis()` costs nothing and reads the same.

Float *parsing* stays, because `progress 0.5` is a fraction on a wire and
`ShowDelay=0.3` is a fraction in a file. It is the shape of finding the size
gate exists for: not a dependency and not a feature, but a formatting trait
nobody would think to look at.

### The order is a dependency order

The protocol is written first because both ends are written against it and a
protocol discovered by implementing a client against a server is a protocol with
no second implementation. Sanitisation comes before the socket that needs it,
rather than after, because it is the mitigation for the highest-severity finding
in [security.md](security.md) — `message` draws attacker-chosen text on a console
as root, and a convincing fake `login:` prompt is a credential harvester. And the
phase machine is pure, so it is finished and tested on the host before any of it
is reachable from a socket.

---

## M5 — Early boot

M4 made RABA drivable. M5 is everything that puts it inside an initramfs, which is
where it is supposed to run and the one place none of the tests so far have been.

| # | What | Status |
|---|---|---|
| 1 | `raba-setup --freeze`: one self-contained configuration, `auto` already resolved | done |
| 2 | mkinitcpio install hook — busybox and systemd, one hook name | done |
| 3 | mkinitcpio runtime hook | done |
| 4 | The hook cannot fail an image build, proven against a stubbed mkinitcpio | done |
| 5 | systemd units: start, quit, quit-wait | done |
| 6 | dracut module — **v1, not later**: Void, Gentoo and Chimera are non-systemd | |
| 7 | initramfs-tools hook (Debian, Ubuntu) | |
| 8 | Service files for OpenRC, runit, s6 and dinit | |
| 9 | `raba-setup`: dry run by default, `--apply`, `--check` | |
| 10 | TPM2 token detected and warned about **before** the initramfs is touched | |
| 11 | A real mkinitcpio image, booted in QEMU, draws | |

### The one rule the hook lives by

mkinitcpio counts a failing `add_*` as a build error — the `RETURN` trap in
`/usr/lib/initcpio/functions` — and a build error fails the whole image, which
fails the kernel update that triggered it. A boot splash that can do *that* to
somebody's `pacman -Syu` has no business being in their `HOOKS`.

So the hook hands nothing to an `add_*` before checking it exists, and `build`
returns 0 on every path. Where another hook would call `error`, this one calls
`warning`: an image without a splash boots, and an image that was never written
does not.

That is a promise about a shell script, so it is tested like one.
`integration/tests/mkinitcpio-build.sh` is a stub of mkinitcpio whose `add_*`
functions fail exactly where the real ones fail and count it the same way, and
the hook is run against nine half-installed machines — no `raba`, no
`raba-setup`, no theme, no unit, no runtime hook, art that vanishes mid-build.
Every case asserts the same two things: nothing was counted, and `build`
returned 0. `cargo test` runs it, because a hook that can break a kernel update
should not be left to a CI job nobody runs locally.

The stub is deliberately stricter than the real thing in one place. Records go
to files rather than shell arrays, because `add_file -` is called from inside a
pipeline — and in real mkinitcpio a failing `add_*` in a pipeline increments a
copy of `_builderrors` that is then thrown away.

### One hook name, two kinds of initramfs

`command -v add_systemd_unit` is the whole test. That function exists only when
mkinitcpio has already sourced its `systemd` install hook into the same shell,
which is exactly the distinction that matters: a systemd initramfs has no
`/init` script to run a runtime hook from, and a busybox one has no systemd to
run a unit from. One name in `HOOKS`, and no `sd-raba` — Plymouth shipped that
and deprecated it, and [packaging.md](packaging.md) already says not to repeat
other people's reversed decisions.

The unit is wanted from **inside the image only**: the hook writes a
`sysinit.target.wants` symlink into the initramfs and touches nothing on the
host. Putting `raba` in `HOOKS` is the consent; installing the package is not.

### Freezing, and why `auto` cannot wait

`Theme=auto` resolves against `os-release`(5), and there is no `os-release` in
an initramfs. So resolution happens where the answer is knowable — at image
build time, on a running system — and `raba-setup --freeze` writes the whole
effective configuration, every path absolute, into `/etc/raba/raba.conf`
*inside the image*. Nothing is layered underneath it there; `raba.defaults` is
not in the image.

Every key is written, not only the ones that differ from a default. A key that
quietly failed to survive the freeze would be a setting that works on the
installed system and not at boot, which is the one kind of difference nobody
would think to look for.

The property that makes this safe is a round trip: what `freeze` writes, the
parser reads back as the same configuration. The first draft failed it — the
report spells a duration `30s` and the parser wants `30` — which is exactly the
bug this test exists to catch, found before the file had ever been near an
initramfs.

### raba-setup is a second binary on purpose

Freezing a configuration, inspecting an image and warning about a TPM all happen
on a machine with a shell, a package manager and a `/etc`. None of them belong
in the binary that has a size gate and goes into an initramfs, and all of them
are allowed to be as large as they need to be in one that does not.

It depends on `raba` as a library for exactly one thing: the configuration
parser. The file it writes is read back by `raba` at boot, and a second
implementation of that format would be a second set of bugs.

---

## M6 — Breadth

- runit, OpenRC, s6, dinit service scripts
- initramfs-tools
- LUKS hide-and-resume: inotify on `/run/systemd/ask-password/`, `cryptsetup` detection
- Full-distribution VM matrix, nightly
- Man pages built with scdoc; packaging
- Progress overlay, `pulse` by default
- **Measured against Plymouth**, once there is something to measure

### Measured against Plymouth

Raised 12 Nov 2026. The README already claims an advantage over Plymouth in initramfs
size and in not needing a GPU driver early, and already says that Plymouth's reputation
for slowing boot is partly a measurement artifact — `plymouth-quit-wait.service` looks
large in `systemd-analyze blame` while most of it runs *after* the login screen is up,
and Fedora maintainers measured ~3.5s of real cost against the 8.3s the tool reported.

Having said that in the README, it would be dishonest to then benchmark RABA with the
same sloppiness. What is worth measuring, and what each number would mean:

| | why it is the honest number |
|---|---|
| initramfs size, with and without each | the claim that is most likely to be true, and the easiest to measure without arguing |
| time from kernel hand-off to first frame drawn | what a person actually waits for; needs timestamps inside both, not `blame` |
| time from "system ready" to the display manager being usable | where Plymouth's reputation comes from, and where the artifact lives |
| boot with a slow GPU driver — `amdgpu`, `nouveau` | the case RABA is actually for: no splash at all versus a splash |
| memory resident while drawing | small, and the least interesting |

Needs the full-distribution VM matrix above, because the only measurement worth having
is on a real distribution with a real initramfs and a real display manager — tier 3a
cannot answer any of this. Not before M5.

---

## Not scheduled

Deliberately unplanned until something justifies them: DRM backend, Esc-to-show-details,
bootloader first-frame image, Level 2 LUKS (collecting the passphrase — see
[luks.md](luks.md) for why this is probably never).

**Palette animation.** The Linux console's `ESC ] P nrrggbb` redefines one palette
entry and recolours everything already on screen, with no redraw. On the 54x24 CachyOS
theme a colour cycle done by redrawing costs 15552 bytes a frame — the whole theme is
15652 — and the same effect through the palette costs 10 bytes a frame. Fade in and
out, pulse and colour cycling all fall out of it. Needs an additive `.raba` v2 op so a
frame can carry a palette change, a `restore` that emits `ESC ] R`, since this changes
the console globally and not just our area, and an OSC 4 spelling for terminal
emulators — which is exactly the difference the `Surface` trait exists to hide.

**Movement.** `ESC [ n P` and `ESC [ n @` shift a line's contents; `ESC [ r` with
`L`/`M` scrolls a region. A logo sliding in costs a few hundred bytes a step that way
against 15552 for redrawing it, though the column entering the screen still has to be
drawn. Needs a shift op in the format, which is a larger change than the palette one.

**[3D rotating logos](3d-logos.md)** — fits the architecture as a `raba-gen` feature,
because the mathematics happens at build time and the boot binary keeps playing
pre-rendered frames. The open question is not whether it works but whether it suits a
sequence whose job is to get out of the way. Prototype with `--preview` before committing.

**`.raba` v2 with dense frames** — ops carry an explicit cell index, which a full-canvas
frame does not need. Would nearly halve the cost of 3D rotation, where delta encoding
contributes nothing. The trigger is 3D logos turning out to be wanted, not the idea being
available.


### Vendoring, and two things it caught

`vendor/fastfetch-logos` holds 545 logos taken at fastfetch 2.71.0, commit
`8c9d9c04`. fastfetch compiles its art into its binary rather than shipping it,
so `scripts/vendor-fastfetch-logos.sh` pulls the repository at that commit and
`scripts/fastfetch-manifest.py` parses the C declarations into a manifest: each
logo's name, its aliases, its art file, and the default colour of each `$N` slot.
`build.rs` turns that into a table. Updating is two constants and a diff.

The manifest was validated against the installed fastfetch rather than trusted:
648 names and aliases on both sides, zero difference. Getting there took two
fixes. Parsing the entries by brace matching got lost in the nested `.colors`
block and silently dropped Xenia; the entries are delimited by their `#ifdef`
guards, which is the file's real structure. And the colour separator was `;`,
which is ambiguous because an SGR value is itself semicolon-separated —
`38;5;36;38;5;36` could be one colour or two.

Two differences between fastfetch's dialect and ours, both found by building
real logos rather than by reading:

**fastfetch's `$` is lenient.** A `$` not followed by a digit or another `$` is
just a dollar sign, and 26 logos depend on it — Debian is drawn out of dollar
signs and contains `,$P"'`. RABA's parser is strict, because a stray `$a` in
hand-written art is a typo worth catching. Both are right for their own input, so
`Logo::art_source` converts at the boundary. Without it `--list-logos` would call
Debian console-safe and `fastfetch:Debian` would fail to build.

**fastfetch renders logos bold.** Verified against the real output —
`ESC[1m ESC[31m`, not assumed. On a console bold turns the eight base colours
bright, so upstream's declared `31` is index 9 on screen and `39` is 15. Taking
the numbers at face value would have shipped every logo a shade dark. Debian's
inherited colours now come out `[9, 15]`, which is exactly what the hand
extraction had already produced independently.

That independence is now a test: `the_shipped_art_matches_what_upstream_actually_has`
compares each shipped theme against the vendored source, and the only permitted
differences are the three recorded hand edits. A hand edit cannot be lost in a
regeneration without a test going red.

`--list-logos` reports, per logo, whether a console can draw it: 515 of 545 can,
and the other 30 are braille, sextants or quadrant blocks that need a terminal
font. That is the question an author actually has, and it is now answered before
the build instead of during a boot.

### A fourth crate

`raba-gen build --preview` needed the stdout surface and the frame loop, which
lived in the `raba` binary. `raba-core` is pure by rule, so they could not move
there, and the alternative was a second copy — of frame timing and cursor
parking, which is exactly the logic that drifts silently when there are two of
it. `raba-term` now holds one copy and both binaries link it. It is std-only and
must stay so: `raba` links it, so anything added lands in an initramfs.

### Golden renders

Every theme is built, drawn through the real ANSI encoder, read back with the
terminal model, and the resulting grid recorded in `themes/golden/`. The grid is
golden rather than the byte stream, so an optimisation that emits shorter escapes
for the same picture does not churn fifty files, and the diff is characters and
colour indices a person can read. `RABA_BLESS=1` re-records.

The test was checked by breaking things on purpose, which is the only way to know
a golden test works. Two perturbations changed nothing and were right not to:
nudging the console palette's green by one does not move any nearest-colour
decision, and `Cell::BLANK` never survives, because frame 0 writes every cell.
Changing one spinner glyph was caught in all 50, each with the line of the first
difference.


### Generated animations

Ly has five built-in animations and four of them are procedural code rather than
art, so there was nothing to import. `raba-gen generate` produces them instead —
`doom`, `matrix`, `colormix`, `gameoflife`, under Ly's own names so a setup can be
carried across. See [generated.md](generated.md).

The interesting constraint is that all of them must **loop**, because `raba`(8)
repeats the boot animation until the system is ready, and a seam shows up in the
same place every few seconds. Each closes its loop differently: matrix by giving
every column a fall cycle that divides the frame count, colormix by using sine
waves over a whole number of turns, Life by *finding* its cycle rather than
imposing one. Fire does not close, and says so — it is chaotic, forcing the state
back would be a visible jump, and a crossfade needs blending a console cannot do.

Life was a random soup first, and measuring it changed the design: on an 80x25
torus **every seed tried settled to period 2**. The cycle detection was returning
an exact and correct answer, and the answer was a two-frame file of things
twitching. The board is now a garden of oscillators with coprime periods — 15, 3
and 2 — so it repeats every 30 generations, which the same search finds
independently.

Two things the tests caught that review had not. The shader dithered its brightest
band against bright yellow as a background, which on a console is the blink bit
rather than a bright colour: it would have flashed. That rule is now enforced at
compile time. And the module comment claiming backgrounds stayed in 0–7 was the
thing being contradicted, which is a reminder that a comment is not a check.

**Delta encoding keeps paying even when most of the screen moves.** Fire changes
about half the canvas every frame and still costs less than a dense format would,
and the black hole that drove the importer changes only 6.8%. The worry recorded in
`3d-logos.md` — that the format stops earning its place once the art is not a logo
with a spinner — turns out not to apply to real animation.

---

## Not scheduled: loading a console font

Raised while fixing the preview size. The console grid is the screen divided by
the font cell, so the font decides how many characters exist — on 1920x1080 that
ranges from 240x180 at 8x6 to 80x33 at 24x32, sixteen times the resolution on the
same screen. RABA could load a font itself, which is all `setfont`(8) does, and
that would remove the whole glyph problem: a font it loaded is a font it knows.

The costs are real and one of them is easy to get backwards. A font with more
than 256 glyphs takes the intensity bit for the second glyph page, so the console
drops to **eight** colours, not sixteen — every theme here uses the bright half.
A 256-glyph font keeps the colours and is still larger than the safe set in use
today. The font also has to be restored on every exit path including a crash.

Recorded in [console-size.md](console-size.md). It belongs after M3 because it
needs the same VT ioctl machinery and the same restore discipline.
