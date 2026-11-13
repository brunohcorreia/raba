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
| 6 | ANSI art and plain text importers | |
| 7 | `--also-dur` for display managers that play their own animation | |

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
centres a canvas that is smaller than the screen and crops one that is larger,
always returning exactly screen-sized output so the encoder's absolute cursor moves
are always in range. Drawing past the last column instead would leave the console
scrolled, which is the one thing a splash must not do. `raba-gen convert` warns at
build time, where the canvas can still be changed.

---

## M3 — The dangerous part

The VT layer, where a mistake hangs a boot. **The VM harness is built before a line of
this is written.**

- Tier 3a harness: QEMU kernel + initramfs, ~1 second per run, `isa-debug-exit`
- `VT_OPENQRY` to acquire a free terminal — never hardcode tty1
- Draw, and restore on **every** exit path including panic
- `TIOCL_SETKMSGREDIRECT` to move kernel messages aside; restore exactly
- Detect a terminal already in `KD_GRAPHICS` and yield
- `SIGKILL` mid-frame leaves a usable terminal — the test that matters most
- Pty tests for the escape encoder

---

## M4 — Lifecycle

- Control socket: `0600`, root-owned, `SO_PEERCRED` check rejecting non-root
- `raba-ctl`: `quit`, `deactivate`, `reactivate`, `message`, `ping`, `status`
- Message sanitisation — no caller bytes reach a terminal
- `/run/raba.state`, resumable: being killed stops being an event
- Phases: boot loop → outro → handoff, with `MaxDuration`
- Timeout **reveals the console** rather than exiting
- Conflict detection in the binary, not in service files

---

## M5 — Early boot

- mkinitcpio hook, busybox and systemd initramfs, one hook name
- dracut module — **v1, not later**: Void, Gentoo and Chimera are non-systemd and use dracut
- systemd units: start, quit, quit-wait
- `raba-setup` with `--dry-run` default, `--apply`, `--check`
- **TPM2 token detection with a warning before touching the initramfs**
- Hook never fails an image build
- Config frozen into the initramfs; `raba-set-theme` rebuilds

---

## M6 — Breadth

- runit, OpenRC, s6, dinit service scripts
- initramfs-tools
- LUKS hide-and-resume: inotify on `/run/systemd/ask-password/`, `cryptsetup` detection
- Full-distribution VM matrix, nightly
- Man pages built with scdoc; packaging
- Progress overlay, `pulse` by default

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
