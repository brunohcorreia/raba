# Maintenance policy

> How this stays maintainable, and how it survives breakage in projects it does not
> control.

This program runs in an environment where you cannot attach a debugger, there is no
shell, and the bug report you receive is "my computer doesn't boot". Normal code quality
is not sufficient. The structure has to make the hard parts testable without a machine.

---

## 1. Push everything into pure functions

The majority of this program is not I/O. Parsing a `.dur`, laying out a canvas,
compositing an overlay, deciding which frame to show at time *t*, downsampling colours —
all of it is a pure transformation.

```
┌─ core: pure, no I/O, no syscalls, 100% unit tested ───────┐
│  dur::parse(bytes)          -> Movie                      │
│  layout::place(movie, size) -> Placement                  │
│  render::compose(movie, overlays, frame) -> Grid          │
│  render::to_ansi(grid, prev) -> Vec<u8>                   │
└────────────────────────────────────────────────────────────┘
┌─ shell: thin, impure, behind a trait ─────────────────────┐
│  trait Surface { fn size(); fn write(); fn restore(); }    │
│    VtSurface      - real virtual terminal   (raba)         │
│    StdoutSurface  - --preview               (raba-term)    │
│    BufferSurface  - tests                   (raba-core)    │
└────────────────────────────────────────────────────────────┘

The trait and `BufferSurface` are in `raba-core` because neither does any I/O.
`StdoutSurface` does, so it lives in `raba-term`, which both binaries link:
`raba` to play a splash and `raba-gen` for `build --preview`. One copy of the
frame timing, not two that drift.
```

The rule: **if it can be a function from data to data, it is one.** The impure part
should be small enough to review line by line, because it is the part that cannot be
unit tested.

A useful consequence: `--preview` is not a debug feature bolted on. It is the same core
driving a different `Surface`. If preview looks right, the core is right, and only the
VT layer remains in doubt.

---

## 2. Dependency budget

Every crate is a liability in a program that must never break. API churn, MSRV bumps,
advisories, and transitive growth all become this project's problem at the worst
possible time.

**Budget: 10 direct dependencies, and a written justification for each.**

`DEPENDENCIES.md` records, per crate: what it does, why the standard library is not
enough, what breaks if it is removed, and whether it could be vendored.

### Split the budget in two

The two binaries have opposite constraints, and conflating them is how a boot program
acquires a JSON parser.

**`raba` — the boot path. Target: zero dependencies beyond a syscall wrapper.** With
`.raba` as the runtime format, there is no JSON and no decompression, so `serde_json` and
`flate2` never enter. What remains is ioctls, termios and a socket, which is `rustix` or
nothing — plus `libc`, added at M4 because `std` has no signal API and a splash that
cannot handle `SIGTERM` is a splash killed mid-frame. It is bindings rather than code:
the release binary is the same size with and without it, measured. Two, and the reasons
for both are in [DEPENDENCIES.md](../DEPENDENCIES.md).

**`raba-gen` — build time. Spend freely.** It never enters an initramfs and is never on
the boot path, so `serde`, `toml`, `flate2` and a JSON parser are all fine here. A
dependency that breaks `raba-gen` is a build failure someone fixes at leisure.

This split is the single biggest reason to own the runtime format.

### Rules

- No async runtime. A frame loop is a loop.
- No crate pulled in for a single function that is twenty lines of `std`.
- `cargo-deny` in CI for licences and advisories.
- Pin a MSRV and test it. AUR users are always on the newest `rustc`; distributions
  packaging this later will not be.

### The size gate

`.binary-size-limit` holds one number: the largest `raba` may be, as a static musl
build, before CI fails. The release profile is already as small as cargo can make it —
`opt-level = "z"`, `lto`, one codegen unit, `panic = "abort"`, `strip` — so the gate is
measuring the code, not the settings.

| | |
|---|---|
| limit | 552960 (540 KiB) |
| `raba`, static musl, 13 November 2026 | 537264 |
| headroom | 15696 bytes, 2.8% |

The gate is sized so that a bug fix does not trip it and a new dependency does: almost
nothing worth linking is under 16 KiB. At the current headroom the next dependency is a
decision rather than an accident, which is the intent — but 2.8% also means a few more
diagnostics will reach it, and the next commit that does must either justify a new limit
in the same commit or reduce the binary.

The limit has moved once. The original 524288 (512 KiB) was a round number chosen before
there was anything to measure; the binary crossed it in `7bfe037c`, the commit that made
a splash which cannot draw say so rather than exit quietly. The diagnostics added since —
the single durable writer for `/etc`, a message for each way a boot can be sabotaged — are
what §7 of this document asks for. Moving the gate and recording the measurement was the
correct response; dropping the diagnostics to stay under a number nobody had derived was
not.

**A gate is only worth having if its result is read.** This one stayed red for fourteen
commits, the 0.1.0 release among them, with the CI badge in the README reporting it the
whole time. The gate worked; nothing was looking at it. CI failures are therefore to be
treated as blocking, including on a release tag.

---

## 3. Make the invariants mechanical, not aspirational

"Never break boot" is a slogan until something enforces it.

```rust
#![forbid(unsafe_code)]          // everywhere except the vt module
#![deny(clippy::unwrap_used)]    // outside tests
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::indexing_slicing)]
```

`unsafe` lives only in `vt/`, each block carrying a comment stating the invariant it
relies on and why it holds.

The panic handler restores the terminal before aborting, but the goal is for it never to
run. Lints are cheaper than a panic handler that was itself never tested.

### The test that matters most

A table-driven test that feeds every failure mode and asserts exit code 0:

| input | expect |
|---|---|
| theme missing | exit 0, nothing drawn |
| theme not gzip | exit 0 |
| gzip valid, JSON invalid | exit 0 |
| JSON valid, schema wrong | exit 0 |
| decompresses to 4 GB | exit 0, refused before allocating |
| canvas 10⁹ × 10⁹ | exit 0, refused |
| zero frames | exit 0 |
| no `/dev/tty` | exit 0 |
| `raba.enable=0` | exit 0 |
| `SIGKILL` mid-frame | terminal usable afterwards |
| config has an unknown key | exit 0, that key ignored, the rest applied |
| config is 4 MB, or not UTF-8 | exit 0, defaults kept |

Rows about configuration live in `crates/raba/tests/config.rs`; the two kernel
parameters and `--check`'s exit codes are in `crates/raba/tests/check.rs`, which
drives the real binary. The rule they enforce is the one that makes a lenient
parser safe: **the boot path never refuses a file, and `raba --check` always
does** — so a typo is still reported, at a keyboard rather than at a boot.

### Fuzzing

The `.dur` parser is the highest-value fuzz target in the project: untrusted-shaped input,
parsed as root, early in boot, where a hang is an unbootable machine. `cargo-fuzz` with
the committed fixtures as a seed corpus, running in CI.

Hard caps enforced *during* decompression, not after: decompressed size, canvas
dimensions, frame count.

---

## 4. Golden tests instead of a virtual machine

Rendering regressions should be caught in milliseconds, not by booting something.

```
themes/golden/            one .txt per shipped theme: the rendered grid
crates/raba-gen/tests/golden.rs
```

A theme is built, drawn through the real renderer, and read back with the terminal model
into a grid; that grid is what is recorded, for all fifty-one themes. **The grid is
golden, not the byte stream** — an optimisation that emits shorter escape sequences for
the same picture should not churn fifty files, and a diff of characters and colour
indices is one a person can actually read.

The v7/v8 colour-map transposition is covered the same way, in
`crates/raba-gen/tests/dur.rs`: the same artwork in both formats has to produce the same
grid. A Durdraw release that changes the format again turns that red before any user
sees it.

Commit a case for every format version, colour format and encoding claimed as supported
in [dur-support.md](dur-support.md). **The support table and the tests are the same
list.** A row with no test is an untested claim.

---

## 5. Coverage, and what the number is not

`scripts/coverage.sh` builds the workspace with rustc's own instrumentation and reads
the counters with the LLVM tools already on the machine. No new dependency, nothing to
install, and it answers one question: **is there code here that no test has ever run?**

"Region", not "branch". rustc emits region counters and leaves llvm-cov's branch
columns empty, so the report cannot say "this `if` was only ever taken one way" in
those words — but a region is a straight-line stretch of code with its own counter,
and each arm of an `if` or a `match` is its own region. A conditional taken only one
way shows up as a missed region on the arm nobody ran, which is the question worth
asking.

Measured 13 November 2026, with `scripts/vm-test.sh --coverage` run first so that the
guest's counters are in the merge:

| what is counted | regions | lines |
|---|---|---|
| everything in `crates/`, less the tests, the test oracle and `raba-vmtest` | **96.1%** | **97.2%** |
| `crates/raba/src/vt/mod.rs`, the half that needs a real console | 81.9% | 86.7% |

Each exclusion is listed with its reason at the foot of `scripts/coverage.sh`. In
short: the suite, the renderer's test oracle and the VM harness are instruments
rather than product, and counting them inflates the figure.

### A low figure is a measurement question first

Earlier readings — 62% / 61% across `crates/`, and 81% / 86% with `vt/` and `boot.rs`
excluded as unmeasurable — were both wrong, because nothing that ran inside the
virtual machine was being counted. Two independent faults, now fixed:

- `crates/raba-vmtest` was built without instrumentation, and it exits through an
  `isa-debug-exit` port write, which runs no `atexit` handler. The guest therefore
  wrote no profile at all, and seventeen `vt_checks` were passing while measuring as
  untested. `--coverage` now builds the harness instrumented, runs the checks in a
  child process that exits normally, and returns the profile over the serial line.
- The staleness guard that decides whether the VM profile may be merged counted
  `#[cfg(test)]` sibling `tests.rs` files as guest sources. Those cannot alter the
  guest binary's coverage map, so every host-side test edit discarded the profile.

### What the figure has found

The number is a search tool rather than a score. What it has surfaced that nothing
else did:

- **`raba-setup`'s command line was 13% covered.** The argument parser and every
  refusal standing in front of the one write this program does. A mistake there does
  not produce a wrong answer, it produces a wrong *action* on somebody's machine.
  `crates/raba-setup/tests/cli.rs` now drives the real binary for each of them, and
  asserts twice every time: the right reason, and a configuration file that is exactly
  as it was.
- **`state::load`'s rejection reasons had never been rendered.** Nine `Display` arms
  explaining why a resume was refused, and the one caller threw the reason away with
  `unwrap_or(None)` — against its own doc comment, which says "the caller logs it".
  A splash that silently restarted at frame 0 could not say why.
- **`write::replace_durably` had never been executed.** The variant with the `fsync`,
  which is the one that writes `/etc/raba/raba.conf`.
- **Four shipped defects in the push from 87% to 96%**, each found while writing the
  test for a line nothing reached: `Outro.Theme` was parsed and never loaded;
  `raba-gen generate`'s size options silently substituted defaults; `raba --check`
  reported the file's `MaxDuration` rather than the effective one; and a missing art
  file was reported without naming the recipe that referred to it, which is not
  actionable when `make` is running over a directory of recipes. Six functions proved
  to be dead and were deleted.

### What remains uncovered

Counts are missed lines and total the 158 the report prints.

| | lines | why |
|---|---|---|
| `vt/mod.rs`, `vt/ioctl.rs`, `signals.rs` | 35 | the error arm of an ioctl or a `sigaction` that the kernel would have to refuse one line after accepting the same call |
| `boot.rs` | 23 | guards against states made impossible a line earlier: the stall watchdog, the resume guards, and the refusal of a theme canvas that `decode` has already rejected |
| `raba-setup`'s `ThisMachine` and its prompt | 16 | the lines that delegate to the real machine, and the question put to a real stdin before rebuilding an initramfs on a TPM-sealed disk |
| `if let Some(…)` and `let … else` guards that cannot fail | 47 | second lines of defence in `ansi.rs`, `life.rs`, `fire.rs`, `build.rs`, `compose.rs`, `place.rs`, `render/`, `play.rs`: a grid index bounded by the loop that produced it, a `.first()` on a `const` array |
| root- or kernel-only runtime paths | 16 | a peer `uid` that is not ours, a `/dev/kmsg` write, a disk filling mid-`write`, a socket failing mid-reply, a terminal reporting a size of zero |
| command-line arms with no reachable caller | 13 | a `--check` field combination the parser already excludes, a frame rate the parser already bounds |
| `std`'s `thread_local` shim | 8 | not RABA's code; inlined into its object, where the ignore list cannot reach it |

A row leaving that table is a gap. A row joining it needs a reason written beside it.

---

## 6. Written-down contracts

These are public API. Breaking them silently is how a maintainable project stops being
one. Each belongs in a document and in a test.

| contract | where |
|---|---|
| `.dur` subset accepted | [dur-support.md](dur-support.md) |
| config keys and defaults | `man 5 raba.conf` |
| `raba-ctl` commands and exit codes | `man 1 raba-ctl` |
| control socket wire protocol | [protocol.md](protocol.md) |
| theme recipe schema | `man 5 raba-theme` |
| exit code 0 always | this document |

Semantic versioning applies to all of them, not just the library.

---

## 7. Observability for an undebuggable environment

You get one shot at collecting information, from a user who is frustrated and possibly
reading this on a phone.

What 0.1.0 does:

- **One line on stderr for anything that went wrong**, and when nothing is capturing
  stderr — an initramfs, where it is a file or `/dev/null` — the same line again as a
  `<3>` record in `/dev/kmsg`, so `dmesg` and `journalctl -b` both have it exactly once.
  The test is a `getsockopt` on stderr and an `isatty`, not a flag. A healthy boot
  leaves nothing of ours in the kernel log at all, and
  `integration/tests/distro/check.sh` asserts both halves.
- **`/run/raba.log`** for the two cases a message cannot go to the console: `raba.debug=1`
  writes its whole report there, and a splash displaced by something taking the screen
  writes why — printing that one would draw it over whatever just took the screen.
- **`raba --check`** validates configuration and theme with no side effects and a
  meaningful exit code. Safe on a live system.
- **`raba-ctl status`** asks a *running* splash what it thinks it is doing: phase,
  terminal, theme, frame. It is what proved a "stalled" Debian boot was advancing at
  125ms a frame all along.

What is not built, and why it is listed rather than forgotten:

- **A post-boot unit copying the log to `/var/log/raba/last-boot.log`.** `/run` is a
  tmpfs, so the evidence from a boot that went wrong is gone by the time somebody
  reboots to ask about it. Nothing writes persistent storage today and adding the first
  thing that does deserves its own decision.
- **`raba-report`**, collecting log, configuration, initramfs contents, `/proc/cmdline`,
  console font, terminal size and versions into something pasteable.

That last one sounds like polish. It is the difference between a reproducible bug and an
issue closed as stale, and it is the first thing to build the day somebody other than us
files one.

---

## 8. Upstream watch

Breakage will come from projects outside this one. Make it arrive as a CI failure
rather than a user report.

| upstream | what breaks | detection |
|---|---|---|
| Durdraw | format version bump | golden fixtures per version, converter only |
| mkinitcpio | hook API | matrix against several versions (v2+) |
| systemd | unit semantics | VM boot test |
| kernel | fbcon, VT behaviour | VM boot test across kernel versions |
| Rust crates | API, advisories | `cargo-deny`, Dependabot |

A weekly scheduled CI run against upstream `HEAD` for each import format turns "a user
reported colours are scrambled" into "a job went red on Tuesday". Because these are
build-time converters, a red job is never an unbootable machine.

---

## 9. What good looks like

A new contributor should be able to:

1. `cargo test` and have real confidence, without a VM.
2. `raba-gen themes/x.toml --preview` and see their work, without a VM.
3. Read `vt/` in one sitting — it is the only part where being wrong is dangerous.
4. Find the reason for any non-obvious decision in `arquitetura.md` or `docs/`.

And a maintainer picking this up in three years should find that the test suite still
passes, tells them what the project promised, and fails loudly when an upstream moved.
