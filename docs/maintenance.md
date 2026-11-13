# Maintenance policy

> How this stays maintainable, and how it survives things we do not control breaking.

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
advisories, and transitive growth all become our problem at the worst possible time.

**Budget: 10 direct dependencies, and a written justification for each.**

`DEPENDENCIES.md` records, per crate: what it does, why the standard library is not
enough, what breaks if it is removed, and whether we could vendor it.

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
tests/golden/
  arch-v7.dur          fixture
  arch-v7.expected     rendered grid, as text
  arch-v8.dur          same art, format 8
  arch-v8.expected     must be byte-identical to v7's output
```

That last pair is the regression test for the v7/v8 colour map transposition. A Durdraw
release that changes the format again should turn this red before any user sees it.

Commit a fixture for every format version, colour format and encoding claimed as
supported in [dur-support.md](dur-support.md). **The support table and the fixture
directory should be the same list.** If a row has no fixture, the claim is untested.

---

## 5. Written-down contracts

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

## 6. Observability for an undebuggable environment

You get one shot at collecting information, from a user who is frustrated and possibly
reading this on a phone.

- `/run/raba.log` written always, regardless of `Level`, for the current boot.
- A post-boot unit copies it to `/var/log/raba/last-boot.log`, because `/run` is a tmpfs
  and a failed boot otherwise leaves nothing.
- `raba --check` validates config and theme with no side effects and a meaningful exit
  code — safe to run on a live system.
- `raba-report` collects log, config, initramfs contents, `/proc/cmdline`, console font,
  terminal size and versions into something pasteable.

The last one sounds like polish. It is the difference between a reproducible bug and a
closed-as-stale issue.

---

## 7. Upstream watch

Breakage will come from projects we do not control. Make it arrive as a CI failure
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

## 8. What good looks like

A new contributor should be able to:

1. `cargo test` and have real confidence, without a VM.
2. `raba-gen themes/x.toml --preview` and see their work, without a VM.
3. Read `vt/` in one sitting — it is the only part where being wrong is dangerous.
4. Find the reason for any non-obvious decision in `arquitetura.md` or `docs/`.

And a maintainer picking this up in three years should find that the test suite still
passes, tells them what the project promised, and fails loudly when an upstream moved.
